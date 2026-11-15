package dev.wearpointer.ui

import android.os.SystemClock
import android.view.HapticFeedbackConstants
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.PointerInputChange
import androidx.compose.ui.input.pointer.PointerInputScope
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.rotary.onRotaryScrollEvent
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.wear.compose.material.MaterialTheme
import androidx.wear.compose.material.Text
import dev.wearpointer.PointerRuntime
import dev.wearpointer.proto.Packets
import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.roundToInt
import kotlin.math.sin
import kotlinx.coroutines.withTimeoutOrNull

private val PitchBlack = Color(0xFF000000)
private val SurfaceIdle = Color(0xFF131519)
private val SurfaceElevated = Color(0xFF1B1E24)
private val SurfaceDrag = Color(0xFF07251C)
private val SurfacePaused = Color(0xFF231807)
private val BorderSubtle = Color(0xFF272A32)
private val BorderStrong = Color(0xFF3E434F)
private val TextPrimary = Color(0xFFF4F4F5)
private val TextSecondary = Color(0xFFA1A1AA)
private val TextMuted = Color(0xFF71717A)
private val AccentEmerald = Color(0xFF10B981)
private val AccentAmber = Color(0xFFF59E0B)
private val AccentRose = Color(0xFFF43F5E)

@Composable
fun RemoteScreen(
    permissionsBlocked: Boolean,
    onRequestPermissions: () -> Unit,
    onRole: (PointerRuntime.Role) -> Unit,
    onPairPc: () -> Unit = {},
    onDpi: (Int) -> Unit,
    onStepDpi: (Int) -> Unit,
    onStop: () -> Unit,
    onStart: () -> Unit,
) {
    val ui by PointerRuntime.state.collectAsState()
    val focus = remember { FocusRequester() }
    val view = LocalView.current
    var showTelemetry by remember { mutableStateOf(false) }
    var showPrivacy by remember { mutableStateOf(false) }
    var rotaryDpiAccum by remember { mutableStateOf(0f) }

    val isStopped = ui.status == "Stopped"
    val hasGapNotice = ui.status.startsWith("Sensor gap")
    val hasLinkDrop = ui.status.startsWith("Link dropped")
    val sightColor = when {
        isStopped -> TextMuted
        ui.paused || hasGapNotice -> AccentAmber
        hasLinkDrop -> AccentRose
        ui.connected -> AccentEmerald
        else -> TextSecondary
    }

    MaterialTheme {
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(PitchBlack)
                // Compose UI's onRotaryScrollEvent receives Wear touch-bezel / crown scroll.
                // On the Settings/Telemetry overlay it steps DPI; otherwise it scrolls the desktop.
                .onRotaryScrollEvent { event ->
                    if (showTelemetry && !showPrivacy) {
                        rotaryDpiAccum += event.verticalScrollPixels
                        val detentPx = 42f
                        while (rotaryDpiAccum >= detentPx) {
                            rotaryDpiAccum -= detentPx
                            view.performHapticFeedback(HapticFeedbackConstants.CLOCK_TICK)
                            onStepDpi(1)
                        }
                        while (rotaryDpiAccum <= -detentPx) {
                            rotaryDpiAccum += detentPx
                            view.performHapticFeedback(HapticFeedbackConstants.CLOCK_TICK)
                            onStepDpi(-1)
                        }
                    } else {
                        PointerRuntime.scrollBy(event.verticalScrollPixels)
                    }
                    true
                }
                .focusRequester(focus)
                .focusable(),
        ) {
            PerimeterInstrumentDial(sightColor = sightColor)

            when {
                !ui.bluetoothGranted -> {
                    CenteredNoticeScreen(
                        badgeColor = AccentAmber,
                        title = "Bluetooth Needed",
                        subtitle = if (permissionsBlocked) {
                            "Bluetooth was denied. Turn on Nearby devices in app permissions."
                        } else {
                            "Allow advertising and GATT link to pair with desktop receiver."
                        },
                        buttonLabel = if (permissionsBlocked) "Open Settings" else "Allow Bluetooth",
                        onAction = onRequestPermissions,
                    )
                }
                ui.peripheralUnsupported -> {
                    CenteredNoticeScreen(
                        badgeColor = AccentRose,
                        title = "Peripheral Blocked",
                        subtitle = "This watch cannot advertise BLE. Switch to built-in BT HID mouse.",
                        buttonLabel = "Use BT HID",
                        onAction = { onRole(PointerRuntime.Role.STANDARD_HID) },
                    )
                }
                else -> {
                    Column(
                        modifier = Modifier
                            .fillMaxSize()
                            .padding(top = 15.dp, bottom = 12.dp),
                        horizontalAlignment = Alignment.CenterHorizontally,
                    ) {
                        StatusCapsule(
                            ui = ui,
                            sightColor = sightColor,
                            onClick = { showTelemetry = true },
                        )
                        Spacer(Modifier.height(5.dp))
                        ClickPad(
                            ui = ui,
                            modifier = Modifier
                                .weight(1f)
                                .fillMaxWidth()
                                .padding(horizontal = 22.dp),
                        )
                        Spacer(Modifier.height(5.dp))
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(horizontal = 24.dp)
                                .height(44.dp),
                            horizontalArrangement = Arrangement.spacedBy(6.dp),
                        ) {
                            DockButton(
                                label = "Right",
                                description = "Right click",
                                shape = RoundedCornerShape(
                                    topStart = 22.dp,
                                    bottomStart = 22.dp,
                                    topEnd = 12.dp,
                                    bottomEnd = 12.dp,
                                ),
                                surface = SurfaceIdle,
                                border = BorderSubtle,
                                contentColor = if (isStopped) TextMuted else TextPrimary,
                                modifier = Modifier.weight(1f),
                                icon = { drawRightClickGlyph(it) },
                                onClick = {
                                    view.performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP)
                                    PointerRuntime.rightClick()
                                },
                            )
                            DockButton(
                                label = "Center",
                                description = "Center cursor",
                                shape = RoundedCornerShape(12.dp),
                                surface = SurfaceElevated,
                                border = if (hasGapNotice) AccentAmber else BorderStrong,
                                contentColor = if (hasGapNotice) AccentAmber else TextPrimary,
                                modifier = Modifier.weight(1f),
                                icon = { drawCrosshairGlyph(it) },
                                onClick = {
                                    view.performHapticFeedback(HapticFeedbackConstants.CONFIRM)
                                    PointerRuntime.center()
                                },
                            )
                            DockButton(
                                label = if (ui.paused) "Resume" else "Pause",
                                description = if (ui.paused) "Resume pointer" else "Pause pointer",
                                shape = RoundedCornerShape(
                                    topStart = 12.dp,
                                    bottomStart = 12.dp,
                                    topEnd = 22.dp,
                                    bottomEnd = 22.dp,
                                ),
                                surface = if (ui.paused) SurfacePaused else SurfaceIdle,
                                border = if (ui.paused) AccentAmber else BorderSubtle,
                                contentColor = if (ui.paused) AccentAmber else TextPrimary,
                                modifier = Modifier.weight(1f),
                                icon = { color ->
                                    if (ui.paused) drawPlayGlyph(color) else drawPauseGlyph(color)
                                },
                                onClick = {
                                    view.performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP)
                                    PointerRuntime.togglePause()
                                },
                            )
                        }
                        Spacer(Modifier.height(5.dp))
                        PowerCapsule(
                            stopped = isStopped,
                            onStop = onStop,
                            onStart = onStart,
                        )
                    }
                }
            }

            if (showTelemetry) {
                TelemetryOverlay(
                    ui = ui,
                    sightColor = sightColor,
                    showPrivacy = showPrivacy,
                    onShowPrivacy = { showPrivacy = it },
                    onRole = onRole,
                    onPairPc = onPairPc,
                    onDpi = onDpi,
                    onStepDpi = onStepDpi,
                    onDismiss = {
                        showPrivacy = false
                        showTelemetry = false
                    },
                )
            }
        }
    }

    LaunchedEffect(ui.connected) {
        focus.requestFocus()
    }
}

@Composable
private fun PerimeterInstrumentDial(sightColor: Color) {
    Canvas(modifier = Modifier.fillMaxSize()) {
        val cx = size.width / 2f
        val cy = size.height / 2f
        val outerR = size.minDimension / 2f - 2.dp.toPx()

        for (deg in 0 until 360 step 15) {
            if (deg == 0) continue
            val rad = (deg - 90) * PI / 180.0
            val cardinal = deg % 90 == 0
            val len = if (cardinal) 5.dp.toPx() else 2.5.dp.toPx()
            val stroke = if (cardinal) 1.4.dp.toPx() else 0.9.dp.toPx()
            val color = if (cardinal) BorderStrong else BorderSubtle
            val start = Offset(
                x = cx + ((outerR - len) * cos(rad)).toFloat(),
                y = cy + ((outerR - len) * sin(rad)).toFloat(),
            )
            val end = Offset(
                x = cx + (outerR * cos(rad)).toFloat(),
                y = cy + (outerR * sin(rad)).toFloat(),
            )
            drawLine(color = color, start = start, end = end, strokeWidth = stroke, cap = StrokeCap.Round)
        }

        // 12 o'clock sight index: point this edge at the desktop display.
        val tipY = 2.dp.toPx()
        val baseY = 8.5.dp.toPx()
        val halfW = 4.5.dp.toPx()
        val chevron = Path().apply {
            moveTo(cx, tipY)
            lineTo(cx - halfW, baseY)
            lineTo(cx + halfW, baseY)
            close()
        }
        drawPath(path = chevron, color = sightColor)
    }
}

@Composable
private fun StatusCapsule(
    ui: PointerRuntime.UiState,
    sightColor: Color,
    onClick: () -> Unit,
) {
    val label = when {
        ui.status == "Stopped" -> "STOPPED"
        ui.status.startsWith("Sensor gap") -> "RECENTERED"
        ui.paused -> "PAUSED · AWAKE"
        ui.connected -> when (ui.mode) {
            PointerRuntime.Mode.ABSOLUTE -> {
                val hz = if (ui.samplesPerSec > 0) "${ui.samplesPerSec}HZ" else "LIVE"
                "$hz · ${ui.dpi} DPI"
            }
            PointerRuntime.Mode.AIR -> "AIR · ${ui.dpi} DPI"
            PointerRuntime.Mode.TOUCH -> "TOUCH · ${ui.dpi} DPI"
        }
        ui.status.startsWith("Link dropped") -> "RECONNECTING"
        ui.status.contains("Discoverable", ignoreCase = true) -> "PAIRABLE · 2M"
        ui.role == PointerRuntime.Role.STANDARD_HID && ui.status != "Bluetooth is off" -> {
            if (ui.status.contains("Connecting", ignoreCase = true)) {
                "HID · CONNECTING"
            } else {
                "BT HID · ${ui.dpi} DPI"
            }
        }
        ui.status.contains("Advertising", ignoreCase = true) -> "ADV · ${ui.dpi} DPI"
        ui.status.contains("Scanning", ignoreCase = true) -> "SCANNING"
        else -> ui.status.uppercase()
    }
    Row(
        modifier = Modifier
            .height(22.dp)
            .clip(CircleShape)
            .background(SurfaceElevated)
            .border(1.dp, BorderSubtle, CircleShape)
            .clickable(onClick = onClick)
            .semantics { contentDescription = "Status and telemetry" }
            .padding(horizontal = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(5.dp),
    ) {
        Box(
            modifier = Modifier
                .size(6.dp)
                .clip(CircleShape)
                .background(sightColor),
        )
        Text(
            text = label,
            color = TextPrimary,
            fontSize = 9.5.sp,
            fontWeight = FontWeight.SemiBold,
            fontFamily = FontFamily.Monospace,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Composable
private fun ClickPad(ui: PointerRuntime.UiState, modifier: Modifier) {
    var pressing by remember { mutableStateOf(false) }
    var dragging by remember { mutableStateOf(false) }
    var sliding by remember { mutableStateOf(false) }
    val holdProgress = remember { Animatable(0f) }
    val view = LocalView.current
    val touchMode = ui.mode == PointerRuntime.Mode.TOUCH

    LaunchedEffect(pressing, dragging, sliding) {
        if (sliding) {
            holdProgress.snapTo(0f)
        } else if (pressing && !dragging) {
            holdProgress.snapTo(0f)
            holdProgress.animateTo(
                targetValue = 1f,
                animationSpec = tween(
                    durationMillis = Packets.HOLD_TO_DRAG_MS.toInt(),
                    easing = LinearEasing,
                ),
            )
        } else if (!pressing && !dragging) {
            holdProgress.snapTo(0f)
        }
    }

    val isStopped = ui.status == "Stopped"
    val padShape = RoundedCornerShape(26.dp)
    val surfaceColor = when {
        dragging -> SurfaceDrag
        ui.paused -> SurfacePaused
        pressing -> SurfaceElevated
        else -> SurfaceIdle
    }
    val borderColor = when {
        dragging -> AccentEmerald
        ui.paused -> AccentAmber.copy(alpha = 0.65f)
        pressing -> BorderStrong
        else -> BorderSubtle
    }
    val borderWidth = if (dragging) 1.5.dp else 1.dp
    val primaryLabel = when {
        isStopped -> "OFFLINE"
        dragging -> "DRAG LOCKED"
        ui.paused -> "PAUSED"
        !ui.connected -> "STANDBY"
        touchMode -> "SLIDE"
        else -> "TAP"
    }
    val isIdleHint = !isStopped && !dragging && !ui.paused && !ui.connected
    val subLabel = when {
        isStopped -> "TAP START BELOW"
        dragging -> if (touchMode) "SLIDE TO DRAG" else "RELEASE TO DROP"
        ui.paused -> if (ui.mode == PointerRuntime.Mode.ABSOLUTE) "TRACKS WHILE AWAKE" else "CLUTCH · CURSOR HELD"
        !ui.connected -> when (ui.role) {
            PointerRuntime.Role.WATCH_ADVERTISES -> "Open PC app · or tap top for BT HID"
            PointerRuntime.Role.STANDARD_HID -> "Pair in PC Bluetooth · tap top to Pair"
            PointerRuntime.Role.DESKTOP_ADVERTISES -> "Scanning · or tap top for BT HID"
        }
        touchMode -> "TAP CLICK · HOLD DRAG"
        ui.mode == PointerRuntime.Mode.AIR -> "AIR MOUSE · HOLD DRAG"
        else -> "HOLD DRAG · BEZEL SCROLL"
    }
    val reticleColor = when {
        isStopped -> TextMuted
        dragging -> AccentEmerald
        ui.paused -> AccentAmber
        ui.connected -> AccentEmerald
        else -> TextSecondary
    }

    Box(
        modifier = modifier
            .clip(padShape)
            .background(surfaceColor)
            .border(borderWidth, borderColor, padShape)
            .semantics {
                contentDescription = if (touchMode) {
                    "Touchpad: slide to move the cursor, tap to click, hold to drag"
                } else {
                    "Click pad: tap for left click, hold to drag"
                }
            }
            .pointerInput(touchMode) {
                if (touchMode) {
                    touchpadGestures(
                        onPressing = { pressing = it },
                        onSliding = { sliding = it },
                        onDragging = { dragging = it },
                        haptic = { view.performHapticFeedback(it) },
                    )
                    return@pointerInput
                }
                awaitEachGesture {
                    awaitFirstDown()
                    pressing = true
                    val downNs = SystemClock.elapsedRealtimeNanos()
                    var up: PointerInputChange? = null
                    val timedOut = withTimeoutOrNull(Packets.HOLD_TO_DRAG_MS) {
                        up = waitForUpOrCancellation()
                        true
                    } == null
                    var holding = false
                    try {
                        if (up != null) {
                            view.performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP)
                            PointerRuntime.leftClick(downNs)
                        } else if (timedOut) {
                            holding = true
                            dragging = true
                            view.performHapticFeedback(HapticFeedbackConstants.LONG_PRESS)
                            PointerRuntime.drag(down = true, eventNs = downNs)
                            waitForUpOrCancellation()
                        }
                    } finally {
                        if (holding) {
                            dragging = false
                            PointerRuntime.drag(
                                down = false,
                                eventNs = SystemClock.elapsedRealtimeNanos() - 50_000_000L,
                            )
                        }
                        pressing = false
                    }
                }
            },
    ) {
        Canvas(modifier = Modifier.fillMaxSize()) {
            val cx = size.width / 2f
            val cy = size.height * 0.37f
            val reachR = (size.minDimension * 0.22f).coerceAtMost(20.5.dp.toPx())

            // Reach ring (±25° desktop edge) and crosshair ticks.
            drawCircle(
                color = BorderSubtle,
                radius = reachR,
                center = Offset(cx, cy),
                style = Stroke(width = 1.dp.toPx()),
            )
            val tickGap = 4.5.dp.toPx()
            val tickExt = 3.dp.toPx()
            drawLine(BorderSubtle, Offset(cx - reachR - tickExt, cy), Offset(cx - tickGap, cy), 1.dp.toPx())
            drawLine(BorderSubtle, Offset(cx + tickGap, cy), Offset(cx + reachR + tickExt, cy), 1.dp.toPx())
            drawLine(BorderSubtle, Offset(cx, cy - reachR - tickExt), Offset(cx, cy - tickGap), 1.dp.toPx())
            drawLine(BorderSubtle, Offset(cx, cy + tickGap), Offset(cx, cy + reachR + tickExt), 1.dp.toPx())

            // Hold-to-drag radial progress / active drag lock ring.
            val sweep = when {
                dragging -> 360f
                pressing -> holdProgress.value * 360f
                else -> 0f
            }
            if (sweep > 1f) {
                val arcR = reachR + 3.2.dp.toPx()
                drawArc(
                    color = AccentEmerald,
                    startAngle = -90f,
                    sweepAngle = sweep,
                    useCenter = false,
                    topLeft = Offset(cx - arcR, cy - arcR),
                    size = Size(arcR * 2f, arcR * 2f),
                    style = Stroke(width = 2.dp.toPx(), cap = StrokeCap.Round),
                )
            }

            if (touchMode) return@Canvas
            // Live spatial aim puck: mirrors desktop cursor direction at active DPI reach.
            val reachDeg = (25f * Packets.BASE_DPI / ui.dpi.coerceAtLeast(Packets.MIN_DPI))
                .coerceIn(6f, 65f)
            val leftDeg = ui.aimLeftCdeg / 100f
            val upDeg = ui.aimUpCdeg / 100f
            val normX = (-leftDeg / reachDeg).coerceIn(-1f, 1f)
            val normY = (-upDeg / reachDeg).coerceIn(-1f, 1f)
            val mag = kotlin.math.hypot(normX, normY).coerceAtLeast(1f)
            val dotCenter = Offset(
                x = cx + (normX / mag) * reachR,
                y = cy + (normY / mag) * reachR,
            )
            drawCircle(
                color = reticleColor.copy(alpha = 0.24f),
                radius = 6.dp.toPx(),
                center = dotCenter,
            )
            drawCircle(
                color = reticleColor,
                radius = 3.2.dp.toPx(),
                center = dotCenter,
            )
        }

        Column(
            modifier = Modifier
                .align(Alignment.BottomCenter)
                .padding(start = 8.dp, end = 8.dp, bottom = 6.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text(
                text = primaryLabel,
                color = if (dragging) AccentEmerald else if (ui.paused) AccentAmber else TextPrimary,
                fontSize = 11.sp,
                fontWeight = FontWeight.Bold,
                letterSpacing = 0.8.sp,
            )
            Text(
                text = subLabel,
                color = TextSecondary,
                fontSize = if (isIdleHint) 7.5.sp else 8.sp,
                fontFamily = if (isIdleHint) FontFamily.Default else FontFamily.Monospace,
                textAlign = TextAlign.Center,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

/**
 * Watch-screen touchpad. Finger travel goes to [PointerRuntime.touchMove] in
 * millimetres; the desktop applies pointer acceleration. A touch that stays
 * within the slop is a click on release, or a drag lock after
 * [Packets.HOLD_TO_DRAG_MS]; once locked, sliding drags.
 */
private suspend fun PointerInputScope.touchpadGestures(
    onPressing: (Boolean) -> Unit,
    onSliding: (Boolean) -> Unit,
    onDragging: (Boolean) -> Unit,
    haptic: (Int) -> Unit,
) {
    val mmPerPx = MM_PER_DP / density
    val slopPx = TOUCH_SLOP_DP * density
    awaitEachGesture {
        val down = awaitFirstDown(requireUnconsumed = false)
        down.consume()
        val downNs = SystemClock.elapsedRealtimeNanos()
        val holdDeadline = down.uptimeMillis + Packets.HOLD_TO_DRAG_MS
        onPressing(true)
        PointerRuntime.touchFinger(true)
        var last = down.position
        var moved = false
        var holding = false
        try {
            while (true) {
                val event = if (!moved && !holding) {
                    val remaining = holdDeadline - SystemClock.uptimeMillis()
                    if (remaining <= 0) null else withTimeoutOrNull(remaining) { awaitPointerEvent() }
                } else {
                    awaitPointerEvent()
                }
                if (event == null) {
                    holding = true
                    onDragging(true)
                    haptic(HapticFeedbackConstants.LONG_PRESS)
                    PointerRuntime.drag(down = true, eventNs = downNs)
                    continue
                }
                val change = event.changes.firstOrNull { it.id == down.id } ?: break
                if (!change.pressed) {
                    if (!moved && !holding) {
                        haptic(HapticFeedbackConstants.KEYBOARD_TAP)
                        PointerRuntime.leftClick(downNs)
                    }
                    break
                }
                if (!moved && !holding && (change.position - down.position).getDistance() > slopPx) {
                    moved = true
                    onSliding(true)
                }
                if (moved || holding) {
                    val delta = change.position - last
                    PointerRuntime.touchMove(delta.x * mmPerPx, delta.y * mmPerPx)
                    last = change.position
                }
                change.consume()
            }
        } finally {
            if (holding) {
                PointerRuntime.drag(
                    down = false,
                    eventNs = SystemClock.elapsedRealtimeNanos() - 50_000_000L,
                )
            }
            PointerRuntime.touchFinger(false)
            onDragging(false)
            onSliding(false)
            onPressing(false)
        }
    }
}

/** 1 dp is 1/160 inch. */
private const val MM_PER_DP = 25.4f / 160f
/** Finger travel before a touch counts as a slide rather than a tap or hold. */
private const val TOUCH_SLOP_DP = 8f

@Composable
private fun DockButton(
    label: String,
    description: String,
    shape: RoundedCornerShape,
    surface: Color,
    border: Color,
    contentColor: Color,
    modifier: Modifier = Modifier,
    icon: DrawScope.(Color) -> Unit,
    onClick: () -> Unit,
) {
    Column(
        modifier = modifier
            .height(44.dp)
            .clip(shape)
            .background(surface)
            .border(1.dp, border, shape)
            .clickable(onClick = onClick)
            .semantics { contentDescription = description },
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Canvas(modifier = Modifier.size(13.dp)) {
            icon(contentColor)
        }
        Spacer(Modifier.height(2.dp))
        Text(
            text = label,
            color = contentColor,
            fontSize = 10.sp,
            fontWeight = FontWeight.Medium,
            textAlign = TextAlign.Center,
            maxLines = 1,
        )
    }
}

@Composable
private fun PowerCapsule(
    stopped: Boolean,
    onStop: () -> Unit,
    onStart: () -> Unit,
) {
    val accent = if (stopped) AccentEmerald else AccentRose
    val text = if (stopped) "Start" else "Stop"
    Row(
        modifier = Modifier
            .width(92.dp)
            .height(26.dp)
            .clip(CircleShape)
            .background(if (stopped) SurfaceDrag else SurfaceIdle)
            .border(1.dp, if (stopped) AccentEmerald else BorderSubtle, CircleShape)
            .clickable(onClick = if (stopped) onStart else onStop)
            .semantics { contentDescription = if (stopped) "Start service" else "Stop service" },
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.Center,
    ) {
        Canvas(modifier = Modifier.size(9.dp)) {
            if (stopped) {
                drawPlayGlyph(accent)
            } else {
                drawRoundRect(
                    color = accent,
                    topLeft = Offset(size.width * 0.12f, size.height * 0.12f),
                    size = Size(size.width * 0.76f, size.height * 0.76f),
                    cornerRadius = CornerRadius(1.8.dp.toPx(), 1.8.dp.toPx()),
                )
            }
        }
        Spacer(Modifier.width(5.dp))
        Text(
            text = text,
            color = if (stopped) AccentEmerald else TextSecondary,
            fontSize = 10.sp,
            fontWeight = FontWeight.SemiBold,
        )
    }
}

@Composable
private fun CenteredNoticeScreen(
    badgeColor: Color,
    title: String,
    subtitle: String,
    buttonLabel: String,
    onAction: () -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(horizontal = 30.dp, vertical = 26.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Box(
            modifier = Modifier
                .size(10.dp)
                .clip(CircleShape)
                .background(badgeColor),
        )
        Spacer(Modifier.height(8.dp))
        Text(
            text = title,
            color = TextPrimary,
            fontSize = 13.sp,
            fontWeight = FontWeight.Bold,
            textAlign = TextAlign.Center,
        )
        Spacer(Modifier.height(4.dp))
        Text(
            text = subtitle,
            color = TextSecondary,
            fontSize = 10.sp,
            textAlign = TextAlign.Center,
        )
        Spacer(Modifier.height(10.dp))
        Box(
            modifier = Modifier
                .height(34.dp)
                .clip(CircleShape)
                .background(SurfaceElevated)
                .border(1.dp, badgeColor, CircleShape)
                .clickable(onClick = onAction)
                .padding(horizontal = 16.dp),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                text = buttonLabel,
                color = TextPrimary,
                fontSize = 11.sp,
                fontWeight = FontWeight.SemiBold,
            )
        }
    }
}

@Composable
private fun TelemetryOverlay(
    ui: PointerRuntime.UiState,
    sightColor: Color,
    showPrivacy: Boolean,
    onShowPrivacy: (Boolean) -> Unit,
    onRole: (PointerRuntime.Role) -> Unit,
    onPairPc: () -> Unit,
    onDpi: (Int) -> Unit,
    onStepDpi: (Int) -> Unit,
    onDismiss: () -> Unit,
) {
    val view = LocalView.current
    if (showPrivacy) {
        PrivacyOverlay(
            sightColor = sightColor,
            onBack = { onShowPrivacy(false) },
        )
        return
    }
    val leftDeg = "%+d°".format((ui.aimLeftCdeg / 100f).roundToInt())
    val upDeg = "%+d°".format((ui.aimUpCdeg / 100f).roundToInt())
    val reachDeg = "%.1f°".format(25f * Packets.BASE_DPI / ui.dpi.coerceAtLeast(Packets.MIN_DPI))
    val canDecrease = ui.dpi > Packets.MIN_DPI
    val canIncrease = ui.dpi < Packets.MAX_DPI
    val showPairPc = ui.role == PointerRuntime.Role.STANDARD_HID && !ui.connected
    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(PitchBlack)
            .clickable(onClick = onDismiss),
        contentAlignment = Alignment.Center,
    ) {
        PerimeterInstrumentDial(sightColor = sightColor)
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(start = 24.dp, end = 24.dp, top = 13.dp, bottom = 17.dp)
                .clickable(enabled = false) {},
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.SpaceBetween,
        ) {
            Text(
                text = "SENSITIVITY · MODE",
                color = TextSecondary,
                fontSize = 8.5.sp,
                fontFamily = FontFamily.Monospace,
                fontWeight = FontWeight.SemiBold,
                letterSpacing = 0.6.sp,
                maxLines = 1,
            )

            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .height(32.dp)
                    .clip(CircleShape)
                    .background(SurfaceElevated)
                    .border(1.dp, BorderStrong, CircleShape)
                    .padding(horizontal = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Box(
                    modifier = Modifier
                        .size(width = 35.dp, height = 24.dp)
                        .clip(CircleShape)
                        .background(SurfaceIdle)
                        .border(1.dp, if (canDecrease) BorderStrong else BorderSubtle, CircleShape)
                        .clickable(enabled = canDecrease) {
                            view.performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP)
                            onStepDpi(-1)
                        }
                        .semantics { contentDescription = "Decrease DPI" },
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        text = "−",
                        color = if (canDecrease) TextPrimary else TextMuted,
                        fontSize = 14.sp,
                        fontWeight = FontWeight.Bold,
                    )
                }

                Column(
                    modifier = Modifier
                        .weight(1f)
                        .clickable {
                            view.performHapticFeedback(HapticFeedbackConstants.CONFIRM)
                            onDpi(Packets.DEFAULT_DPI)
                        }
                        .semantics { contentDescription = "Reset DPI to default" },
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.Center,
                ) {
                    Text(
                        text = "${ui.dpi} DPI",
                        color = AccentEmerald,
                        fontSize = 10.5.sp,
                        fontFamily = FontFamily.Monospace,
                        fontWeight = FontWeight.Bold,
                        maxLines = 1,
                    )
                    Text(
                        text = "±$reachDeg · BEZEL TUNES",
                        color = TextMuted,
                        fontSize = 6.8.sp,
                        fontFamily = FontFamily.Monospace,
                        maxLines = 1,
                    )
                }

                Box(
                    modifier = Modifier
                        .size(width = 35.dp, height = 24.dp)
                        .clip(CircleShape)
                        .background(SurfaceIdle)
                        .border(1.dp, if (canIncrease) BorderStrong else BorderSubtle, CircleShape)
                        .clickable(enabled = canIncrease) {
                            view.performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP)
                            onStepDpi(1)
                        }
                        .semantics { contentDescription = "Increase DPI" },
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        text = "+",
                        color = if (canIncrease) TextPrimary else TextMuted,
                        fontSize = 14.sp,
                        fontWeight = FontWeight.Bold,
                    )
                }
            }

            ModeSelector(
                mode = ui.mode,
                onMode = { mode ->
                    view.performHapticFeedback(HapticFeedbackConstants.CONFIRM)
                    PointerRuntime.setMode(mode)
                },
            )

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                MetricTile("SENSOR", "${ui.samplesPerSec} Hz", Modifier.weight(1f))
                MetricTile("SENT/S", "${ui.sentPerSec}", Modifier.weight(1f))
                MetricTile("AIM L/U", "$leftDeg $upDeg", Modifier.weight(1.4f), compact = true)
            }

            LinkSelector(
                role = ui.role,
                onRole = { role ->
                    view.performHapticFeedback(HapticFeedbackConstants.CONFIRM)
                    onRole(role)
                },
            )

            Row(
                horizontalArrangement = Arrangement.spacedBy(if (showPairPc) 5.dp else 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (showPairPc) {
                    Box(
                        modifier = Modifier
                            .height(22.dp)
                            .clip(CircleShape)
                            .background(SurfaceDrag)
                            .border(1.dp, AccentEmerald, CircleShape)
                            .clickable {
                                view.performHapticFeedback(HapticFeedbackConstants.CONFIRM)
                                onPairPc()
                            }
                            .semantics { contentDescription = "Pair PC via Bluetooth" }
                            .padding(horizontal = 8.dp),
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(
                            text = "Pair PC",
                            color = AccentEmerald,
                            fontSize = 8.5.sp,
                            fontWeight = FontWeight.SemiBold,
                            maxLines = 1,
                        )
                    }
                }
                Box(
                    modifier = Modifier
                        .height(22.dp)
                        .clip(CircleShape)
                        .background(SurfaceIdle)
                        .border(1.dp, BorderSubtle, CircleShape)
                        .clickable { onShowPrivacy(true) }
                        .semantics { contentDescription = "Privacy Policy" }
                        .padding(horizontal = if (showPairPc) 8.dp else 12.dp),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        text = "Privacy",
                        color = TextSecondary,
                        fontSize = if (showPairPc) 8.5.sp else 9.sp,
                        fontWeight = FontWeight.Medium,
                        maxLines = 1,
                    )
                }
                Box(
                    modifier = Modifier
                        .then(if (showPairPc) Modifier.padding(horizontal = 0.dp) else Modifier.width(64.dp))
                        .height(22.dp)
                        .clip(CircleShape)
                        .background(SurfaceElevated)
                        .border(1.dp, BorderStrong, CircleShape)
                        .clickable(onClick = onDismiss)
                        .semantics { contentDescription = "Done" }
                        .padding(horizontal = if (showPairPc) 9.dp else 0.dp),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        text = "Done",
                        color = TextPrimary,
                        fontSize = if (showPairPc) 8.5.sp else 9.5.sp,
                        fontWeight = FontWeight.SemiBold,
                        maxLines = 1,
                    )
                }
            }
        }
    }
}

/** 3-segment Link Bar: BLE Desktop App · Standard Bluetooth HID Mouse · Alternate Central. */
@Composable
private fun LinkSelector(
    role: PointerRuntime.Role,
    onRole: (PointerRuntime.Role) -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 8.dp)
            .height(25.dp)
            .clip(CircleShape)
            .background(SurfaceIdle)
            .border(1.dp, BorderSubtle, CircleShape)
            .padding(2.dp),
        horizontalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        PointerRuntime.Role.entries.forEach { option ->
            val selected = option == role
            val (short, weight, desc) = when (option) {
                PointerRuntime.Role.WATCH_ADVERTISES -> Triple("BLE APP", 1.15f, "BLE Desktop App")
                PointerRuntime.Role.STANDARD_HID -> Triple("BT HID", 1.05f, "Standard Bluetooth HID Mouse")
                PointerRuntime.Role.DESKTOP_ADVERTISES -> Triple("ALT", 0.8f, "Alternate Desktop Advertiser")
            }
            Box(
                modifier = Modifier
                    .weight(weight)
                    .fillMaxSize()
                    .clip(CircleShape)
                    .background(if (selected) SurfaceDrag else SurfaceIdle)
                    .border(1.dp, if (selected) AccentEmerald else SurfaceIdle, CircleShape)
                    .clickable(enabled = !selected) { onRole(option) }
                    .semantics { contentDescription = "Link transport: $desc" },
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    text = short,
                    color = if (selected) AccentEmerald else TextSecondary,
                    fontSize = 7.8.sp,
                    fontFamily = FontFamily.Monospace,
                    fontWeight = if (selected) FontWeight.Bold else FontWeight.Medium,
                    maxLines = 1,
                    softWrap = false,
                )
            }
        }
    }
}

/** Absolute / Air mouse / Touchpad. The desktop follows the watch's choice. */
@Composable
private fun ModeSelector(
    mode: PointerRuntime.Mode,
    onMode: (PointerRuntime.Mode) -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .height(26.dp)
            .clip(CircleShape)
            .background(SurfaceIdle)
            .border(1.dp, BorderSubtle, CircleShape)
            .padding(2.dp),
        horizontalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        PointerRuntime.Mode.entries.forEach { option ->
            val selected = option == mode
            val short = when (option) {
                PointerRuntime.Mode.ABSOLUTE -> "LASER"
                PointerRuntime.Mode.AIR -> "AIR"
                PointerRuntime.Mode.TOUCH -> "TOUCH"
            }
            Box(
                modifier = Modifier
                    .weight(1f)
                    .fillMaxSize()
                    .clip(CircleShape)
                    .background(if (selected) SurfaceDrag else SurfaceIdle)
                    .border(1.dp, if (selected) AccentEmerald else SurfaceIdle, CircleShape)
                    .clickable(enabled = !selected) { onMode(option) }
                    .semantics { contentDescription = "Pointing mode: ${option.label}" },
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    text = short,
                    color = if (selected) AccentEmerald else TextSecondary,
                    fontSize = 8.5.sp,
                    fontFamily = FontFamily.Monospace,
                    fontWeight = if (selected) FontWeight.Bold else FontWeight.Medium,
                    maxLines = 1,
                )
            }
        }
    }
}

@Composable
private fun PrivacyOverlay(
    sightColor: Color,
    onBack: () -> Unit,
) {
    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(PitchBlack)
            .clickable(onClick = onBack),
        contentAlignment = Alignment.Center,
    ) {
        PerimeterInstrumentDial(sightColor = sightColor)
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(horizontal = 28.dp, vertical = 20.dp)
                .clickable(enabled = false) {},
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.SpaceBetween,
        ) {
            Text(
                text = "PRIVACY POLICY",
                color = AccentEmerald,
                fontSize = 9.5.sp,
                fontFamily = FontFamily.Monospace,
                fontWeight = FontWeight.SemiBold,
                letterSpacing = 1.sp,
            )
            Text(
                text = "Zero personal data collected, stored, or shared. Motion sensors & BLE stream locally to your computer cursor only. No INTERNET permission, analytics, or ads.\n\naa22396584.gitlab.io/wear-pointer/privacy.html",
                color = TextSecondary,
                fontSize = 8.5.sp,
                textAlign = TextAlign.Center,
            )
            Box(
                modifier = Modifier
                    .width(76.dp)
                    .height(24.dp)
                    .clip(CircleShape)
                    .background(SurfaceElevated)
                    .border(1.dp, BorderStrong, CircleShape)
                    .clickable(onClick = onBack),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    text = "Back",
                    color = TextPrimary,
                    fontSize = 10.sp,
                    fontWeight = FontWeight.SemiBold,
                )
            }
        }
    }
}

@Composable
private fun MetricTile(
    label: String,
    value: String,
    modifier: Modifier = Modifier,
    compact: Boolean = false,
) {
    Column(
        modifier = modifier
            .height(29.dp)
            .clip(RoundedCornerShape(10.dp))
            .background(SurfaceIdle)
            .border(1.dp, BorderSubtle, RoundedCornerShape(10.dp))
            .padding(horizontal = 4.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text(
            text = label,
            color = TextMuted,
            fontSize = 7.5.sp,
            fontFamily = FontFamily.Monospace,
            maxLines = 1,
            softWrap = false,
        )
        Text(
            text = value,
            color = TextPrimary,
            fontSize = if (compact) 8.5.sp else 10.sp,
            fontFamily = FontFamily.Monospace,
            fontWeight = FontWeight.SemiBold,
            maxLines = 1,
            softWrap = false,
        )
    }
}

private fun DrawScope.drawCrosshairGlyph(color: Color) {
    val c = Offset(size.width / 2f, size.height / 2f)
    val r = size.minDimension * 0.30f
    val s = 1.4.dp.toPx()
    drawCircle(color = color, radius = r, center = c, style = Stroke(width = s))
    drawLine(color, Offset(c.x, 0f), Offset(c.x, c.y - r * 0.55f), s, StrokeCap.Round)
    drawLine(color, Offset(c.x, c.y + r * 0.55f), Offset(c.x, size.height), s, StrokeCap.Round)
    drawLine(color, Offset(0f, c.y), Offset(c.x - r * 0.55f, c.y), s, StrokeCap.Round)
    drawLine(color, Offset(c.x + r * 0.55f, c.y), Offset(size.width, c.y), s, StrokeCap.Round)
}

private fun DrawScope.drawRightClickGlyph(color: Color) {
    val w = size.width * 0.72f
    val h = size.height * 0.90f
    val left = (size.width - w) / 2f
    val top = (size.height - h) / 2f
    val s = 1.3.dp.toPx()
    drawRoundRect(
        color = color,
        topLeft = Offset(left, top),
        size = Size(w, h),
        cornerRadius = CornerRadius(w * 0.42f, w * 0.42f),
        style = Stroke(width = s),
    )
    drawLine(
        color = color,
        start = Offset(size.width / 2f, top),
        end = Offset(size.width / 2f, top + h * 0.46f),
        strokeWidth = s,
    )
    drawRoundRect(
        color = color,
        topLeft = Offset(size.width / 2f + s * 0.3f, top + s * 0.6f),
        size = Size(w / 2f - s * 0.9f, h * 0.38f),
        cornerRadius = CornerRadius(w * 0.2f, w * 0.2f),
    )
}

private fun DrawScope.drawPauseGlyph(color: Color) {
    val barW = size.width * 0.22f
    val barH = size.height * 0.78f
    val top = (size.height - barH) / 2f
    val cr = CornerRadius(1.2.dp.toPx(), 1.2.dp.toPx())
    drawRoundRect(color, Offset(size.width * 0.20f, top), Size(barW, barH), cr)
    drawRoundRect(color, Offset(size.width * 0.58f, top), Size(barW, barH), cr)
}

private fun DrawScope.drawPlayGlyph(color: Color) {
    val p = Path().apply {
        moveTo(size.width * 0.22f, size.height * 0.12f)
        lineTo(size.width * 0.86f, size.height * 0.50f)
        lineTo(size.width * 0.22f, size.height * 0.88f)
        close()
    }
    drawPath(p, color)
}
