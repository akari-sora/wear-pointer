package dev.wearpointer.ble

import dev.wearpointer.proto.Packets
import java.util.ArrayDeque

/**
 * One in-flight GATT operation at a time, in FIFO order. Not thread-safe:
 * the owning transport calls it from a single handler thread.
 *
 * Sequence numbers are written when a packet is first handed to the stack,
 * not when the sensor loop builds it, so the receiver sees them in send
 * order. A coalesced aim never receives a sequence. A newer aim replaces only
 * a queued aim that is still the tail, so a click already behind that aim is
 * not jumped.
 *
 * Center and Pause are calibration / state boundaries. The aim at the tail
 * (built against the old base and not yet sent) is dropped. An aim that is
 * already ahead of a click stays: it goes out before the boundary, so the
 * receiver still reads it against the old base.
 *
 * API rejection ([onApiRejected]): the stack did not take the bytes.
 * - retryable (busy): the same bytes, same sequence, are sent again after
 *   [retryDelayMs], up to [MAX_API_ATTEMPTS] attempts in total;
 * - permanent, or the attempt budget is spent: [Transmit.DropLink].
 *
 * Callback failure ([onFinished] with `false`) or [onTimeout]: delivery is
 * unknown, so nothing is resent (a click could be applied twice).
 * - aim or stats: counted, the queue moves on. Later samples/windows supersede them.
 * - anything else: [Transmit.DropLink]. The desktop releases buttons on
 *   disconnect. A lost drag-up while later aims keep arriving would not trip
 *   the receiver's 300 ms no-aim release, so the link drop is what frees it.
 *
 * After [Transmit.DropLink] the waiting packets are discarded with the link.
 */
class OutboundQueue {
    sealed class Transmit {
        /** Hand these bytes to the stack now. */
        data class Send(val packet: ByteArray) : Transmit()

        /** Hand the same bytes to the stack again after [delayMs]. */
        data class Retry(val packet: ByteArray, val delayMs: Long) : Transmit()
        data object Idle : Transmit()
        data object DropLink : Transmit()
    }

    data class Counters(
        val accepted: Int = 0,
        val completed: Int = 0,
        val rejected: Int = 0,
        val coalesced: Int = 0,
    )

    private val waiting = ArrayDeque<ByteArray>()
    private var inFlight: ByteArray? = null
    private var attempts = 0
    private var nextSequence = 0
    private var accepted = 0
    private var completed = 0
    private var rejected = 0
    private var coalesced = 0

    fun enqueue(packet: ByteArray): Transmit {
        when (kind(packet)) {
            Packets.KIND_AIM -> enqueueAim(packet)
            Packets.KIND_CENTER, Packets.KIND_PAUSE -> {
                dropTrailingAim()
                waiting.addLast(packet)
            }
            else -> waiting.addLast(packet)
        }
        return poll()
    }

    /** The stack accepted the bytes. Completion is [onFinished]. */
    fun markAccepted() {
        accepted++
    }

    /**
     * The stack refused the bytes synchronously.
     * @param retryable the refusal is transient (busy); the same bytes may be
     * handed over again. Otherwise the link is dropped at once.
     */
    fun onApiRejected(retryable: Boolean): Transmit {
        val packet = inFlight ?: return Transmit.Idle
        rejected++
        if (retryable && attempts < MAX_API_ATTEMPTS) {
            val delay = retryDelayMs(attempts)
            attempts++
            return Transmit.Retry(packet, delay)
        }
        abandon()
        return Transmit.DropLink
    }

    /** The completion callback for the packet in flight. */
    fun onFinished(success: Boolean): Transmit {
        val packet = inFlight ?: return Transmit.Idle
        if (success) {
            completed++
        } else {
            rejected++
            val k = kind(packet)
            if (k != Packets.KIND_AIM && k != Packets.KIND_STATS) {
                abandon()
                return Transmit.DropLink
            }
        }
        inFlight = null
        attempts = 0
        return poll()
    }

    /** No completion callback arrived. The stack is stuck; delivery unknown. */
    fun onTimeout(): Transmit {
        if (inFlight == null) return Transmit.Idle
        rejected++
        abandon()
        return Transmit.DropLink
    }

    /** New link or no link: forget everything, sequence restarts at 0. */
    fun clear() {
        waiting.clear()
        inFlight = null
        attempts = 0
        nextSequence = 0
    }

    fun takeCounters(): Counters {
        val snapshot = Counters(accepted, completed, rejected, coalesced)
        accepted = 0
        completed = 0
        rejected = 0
        coalesced = 0
        return snapshot
    }

    fun queuedCritical(): Int = waiting.count { kind(it) != Packets.KIND_AIM }

    fun hasPendingAim(): Boolean = waiting.any { kind(it) == Packets.KIND_AIM }

    fun isBusy(): Boolean = inFlight != null

    private fun enqueueAim(packet: ByteArray) {
        dropTrailingAim()
        waiting.addLast(packet)
    }

    private fun dropTrailingAim() {
        val tail = waiting.peekLast() ?: return
        if (kind(tail) == Packets.KIND_AIM) {
            waiting.removeLast()
            coalesced++
        }
    }

    private fun poll(): Transmit {
        if (inFlight != null) return Transmit.Idle
        val next = waiting.pollFirst() ?: return Transmit.Idle
        Packets.stampSequence(next, nextSequence)
        nextSequence = (nextSequence + 1) and 0xFFFF
        inFlight = next
        attempts = 1
        return Transmit.Send(next)
    }

    /** Link is going down. Nothing stays in flight, nothing waits. */
    private fun abandon() {
        inFlight = null
        attempts = 0
        waiting.clear()
    }

    private fun kind(packet: ByteArray): Int =
        if (packet.size < 2) -1 else packet[1].toInt() and 0xFF

    companion object {
        /** Total hand-overs of one packet, first try included. */
        const val MAX_API_ATTEMPTS = 5

        /** 5, 10, 20, 40 ms: about 75 ms of busy before the link is dropped. */
        fun retryDelayMs(attemptsSoFar: Int): Long = 5L shl (attemptsSoFar - 1).coerceIn(0, 4)
    }
}
