package dev.wearpointer.ble

import dev.wearpointer.proto.Packets
import java.util.ArrayDeque

/**
 * One in-flight GATT notification at a time, in FIFO order.
 *
 * Sequence numbers are written when a packet is first handed to the stack,
 * not when the sensor loop builds it. A coalesced aim never receives a
 * sequence. A newer aim replaces only a queued aim that is still the tail,
 * so a click already behind that aim is not jumped. Center and Pause drop
 * every aim still waiting, because those poses belong to the old base.
 *
 * [Transmit.Send] on an API rejection is the same bytes again (the stack did
 * not accept them), up to [MAX_API_ATTEMPTS]. A callback failure means
 * delivery is unknown, so the packet is not resent. The caller must drop the
 * link; the desktop then releases buttons. A lost drag-up while later aims
 * are still flowing does not trip the receiver's 300 ms silence release.
 */
class OutboundQueue {
    sealed class Transmit {
        data class Send(val packet: ByteArray) : Transmit()
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
                dropQueuedAims()
                waiting.addLast(packet)
            }
            else -> waiting.addLast(packet)
        }
        return poll()
    }

    /** The stack accepted the notify call. Completion is [onFinished]. */
    fun markAccepted() {
        accepted++
    }

    /**
     * `notifyCharacteristicChanged` returned an error. The packet was not
     * accepted, so the same bytes are retried until [MAX_API_ATTEMPTS].
     */
    fun onApiRejected(): Transmit {
        val packet = inFlight ?: return Transmit.Idle
        rejected++
        if (attempts < MAX_API_ATTEMPTS) {
            attempts++
            return Transmit.Send(packet)
        }
        abandonInFlight()
        return Transmit.DropLink
    }

    /**
     * `onNotificationSent`. A failure does not resend: the central may already
     * have applied a click.
     */
    fun onFinished(success: Boolean): Transmit {
        if (inFlight == null) return Transmit.Idle
        if (!success) {
            rejected++
            abandonInFlight()
            return Transmit.DropLink
        }
        completed++
        inFlight = null
        attempts = 0
        return poll()
    }

    /** No callback arrived. Delivery is unknown, same as a failed callback. */
    fun onTimeout(): Transmit {
        if (inFlight == null) return Transmit.Idle
        rejected++
        abandonInFlight()
        return Transmit.DropLink
    }

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

    private fun enqueueAim(packet: ByteArray) {
        val tail = waiting.lastOrNull()
        if (tail != null && kind(tail) == Packets.KIND_AIM) {
            waiting.removeLast()
            coalesced++
        }
        waiting.addLast(packet)
    }

    private fun dropQueuedAims() {
        val kept = ArrayDeque<ByteArray>()
        for (packet in waiting) {
            if (kind(packet) == Packets.KIND_AIM) coalesced++ else kept.addLast(packet)
        }
        waiting.clear()
        waiting.addAll(kept)
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

    private fun abandonInFlight() {
        inFlight = null
        attempts = 0
        waiting.clear()
    }

    private fun kind(packet: ByteArray): Int =
        if (packet.size < 2) -1 else packet[1].toInt() and 0xFF

    companion object {
        const val MAX_API_ATTEMPTS = 3
    }
}
