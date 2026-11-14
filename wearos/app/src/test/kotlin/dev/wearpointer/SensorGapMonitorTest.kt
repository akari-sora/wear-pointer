package dev.wearpointer

import dev.wearpointer.motion.SensorGapMonitor
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SensorGapMonitorTest {
    private val ms = 1_000_000L

    @Test
    fun continuousSamplesNeverRebase() {
        val monitor = SensorGapMonitor(500 * ms)
        for (i in 0 until 1_000) assertFalse(monitor.onSample(i * 10 * ms, paused = false))
        assertFalse(monitor.onResume())
    }

    @Test
    fun holeWhileActiveRebasesAtOnce() {
        val monitor = SensorGapMonitor(500 * ms)
        monitor.onSample(0, paused = false)
        assertTrue(monitor.onSample(800 * ms, paused = false))
        assertFalse(monitor.onSample(810 * ms, paused = false))
    }

    /** fed1967 behaviour missed this: the hole is seen while paused, and resume checked only wall time. */
    @Test
    fun holeWhilePausedIsReportedOnResume() {
        val monitor = SensorGapMonitor(500 * ms)
        monitor.onSample(0, paused = false)
        monitor.onSample(10 * ms, paused = true)
        assertFalse(monitor.onSample(5_000 * ms, paused = true))
        // Samples keep flowing after the hole, before the user resumes.
        assertFalse(monitor.onSample(5_010 * ms, paused = true))
        assertTrue(monitor.onResume())
        assertFalse(monitor.onResume())
    }

    @Test
    fun lateButCompleteFifoIsNotAHole() {
        val monitor = SensorGapMonitor(500 * ms)
        monitor.onSample(0, paused = true)
        // Delivered in one burst after the CPU woke, timestamps still 10 ms apart.
        for (i in 1..600) assertFalse(monitor.onSample(i * 10 * ms, paused = true))
        assertFalse(monitor.onResume())
    }

    @Test
    fun smallGapsAreCountedForTheLog() {
        val monitor = SensorGapMonitor(500 * ms)
        monitor.onSample(0, paused = false)
        monitor.onSample(100 * ms, paused = false)
        monitor.onSample(110 * ms, paused = false)
        monitor.onSample(300 * ms, paused = false)
        assertEquals(2, monitor.takeSmallGaps())
        assertEquals(0, monitor.takeSmallGaps())
    }
}
