package dev.wearpointer

import dev.wearpointer.ble.OutboundQueue
import dev.wearpointer.proto.Packets
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class OutboundQueueTest {
    @Test
    fun aimCoalescesWhileANotificationIsInFlight() {
        val queue = OutboundQueue()
        val first = queue.enqueue(Packets.aim(1, 0, 0, 0))
        assertNotNull(first)
        assertNull(queue.enqueue(Packets.aim(2, 0, 10, 0)))
        assertNull(queue.enqueue(Packets.aim(3, 0, 20, 0)))
        val next = queue.onFinished(success = true)
        assertEquals(3, next!![2].toInt() and 0xFF) // sequence low byte
        assertEquals(1, queue.takeCounters().coalesced)
    }

    @Test
    fun dragUpIsNotReplacedByAimTraffic() {
        val queue = OutboundQueue()
        assertNotNull(queue.enqueue(Packets.aim(1, 0, 0, 0)))
        repeat(8) { queue.enqueue(Packets.aim(2 + it, 0, it, 0)) }
        val dragUp = Packets.drag(20, 50_000, down = false)
        assertNull(queue.enqueue(dragUp))
        val sent = mutableListOf<ByteArray>()
        var next = queue.onFinished(success = true)
        while (next != null) {
            sent += next
            next = queue.onFinished(success = true)
        }
        assertTrue(sent.any { it.contentEquals(dragUp) })
    }

    @Test
    fun callbackErrorStillSendsTheQueuedClick() {
        val queue = OutboundQueue()
        queue.enqueue(Packets.leftClick(1, 10))
        queue.enqueue(Packets.leftClick(2, 20))
        val next = queue.onFinished(success = false)
        assertEquals(2, next!![2].toInt() and 0xFF)
        assertEquals(1, queue.takeCounters().rejected)
    }

    @Test
    fun apiRejectionDoesNotDropTheNextClick() {
        val queue = OutboundQueue()
        val click = queue.enqueue(Packets.leftClick(1, 10))
        assertNotNull(click)
        queue.enqueue(Packets.rightClick(2, 11))
        val next = queue.onApiRejected()
        assertEquals(Packets.KIND_RIGHT_CLICK, next!![1].toInt() and 0xFF)
    }

    @Test
    fun disconnectClearsQueuedAimAndEvents() {
        val queue = OutboundQueue()
        queue.enqueue(Packets.aim(1, 0, 0, 0))
        queue.enqueue(Packets.drag(2, 1, down = false))
        queue.clear()
        assertNull(queue.onFinished(success = true))
        assertFalse(queue.hasPendingAim())
        assertEquals(0, queue.queuedCritical())
    }

    @Test
    fun clickGoesOutBeforeACoalescedAim() {
        val queue = OutboundQueue()
        queue.enqueue(Packets.aim(1, 0, 0, 0))
        queue.enqueue(Packets.aim(2, 0, 5, 0))
        queue.enqueue(Packets.leftClick(3, 1_000))
        val next = queue.onFinished(success = true)
        assertEquals(Packets.KIND_LEFT_CLICK, next!![1].toInt() and 0xFF)
    }
}
