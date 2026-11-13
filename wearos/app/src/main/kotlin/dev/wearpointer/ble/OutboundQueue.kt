package dev.wearpointer.ble

import dev.wearpointer.proto.Packets
import java.util.ArrayDeque

/**
 * One in-flight GATT notification at a time.
 *
 * The characteristic is notify-only. `confirm` on
 * `notifyCharacteristicChanged` must stay false; this queue does not turn a
 * packet into an indication. Aim samples coalesce to the latest while a
 * notification is outstanding. Clicks and drags are queued and are not
 * replaced by aim traffic. Disconnect clears anything still queued, because
 * the link cannot deliver it.
 */
class OutboundQueue {
    data class Counters(
        val accepted: Int = 0,
        val completed: Int = 0,
        val rejected: Int = 0,
        val coalesced: Int = 0,
    )

    private var inFlight = false
    private var pendingAim: ByteArray? = null
    private val critical = ArrayDeque<ByteArray>()
    private var accepted = 0
    private var completed = 0
    private var rejected = 0
    private var coalesced = 0

    /** Packet to hand to `notifyCharacteristicChanged`, or null if one is already in flight. */
    fun enqueue(packet: ByteArray): ByteArray? {
        if (packet.size >= 2 && packet[1].toInt() and 0xFF == Packets.KIND_AIM) {
            if (pendingAim != null) coalesced++
            pendingAim = packet
        } else {
            critical.addLast(packet)
        }
        return poll()
    }

    /** The stack accepted the notify call. Completion is [onFinished]. */
    fun markAccepted() {
        accepted++
    }

    /** `notifyCharacteristicChanged` returned an error before a callback. */
    fun onApiRejected(): ByteArray? {
        if (!inFlight) return null
        inFlight = false
        rejected++
        return poll()
    }

    /** `onNotificationSent`. [success] is GATT_SUCCESS. */
    fun onFinished(success: Boolean): ByteArray? {
        if (!inFlight) return null
        inFlight = false
        if (success) completed++ else rejected++
        return poll()
    }

    fun clear() {
        pendingAim = null
        critical.clear()
        inFlight = false
    }

    fun takeCounters(): Counters {
        val snapshot = Counters(accepted, completed, rejected, coalesced)
        accepted = 0
        completed = 0
        rejected = 0
        coalesced = 0
        return snapshot
    }

    fun queuedCritical(): Int = critical.size

    fun hasPendingAim(): Boolean = pendingAim != null

    private fun poll(): ByteArray? {
        if (inFlight) return null
        val next = critical.pollFirst() ?: pendingAim?.also { pendingAim = null } ?: return null
        inFlight = true
        return next
    }
}
