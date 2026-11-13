package dev.wearpointer

import dev.wearpointer.ble.OutboundQueue
import dev.wearpointer.proto.Packets
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class OutboundQueueTest {
    @Test
    fun aimCoalescesWhileANotificationIsInFlight() {
        val queue = OutboundQueue()
        val first = send(queue.enqueue(Packets.aim(99, 0, 0, 0)))
        assertEquals(0, sequenceOf(first))
        assertTrue(queue.enqueue(Packets.aim(99, 0, 10, 0)) is OutboundQueue.Transmit.Idle)
        assertTrue(queue.enqueue(Packets.aim(99, 0, 20, 0)) is OutboundQueue.Transmit.Idle)
        val next = send(queue.onFinished(success = true))
        assertEquals(Packets.KIND_AIM, kind(next))
        assertEquals(1, sequenceOf(next))
        assertEquals(20, leftCdeg(next))
        assertEquals(1, queue.takeCounters().coalesced)
        assertTrue(queue.onFinished(success = true) is OutboundQueue.Transmit.Idle)
    }

    @Test
    fun dragUpIsNotReplacedByAimTraffic() {
        val queue = OutboundQueue()
        send(queue.enqueue(Packets.aim(0, 0, 0, 0)))
        repeat(8) {
            queue.enqueue(Packets.aim(0, 0, it, 0))
        }
        val dragUp = Packets.drag(0, 50_000, down = false)
        assertTrue(queue.enqueue(dragUp) is OutboundQueue.Transmit.Idle)
        val sent = mutableListOf<ByteArray>()
        var step = queue.onFinished(success = true)
        while (step is OutboundQueue.Transmit.Send) {
            sent += step.packet
            step = queue.onFinished(success = true)
        }
        assertTrue(sent.any { kind(it) == Packets.KIND_DRAG && it[8].toInt() == 0 })
    }

    @Test
    fun pendingAimIsNotOvertakenAndSequencesFollowSendOrder() {
        val queue = OutboundQueue()
        val sent = ArrayList<ByteArray>()
        fun push(packet: ByteArray) {
            val step = queue.enqueue(packet)
            if (step is OutboundQueue.Transmit.Send) sent += step.packet
        }
        push(Packets.aim(1, 1_000, 11, 0))
        push(Packets.aim(2, 2_000, 22, 0))
        push(Packets.leftClick(3, 3_000))
        var step = queue.onFinished(success = true)
        while (step is OutboundQueue.Transmit.Send) {
            sent += step.packet
            step = queue.onFinished(success = true)
        }
        assertEquals(
            listOf(Packets.KIND_AIM, Packets.KIND_AIM, Packets.KIND_LEFT_CLICK),
            sent.map { kind(it) },
        )
        val sequences = sent.map { sequenceOf(it) }
        assertEquals(listOf(0, 1, 2), sequences)
        assertEquals(0, lossOf(sequences))
        assertEquals(22, leftCdeg(sent[1]))
    }

    @Test
    fun coalescedAimsDoNotOpenASequenceGap() {
        val queue = OutboundQueue()
        val first = send(queue.enqueue(Packets.aim(5, 0, 1, 0)))
        queue.enqueue(Packets.aim(6, 0, 2, 0))
        queue.enqueue(Packets.aim(7, 0, 3, 0))
        queue.enqueue(Packets.aim(8, 0, 4, 0))
        val second = send(queue.onFinished(success = true))
        assertEquals(listOf(0, 1), listOf(sequenceOf(first), sequenceOf(second)))
        assertEquals(4, leftCdeg(second))
        assertEquals(0, lossOf(listOf(0, 1)))
        assertTrue(queue.takeCounters().coalesced >= 1)
    }

    @Test
    fun apiRejectRetriesTheSameDragUpInsteadOfTheNextAim() {
        val queue = OutboundQueue()
        val drag = send(queue.enqueue(Packets.drag(7, 1_000, down = false)))
        assertTrue(queue.enqueue(Packets.aim(8, 2_000, 4, 0)) is OutboundQueue.Transmit.Idle)
        val retried = send(queue.onApiRejected())
        assertEquals(Packets.KIND_DRAG, kind(retried))
        assertTrue(retried.contentEquals(drag))
        assertEquals(sequenceOf(drag), sequenceOf(retried))
    }

    @Test
    fun apiRejectDropsTheLinkAfterTheAttemptBudget() {
        val queue = OutboundQueue()
        send(queue.enqueue(Packets.drag(1, 1, down = false)))
        queue.enqueue(Packets.aim(2, 2, 1, 0))
        assertTrue(queue.onApiRejected() is OutboundQueue.Transmit.Send)
        assertTrue(queue.onApiRejected() is OutboundQueue.Transmit.Send)
        assertTrue(queue.onApiRejected() is OutboundQueue.Transmit.DropLink)
        assertTrue(queue.onFinished(success = true) is OutboundQueue.Transmit.Idle)
        assertFalse(queue.hasPendingAim())
    }

    @Test
    fun callbackFailureDoesNotResendTheClick() {
        val queue = OutboundQueue()
        send(queue.enqueue(Packets.leftClick(1, 10)))
        queue.enqueue(Packets.leftClick(2, 20))
        assertTrue(queue.onFinished(success = false) is OutboundQueue.Transmit.DropLink)
        assertTrue(queue.onFinished(success = true) is OutboundQueue.Transmit.Idle)
    }

    @Test
    fun centerDropsThePendingPreCalibrationAim() {
        val queue = OutboundQueue()
        send(queue.enqueue(Packets.aim(1, 1_000, 11, 0)))
        queue.enqueue(Packets.aim(2, 2_000, 22, 0))
        assertTrue(queue.enqueue(Packets.center(3, 3_000)) is OutboundQueue.Transmit.Idle)
        val next = send(queue.onFinished(success = true))
        assertEquals(Packets.KIND_CENTER, kind(next))
        assertTrue(queue.onFinished(success = true) is OutboundQueue.Transmit.Idle)
        assertFalse(queue.hasPendingAim())
    }

    @Test
    fun pauseDropsPendingAimAndResumeAimsFollowIt() {
        val queue = OutboundQueue()
        send(queue.enqueue(Packets.aim(1, 1, 1, 0)))
        queue.enqueue(Packets.aim(2, 2, 9, 0))
        queue.enqueue(Packets.pause(3, 3, paused = true))
        val pause = send(queue.onFinished(success = true))
        assertEquals(Packets.KIND_PAUSE, kind(pause))
        assertTrue(queue.onFinished(success = true) is OutboundQueue.Transmit.Idle)
        val resumed = send(queue.enqueue(Packets.aim(0, 4, 3, 0)))
        assertEquals(Packets.KIND_AIM, kind(resumed))
        assertEquals(3, leftCdeg(resumed))
        assertEquals(sequenceOf(pause) + 1, sequenceOf(resumed))
    }

    @Test
    fun disconnectClearsQueuedAimAndEvents() {
        val queue = OutboundQueue()
        queue.enqueue(Packets.aim(1, 0, 0, 0))
        queue.enqueue(Packets.drag(2, 1, down = false))
        queue.clear()
        assertTrue(queue.onFinished(success = true) is OutboundQueue.Transmit.Idle)
        assertFalse(queue.hasPendingAim())
        assertEquals(0, queue.queuedCritical())
    }

    private fun send(step: OutboundQueue.Transmit): ByteArray {
        assertTrue(step is OutboundQueue.Transmit.Send)
        return (step as OutboundQueue.Transmit.Send).packet
    }

    private fun kind(packet: ByteArray): Int = packet[1].toInt() and 0xFF

    private fun sequenceOf(packet: ByteArray): Int =
        (packet[2].toInt() and 0xFF) or ((packet[3].toInt() and 0xFF) shl 8)

    private fun leftCdeg(packet: ByteArray): Int =
        (packet[8].toInt() and 0xFF) or ((packet[9].toInt() and 0xFF) shl 8)

    /** Same u16 gap rule as LinkStats.observe. */
    private fun lossOf(sequences: List<Int>): Int {
        var lost = 0
        var prev: Int? = null
        for (sequence in sequences) {
            val earlier = prev
            if (earlier != null) {
                val gap = (sequence - earlier) and 0xFFFF
                if (gap != 0) lost += gap - 1
            }
            prev = sequence
        }
        return lost
    }
}
