package dev.wearpointer.ui

import android.os.SystemClock
import android.view.HapticFeedbackConstants
import androidx.compose.foundation.background
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.input.rotary.onRotaryScrollEvent
import androidx.wear.compose.material.MaterialTheme
import androidx.wear.compose.material.Text
import androidx.compose.foundation.clickable
import androidx.compose.runtime.collectAsState
import dev.wearpointer.PointerRuntime
import dev.wearpointer.proto.Packets
import kotlinx.coroutines.withTimeoutOrNull

@Composable
fun RemoteScreen(
    onRequestPermissions: () -> Unit,
    onRole: (PointerRuntime.Role) -> Unit,
) {
    val ui by PointerRuntime.state.collectAsState()
    val focus = remember { FocusRequester() }
    val view = LocalView.current
    MaterialTheme {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .background(Color.Black)
                // Compose UI's onRotaryScrollEvent. Wear Compose 1.4's rotaryScrollable
                // is for scrollable lists; the bezel still arrives as this event.
                .onRotaryScrollEvent { event ->
                    PointerRuntime.scrollBy(event.verticalScrollPixels)
                    true
                }
                .focusRequester(focus)
                .focusable()
                .padding(horizontal = 8.dp, vertical = 6.dp),
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            Text(
                text = statusLine(ui),
                fontSize = 11.sp,
                color = Color(0xFFB5B5B5),
                textAlign = TextAlign.Center,
                modifier = Modifier.fillMaxWidth(),
            )
            when {
                !ui.bluetoothGranted -> {
                    ActionButton("Allow Bluetooth", onClick = onRequestPermissions)
                }
                ui.peripheralUnsupported -> {
                    Text(
                        "This watch cannot advertise as a peripheral.",
                        fontSize = 12.sp,
                        color = Color.White,
                        textAlign = TextAlign.Center,
                    )
                    ActionButton("Use desktop advertiser") {
                        onRole(PointerRuntime.Role.DESKTOP_ADVERTISES)
                    }
                }
                else -> {
                    ClickPad(
                        paused = ui.paused,
                        connected = ui.connected,
                        modifier = Modifier
                            .fillMaxWidth()
                            .weight(1f),
                    )
                    Row(
                        modifier = Modifier.fillMaxWidth().height(40.dp),
                        horizontalArrangement = Arrangement.spacedBy(4.dp),
                    ) {
                        ActionButton("Right", Modifier.weight(1f)) {
                            view.performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP)
                            PointerRuntime.rightClick()
                        }
                        ActionButton("Center", Modifier.weight(1f)) {
                            view.performHapticFeedback(HapticFeedbackConstants.CONFIRM)
                            PointerRuntime.center()
                        }
                        ActionButton(if (ui.paused) "Play" else "Pause", Modifier.weight(1f)) {
                            PointerRuntime.togglePause()
                        }
                    }
                }
            }
            val roleLabel = if (ui.role == PointerRuntime.Role.WATCH_ADVERTISES) {
                "Role: watch advertises"
            } else {
                "Role: desktop advertises"
            }
            Text(
                text = roleLabel,
                fontSize = 10.sp,
                color = Color(0xFF8A8A8A),
                textAlign = TextAlign.Center,
                modifier = Modifier
                    .fillMaxWidth()
                    .clickable {
                        val next = if (ui.role == PointerRuntime.Role.WATCH_ADVERTISES) {
                            PointerRuntime.Role.DESKTOP_ADVERTISES
                        } else {
                            PointerRuntime.Role.WATCH_ADVERTISES
                        }
                        onRole(next)
                    },
            )
        }
    }
    LaunchedEffect(ui.connected) {
        focus.requestFocus()
    }
}

@Composable
private fun ClickPad(paused: Boolean, connected: Boolean, modifier: Modifier) {
    var pressing by remember { mutableStateOf(false) }
    var dragging by remember { mutableStateOf(false) }
    val view = LocalView.current
    val fill = when {
        dragging -> Color.White
        paused -> Color(0xFF5A3A10)
        pressing -> Color(0xFF3A3A3A)
        else -> Color(0xFF242424)
    }
    val label = when {
        dragging -> "Drag"
        paused -> "Paused"
        !connected -> "Wait"
        else -> "Tap"
    }
    Box(
        modifier = modifier
            .background(fill, RoundedCornerShape(18.dp))
            .pointerInput(Unit) {
                awaitEachGesture {
                    awaitFirstDown()
                    pressing = true
                    val downNs = SystemClock.elapsedRealtimeNanos()
                    val released = withTimeoutOrNull(Packets.HOLD_TO_DRAG_MS) {
                        waitForUpOrCancellation()
                    }
                    if (released != null) {
                        pressing = false
                        view.performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP)
                        PointerRuntime.leftClick(downNs)
                    } else {
                        dragging = true
                        view.performHapticFeedback(HapticFeedbackConstants.LONG_PRESS)
                        PointerRuntime.drag(down = true, eventNs = downNs)
                        waitForUpOrCancellation()
                        dragging = false
                        pressing = false
                        PointerRuntime.drag(
                            down = false,
                            eventNs = SystemClock.elapsedRealtimeNanos() - 50_000_000L,
                        )
                    }
                }
            },
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = label,
            color = if (dragging) Color.Black else Color.White,
            fontSize = 16.sp,
        )
    }
}

@Composable
private fun ActionButton(label: String, modifier: Modifier = Modifier, onClick: () -> Unit) {
    Box(
        modifier = modifier
            .fillMaxWidth()
            .height(36.dp)
            .background(Color(0xFF2A2A2A), RoundedCornerShape(12.dp))
            .clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = Color.White, fontSize = 12.sp, textAlign = TextAlign.Center)
    }
}

private fun statusLine(ui: PointerRuntime.UiState): String {
    val link = if (ui.connected) "link" else "no link"
    return "$link  ${ui.samplesPerSec} Hz  sent ${ui.sentPerSec}  drop ${ui.droppedPerSec}\n${ui.status}"
}
