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
    onRequestPermissions: () -> Unit,
    onRole: (PointerRuntime.Role) -> Unit,
    onStop: () -> Unit,
    onStart: () -> Unit,
) {
    val ui by PointerRuntime.state.collectAsState()
    val focus = remember { FocusRequester() }
    val view = LocalView.current
    var showTelemetry by remember { mutableStateOf(false) }

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
                .onRotaryScrollEvent { event ->
                    PointerRuntime.scrollBy(event.verticalScrollPixels)
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
                        subtitle = "Allow advertising and GATT link to pair with desktop receiver.",
                        buttonLabel = "Allow Bluetooth",
                        onAction = onRequestPermissions,
                    )
                }
                ui.peripheralUnsupported -> {
                    CenteredNoticeScreen(
                        badgeColor = AccentRose,
                        title = "Peripheral Blocked",
                        subtitle = "This watch cannot advertise BLE. Switch to desktop advertiser.",
                        buttonLabel = "Switch Role",
                        onAction = { onRole(PointerRuntime.Role.DESKTOP_ADVERTISES) },
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
                    onRole = onRole,
                    onDismiss = { showTelemetry = false },
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
        ui.connected -> {
            val hz = if (ui.samplesPerSec > 0) "${ui.samplesPerSec} HZ" else "LIVE"
            "LINKED · $hz"
        }
        ui.status.startsWith("Link dropped") -> "RECONNECTING"
        ui.status.contains("Advertising", ignoreCase = true) -> "ADVERTISING"
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
    val holdProgress = remember { Animatable(0f) }
    val view = LocalView.current

    LaunchedEffect(pressing, dragging) {
        if (pressing && !dragging) {
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
        else -> "TAP"
    }
    val subLabel = when {
        isStopped -> "TAP START BELOW"
        dragging -> "RELEASE TO DROP"
        ui.paused -> "TRACKS WHILE AWAKE"
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
            .semantics { contentDescription = "Touchpad: tap for left click, hold to drag" }
            .pointerInput(Unit) {
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

            // Live spatial aim puck: mirrors desktop cursor direction (±25° -> reachR).
            val leftDeg = ui.aimLeftCdeg / 100f
            val upDeg = ui.aimUpCdeg / 100f
            val normX = (-leftDeg / 25f).coerceIn(-1f, 1f)
            val normY = (-upDeg / 25f).coerceIn(-1f, 1f)
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
                .padding(bottom = 6.dp),
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
                fontSize = 8.sp,
                fontFamily = FontFamily.Monospace,
                maxLines = 1,
            )
        }
    }
}

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
    onRole: (PointerRuntime.Role) -> Unit,
    onDismiss: () -> Unit,
) {
    val leftDeg = "%+.1f°".format(ui.aimLeftCdeg / 100f)
    val upDeg = "%+.1f°".format(ui.aimUpCdeg / 100f)
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
                .padding(horizontal = 24.dp, vertical = 18.dp)
                .clickable(enabled = false) {},
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.SpaceBetween,
        ) {
            Text(
                text = "TELEMETRY",
                color = TextSecondary,
                fontSize = 9.5.sp,
                fontFamily = FontFamily.Monospace,
                fontWeight = FontWeight.SemiBold,
                letterSpacing = 1.sp,
            )

            Column(
                modifier = Modifier.fillMaxWidth(),
                verticalArrangement = Arrangement.spacedBy(5.dp),
            ) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(5.dp),
                ) {
                    MetricTile("SENSOR", "${ui.samplesPerSec} Hz", Modifier.weight(1f))
                    MetricTile("SENT/S", "${ui.sentPerSec}", Modifier.weight(1f))
                }
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(5.dp),
                ) {
                    MetricTile("DROP/S", "${ui.droppedPerSec}", Modifier.weight(1f))
                    MetricTile("AIM L/U", "$leftDeg $upDeg", Modifier.weight(1f), compact = true)
                }
            }

            Text(
                text = ui.status,
                color = TextSecondary,
                fontSize = 9.sp,
                textAlign = TextAlign.Center,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )

            val watchAdv = ui.role == PointerRuntime.Role.WATCH_ADVERTISES
            Box(
                modifier = Modifier
                    .height(26.dp)
                    .clip(CircleShape)
                    .background(SurfaceIdle)
                    .border(1.dp, BorderSubtle, CircleShape)
                    .clickable {
                        val next = if (watchAdv) {
                            PointerRuntime.Role.DESKTOP_ADVERTISES
                        } else {
                            PointerRuntime.Role.WATCH_ADVERTISES
                        }
                        onRole(next)
                    }
                    .padding(horizontal = 12.dp),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    text = if (watchAdv) "Role: Watch Adv" else "Role: Desktop Adv",
                    color = TextPrimary,
                    fontSize = 9.5.sp,
                    fontFamily = FontFamily.Monospace,
                )
            }

            Box(
                modifier = Modifier
                    .width(76.dp)
                    .height(24.dp)
                    .clip(CircleShape)
                    .background(SurfaceElevated)
                    .border(1.dp, BorderStrong, CircleShape)
                    .clickable(onClick = onDismiss),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    text = "Done",
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
            .height(34.dp)
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
