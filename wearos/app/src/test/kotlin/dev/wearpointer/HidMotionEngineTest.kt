package dev.wearpointer

import dev.wearpointer.ble.HidMotionEngine
import dev.wearpointer.ble.HidReport
import dev.wearpointer.ble.HidReportScheduler
import dev.wearpointer.proto.Packets
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.math.abs

class HidMotionEngineTest {
    @Test
    fun mouseReportDescriptorValidatesStructureAndSixBytePayload() {
        assertEquals(1, HidMotionEngine.HID_REPORT_ID)
        val desc = HidMotionEngine.MOUSE_REPORT_DESCRIPTOR
        assertTrue(desc.isNotEmpty())

        // Parse every short item per USB HID 1.11 §6.2.2.2
        var index = 0
        var collectionDepth = 0
        var openCollections = 0
        var reportSize = 0
        var reportCount = 0
        var totalInputBits = 0
        var sawReportIdOne = false
        var sawMouseUsage = false

        while (index < desc.size) {
            val prefix = desc[index].toInt() and 0xFF
            assertTrue("Long HID items (0xFE) must not appear", prefix != 0xFE)
            val sizeCode = prefix and 0x03
            val payloadLen = if (sizeCode == 3) 4 else sizeCode
            val type = (prefix shr 2) and 0x03
            val tag = (prefix shr 4) and 0x0F
            assertTrue("Truncated HID item at $index", index + 1 + payloadLen <= desc.size)

            var uVal = 0
            for (b in 0 until payloadLen) {
                uVal = uVal or ((desc[index + 1 + b].toInt() and 0xFF) shl (8 * b))
            }

            when (type) {
                0 -> when (tag) { // Main items
                    0x0A -> { // Collection
                        collectionDepth++
                        openCollections++
                    }
                    0x0C -> { // End Collection
                        collectionDepth--
                        assertTrue("Unbalanced End Collection", collectionDepth >= 0)
                    }
                    0x08 -> { // Input
                        totalInputBits += reportSize * reportCount
                    }
                }
                1 -> when (tag) { // Global items
                    0x07 -> reportSize = uVal
                    0x09 -> reportCount = uVal
                    0x08 -> if (uVal == 1) sawReportIdOne = true
                }
                2 -> when (tag) { // Local items
                    0x00 -> if (uVal == 0x02 && collectionDepth == 0) sawMouseUsage = true
                }
            }
            index += 1 + payloadLen
        }

        assertEquals("Collections must be balanced", 0, collectionDepth)
        assertEquals("Application + Physical collections expected", 2, openCollections)
        assertTrue("Report ID 1 must be declared", sawReportIdOne)
        assertTrue("Usage(Mouse) must be declared", sawMouseUsage)
        assertEquals("Input report payload must be 48 bits (6 bytes)", 48, totalInputBits)
    }

    @Test
    fun hidReportEncodesSixBytesLittleEndianAndClampsDescriptorBounds() {
        val report = HidReport(buttons = 0x03, dx = -300, dy = 512, wheel = -5)
        val bytes = report.toBytes()
        assertEquals(6, bytes.size)
        // -300 = 0xFED4 -> D4 FE; +512 = 0x0200 -> 00 02; -5 = 0xFB
        assertArrayEquals(
            byteArrayOf(0x03, 0xD4.toByte(), 0xFE.toByte(), 0x00, 0x02, 0xFB.toByte()),
            bytes,
        )

        // Clamp out-of-range values to [-32767, 32767] and [-127, 127], and button mask to 3 bits
        val clampedNeg = HidReport(buttons = 0xFF, dx = -40_000, dy = 40_000, wheel = -200).toBytes()
        assertArrayEquals(
            byteArrayOf(
                0x07,
                0x01,
                0x80.toByte(), // -32767 LE
                0xFF.toByte(),
                0x7F, // +32767 LE
                0x81.toByte(), // -127
            ),
            clampedNeg,
        )
    }

    @Test
    fun airModeSuppressesStillnessTremorAndAcceleratesFastFlicks() {
        val engine = HidMotionEngine(initialMode = Packets.MODE_AIR, initialDpi = 1600)

        // 0.4 deg/s tremor (< DEAD_START = 0.8 deg/s): 0.004 deg/10ms -> alternates 0..1 cdeg slowly
        var tUs = 10_000L
        var tremorReports = 0
        for (i in 0 until 40) {
            // 1 cdeg every 50ms = 0.2 deg/s
            val cdeg = i / 5
            tremorReports += engine.process(Packets.aim(i, tUs, cdeg, 0)).size
            tUs += 10_000L
        }
        assertEquals("Hand stillness tremor below deadzone must emit 0 HID reports", 0, tremorReports)

        // Slow deliberate turn right (leftCdeg decreasing by 6 cdeg per 10ms = 6 deg/s, total 1.2 deg)
        val slowEngine = HidMotionEngine(initialMode = Packets.MODE_AIR, initialDpi = 1600)
        var slowDx = 0
        for (i in 0..20) {
            val reports = slowEngine.process(Packets.aim(i, (i + 1) * 10_000L, -i * 6, 0))
            slowDx += reports.sumOf { it.dx }
        }
        val slowPxPerDeg = slowDx / 1.2
        assertTrue("Slow motion should move right smoothly (got $slowDx px)", slowDx in 10..22)

        // Fast wrist flick right (leftCdeg decreasing by 100 cdeg per 10ms = 100 deg/s, total 20 deg)
        val fastEngine = HidMotionEngine(initialMode = Packets.MODE_AIR, initialDpi = 1600)
        var fastDx = 0
        for (i in 0..20) {
            val reports = fastEngine.process(Packets.aim(i, (i + 1) * 10_000L, -i * 100, 0))
            fastDx += reports.sumOf { it.dx }
        }
        val fastPxPerDeg = fastDx / 20.0
        assertTrue(
            "Fast flick ($fastPxPerDeg px/deg) must accelerate well above slow ($slowPxPerDeg px/deg)",
            fastPxPerDeg > slowPxPerDeg * 3.5,
        )

        // DPI scaling: 3200 DPI should double 1600 DPI travel
        val highDpiEngine = HidMotionEngine(initialMode = Packets.MODE_AIR, initialDpi = 1600)
        highDpiEngine.process(Packets.sensitivity(0, 1_000L, 3200))
        var highDpiDx = 0
        for (i in 0..20) {
            val reports = highDpiEngine.process(Packets.aim(i, (i + 1) * 10_000L, -i * 100, 0))
            highDpiDx += reports.sumOf { it.dx }
        }
        assertTrue("3200 DPI ($highDpiDx) should be ~2x 1600 DPI ($fastDx)", abs(highDpiDx - fastDx * 2) <= 3)
    }

    @Test
    fun airModeHandlesHeadingWrapAroundDropsRebaseJumpsAndClutchesOnPause() {
        val engine = HidMotionEngine(initialMode = Packets.MODE_AIR, initialDpi = 1600)

        // Anchor at -179.50° (-17950 cdeg), then turn right 1.0° across ±180° boundary to +179.50° (+17950 cdeg)
        assertTrue(engine.process(Packets.aim(0, 10_000L, -17_950, 0)).isEmpty())
        val wrapped = engine.process(Packets.aim(1, 20_000L, 17_950, 0))
        val wrappedDx = wrapped.sumOf { it.dx }
        assertTrue("Crossing ±180° rightwards should emit positive dx, got $wrappedDx", wrappedDx > 0)

        // One-off rebase jump > 30° (179.5° -> 0° in 10 ms) is dropped cleanly without warping
        val jump = engine.process(Packets.aim(2, 30_000L, 0, 0))
        assertTrue("Jump > MAX_STEP_DEG must be dropped", jump.isEmpty())

        // Clutch via KIND_PAUSE: move wrist 25° while paused, then resume -> zero jump
        engine.process(Packets.pause(3, 40_000L, paused = true))
        assertTrue(engine.process(Packets.aim(4, 50_000L, 1_500, -800)).isEmpty())
        engine.process(Packets.pause(5, 60_000L, paused = false))
        assertTrue(
            "First sample after unpause must re-anchor with zero jump",
            engine.process(Packets.aim(6, 70_000L, 2_500, -1_200)).isEmpty(),
        )
        val afterResume = engine.process(Packets.aim(7, 80_000L, 2_400, -1_200))
        assertTrue("Subsequent motion after unpause moves normally", afterResume.sumOf { it.dx } > 0)
    }

    @Test
    fun touchModeSwipesAcceleratesResetsOnFingerLiftAndWraps16BitTravel() {
        val engine = HidMotionEngine(initialMode = Packets.MODE_ABSOLUTE, initialDpi = 1600)
        engine.process(Packets.mode(0, 1_000L, Packets.MODE_TOUCH))
        assertEquals(Packets.MODE_TOUCH, engine.mode)

        // Finger up: no motion even when accumulator values vary
        assertTrue(engine.process(Packets.touch(1, 10_000L, 500, 500, down = false)).isEmpty())
        assertTrue(engine.process(Packets.touch(2, 20_000L, 1_000, 1_000, down = false)).isEmpty())

        // Slow finger drag: 0.1 mm (10 centi-mm) per 10ms = 10 mm/s (< SPEED_SLOW=15) -> GAIN_SLOW=6 px/mm
        var slowDx = 0
        for (i in 0..20) {
            val reports = engine.process(Packets.touch(i, (i + 3) * 10_000L, 1_000 + i * 10, 1_000, down = true))
            slowDx += reports.sumOf { it.dx }
        }
        // 2.0 mm * 6.0 px/mm = 12 px
        assertEquals(12, slowDx)

        // Lift finger, reposition elsewhere, touch down again -> zero jump on touch-down
        assertTrue(engine.process(Packets.touch(30, 300_000L, 3_000, 2_000, down = false)).isEmpty())
        assertTrue(engine.process(Packets.touch(31, 310_000L, 32_700, -5_000, down = true)).isEmpty())

        // Cross 16-bit signed wrap (+32700 + 200 = +32900 -> wraps to -32636 on wire; 2.0 mm in 10ms = 200 mm/s fast!)
        val wrapped = engine.process(Packets.touch(32, 320_000L, 32_700 + 200, -5_000 + 100, down = true))
        val wrapDx = wrapped.sumOf { it.dx }
        val wrapDy = wrapped.sumOf { it.dy }
        assertTrue("16-bit wrapped +2.0mm fast swipe must move right > 12px (got $wrapDx)", wrapDx > 12)
        assertTrue("Downward +1.0mm swipe must move down > 0 (got $wrapDy)", wrapDy > 0)
    }

    @Test
    fun absoluteModeProjectsOneEuroClampsEdgesAndCentersWithZeroJump() {
        val engine = HidMotionEngine(initialMode = Packets.MODE_ABSOLUTE, initialDpi = 1600)

        // First sample at non-zero angle anchors virtualPos with zero jump delta
        val initial = engine.process(Packets.aim(0, 10_000L, 800, -400))
        assertTrue("First sample after start must anchor with zero delta", initial.isEmpty())

        // Reset via Center to (960, 540), anchor at (0, 0), then sweep right past full reach (-2000 cdeg = 20° > 15.625°)
        assertTrue(engine.process(Packets.center(1, 20_000L)).isEmpty())
        assertEquals(960.0, engine.virtualX, 1e-6)
        assertEquals(540.0, engine.virtualY, 1e-6)
        assertTrue(engine.process(Packets.aim(2, 30_000L, 0, 0)).isEmpty())

        var totalDx = 0
        var totalDy = 0
        var ts = 40_000L
        for (step in 1..80) {
            val leftCdeg = (-step * 30).coerceAtLeast(-2_200) // sweeps right and clamps at 1920
            val upCdeg = (step * 15).coerceAtMost(1_200) // sweeps up and clamps at 0
            val reports = engine.process(Packets.aim(step + 2, ts, leftCdeg, upCdeg))
            totalDx += reports.sumOf { it.dx }
            totalDy += reports.sumOf { it.dy }
            ts += 10_000L
        }

        // From center (960, 540) to top-right corner (1920, 0) = (+960, -540)
        assertTrue("Total dx ($totalDx) must converge to +960 screen edge without overshooting", totalDx in 958..960)
        assertTrue("Total dy ($totalDy) must converge to -540 top edge without overshooting", totalDy in -540..-538)

        // Press Center while virtualPos is at top-right corner (1920, 0): MUST emit ZERO jump delta!
        val centerReports = engine.process(Packets.center(100, ts))
        assertTrue("KIND_CENTER must emit zero jump delta", centerReports.isEmpty())
        assertEquals(960.0, engine.virtualX, 1e-6)
        assertEquals(540.0, engine.virtualY, 1e-6)

        // Next sample from recentered watch (0, 0) also emits zero delta, then moves relative to new center
        ts += 10_000L
        assertTrue(engine.process(Packets.aim(101, ts, 0, 0)).isEmpty())
        ts += 10_000L
        val moveLeft = engine.process(Packets.aim(102, ts, 400, 0))
        assertTrue("Turning wrist left from new center moves cursor left (dx < 0)", moveLeft.sumOf { it.dx } < 0)
    }

    @Test
    fun buttonStateMachinePreservesDragAcrossMovesRightClickAndResetLink() {
        val engine = HidMotionEngine(initialMode = Packets.MODE_AIR, initialDpi = 1600)

        // Left click: press (0x01) then release (0x00)
        val left = engine.process(Packets.leftClick(1, 10_000L))
        assertEquals(
            listOf(
                HidReport(buttons = 0x01, dx = 0, dy = 0, wheel = 0, buttonEvent = true),
                HidReport(buttons = 0x00, dx = 0, dy = 0, wheel = 0, buttonEvent = true),
            ),
            left,
        )

        // Tap-jolt stabilization: 0.15° (15 deg/s instantaneous) glass-tap recoil 20 ms after click is suppressed
        engine.process(Packets.aim(2, 20_000L, 0, 0))
        val recoil = engine.process(Packets.aim(3, 30_000L, -15, 0))
        assertTrue("Post-click tap recoil must be stabilized", recoil.isEmpty())

        // Drag down holds 0x01 across motion, scroll, and concurrent right-click
        val dragDown = engine.process(Packets.drag(4, 100_000L, down = true))
        assertEquals(listOf(HidReport(0x01, 0, 0, 0, buttonEvent = true)), dragDown)
        assertEquals(0x01, engine.heldButtons)

        engine.process(Packets.aim(5, 110_000L, 0, 0))
        val dragMove = engine.process(Packets.aim(6, 120_000L, -80, 0))
        assertTrue(dragMove.isNotEmpty())
        assertTrue("Motion during drag must keep left button held", dragMove.all { it.buttons == 0x01 })

        val rightDuringDrag = engine.process(Packets.rightClick(7, 130_000L))
        assertEquals(
            listOf(
                HidReport(buttons = 0x03, dx = 0, dy = 0, wheel = 0, buttonEvent = true),
                HidReport(buttons = 0x01, dx = 0, dy = 0, wheel = 0, buttonEvent = true),
            ),
            rightDuringDrag,
        )

        // Disconnect / stop resetLink clears heldButtons immediately
        engine.resetLink()
        assertEquals(0, engine.heldButtons)
        assertEquals(0, engine.currentReportBytes()[0].toInt())
    }

    @Test
    fun scrollAccumulatesBezelNotchesAndMatchesStandardHidWheelSign() {
        val engine = HidMotionEngine()

        // Clockwise bezel notch (+136 px = scroll down) -> negative HID wheel step (-1), remainder kept
        val down1 = engine.process(Packets.scroll(1, 10_000L, 136))
        assertEquals(1, down1.size)
        assertEquals(-1, down1[0].wheel)

        // Second clockwise notch (+136 px): -0.511 + (-1.511) = -2.022 -> emits -2!
        val down2 = engine.process(Packets.scroll(2, 20_000L, 136))
        assertEquals(1, down2.size)
        assertEquals(-2, down2[0].wheel)

        // Reset and test fine counter-clockwise crown steps (-35 px * 3 = -105 px = scroll up -> +1)
        engine.resetLink()
        assertTrue(engine.process(Packets.scroll(3, 30_000L, -35)).isEmpty())
        assertTrue(engine.process(Packets.scroll(4, 40_000L, -35)).isEmpty())
        val up = engine.process(Packets.scroll(5, 50_000L, -35))
        assertEquals(1, up.size)
        assertEquals(1, up[0].wheel)
    }

    @Test
    fun reportSchedulerCoalescesSub7msAndBusyMotionWithoutCoalescingButtons() {
        val scheduler = HidReportScheduler(minMotionIntervalMs = 7L)
        val wire = ArrayList<HidReport>()

        // t=0ms: dx=5 goes out immediately; t=2ms (dx=6) & t=5ms (dx=4) coalesce until t=7ms
        assertNull(scheduler.submit(HidReport(0, 5, 0, 0), nowMs = 0L) { wire.add(it) })
        assertEquals(1, wire.size)
        assertEquals(5, wire[0].dx)

        val d1 = scheduler.submit(HidReport(0, 6, -2, 0), nowMs = 2L) { wire.add(it) }
        assertEquals(5L, d1)
        val d2 = scheduler.submit(HidReport(0, 4, -3, 1), nowMs = 5L) { wire.add(it) }
        assertEquals(2L, d2)
        assertEquals(1, wire.size)

        assertNull(scheduler.flushDue(nowMs = 7L) { wire.add(it) })
        assertEquals(2, wire.size)
        assertEquals(HidReport(0, 10, -5, 1), wire[1])
        val c1 = scheduler.takeCounters()
        assertEquals(2, c1.accepted)
        assertEquals(2, c1.completed)
        assertEquals(1, c1.coalesced)
        assertEquals(0, c1.rejected)

        // Sub-7ms motion at t=9ms (dx=12) followed by Left Click at t=10ms:
        // pre-click motion MUST flush ahead of press & release, and press+release MUST NOT coalesce!
        wire.clear()
        assertNotNull(scheduler.submit(HidReport(0, 12, 3, 0), nowMs = 9L) { wire.add(it) })
        assertEquals(0, wire.size)
        assertNull(scheduler.submit(HidReport(0x01, 0, 0, 0, buttonEvent = true), nowMs = 10L) { wire.add(it) })
        assertNull(scheduler.submit(HidReport(0x00, 0, 0, 0, buttonEvent = true), nowMs = 10L) { wire.add(it) })
        assertEquals(
            listOf(
                HidReport(0, 12, 3, 0, buttonEvent = false),
                HidReport(0x01, 0, 0, 0, buttonEvent = true),
                HidReport(0x00, 0, 0, 0, buttonEvent = true),
            ),
            wire,
        )

        // Rate-limited (sendReport == false) retains motion & retries button transitions without loss
        wire.clear()
        var allowSend = false
        val retryDelay = scheduler.submit(HidReport(0, 7, 0, 0), nowMs = 30L) {
            if (allowSend) wire.add(it) else false
        }
        assertEquals(HidReportScheduler.BUSY_RETRY_MS, retryDelay)
        scheduler.submit(HidReport(0x01, 0, 0, 0, buttonEvent = true), nowMs = 31L) {
            if (allowSend) wire.add(it) else false
        }
        assertEquals(0, wire.size)

        allowSend = true
        assertNull(scheduler.flushDue(nowMs = 34L) { wire.add(it) })
        assertEquals(
            listOf(
                HidReport(0, 7, 0, 0, buttonEvent = false),
                HidReport(0x01, 0, 0, 0, buttonEvent = true),
            ),
            wire,
        )
    }
}
