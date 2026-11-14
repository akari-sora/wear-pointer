package dev.wearpointer.motion

/**
 * Detects holes in the gyro stream by sensor timestamp.
 *
 * Non-wakeup sensors keep running while the CPU suspends, but their events
 * sit in a hardware FIFO of limited size. A full FIFO drops events; a sensor
 * without one loses everything from the suspended stretch. The integrated
 * heading across such a hole is wrong, so the heading must be rebased.
 *
 * - Active: a hole reported by [onSample] means rebase now.
 * - Paused: the hole is remembered, and [onResume] reports it.
 *
 * Detection uses event timestamps, not delivery time, so a FIFO that is
 * delivered late but complete is not a hole.
 */
class SensorGapMonitor(private val gapNs: Long) {
    private var lastNs: Long? = null
    private var staleWhilePaused = false

    /** Gaps above [SMALL_GAP_NS] since the last [takeSmallGaps], for logging. */
    private var smallGaps = 0

    /** @return true when the heading must be rebased now. */
    fun onSample(timestampNs: Long, paused: Boolean): Boolean {
        val gap = lastNs?.let { timestampNs - it } ?: 0L
        lastNs = timestampNs
        if (gap > SMALL_GAP_NS) smallGaps++
        if (gap <= gapNs) return false
        if (paused) {
            staleWhilePaused = true
            return false
        }
        return true
    }

    /** @return true when a hole opened while paused, so the heading must be rebased. */
    fun onResume(): Boolean {
        val stale = staleWhilePaused
        staleWhilePaused = false
        return stale
    }

    fun takeSmallGaps(): Int {
        val count = smallGaps
        smallGaps = 0
        return count
    }

    fun reset() {
        lastNs = null
        staleWhilePaused = false
        smallGaps = 0
    }

    companion object {
        /**
         * [AimTracker] clamps one integration step to 50 ms. Gaps between this
         * and the rebase threshold lose some heading without a rebase. They are
         * counted so Stage 0 logs show whether that happens on a Watch7.
         */
        const val SMALL_GAP_NS = 50_000_000L
    }
}
