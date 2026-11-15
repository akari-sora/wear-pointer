package dev.wearpointer.ble

import dev.wearpointer.proto.Packets
import java.util.ArrayDeque
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.exp
import kotlin.math.hypot
import kotlin.math.roundToInt

/**
 * Standard 6-byte Bluetooth/USB HID Mouse Input Report payload for [HidMotionEngine.HID_REPORT_ID].
 *
 * Note: `BluetoothHidDevice.sendReport(device, HID_REPORT_ID, report.toBytes())` takes only the
 * 6 payload bytes WITHOUT the leading Report ID byte (Android's HID stack prefixes the ID).
 */
data class HidReport(
    val buttons: Int,
    val dx: Int,
    val dy: Int,
    val wheel: Int,
    val buttonEvent: Boolean = false,
) {
    fun toBytes(): ByteArray {
        val clampedDx = dx.coerceIn(-MAX_AXIS_DELTA, MAX_AXIS_DELTA)
        val clampedDy = dy.coerceIn(-MAX_AXIS_DELTA, MAX_AXIS_DELTA)
        val clampedWheel = wheel.coerceIn(-MAX_WHEEL_DELTA, MAX_WHEEL_DELTA)
        return byteArrayOf(
            (buttons and BUTTON_MASK).toByte(),
            (clampedDx and 0xFF).toByte(),
            ((clampedDx shr 8) and 0xFF).toByte(),
            (clampedDy and 0xFF).toByte(),
            ((clampedDy shr 8) and 0xFF).toByte(),
            clampedWheel.toByte(),
        )
    }

    companion object {
        const val REPORT_BYTES: Int = 6
        const val BUTTON_MASK: Int = 0x07
        const val MAX_AXIS_DELTA: Int = 32_767
        const val MAX_WHEEL_DELTA: Int = 127
    }
}

/**
 * Casiez et al. (CHI 2012) 1€ filter for jitter-free absolute laser projection.
 * Pure Kotlin with zero Android dependencies so JVM tests exercise it directly.
 */
class OneEuroFilter(
    private val steadyHz: Double = 0.8,
    private val quickness: Double = 0.01,
    private val dCutoffHz: Double = 1.0,
) {
    private var initialized = false
    private var xPrev = 0.0
    private var dxPrev = 0.0
    private var lastTsUs = 0L

    fun reset() {
        initialized = false
        xPrev = 0.0
        dxPrev = 0.0
        lastTsUs = 0L
    }

    fun filter(value: Double, timestampUs: Long): Double {
        if (!initialized) {
            initialized = true
            xPrev = value
            dxPrev = 0.0
            lastTsUs = timestampUs
            return value
        }
        val dtUs = timestampUs.toInt() - lastTsUs.toInt()
        if (dtUs <= 0) return xPrev
        if (dtUs >= HidMotionEngine.MAX_DT_US) {
            xPrev = value
            dxPrev = 0.0
            lastTsUs = timestampUs
            return value
        }
        val dt = (dtUs / 1_000_000.0).coerceAtLeast(1e-4)
        lastTsUs = timestampUs

        val edx = (value - xPrev) / dt
        val dxHat = lowPass(edx, dxPrev, alpha(dt, dCutoffHz))
        val cutoff = steadyHz + quickness * abs(dxHat)
        val xHat = lowPass(value, xPrev, alpha(dt, cutoff))
        xPrev = xHat
        dxPrev = dxHat
        return xHat
    }

    private fun alpha(dt: Double, cutoffHz: Double): Double {
        val tau = 1.0 / (2.0 * PI * cutoffHz)
        return 1.0 / (1.0 + tau / dt)
    }

    private fun lowPass(x: Double, prev: Double, a: Double): Double = prev + a * (x - prev)
}

/**
 * Pure, framework-free on-watch motion & HID report engine.
 *
 * Consumes the exact protocol v1 packets emitted by `PointerRuntime` and converts them into
 * standard 6-byte relative HID mouse reports (`Buttons`, `X` i16 LE, `Y` i16 LE, `Wheel` i8).
 */
class HidMotionEngine(
    initialMode: Int = Packets.MODE_ABSOLUTE,
    initialDpi: Int = Packets.DEFAULT_DPI,
) {
    var mode: Int = initialMode
        private set
    var dpi: Int = initialDpi.coerceIn(Packets.MIN_DPI, Packets.MAX_DPI)
        private set
    var paused: Boolean = false
        private set
    var heldButtons: Int = 0
        private set

    // AirMouse state
    private var airHasPrev = false
    private var airPrevLeftDeg = 0.0
    private var airPrevUpDeg = 0.0
    private var airPrevTsUs = 0L
    private var airSmoothSpeed = 0.0
    private var airResX = 0.0
    private var airResY = 0.0

    // Absolute (1€ + 1920x1080 virtual projection) state
    private val absFilterX = OneEuroFilter(steadyHz = ABS_STEADY_HZ, quickness = ABS_QUICKNESS)
    private val absFilterY = OneEuroFilter(steadyHz = ABS_STEADY_HZ, quickness = ABS_QUICKNESS)
    private var absAnchored = false
    var virtualX: Double = SCREEN_CENTER_X
        private set
    var virtualY: Double = SCREEN_CENTER_Y
        private set
    private var absResX = 0.0
    private var absResY = 0.0

    // TouchPad state
    private var touchHasPrev = false
    private var touchPrevXAcc = 0
    private var touchPrevYAcc = 0
    private var touchPrevTsUs = 0L
    private var touchSmoothSpeed = 0.0
    private var touchResX = 0.0
    private var touchResY = 0.0

    // Scroll residual accumulator
    private var scrollResidual = 0.0

    // Tap-jolt stabilization tracking
    private var lastAimTsUs: Long? = null
    private var stabilizeUntilUs: Long? = null

    /** Current static button state report (used for HID GET_REPORT requests). */
    fun currentReportBytes(): ByteArray = HidReport(heldButtons, 0, 0, 0).toBytes()

    /** Clears held buttons and resets all motion anchors on disconnect or stop. */
    fun resetLink() {
        heldButtons = 0
        paused = false
        scrollResidual = 0.0
        lastAimTsUs = null
        stabilizeUntilUs = null
        resetMotionAnchors()
    }

    /** Decodes a protocol v1 packet and emits zero, one, or two [HidReport]s. */
    fun process(packet: ByteArray): List<HidReport> {
        if (packet.size < HEADER_BYTES) return emptyList()
        if ((packet[0].toInt() and 0xFF) != Packets.VERSION) return emptyList()
        val kind = packet[1].toInt() and 0xFF
        val tsUs = readU32(packet, 4)

        return when (kind) {
            Packets.KIND_AIM -> {
                if (packet.size < 12) emptyList()
                else handleAim(tsUs, readI16(packet, 8), readI16(packet, 10))
            }
            Packets.KIND_TOUCH -> {
                if (packet.size < 13) emptyList()
                else handleTouch(
                    tsUs = tsUs,
                    xAccCentiMm = readI16(packet, 8),
                    yAccCentiMm = readI16(packet, 10),
                    down = packet[12].toInt() != 0,
                )
            }
            Packets.KIND_LEFT_CLICK -> handleLeftClick(tsUs)
            Packets.KIND_RIGHT_CLICK -> handleRightClick(tsUs)
            Packets.KIND_DRAG -> {
                if (packet.size < 9) emptyList()
                else handleDrag(packet[8].toInt() != 0)
            }
            Packets.KIND_SCROLL -> {
                if (packet.size < 10) emptyList()
                else handleScroll(readI16(packet, 8))
            }
            Packets.KIND_CENTER -> handleCenter()
            Packets.KIND_PAUSE -> {
                if (packet.size < 9) emptyList()
                else handlePause(packet[8].toInt() != 0)
            }
            Packets.KIND_SENSITIVITY -> {
                if (packet.size < 10) emptyList()
                else handleSensitivity(readU16(packet, 8))
            }
            Packets.KIND_MODE -> {
                if (packet.size < 9) emptyList()
                else handleMode(packet[8].toInt() and 0xFF)
            }
            else -> emptyList()
        }
    }

    private fun handleMode(nextMode: Int): List<HidReport> {
        if (nextMode != Packets.MODE_ABSOLUTE &&
            nextMode != Packets.MODE_AIR &&
            nextMode != Packets.MODE_TOUCH
        ) {
            return emptyList()
        }
        if (nextMode != mode) {
            mode = nextMode
            resetMotionAnchors()
        }
        return emptyList()
    }

    private fun handleSensitivity(rawDpi: Int): List<HidReport> {
        val nextDpi = rawDpi.coerceIn(Packets.MIN_DPI, Packets.MAX_DPI)
        if (nextDpi != dpi) {
            dpi = nextDpi
            // Re-anchor absolute reach on the next sample so bezel DPI tuning never warps the cursor.
            absAnchored = false
            absResX = 0.0
            absResY = 0.0
        }
        return emptyList()
    }

    private fun handlePause(nextPaused: Boolean): List<HidReport> {
        paused = nextPaused
        resetMotionAnchors()
        return emptyList()
    }

    private fun handleCenter(): List<HidReport> {
        absFilterX.reset()
        absFilterY.reset()
        virtualX = SCREEN_CENTER_X
        virtualY = SCREEN_CENTER_Y
        absResX = 0.0
        absResY = 0.0
        absAnchored = false
        return emptyList()
    }

    private fun handleLeftClick(clickTsUs: Long): List<HidReport> {
        armClickStabilizer(clickTsUs)
        val pressed = heldButtons or BUTTON_LEFT
        heldButtons = heldButtons and BUTTON_LEFT.inv()
        return listOf(
            HidReport(buttons = pressed, dx = 0, dy = 0, wheel = 0, buttonEvent = true),
            HidReport(buttons = heldButtons, dx = 0, dy = 0, wheel = 0, buttonEvent = true),
        )
    }

    private fun handleRightClick(clickTsUs: Long): List<HidReport> {
        armClickStabilizer(clickTsUs)
        val pressed = heldButtons or BUTTON_RIGHT
        return listOf(
            HidReport(buttons = pressed, dx = 0, dy = 0, wheel = 0, buttonEvent = true),
            HidReport(buttons = heldButtons, dx = 0, dy = 0, wheel = 0, buttonEvent = true),
        )
    }

    private fun handleDrag(down: Boolean): List<HidReport> {
        heldButtons = if (down) {
            heldButtons or BUTTON_LEFT
        } else {
            heldButtons and BUTTON_LEFT.inv()
        }
        return listOf(HidReport(buttons = heldButtons, dx = 0, dy = 0, wheel = 0, buttonEvent = true))
    }

    /**
     * Converts Wear rotary pixels (`~136 px` per bezel notch) into HID Wheel steps.
     *
     * Sign convention: Android Compose `RotaryScrollEvent.verticalScrollPixels` is positive (`+136`)
     * when turning the bezel clockwise / scrolling down a list, whereas standard USB/Bluetooth HID
     * Wheel (`0x38`) defines `+1` as scroll up (away from user) and `-1` as scroll down.
     */
    private fun handleScroll(deltaPx: Int): List<HidReport> {
        if (deltaPx == 0) return emptyList()
        scrollResidual += (-deltaPx / SCROLL_PX_PER_WHEEL_TICK)
        val ticks = scrollResidual.toInt().coerceIn(-HidReport.MAX_WHEEL_DELTA, HidReport.MAX_WHEEL_DELTA)
        if (ticks == 0) return emptyList()
        scrollResidual -= ticks
        return listOf(HidReport(buttons = heldButtons, dx = 0, dy = 0, wheel = ticks))
    }

    private fun handleAim(tsUs: Long, leftCdeg: Int, upCdeg: Int): List<HidReport> {
        if (paused || mode == Packets.MODE_TOUCH) return emptyList()
        lastAimTsUs = tsUs
        val leftDeg = leftCdeg / 100.0
        val upDeg = upCdeg / 100.0
        return when (mode) {
            Packets.MODE_AIR -> stepAir(tsUs, leftDeg, upDeg)
            Packets.MODE_ABSOLUTE -> stepAbsolute(tsUs, leftDeg, upDeg)
            else -> emptyList()
        }
    }

    private fun stepAir(tsUs: Long, leftDeg: Double, upDeg: Double): List<HidReport> {
        if (!airHasPrev) {
            airHasPrev = true
            airPrevLeftDeg = leftDeg
            airPrevUpDeg = upDeg
            airPrevTsUs = tsUs
            airSmoothSpeed = 0.0
            return emptyList()
        }

        val dRightDeg = wrapDegDelta(-(leftDeg - airPrevLeftDeg))
        val dDownDeg = -(upDeg - airPrevUpDeg)
        airPrevLeftDeg = leftDeg
        airPrevUpDeg = upDeg

        val dtUs = tsUs.toInt() - airPrevTsUs.toInt()
        airPrevTsUs = tsUs
        if (dtUs <= 0 || dtUs >= MAX_DT_US) {
            airSmoothSpeed = 0.0
            return emptyList()
        }
        val dt = (dtUs / 1_000_000.0).coerceAtLeast(1e-4)

        val stepDeg = hypot(dRightDeg, dDownDeg)
        if (stepDeg > AIR_MAX_STEP_DEG) {
            airSmoothSpeed = 0.0
            return emptyList()
        }

        val instSpeed = (stepDeg / dt).coerceIn(0.0, AIR_MAX_SPEED)
        val alpha = dt / (AIR_SPEED_TAU_S + dt)
        airSmoothSpeed += alpha * (instSpeed - airSmoothSpeed)

        val stabilizing = isStabilizing(tsUs)
        val deadStart = if (stabilizing) AIR_CLICK_DEAD_START else AIR_DEAD_START
        val deadFull = if (stabilizing) AIR_CLICK_DEAD_FULL else AIR_DEAD_FULL
        val dead = when {
            airSmoothSpeed <= deadStart -> 0.0
            airSmoothSpeed >= deadFull -> 1.0
            else -> {
                val t = (airSmoothSpeed - deadStart) / (deadFull - deadStart)
                t * t * (3.0 - 2.0 * t)
            }
        }
        if (dead == 0.0) return emptyList()

        val sigmoid = 1.0 / (1.0 + exp(-(airSmoothSpeed - AIR_SPEED_MID) / AIR_SPEED_WIDTH))
        val scale = dpi / 1600.0
        val gain = (AIR_GAIN_MIN + (AIR_GAIN_MAX - AIR_GAIN_MIN) * sigmoid) * dead * scale

        airResX += dRightDeg * gain
        airResY += dDownDeg * gain
        val dx = airResX.toInt()
        val dy = airResY.toInt()
        airResX -= dx
        airResY -= dy

        return if (dx != 0 || dy != 0) {
            listOf(HidReport(buttons = heldButtons, dx = dx, dy = dy, wheel = 0))
        } else {
            emptyList()
        }
    }

    private fun stepAbsolute(tsUs: Long, leftDeg: Double, upDeg: Double): List<HidReport> {
        val speed = dpi / 1000.0
        val reachX = 25.0 / speed
        val reachY = reachX * (SCREEN_HEIGHT / SCREEN_WIDTH)

        val rawX = (SCREEN_CENTER_X + (-leftDeg / reachX) * SCREEN_CENTER_X).coerceIn(0.0, SCREEN_WIDTH)
        val rawY = (SCREEN_CENTER_Y + (-upDeg / reachY) * SCREEN_CENTER_Y).coerceIn(0.0, SCREEN_HEIGHT)

        val targetX = absFilterX.filter(rawX, tsUs).coerceIn(0.0, SCREEN_WIDTH)
        val targetY = absFilterY.filter(rawY, tsUs).coerceIn(0.0, SCREEN_HEIGHT)

        if (!absAnchored) {
            virtualX = targetX
            virtualY = targetY
            absResX = 0.0
            absResY = 0.0
            absAnchored = true
            return emptyList()
        }

        val stepX = targetX - virtualX
        val stepY = targetY - virtualY
        if (isStabilizing(tsUs) && hypot(stepX, stepY) < ABS_CLICK_JOLT_PX) {
            return emptyList()
        }

        virtualX = targetX
        virtualY = targetY
        absResX += stepX
        absResY += stepY

        val dx = absResX.roundToInt()
        val dy = absResY.roundToInt()
        absResX -= dx
        absResY -= dy

        return if (dx != 0 || dy != 0) {
            listOf(HidReport(buttons = heldButtons, dx = dx, dy = dy, wheel = 0))
        } else {
            emptyList()
        }
    }

    private fun handleTouch(
        tsUs: Long,
        xAccCentiMm: Int,
        yAccCentiMm: Int,
        down: Boolean,
    ): List<HidReport> {
        if (paused || !down) {
            resetTouchAnchor()
            return emptyList()
        }
        if (!touchHasPrev) {
            touchHasPrev = true
            touchPrevXAcc = xAccCentiMm
            touchPrevYAcc = yAccCentiMm
            touchPrevTsUs = tsUs
            touchSmoothSpeed = 0.0
            return emptyList()
        }

        val dxMm = (xAccCentiMm - touchPrevXAcc).toShort().toInt() / 100.0
        val dyMm = (yAccCentiMm - touchPrevYAcc).toShort().toInt() / 100.0
        touchPrevXAcc = xAccCentiMm
        touchPrevYAcc = yAccCentiMm

        val dtUs = tsUs.toInt() - touchPrevTsUs.toInt()
        touchPrevTsUs = tsUs
        if (dtUs <= 0 || dtUs >= MAX_DT_US) {
            touchSmoothSpeed = 0.0
            return emptyList()
        }
        val dt = (dtUs / 1_000_000.0).coerceAtLeast(1e-4)

        val stepMm = hypot(dxMm, dyMm)
        if (stepMm > TOUCH_MAX_STEP_MM) {
            touchSmoothSpeed = 0.0
            return emptyList()
        }

        val instSpeed = stepMm / dt
        val alpha = dt / (TOUCH_SPEED_TAU_S + dt)
        touchSmoothSpeed += alpha * (instSpeed - touchSmoothSpeed)
        if (stepMm == 0.0) return emptyList()

        val t = ((touchSmoothSpeed - TOUCH_SPEED_SLOW) / (TOUCH_SPEED_FAST - TOUCH_SPEED_SLOW))
            .coerceIn(0.0, 1.0)
        val curve = t * t * (3.0 - 2.0 * t)
        val scale = dpi / 1600.0
        val gain = (TOUCH_GAIN_SLOW + (TOUCH_GAIN_FAST - TOUCH_GAIN_SLOW) * curve) * scale

        touchResX += dxMm * gain
        touchResY += dyMm * gain
        val dx = touchResX.toInt()
        val dy = touchResY.toInt()
        touchResX -= dx
        touchResY -= dy

        return if (dx != 0 || dy != 0) {
            listOf(HidReport(buttons = heldButtons, dx = dx, dy = dy, wheel = 0))
        } else {
            emptyList()
        }
    }

    private fun armClickStabilizer(clickTsUs: Long) {
        // Drop sub-pixel residuals so a borderline 0.9px remainder cannot tip on tap recoil.
        airResX = 0.0
        airResY = 0.0
        absResX = 0.0
        absResY = 0.0
        val baseUs = lastAimTsUs ?: clickTsUs
        stabilizeUntilUs = (baseUs + CLICK_STABILIZE_US) and 0xFFFF_FFFFL
    }

    private fun isStabilizing(tsUs: Long): Boolean {
        val until = stabilizeUntilUs ?: return false
        val remaining = until.toInt() - tsUs.toInt()
        if (remaining <= 0 || remaining > CLICK_STABILIZE_US.toInt()) {
            stabilizeUntilUs = null
            return false
        }
        return true
    }

    private fun resetMotionAnchors() {
        airHasPrev = false
        airSmoothSpeed = 0.0
        airResX = 0.0
        airResY = 0.0

        absFilterX.reset()
        absFilterY.reset()
        absAnchored = false
        absResX = 0.0
        absResY = 0.0

        resetTouchAnchor()
    }

    private fun resetTouchAnchor() {
        touchHasPrev = false
        touchSmoothSpeed = 0.0
        touchResX = 0.0
        touchResY = 0.0
    }

    private fun wrapDegDelta(delta: Double): Double {
        var d = delta % 360.0
        if (d > 180.0) d -= 360.0
        if (d < -180.0) d += 360.0
        return d
    }

    private fun readU16(buf: ByteArray, offset: Int): Int =
        (buf[offset].toInt() and 0xFF) or ((buf[offset + 1].toInt() and 0xFF) shl 8)

    private fun readI16(buf: ByteArray, offset: Int): Int =
        readU16(buf, offset).toShort().toInt()

    private fun readU32(buf: ByteArray, offset: Int): Long =
        ((buf[offset].toLong() and 0xFFL) or
            ((buf[offset + 1].toLong() and 0xFFL) shl 8) or
            ((buf[offset + 2].toLong() and 0xFFL) shl 16) or
            ((buf[offset + 3].toLong() and 0xFFL) shl 24)) and 0xFFFF_FFFFL

    companion object {
        const val HID_REPORT_ID: Int = 1
        const val BUTTON_LEFT: Int = 0x01
        const val BUTTON_RIGHT: Int = 0x02
        const val BUTTON_MIDDLE: Int = 0x04

        private const val HEADER_BYTES = 8
        const val MAX_DT_US: Int = 500_000

        // AirMouse transfer constants (from receiver/crates/core/src/relative.rs)
        const val AIR_GAIN_MIN: Double = 10.0
        const val AIR_GAIN_MAX: Double = 70.0
        const val AIR_SPEED_MID: Double = 45.0
        const val AIR_SPEED_WIDTH: Double = 12.0
        const val AIR_DEAD_START: Double = 0.8
        const val AIR_DEAD_FULL: Double = 1.8
        const val AIR_MAX_STEP_DEG: Double = 30.0
        const val AIR_MAX_SPEED: Double = 1000.0
        const val AIR_SPEED_TAU_S: Double = 0.02

        // Brief post-tap recoil dampening (suppresses ~0.15° / 15 deg/s glass tap wobble for 50 ms)
        private const val CLICK_STABILIZE_US = 50_000L
        private const val AIR_CLICK_DEAD_START = 6.0
        private const val AIR_CLICK_DEAD_FULL = 12.0
        private const val ABS_CLICK_JOLT_PX = 6.0

        // Absolute virtual display constants
        const val SCREEN_WIDTH: Double = 1920.0
        const val SCREEN_HEIGHT: Double = 1080.0
        const val SCREEN_CENTER_X: Double = 960.0
        const val SCREEN_CENTER_Y: Double = 540.0
        const val ABS_STEADY_HZ: Double = 0.8
        const val ABS_QUICKNESS: Double = 0.01

        // TouchPad transfer constants (from receiver/crates/core/src/relative.rs)
        const val TOUCH_GAIN_SLOW: Double = 6.0
        const val TOUCH_GAIN_FAST: Double = 45.0
        const val TOUCH_SPEED_SLOW: Double = 15.0
        const val TOUCH_SPEED_FAST: Double = 150.0
        const val TOUCH_MAX_STEP_MM: Double = 60.0
        const val TOUCH_SPEED_TAU_S: Double = 0.03

        // Bezel rotary ~136 px/notch -> 90 px/tick guarantees >= 1 HID wheel step per notch.
        const val SCROLL_PX_PER_WHEEL_TICK: Double = 90.0

        /**
         * Standard 6-byte HID Mouse Input Report Descriptor (Report ID 1):
         * - Byte 0: Buttons 1..3 (3 bits) + 5-bit constant padding
         * - Bytes 1..2: X signed 16-bit relative (-32767..32767, little-endian)
         * - Bytes 3..4: Y signed 16-bit relative (-32767..32767, little-endian)
         * - Byte 5: Wheel signed 8-bit relative (-127..127)
         */
        val MOUSE_REPORT_DESCRIPTOR: ByteArray = byteArrayOf(
            0x05, 0x01, // Usage Page (Generic Desktop)
            0x09, 0x02, // Usage (Mouse)
            0xA1.toByte(), 0x01, // Collection (Application)
            0x09, 0x01, //   Usage (Pointer)
            0xA1.toByte(), 0x00, //   Collection (Physical)
            0x85.toByte(), HID_REPORT_ID.toByte(), // Report ID (1)
            0x05, 0x09, //     Usage Page (Button)
            0x19, 0x01, //     Usage Minimum (Button 1)
            0x29, 0x03, //     Usage Maximum (Button 3)
            0x15, 0x00, //     Logical Minimum (0)
            0x25, 0x01, //     Logical Maximum (1)
            0x95.toByte(), 0x03, //     Report Count (3)
            0x75, 0x01, //     Report Size (1)
            0x81.toByte(), 0x02, //     Input (Data, Variable, Absolute)
            0x95.toByte(), 0x01, //     Report Count (1)
            0x75, 0x05, //     Report Size (5)
            0x81.toByte(), 0x01, //     Input (Constant)
            0x05, 0x01, //     Usage Page (Generic Desktop)
            0x09, 0x30, //     Usage (X)
            0x09, 0x31, //     Usage (Y)
            0x16, 0x01, 0x80.toByte(), // Logical Minimum (-32767)
            0x26, 0xFF.toByte(), 0x7F, // Logical Maximum (32767)
            0x75, 0x10, //     Report Size (16)
            0x95.toByte(), 0x02, //     Report Count (2)
            0x81.toByte(), 0x06, //     Input (Data, Variable, Relative)
            0x09, 0x38, //     Usage (Wheel)
            0x15, 0x81.toByte(), //     Logical Minimum (-127)
            0x25, 0x7F, //     Logical Maximum (127)
            0x75, 0x08, //     Report Size (8)
            0x95.toByte(), 0x01, //     Report Count (1)
            0x81.toByte(), 0x06, //     Input (Data, Variable, Relative)
            0xC0.toByte(), //   End Collection (Physical)
            0xC0.toByte(), // End Collection (Application)
        )
    }
}

/**
 * Rate-limits and coalesces outgoing HID motion reports while NEVER coalescing button transitions.
 *
 * - Motion/scroll reports arriving within `< [minMotionIntervalMs]` (default 7 ms) or while the
 *   Bluetooth HID interrupt queue is busy (`sendReport == false`) accumulate losslessly into
 *   `(pendingDx, pendingDy, pendingWheel)`.
 * - Button transitions (`Left`, `Right`, `Drag`) flush any accumulated pre-click motion into a
 *   FIFO queue ahead of the button transition and are never merged or reordered.
 */
class HidReportScheduler(
    private val minMotionIntervalMs: Long = DEFAULT_MIN_MOTION_INTERVAL_MS,
) {
    private val criticalQueue = ArrayDeque<HidReport>()
    private var criticalAttempts = 0

    private var hasPendingMotion = false
    private var pendingButtons = 0
    private var pendingDx = 0
    private var pendingDy = 0
    private var pendingWheel = 0
    private var motionBusyRetries = 0
    private var lastMotionSentMs = UNSET_TIME
    private var currentWireButtons = 0

    private var accepted = 0
    private var completed = 0
    private var rejected = 0
    private var coalesced = 0

    /**
     * Submits a [HidReport] for transmission.
     * @return delay in milliseconds after which [flushDue] should be invoked, or `null` if idle.
     */
    fun submit(
        report: HidReport,
        nowMs: Long,
        sender: (HidReport) -> Boolean,
    ): Long? {
        if (report.buttonEvent) {
            enqueuePendingMotionForCritical(currentWireButtons)
            currentWireButtons = report.buttons
            criticalQueue.addLast(report)
            return drain(nowMs, sender)
        }

        if (report.dx == 0 && report.dy == 0 && report.wheel == 0) {
            return scheduleDelay(nowMs)
        }

        if (hasPendingMotion) {
            coalesced++
        }
        hasPendingMotion = true
        pendingButtons = report.buttons
        pendingDx += report.dx
        pendingDy += report.dy
        pendingWheel += report.wheel
        return drain(nowMs, sender)
    }

    /** Flushes any due coalesced motion or retried button transition at [nowMs]. */
    fun flushDue(
        nowMs: Long,
        sender: (HidReport) -> Boolean,
    ): Long? = drain(nowMs, sender)

    fun clear() {
        criticalQueue.clear()
        criticalAttempts = 0
        clearPendingMotion()
        lastMotionSentMs = UNSET_TIME
        currentWireButtons = 0
    }

    fun takeCounters(): SendCounters {
        val out = SendCounters(
            accepted = accepted,
            completed = completed,
            rejected = rejected,
            coalesced = coalesced,
        )
        accepted = 0
        completed = 0
        rejected = 0
        coalesced = 0
        return out
    }

    private fun drain(
        nowMs: Long,
        sender: (HidReport) -> Boolean,
    ): Long? {
        while (criticalQueue.isNotEmpty()) {
            val head = criticalQueue.peekFirst() ?: break
            if (sender(head)) {
                criticalQueue.removeFirst()
                criticalAttempts = 0
                accepted++
                completed++
                if (!head.buttonEvent) {
                    lastMotionSentMs = nowMs
                }
            } else {
                criticalAttempts++
                if (criticalAttempts >= MAX_CRITICAL_ATTEMPTS) {
                    criticalQueue.removeFirst()
                    criticalAttempts = 0
                    rejected++
                    continue
                }
                return BUSY_RETRY_MS
            }
        }

        if (!hasPendingMotion) return null

        val elapsed = if (lastMotionSentMs == UNSET_TIME) Long.MAX_VALUE else nowMs - lastMotionSentMs
        if (elapsed < minMotionIntervalMs) {
            return (minMotionIntervalMs - elapsed).coerceAtLeast(1L)
        }

        val chunk = nextMotionChunk(pendingButtons)
        if (sender(chunk)) {
            accepted++
            completed++
            lastMotionSentMs = nowMs
            motionBusyRetries = 0
            pendingDx -= chunk.dx
            pendingDy -= chunk.dy
            pendingWheel -= chunk.wheel
            if (pendingDx == 0 && pendingDy == 0 && pendingWheel == 0) {
                hasPendingMotion = false
                return null
            }
            return minMotionIntervalMs
        } else {
            motionBusyRetries++
            if (motionBusyRetries >= MAX_MOTION_BUSY_RETRIES) {
                rejected++
                clearPendingMotion()
                return null
            }
            return BUSY_RETRY_MS
        }
    }

    private fun enqueuePendingMotionForCritical(buttons: Int) {
        if (!hasPendingMotion) return
        while (pendingDx != 0 || pendingDy != 0 || pendingWheel != 0) {
            val chunk = nextMotionChunk(buttons)
            pendingDx -= chunk.dx
            pendingDy -= chunk.dy
            pendingWheel -= chunk.wheel
            criticalQueue.addLast(chunk)
        }
        hasPendingMotion = false
        motionBusyRetries = 0
    }

    private fun nextMotionChunk(buttons: Int): HidReport = HidReport(
        buttons = buttons,
        dx = pendingDx.coerceIn(-HidReport.MAX_AXIS_DELTA, HidReport.MAX_AXIS_DELTA),
        dy = pendingDy.coerceIn(-HidReport.MAX_AXIS_DELTA, HidReport.MAX_AXIS_DELTA),
        wheel = pendingWheel.coerceIn(-HidReport.MAX_WHEEL_DELTA, HidReport.MAX_WHEEL_DELTA),
        buttonEvent = false,
    )

    private fun clearPendingMotion() {
        hasPendingMotion = false
        pendingDx = 0
        pendingDy = 0
        pendingWheel = 0
        motionBusyRetries = 0
    }

    private fun scheduleDelay(nowMs: Long): Long? {
        if (criticalQueue.isNotEmpty()) return BUSY_RETRY_MS
        if (!hasPendingMotion) return null
        val elapsed = if (lastMotionSentMs == UNSET_TIME) 0L else nowMs - lastMotionSentMs
        return (minMotionIntervalMs - elapsed).coerceAtLeast(1L)
    }

    companion object {
        const val DEFAULT_MIN_MOTION_INTERVAL_MS: Long = 7L
        const val BUSY_RETRY_MS: Long = 3L
        private const val MAX_CRITICAL_ATTEMPTS = 10
        private const val MAX_MOTION_BUSY_RETRIES = 25
        private const val UNSET_TIME = Long.MIN_VALUE
    }
}
