package dev.wearpointer.proto

import java.util.UUID

/**
 * Protocol v1 encoder. Layout is [protocol/SPEC.md] at the repo root.
 * Constants must match `protocol/v1.json`.
 */
object Packets {
    const val VERSION: Int = 1
    const val KIND_AIM: Int = 0
    const val KIND_LEFT_CLICK: Int = 1
    const val KIND_RIGHT_CLICK: Int = 2
    const val KIND_SCROLL: Int = 3
    const val KIND_CENTER: Int = 4
    const val KIND_PAUSE: Int = 5
    const val KIND_INFO: Int = 6
    const val KIND_DRAG: Int = 7
    const val KIND_STATS: Int = 8
    const val KIND_SENSITIVITY: Int = 9
    const val CONTROL_REQUEST_STATS: Int = 0x01
    const val HOLD_TO_DRAG_MS: Long = 350
    const val BASE_DPI: Int = 1000
    const val DEFAULT_DPI: Int = 1600
    const val MIN_DPI: Int = 400
    const val MAX_DPI: Int = 3200
    const val DPI_STEP: Int = 200
    const val MAX_PACKET_BYTES: Int = 20

    val SERVICE_UUID: UUID = UUID.fromString("7a1e0001-4c3b-4e2a-9f10-6d5c4b3a2910")
    val POINTER_UUID: UUID = UUID.fromString("7a1e0002-4c3b-4e2a-9f10-6d5c4b3a2910")
    val CONTROL_UUID: UUID = UUID.fromString("7a1e0003-4c3b-4e2a-9f10-6d5c4b3a2910")
    val CCCD_UUID: UUID = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb")

    /** Overwrites the sequence field. Called when a packet is first handed to the stack. */
    fun stampSequence(packet: ByteArray, sequence: Int) {
        val value = sequence and 0xFFFF
        packet[2] = (value and 0xFF).toByte()
        packet[3] = (value shr 8 and 0xFF).toByte()
    }

    fun aim(sequence: Int, timestampUs: Long, leftCdeg: Int, upCdeg: Int): ByteArray {
        val out = ByteArray(12)
        header(out, KIND_AIM, sequence, timestampUs)
        putI16(out, 8, leftCdeg)
        putI16(out, 10, upCdeg)
        return out
    }

    fun leftClick(sequence: Int, timestampUs: Long): ByteArray = bare(KIND_LEFT_CLICK, sequence, timestampUs)

    fun rightClick(sequence: Int, timestampUs: Long): ByteArray = bare(KIND_RIGHT_CLICK, sequence, timestampUs)

    fun center(sequence: Int, timestampUs: Long): ByteArray = bare(KIND_CENTER, sequence, timestampUs)

    fun scroll(sequence: Int, timestampUs: Long, deltaPx: Int): ByteArray {
        val out = ByteArray(10)
        header(out, KIND_SCROLL, sequence, timestampUs)
        putI16(out, 8, deltaPx)
        return out
    }

    fun pause(sequence: Int, timestampUs: Long, paused: Boolean): ByteArray {
        val out = ByteArray(9)
        header(out, KIND_PAUSE, sequence, timestampUs)
        out[8] = if (paused) 1 else 0
        return out
    }

    fun drag(sequence: Int, timestampUs: Long, down: Boolean): ByteArray {
        val out = ByteArray(9)
        header(out, KIND_DRAG, sequence, timestampUs)
        out[8] = if (down) 1 else 0
        return out
    }

    fun info(
        sequence: Int,
        timestampUs: Long,
        batteryPct: Int,
        charging: Boolean,
        sensorHzX10: Int,
        name: String,
    ): ByteArray {
        val nameBytes = truncateUtf8(name, 8)
        val out = ByteArray(12 + nameBytes.size)
        header(out, KIND_INFO, sequence, timestampUs)
        out[8] = batteryPct.coerceIn(0, 255).toByte()
        out[9] = if (charging) 1 else 0
        putU16(out, 10, sensorHzX10)
        nameBytes.copyInto(out, 12)
        return out
    }

    fun stats(
        sequence: Int,
        timestampUs: Long,
        samples: Int,
        notifiesOk: Int,
        notifiesDrop: Int,
        windowMs: Int,
    ): ByteArray {
        val out = ByteArray(16)
        header(out, KIND_STATS, sequence, timestampUs)
        putU16(out, 8, samples)
        putU16(out, 10, notifiesOk)
        putU16(out, 12, notifiesDrop)
        putU16(out, 14, windowMs)
        return out
    }

    fun sensitivity(
        sequence: Int,
        timestampUs: Long,
        dpi: Int,
    ): ByteArray {
        val out = ByteArray(10)
        header(out, KIND_SENSITIVITY, sequence, timestampUs)
        putU16(out, 8, dpi.coerceIn(MIN_DPI, MAX_DPI))
        return out
    }

    private fun bare(kind: Int, sequence: Int, timestampUs: Long): ByteArray {
        val out = ByteArray(8)
        header(out, kind, sequence, timestampUs)
        return out
    }

    private fun header(out: ByteArray, kind: Int, sequence: Int, timestampUs: Long) {
        out[0] = VERSION.toByte()
        out[1] = kind.toByte()
        putU16(out, 2, sequence)
        putU32(out, 4, timestampUs)
    }

    private fun putU16(out: ByteArray, offset: Int, value: Int) {
        val v = value and 0xFFFF
        out[offset] = (v and 0xFF).toByte()
        out[offset + 1] = (v shr 8 and 0xFF).toByte()
    }

    private fun putI16(out: ByteArray, offset: Int, value: Int) {
        val v = value.coerceIn(Short.MIN_VALUE.toInt(), Short.MAX_VALUE.toInt())
        putU16(out, offset, v)
    }

    private fun putU32(out: ByteArray, offset: Int, value: Long) {
        val v = value and 0xFFFF_FFFFL
        out[offset] = (v and 0xFF).toByte()
        out[offset + 1] = (v shr 8 and 0xFF).toByte()
        out[offset + 2] = (v shr 16 and 0xFF).toByte()
        out[offset + 3] = (v shr 24 and 0xFF).toByte()
    }

    private fun truncateUtf8(text: String, max: Int): ByteArray {
        val bytes = text.encodeToByteArray()
        if (bytes.size <= max) return bytes
        var end = max
        while (end > 0 && (bytes[end].toInt() and 0xC0) == 0x80) {
            end--
        }
        return bytes.copyOf(end)
    }
}

fun nanosToUs(timestampNs: Long): Long = (timestampNs / 1000L) and 0xFFFF_FFFFL
