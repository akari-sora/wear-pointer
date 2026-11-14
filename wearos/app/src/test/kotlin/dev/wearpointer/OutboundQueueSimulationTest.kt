package dev.wearpointer

import dev.wearpointer.ble.OutboundQueue
import dev.wearpointer.ble.OutboundQueue.Transmit
import dev.wearpointer.proto.Packets
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.random.Random

/**
 * Random interleavings of sensor aims, clicks, drags, Center, stack busy
 * rejects, permanent rejects, callback failures, and timeouts. For every link
 * (until a DropLink) it checks what the receiver would see on air:
 *
 * 1. sequences are 0, 1, 2, ... with no gap and no repeat, so the receiver's
 *    `wrapping_sub - 1` loss count is 0 while nothing is lost on the radio;
 * 2. no aim built before a Center lands after that Center, and no aim built
 *    after it lands before it;
 * 3. discrete events land once each, in order, as a prefix of what was
 *    enqueued (all of it when the link survives).
 */
class OutboundQueueSimulationTest {
    @Test
    fun randomInterleavingsKeepSequenceOrderAndTheCenterBoundary() {
        var links = 0
        var drops = 0
        repeat(300) { seed ->
            val run = Run(Random(seed))
            run.go(steps = 3_000)
            links += run.links
            drops += run.drops
        }
        // The model must actually exercise both outcomes.
        assertTrue(links > drops)
        assertTrue(drops > 0)
    }

    private class Run(private val rnd: Random) {
        val queue = OutboundQueue()
        var links = 0
        var drops = 0

        private var epoch = 0
        private var nextId = 1
        private var dragDown = false
        private val centerEpoch = HashMap<Int, Int>()

        // Per link.
        private val onAir = ArrayList<ByteArray>()
        private val enqueuedDiscrete = ArrayList<Int>()
        private var linkStartEpoch = 0

        // Stack model.
        private var handOver: ByteArray? = null
        private var awaitingCallback = false

        fun go(steps: Int) {
            startLink()
            repeat(steps) { step() }
            // Let the stack finish everything cleanly.
            while (handOver != null || awaitingCallback) {
                handOver?.let { packet ->
                    handOver = null
                    accept(packet)
                }
                if (awaitingCallback) {
                    awaitingCallback = false
                    apply(queue.onFinished(success = true))
                }
            }
            endLink(survived = true)
        }

        private fun step() {
            val pending = handOver
            if (pending != null) {
                handOver = null
                val roll = rnd.nextDouble()
                when {
                    roll < 0.85 -> accept(pending)
                    roll < 0.97 -> apply(queue.onApiRejected(retryable = true))
                    else -> apply(queue.onApiRejected(retryable = false))
                }
                return
            }
            if (awaitingCallback && rnd.nextDouble() < 0.6) {
                awaitingCallback = false
                val roll = rnd.nextDouble()
                when {
                    roll < 0.97 -> apply(queue.onFinished(success = true))
                    roll < 0.995 -> apply(queue.onFinished(success = false))
                    else -> apply(queue.onTimeout())
                }
                return
            }
            val id = nextId++
            val roll = rnd.nextDouble()
            val packet = when {
                roll < 0.80 -> Packets.aim(0, 0, id % 30_000, epoch)
                roll < 0.90 -> discrete(id, Packets.leftClick(0, id.toLong()))
                roll < 0.95 -> {
                    dragDown = !dragDown
                    discrete(id, Packets.drag(0, id.toLong(), dragDown))
                }
                else -> {
                    epoch++
                    centerEpoch[id] = epoch
                    discrete(id, Packets.center(0, id.toLong()))
                }
            }
            apply(queue.enqueue(packet))
        }

        private fun discrete(id: Int, packet: ByteArray): ByteArray {
            enqueuedDiscrete += id
            return packet
        }

        private fun accept(packet: ByteArray) {
            queue.markAccepted()
            onAir += packet.copyOf()
            awaitingCallback = true
        }

        private fun apply(step: Transmit) {
            when (step) {
                is Transmit.Send -> handOver = step.packet
                is Transmit.Retry -> handOver = step.packet
                Transmit.DropLink -> {
                    handOver = null
                    awaitingCallback = false
                    endLink(survived = false)
                    queue.clear()
                    startLink()
                }
                Transmit.Idle -> Unit
            }
        }

        private fun startLink() {
            links++
            onAir.clear()
            enqueuedDiscrete.clear()
            linkStartEpoch = epoch
        }

        private fun endLink(survived: Boolean) {
            if (!survived) drops++
            onAir.forEachIndexed { index, packet ->
                assertEquals("sequence on air", index, sequenceOf(packet))
            }
            var receiverEpoch = linkStartEpoch
            val landedDiscrete = ArrayList<Int>()
            for (packet in onAir) {
                when (kind(packet)) {
                    Packets.KIND_AIM -> assertEquals("aim base vs last Center", receiverEpoch, upCdeg(packet))
                    Packets.KIND_CENTER -> {
                        val id = timestamp(packet)
                        receiverEpoch = centerEpoch.getValue(id)
                        landedDiscrete += id
                    }
                    else -> landedDiscrete += timestamp(packet)
                }
            }
            if (survived) {
                assertEquals(enqueuedDiscrete, landedDiscrete)
            } else {
                assertEquals(enqueuedDiscrete.take(landedDiscrete.size), landedDiscrete)
            }
        }
    }

    companion object {
        private fun kind(packet: ByteArray): Int = packet[1].toInt() and 0xFF

        private fun sequenceOf(packet: ByteArray): Int =
            (packet[2].toInt() and 0xFF) or ((packet[3].toInt() and 0xFF) shl 8)

        private fun timestamp(packet: ByteArray): Int =
            (packet[4].toInt() and 0xFF) or ((packet[5].toInt() and 0xFF) shl 8) or
                ((packet[6].toInt() and 0xFF) shl 16) or ((packet[7].toInt() and 0xFF) shl 24)

        private fun upCdeg(packet: ByteArray): Int =
            ((packet[10].toInt() and 0xFF) or ((packet[11].toInt() and 0xFF) shl 8)).toShort().toInt()
    }
}
