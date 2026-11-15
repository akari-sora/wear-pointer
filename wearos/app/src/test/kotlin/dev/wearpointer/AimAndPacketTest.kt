package dev.wearpointer

import dev.wearpointer.motion.AimTracker
import dev.wearpointer.proto.Packets
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class AimAndPacketTest {
    @Test
    fun yawAboutVerticalMatchesTheRustVector() {
        val tracker = AimTracker()
        val gravity = floatArrayOf(0f, 0f, 9.81f)
        tracker.push(0L, gravity, floatArrayOf(0f, 0f, 0f))
        tracker.push(10_000_000L, gravity, floatArrayOf(0f, 0f, 0.2f))
        assertEquals(11, tracker.leftCdeg())
        assertEquals(0, tracker.upCdeg())
    }

    @Test
    fun twelveOClockUpIsNinetyDegrees() {
        val tracker = AimTracker()
        tracker.push(0L, floatArrayOf(0f, 9.81f, 0f), floatArrayOf(0f, 0f, 0f))
        assertEquals(9000, tracker.upCdeg())
    }

    @Test
    fun deadZoneDropsSlowYaw() {
        val tracker = AimTracker(deadZoneRadPerSec = 1.0)
        val gravity = floatArrayOf(0f, 0f, 9.81f)
        tracker.push(0L, gravity, floatArrayOf(0f, 0f, 0f))
        tracker.push(10_000_000L, gravity, floatArrayOf(0f, 0f, 0.2f))
        assertEquals(0, tracker.leftCdeg())
    }

    @Test
    fun centerZeroesTheAim() {
        val tracker = AimTracker()
        tracker.push(0L, floatArrayOf(0f, 9.81f, 0f), floatArrayOf(0f, 0f, 0f))
        tracker.center()
        assertEquals(0, tracker.leftCdeg())
        assertEquals(0, tracker.upCdeg())
    }

    @Test
    fun aimPacketMatchesTheSpecVector() {
        val bytes = Packets.aim(sequence = 1, timestampUs = 1000, leftCdeg = 1234, upCdeg = -50)
        assertEquals("01000100e8030000d204ceff", bytes.toHex())
        assertTrue(bytes.size <= Packets.MAX_PACKET_BYTES)
    }

    @Test
    fun statsPacketIsSixteenBytes() {
        val bytes = Packets.stats(1, 0, 100, 97, 3, 1000)
        assertEquals(16, bytes.size)
        assertEquals(Packets.KIND_STATS.toByte(), bytes[1])
    }

    @Test
    fun sensitivityPacketEncodesLittleEndianDpiAndClampsBounds() {
        assertEquals(1600, Packets.DEFAULT_DPI)
        val def = Packets.sensitivity(2, 500, Packets.DEFAULT_DPI)
        assertEquals(10, def.size)
        assertEquals(Packets.KIND_SENSITIVITY.toByte(), def[1])
        val dpi = (def[8].toInt() and 0xFF) or ((def[9].toInt() and 0xFF) shl 8)
        assertEquals(1600, dpi)

        val low = Packets.sensitivity(3, 600, 50)
        val lowDpi = (low[8].toInt() and 0xFF) or ((low[9].toInt() and 0xFF) shl 8)
        assertEquals(Packets.MIN_DPI, lowDpi)

        val high = Packets.sensitivity(4, 700, 9999)
        val highDpi = (high[8].toInt() and 0xFF) or ((high[9].toInt() and 0xFF) shl 8)
        assertEquals(Packets.MAX_DPI, highDpi)
    }

    private fun ByteArray.toHex(): String = joinToString("") { "%02x".format(it) }
}
