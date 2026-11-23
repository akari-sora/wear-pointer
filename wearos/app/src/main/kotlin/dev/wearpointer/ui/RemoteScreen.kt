package dev.wearpointer.ui

import android.os.SystemClock
import android.view.HapticFeedbackConstants
import androidx.activity.compose.BackHandler
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
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
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
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.rotary.onRotaryScrollEvent
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.wear.compose.foundation.lazy.AutoCenteringParams
import androidx.wear.compose.foundation.lazy.ScalingLazyColumn
import androidx.wear.compose.foundation.lazy.ScalingLazyColumnDefaults
import androidx.wear.compose.foundation.lazy.ScalingLazyListScope
import androidx.wear.compose.foundation.lazy.ScalingLazyListState
import androidx.wear.compose.foundation.lazy.rememberScalingLazyListState
import androidx.wear.compose.material.MaterialTheme
import androidx.wear.compose.foundation.rememberSwipeToDismissBoxState
import androidx.wear.compose.material.PositionIndicator
import androidx.wear.compose.material.SwipeToDismissBox
import androidx.wear.compose.material.Text
import dev.wearpointer.PointerRuntime
import dev.wearpointer.proto.Packets
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.roundToInt
import kotlin.math.sin
import kotlin.math.sqrt
import kotlinx.coroutines.launch
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
    onExit: () -> Unit,
) {
    val ui by PointerRuntime.state.collectAsState()
    val focus = remember { FocusRequester() }
    val view = LocalView.current
    var showTelemetry by remember { mutableStateOf(false) }
    var showPrivacy by remember { mutableStateOf(false) }
    var rotaryDpiAccum by remember { mutableStateOf(0f) }
    val noticeList = rememberScalingLazyListState()
    val settingsList = rememberScalingLazyListState()
    val privacyList = rememberScalingLazyListState()
    val scope = rememberCoroutineScope()

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
    val showNotice = !ui.bluetoothGranted || ui.peripheralUnsupported

    // The watch's Back key closes Privacy, then Settings, before it leaves the app.
    BackHandler(enabled = showTelemetry) {
        if (showPrivacy) showPrivacy = false else showTelemetry = false
    }

    MaterialTheme {
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(PitchBlack)
                // Compose UI's onRotaryScrollEvent receives Wear touch-bezel / crown scroll.
                // On Settings it steps DPI, on the text screens it scrolls the page,
                // otherwise it scrolls the desktop.
                .onRotaryScrollEvent { event ->
                    when {
                        showTelemetry && showPrivacy -> privacyList.dispatchRawDelta(event.verticalScrollPixels)
                        showTelemetry -> {
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
                        }
                        showNotice -> noticeList.dispatchRawDelta(event.verticalScrollPixels)
                        else -> PointerRuntime.scrollBy(event.verticalScrollPixels)
                    }
                    true
                }
                .focusRequester(focus)
                .focusable(),
        ) {
            // Base level: the instrument, or a permission notice in its place.
            val base: @Composable () -> Unit = {
                when {
                    !ui.bluetoothGranted -> ExitSwipe(key = "notice-bluetooth", onExit = onExit) {
                        NoticeScreen(
                            listState = noticeList,
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
                    ui.peripheralUnsupported -> ExitSwipe(key = "notice-peripheral", onExit = onExit) {
                        NoticeScreen(
                            listState = noticeList,
                            badgeColor = AccentRose,
                            title = "Peripheral Blocked",
                            subtitle = "This watch cannot advertise BLE. Switch to built-in BT HID mouse.",
                            buttonLabel = "Use BT HID",
                            onAction = { onRole(PointerRuntime.Role.STANDARD_HID) },
                        )
                    }
                    // No swipe-to-dismiss here: touchpad mode reads horizontal slides.
                    else -> MainInstrument(
                        ui = ui,
                        sightColor = sightColor,
                        onOpenSettings = {
                            scope.launch { settingsList.scrollToItem(0) }
                            showTelemetry = true
                        },
                        onStop = onStop,
                        onStart = onStart,
                    )
                }
            }
            if (!showTelemetry) {
                base()
            } else {
                // Swipe right goes back one level: Privacy to Settings, Settings to the instrument.
                SwipeToDismissBox(
                    onDismissed = {
                        showPrivacy = false
                        showTelemetry = false
                    },
                    state = rememberSwipeToDismissBoxState(),
                    backgroundKey = "instrument",
                    contentKey = "settings",
                ) { isBackground ->
                    if (isBackground) {
                        // Shown while Settings slides away: a picture of the instrument.
                        // It takes no touches (a second finger must not click the
                        // desktop) and TalkBack does not read it.
                        Box(
                            modifier = Modifier
                                .fillMaxSize()
                                .clearAndSetSemantics {},
                        ) {
                            base()
                            Box(
                                modifier = Modifier
                                    .matchParentSize()
                                    .pointerInput(Unit) {
                                        awaitPointerEventScope {
                                            while (true) {
                                                awaitPointerEvent(PointerEventPass.Initial)
                                                    .changes
                                                    .forEach { it.consume() }
                                            }
                                        }
                                    },
                            )
                        }
                    } else {
                        SwipeToDismissBox(
                            onDismissed = { showPrivacy = false },
                            state = rememberSwipeToDismissBoxState(),
                            backgroundKey = "settings-below-privacy",
                            contentKey = if (showPrivacy) "privacy" else "settings",
                            // Off on Settings itself, so the outer box takes that swipe.
                            hasBackground = showPrivacy,
                        ) { isSettingsBehind ->
                            if (showPrivacy && !isSettingsBehind) {
                                PrivacyScreen(
                                    listState = privacyList,
                                    onBack = { showPrivacy = false },
                                )
                            } else {
                                SettingsScreen(
                                    ui = ui,
                                    listState = settingsList,
                                    onShowPrivacy = {
                                        scope.launch { privacyList.scrollToItem(0) }
                                        showPrivacy = true
                                    },
                                    onRole = onRole,
                                    onPairPc = onPairPc,
                                    onDpi = onDpi,
                                    onStepDpi = onStepDpi,
                                    onDismiss = { showTelemetry = false },
                                    onExit = onExit,
                                )
                            }
                        }
                    }
                }
            }
        }
    }

    // Each page change re-focuses the root, which owns the bezel. A page's
    // SwipeToDismissBox can clear focus as it comes in, so the notice kind is a key too.
    LaunchedEffect(ui.connected, showTelemetry, showPrivacy, ui.bluetoothGranted, ui.peripheralUnsupported) {
        focus.requestFocus()
    }
}

/**
 * The pointer instrument: status capsule, click pad, dock and Start/Stop.
 * Text is 12sp or larger; each piece shows the longest wording that fits its
 * round-safe space, and a label that does not fit is left out, not cut.
 */
@Composable
private fun MainInstrument(
    ui: PointerRuntime.UiState,
    sightColor: Color,
    onOpenSettings: () -> Unit,
    onStop: () -> Unit,
    onStart: () -> Unit,
) {
    val view = LocalView.current
    val isStopped = ui.status == "Stopped"
    val hasGapNotice = ui.status.startsWith("Sensor gap")
    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(PitchBlack),
    ) {
        PerimeterInstrumentDial(sightColor = sightColor)
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(top = MainTopInset, bottom = MainBottomInset),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            StatusCapsule(
                ui = ui,
                sightColor = sightColor,
                onClick = onOpenSettings,
            )
            Spacer(Modifier.height(MainGap))
            ClickPad(
                ui = ui,
                modifier = Modifier
                    .weight(1f)
                    .fillMaxWidth()
                    .padding(horizontal = 22.dp),
            )
            Spacer(Modifier.height(MainGap))
            BoxWithConstraints(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 24.dp)
                    .height(DockHeight),
            ) {
                // One decision for the whole dock, so the three buttons always match:
                // words when every label (Pause and Resume included) fits, else glyphs.
                val buttonWidth = (maxWidth - DockSpacing * 2) / 3
                val measurer = rememberTextMeasurer()
                val density = LocalDensity.current
                val showLabels = remember(buttonWidth, density) {
                    val roomPx = with(density) { (buttonWidth - 8.dp).toPx() }
                    DockLabels.all { measurer.measure(it, DockLabelStyle, maxLines = 1).size.width <= roomPx }
                }
                Row(
                    modifier = Modifier.fillMaxSize(),
                    horizontalArrangement = Arrangement.spacedBy(DockSpacing),
                ) {
                    DockButton(
                        label = "Right",
                        showLabel = showLabels,
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
                        showLabel = showLabels,
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
                        showLabel = showLabels,
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
            }
            Spacer(Modifier.height(MainGap))
            PowerCapsule(
                stopped = isStopped,
                onStop = onStop,
                onStart = onStart,
            )
        }
    }
}

/** A permission notice: a swipe right leaves the app, as on any Wear OS app. */
@Composable
private fun ExitSwipe(key: String, onExit: () -> Unit, content: @Composable () -> Unit) {
    SwipeToDismissBox(
        onDismissed = onExit,
        state = rememberSwipeToDismissBoxState(),
        backgroundKey = "$key-exit",
        contentKey = key,
    ) { isBackground ->
        if (isBackground) {
            Box(Modifier.fillMaxSize().background(PitchBlack))
        } else {
            content()
        }
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

/** Top inset of the main column; the status capsule starts here. */
private val MainTopInset = 15.dp
/** Bottom inset of the main column; keeps the Start/Stop pill inside the round edge. */
private val MainBottomInset = 12.dp
/** Vertical gap between the capsule, pad, dock and Start/Stop. */
private val MainGap = 3.dp
private val StatusCapsuleHeight = 24.dp
private val DockHeight = 44.dp
private val DockSpacing = 6.dp

/*
 * Main-screen text follows the Wear OS quality guideline: 12sp or larger. Each
 * style sets its line height, so a measured line is the line that is drawn.
 */
private val StatusTextStyle = TextStyle(
    fontSize = 12.sp,
    lineHeight = 14.sp,
    letterSpacing = 0.sp,
    fontWeight = FontWeight.SemiBold,
    fontFamily = FontFamily.Monospace,
)
private val PadTitleStyle = TextStyle(
    fontSize = 12.sp,
    lineHeight = 14.sp,
    fontWeight = FontWeight.Bold,
    letterSpacing = 0.8.sp,
    textAlign = TextAlign.Center,
)
private val PadHintStyle = TextStyle(
    fontSize = 12.sp,
    lineHeight = 14.sp,
    letterSpacing = 0.sp,
    textAlign = TextAlign.Center,
)
private val DockLabelStyle = TextStyle(
    fontSize = 12.sp,
    lineHeight = 14.sp,
    letterSpacing = 0.sp,
    fontWeight = FontWeight.Medium,
    textAlign = TextAlign.Center,
)
private val PowerLabelStyle = TextStyle(
    fontSize = 12.sp,
    lineHeight = 14.sp,
    letterSpacing = 0.sp,
    fontWeight = FontWeight.SemiBold,
)
/** Height the pad's reticle needs above its text before the hint line is shown. */
private val PadMinReticleZone = 23.dp
/** Every word a dock button can show; the dock shows words only when all of them fit. */
private val DockLabels = listOf("Right", "Center", "Pause", "Resume")
/** Gap kept between any control and the edge of a round display. */
private val RoundEdgeMargin = 4.dp

/**
 * Widest pill (rounded ends, [height] tall) that stays inside a round display of
 * [diameter] when its top edge is [top] from the top of the screen. A square
 * display is wider than the circle at every row, so the same limit is safe there.
 */
private fun pillMaxWidth(diameter: Dp, top: Dp, height: Dp): Dp {
    val endRadius = height.value / 2f
    val reach = diameter.value / 2f - RoundEdgeMargin.value - endRadius
    val rise = diameter.value / 2f - top.value - endRadius
    if (reach <= abs(rise)) return height
    return (2f * sqrt(reach * reach - rise * rise) + height.value).dp
}

/**
 * Status texts from longest to shortest. The capsule shows the first one that
 * fits its round-safe width, so nothing reaches the edge of the display at any
 * screen or font size.
 */
private fun statusLabels(ui: PointerRuntime.UiState): List<String> {
    val dpi = ui.dpi
    return when {
        ui.status == "Stopped" -> listOf("STOPPED")
        ui.status.startsWith("Sensor gap") -> listOf("RECENTERED", "CENTERED")
        ui.paused -> listOf("PAUSED · AWAKE", "PAUSED")
        ui.connected -> when (ui.mode) {
            PointerRuntime.Mode.ABSOLUTE -> {
                val hz = if (ui.samplesPerSec > 0) "${ui.samplesPerSec}HZ" else "LIVE"
                listOf("$hz · $dpi DPI", "$hz · $dpi", hz)
            }
            PointerRuntime.Mode.AIR -> listOf("AIR · $dpi DPI", "AIR · $dpi", "AIR")
            PointerRuntime.Mode.TOUCH -> listOf("TOUCH · $dpi DPI", "TOUCH · $dpi", "TOUCH")
        }
        ui.status.startsWith("Link dropped") -> listOf("RECONNECTING", "RETRYING")
        ui.status.contains("Discoverable", ignoreCase = true) -> listOf("PAIRABLE · 2M", "PAIRABLE")
        ui.status == "Bluetooth is off" -> listOf("BLUETOOTH OFF", "BT OFF")
        ui.status == "Waiting for link" -> listOf("STANDBY")
        ui.role == PointerRuntime.Role.STANDARD_HID -> {
            if (ui.status.contains("Connecting", ignoreCase = true)) {
                listOf("HID · CONNECTING", "CONNECTING", "LINKING", "HID")
            } else {
                listOf("BT HID · $dpi DPI", "BT HID · $dpi", "BT HID")
            }
        }
        ui.status.contains("Advertising", ignoreCase = true) -> listOf("ADV · $dpi DPI", "ADV · $dpi", "ADV")
        ui.status.contains("Scanning", ignoreCase = true) -> listOf("SCANNING", "SCAN")
        ui.status.contains("Connecting", ignoreCase = true) -> listOf("CONNECTING", "LINKING")
        ui.status.startsWith("Starting") -> listOf("STARTING")
        // Rare transport errors: shown in full when they fit, else ellipsized inside the capsule.
        else -> listOf(ui.status.uppercase())
    }
}

@Composable
private fun StatusCapsule(
    ui: PointerRuntime.UiState,
    sightColor: Color,
    onClick: () -> Unit,
) {
    val screen = LocalConfiguration.current
    val diameter = minOf(screen.screenWidthDp, screen.screenHeightDp).dp
    val maxWidth = pillMaxWidth(diameter, MainTopInset, StatusCapsuleHeight)
    val padding = 9.dp
    val dot = 6.dp
    val gap = 5.dp
    val style = StatusTextStyle
    val measurer = rememberTextMeasurer()
    val density = LocalDensity.current
    val labels = statusLabels(ui)
    val label = remember(labels, maxWidth, density) {
        val roomPx = with(density) { (maxWidth - padding * 2 - dot - gap).toPx() }
        labels.firstOrNull { measurer.measure(it, style, maxLines = 1).size.width <= roomPx } ?: labels.last()
    }
    Row(
        modifier = Modifier
            .widthIn(max = maxWidth)
            .height(StatusCapsuleHeight)
            .clip(CircleShape)
            .background(SurfaceElevated)
            .border(1.dp, BorderSubtle, CircleShape)
            .clickable(onClick = onClick)
            .semantics { contentDescription = "Status and telemetry" }
            .padding(horizontal = padding),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(gap),
    ) {
        Box(
            modifier = Modifier
                .size(dot)
                .clip(CircleShape)
                .background(sightColor),
        )
        Text(
            text = label,
            color = TextPrimary,
            style = style,
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
    // Longest first; the pad shows the first hint that fits on one line.
    val subLabels = when {
        isStopped -> listOf("TAP START BELOW", "TAP START")
        dragging -> if (touchMode) listOf("SLIDE TO DRAG", "SLIDE") else listOf("RELEASE TO DROP", "RELEASE")
        ui.paused -> if (ui.mode == PointerRuntime.Mode.ABSOLUTE) {
            listOf("TRACKS WHILE AWAKE", "AWAKE")
        } else {
            listOf("CLUTCH · CURSOR HELD", "CURSOR HELD")
        }
        !ui.connected -> when (ui.role) {
            PointerRuntime.Role.WATCH_ADVERTISES ->
                listOf("Open PC app · or tap top for BT HID", "Open the PC app", "Open PC app")
            PointerRuntime.Role.STANDARD_HID ->
                listOf("Pair in PC Bluetooth · tap top to Pair", "Pair in PC Bluetooth", "Pair with PC")
            PointerRuntime.Role.DESKTOP_ADVERTISES ->
                listOf("Scanning · or tap top for BT HID", "Scanning for PC", "Scanning")
        }
        touchMode -> listOf("TAP CLICK · HOLD DRAG", "HOLD TO DRAG", "HOLD")
        ui.mode == PointerRuntime.Mode.AIR -> listOf("AIR MOUSE · HOLD DRAG", "HOLD TO DRAG", "HOLD")
        else -> listOf("HOLD DRAG · BEZEL SCROLL", "HOLD TO DRAG", "HOLD")
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
        val hintStyle = if (isIdleHint) PadHintStyle else PadHintStyle.copy(fontFamily = FontFamily.Monospace)
        val measurer = rememberTextMeasurer()
        val density = LocalDensity.current
        BoxWithConstraints(
            modifier = Modifier
                .fillMaxSize()
                .padding(start = 8.dp, end = 8.dp, top = 4.dp, bottom = 5.dp),
        ) {
            val roomPx = constraints.maxWidth
            val heightPx = constraints.maxHeight
            // The hint is the longest wording that fits on one line, and it is shown only
            // while the reticle above the text keeps a readable size; on a small watch
            // with a large font the pad shows the reticle and the title alone.
            val subLabel = remember(subLabels, primaryLabel, hintStyle, roomPx, heightPx, density) {
                val fitting = subLabels.firstOrNull {
                    measurer.measure(it, hintStyle, maxLines = 1).size.width <= roomPx
                }
                if (fitting == null) {
                    null
                } else {
                    val textPx = measurer.measure(primaryLabel, PadTitleStyle, maxLines = 1).size.height +
                        measurer.measure(fitting, hintStyle, maxLines = 1).size.height
                    val reticlePx = heightPx - textPx
                    if (reticlePx >= with(density) { PadMinReticleZone.toPx() }) fitting else null
                }
            }
            Column(
                modifier = Modifier.fillMaxSize(),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                // The reticle takes the space above the text, so the two never overlap.
                Canvas(
                    modifier = Modifier
                        .weight(1f)
                        .fillMaxWidth(),
                ) {
                    val cx = size.width / 2f
                    val cy = size.height / 2f
                    val reachR = minOf(
                        20.5.dp.toPx(),
                        size.height / 2f - 4.dp.toPx(),
                        size.width * 0.22f,
                    ).coerceAtLeast(4.dp.toPx())

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
                Text(
                    text = primaryLabel,
                    color = if (dragging) AccentEmerald else if (ui.paused) AccentAmber else TextPrimary,
                    style = PadTitleStyle,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                if (subLabel != null) {
                    Text(
                        text = subLabel,
                        color = TextSecondary,
                        style = hintStyle,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
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
    showLabel: Boolean,
    description: String,
    shape: RoundedCornerShape,
    surface: Color,
    border: Color,
    contentColor: Color,
    modifier: Modifier = Modifier,
    icon: DrawScope.(Color) -> Unit,
    onClick: () -> Unit,
) {
    Box(
        modifier = modifier
            .height(DockHeight)
            .clip(shape)
            .background(surface)
            .border(1.dp, border, shape)
            .clickable(onClick = onClick)
            .semantics { contentDescription = description },
        contentAlignment = Alignment.Center,
    ) {
        // Without the word (small watch or large font) the glyph stands alone and
        // larger; TalkBack still reads the description.
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Canvas(modifier = Modifier.size(if (showLabel) 13.dp else 18.dp)) {
                icon(contentColor)
            }
            if (showLabel) {
                Spacer(Modifier.height(2.dp))
                Text(
                    text = label,
                    color = contentColor,
                    style = DockLabelStyle,
                    maxLines = 1,
                )
            }
        }
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
            style = PowerLabelStyle,
            maxLines = 1,
        )
    }
}

/**
 * Scrolling page for everything that is not the pointer instrument: the
 * permission notices, Settings and Privacy. A list is centred row by row, so
 * its content stays inside a round display at any screen and font size; long
 * text wraps and the page scrolls instead of being cut off.
 */
@Composable
private fun InstrumentList(
    listState: ScalingLazyListState,
    content: ScalingLazyListScope.() -> Unit,
) {
    val screen = LocalConfiguration.current
    val side = (screen.screenWidthDp * 0.1f).dp
    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(PitchBlack),
    ) {
        ScalingLazyColumn(
            modifier = Modifier.fillMaxSize(),
            state = listState,
            contentPadding = PaddingValues(horizontal = side),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(6.dp),
            autoCentering = AutoCenteringParams(itemIndex = 0),
            // The root of RemoteScreen owns the bezel (DPI on Settings, page scroll elsewhere).
            rotaryScrollableBehavior = null,
            // Rows shrink and fade earlier than the default as they near the top
            // and bottom, so they follow the curve of a round display.
            scalingParams = ScalingLazyColumnDefaults.scalingParams(
                edgeScale = 0.5f,
                edgeAlpha = 0.3f,
                minTransitionArea = 0.45f,
                maxTransitionArea = 0.7f,
            ),
            content = content,
        )
        PositionIndicator(scalingLazyListState = listState)
    }
}

@Composable
private fun ListTitle(text: String, color: Color = TextSecondary) {
    Text(
        text = text,
        color = color,
        fontSize = 12.sp,
        fontFamily = FontFamily.Monospace,
        fontWeight = FontWeight.SemiBold,
        letterSpacing = 0.6.sp,
        textAlign = TextAlign.Center,
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 14.dp, vertical = 2.dp),
    )
}

@Composable
private fun ListNote(text: String, mono: Boolean = false) {
    Text(
        text = text,
        color = TextSecondary,
        fontSize = 12.sp,
        fontFamily = if (mono) FontFamily.Monospace else FontFamily.Default,
        textAlign = TextAlign.Center,
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 6.dp),
    )
}

/** Full-width 48dp row: a choice (with [selected]) or an action. Text wraps; the row grows. */
@Composable
private fun ListRow(
    label: String,
    description: String,
    secondary: String? = null,
    selected: Boolean = false,
    accent: Color? = null,
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    val shape = RoundedCornerShape(24.dp)
    val tint = if (selected) AccentEmerald else accent
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 48.dp)
            .clip(shape)
            .background(if (selected) SurfaceDrag else SurfaceIdle)
            .border(1.dp, tint ?: BorderSubtle, shape)
            .clickable(enabled = enabled, onClick = onClick)
            .semantics { contentDescription = description }
            .padding(horizontal = 14.dp, vertical = 7.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text(
            text = label,
            color = tint ?: TextPrimary,
            fontSize = 14.sp,
            fontWeight = if (selected) FontWeight.Bold else FontWeight.SemiBold,
            textAlign = TextAlign.Center,
        )
        if (secondary != null) {
            Text(
                text = secondary,
                color = TextSecondary,
                fontSize = 12.sp,
                textAlign = TextAlign.Center,
            )
        }
    }
}

@Composable
private fun NoticeScreen(
    listState: ScalingLazyListState,
    badgeColor: Color,
    title: String,
    subtitle: String,
    buttonLabel: String,
    onAction: () -> Unit,
) {
    InstrumentList(listState = listState) {
        item {
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                Box(
                    modifier = Modifier
                        .size(10.dp)
                        .clip(CircleShape)
                        .background(badgeColor),
                )
                Spacer(Modifier.height(6.dp))
                Text(
                    text = title,
                    color = TextPrimary,
                    fontSize = 15.sp,
                    fontWeight = FontWeight.Bold,
                    textAlign = TextAlign.Center,
                )
            }
        }
        item { ListNote(subtitle) }
        item {
            ListRow(
                label = buttonLabel,
                description = buttonLabel,
                accent = badgeColor,
                onClick = onAction,
            )
        }
    }
}

@Composable
private fun SettingsScreen(
    ui: PointerRuntime.UiState,
    listState: ScalingLazyListState,
    onShowPrivacy: () -> Unit,
    onRole: (PointerRuntime.Role) -> Unit,
    onPairPc: () -> Unit,
    onDpi: (Int) -> Unit,
    onStepDpi: (Int) -> Unit,
    onDismiss: () -> Unit,
    onExit: () -> Unit,
) {
    val view = LocalView.current
    val leftDeg = "%+d°".format((ui.aimLeftCdeg / 100f).roundToInt())
    val upDeg = "%+d°".format((ui.aimUpCdeg / 100f).roundToInt())
    val reachDeg = "%.1f°".format(25f * Packets.BASE_DPI / ui.dpi.coerceAtLeast(Packets.MIN_DPI))
    val showPairPc = ui.role == PointerRuntime.Role.STANDARD_HID && !ui.connected
    InstrumentList(listState = listState) {
        item { ListTitle("SENSITIVITY") }
        item {
            DpiStepper(
                dpi = ui.dpi,
                onStep = { step ->
                    view.performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP)
                    onStepDpi(step)
                },
                onReset = {
                    view.performHapticFeedback(HapticFeedbackConstants.CONFIRM)
                    onDpi(Packets.DEFAULT_DPI)
                },
            )
        }
        item { ListNote("Reach ±$reachDeg. Turn the bezel to tune, tap the value to reset.") }

        item { ListTitle("POINTING MODE") }
        items(PointerRuntime.Mode.entries.size) { index ->
            val option = PointerRuntime.Mode.entries[index]
            val (label, hint) = when (option) {
                PointerRuntime.Mode.ABSOLUTE -> "Laser" to "Point at the screen"
                PointerRuntime.Mode.AIR -> "Air mouse" to "Move by wrist motion"
                PointerRuntime.Mode.TOUCH -> "Touchpad" to "Slide on the watch"
            }
            ListRow(
                label = label,
                secondary = hint,
                description = "Pointing mode: ${option.label}",
                selected = option == ui.mode,
                enabled = option != ui.mode,
                onClick = {
                    view.performHapticFeedback(HapticFeedbackConstants.CONFIRM)
                    PointerRuntime.setMode(option)
                },
            )
        }

        item { ListTitle("LINK") }
        items(PointerRuntime.Role.entries.size) { index ->
            val option = PointerRuntime.Role.entries[index]
            val (label, hint, desc) = when (option) {
                PointerRuntime.Role.WATCH_ADVERTISES ->
                    Triple("BLE app", "Desktop receiver app", "BLE Desktop App")
                PointerRuntime.Role.STANDARD_HID ->
                    Triple("BT HID", "Bluetooth mouse", "Standard Bluetooth HID Mouse")
                PointerRuntime.Role.DESKTOP_ADVERTISES ->
                    Triple("Alt", "Desktop advertises", "Alternate Desktop Advertiser")
            }
            ListRow(
                label = label,
                secondary = hint,
                description = "Link transport: $desc",
                selected = option == ui.role,
                enabled = option != ui.role,
                onClick = {
                    view.performHapticFeedback(HapticFeedbackConstants.CONFIRM)
                    onRole(option)
                },
            )
        }
        if (showPairPc) {
            item {
                ListRow(
                    label = "Pair PC",
                    secondary = "Discoverable for 2 min",
                    description = "Pair PC via Bluetooth",
                    accent = AccentEmerald,
                    onClick = {
                        view.performHapticFeedback(HapticFeedbackConstants.CONFIRM)
                        onPairPc()
                    },
                )
            }
        }

        item { ListTitle("TELEMETRY") }
        item {
            ListNote(
                "Sensor ${ui.samplesPerSec} Hz\nSent ${ui.sentPerSec}/s\nAim L $leftDeg U $upDeg",
                mono = true,
            )
        }

        item {
            ListRow(
                label = "Privacy",
                description = "Privacy Policy",
                onClick = onShowPrivacy,
            )
        }
        item {
            ListRow(
                label = "Done",
                description = "Done",
                onClick = onDismiss,
            )
        }
        // The instrument screen keeps swipe-to-close off (touchpad mode reads
        // horizontal slides), so the app offers an explicit way to close it.
        item {
            ListRow(
                label = "Close app",
                secondary = if (ui.status == "Stopped") null else "Pointer keeps running",
                description = "Close app",
                onClick = onExit,
            )
        }
    }
}

/** − / value / + with 48dp targets. Tapping the value resets to the default DPI. */
@Composable
private fun DpiStepper(
    dpi: Int,
    onStep: (Int) -> Unit,
    onReset: () -> Unit,
) {
    val canDecrease = dpi > Packets.MIN_DPI
    val canIncrease = dpi < Packets.MAX_DPI
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 56.dp)
            .clip(RoundedCornerShape(28.dp))
            .background(SurfaceElevated)
            .border(1.dp, BorderStrong, RoundedCornerShape(28.dp))
            .padding(4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        StepButton(
            glyph = "−",
            description = "Decrease DPI",
            enabled = canDecrease,
            onClick = { onStep(-1) },
        )
        Column(
            modifier = Modifier
                .weight(1f)
                .heightIn(min = 48.dp)
                .clip(RoundedCornerShape(12.dp))
                .clickable(onClick = onReset)
                .semantics { contentDescription = "Reset DPI to default" },
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
        ) {
            Text(
                text = "$dpi",
                color = AccentEmerald,
                fontSize = 16.sp,
                fontFamily = FontFamily.Monospace,
                fontWeight = FontWeight.Bold,
                maxLines = 1,
                softWrap = false,
            )
            Text(
                text = "DPI",
                color = TextSecondary,
                fontSize = 12.sp,
                fontFamily = FontFamily.Monospace,
                maxLines = 1,
            )
        }
        StepButton(
            glyph = "+",
            description = "Increase DPI",
            enabled = canIncrease,
            onClick = { onStep(1) },
        )
    }
}

@Composable
private fun StepButton(
    glyph: String,
    description: String,
    enabled: Boolean,
    onClick: () -> Unit,
) {
    Box(
        modifier = Modifier
            .size(48.dp)
            .clip(CircleShape)
            .background(SurfaceIdle)
            .border(1.dp, if (enabled) BorderStrong else BorderSubtle, CircleShape)
            .clickable(enabled = enabled, onClick = onClick)
            .semantics { contentDescription = description },
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = glyph,
            color = if (enabled) TextPrimary else TextMuted,
            fontSize = 18.sp,
            fontWeight = FontWeight.Bold,
        )
    }
}

@Composable
private fun PrivacyScreen(
    listState: ScalingLazyListState,
    onBack: () -> Unit,
) {
    InstrumentList(listState = listState) {
        item { ListTitle("PRIVACY POLICY", color = AccentEmerald) }
        // One short row per sentence: each row scales with the round edge on its own.
        item { ListNote("Zero personal data collected, stored, or shared.") }
        item { ListNote("Motion sensors & BLE stream locally to your computer cursor only.") }
        item { ListNote("No INTERNET permission, analytics, or ads.") }
        item { ListNote("aa22396584.gitlab.io/\nwear-pointer/privacy.html") }
        item {
            ListRow(
                label = "Back",
                description = "Back",
                onClick = onBack,
            )
        }
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
