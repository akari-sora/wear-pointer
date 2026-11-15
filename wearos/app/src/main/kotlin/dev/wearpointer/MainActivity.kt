package dev.wearpointer

import android.Manifest
import android.content.ActivityNotFoundException
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Bundle
import android.provider.Settings
import android.util.Log
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import dev.wearpointer.ui.RemoteScreen

class MainActivity : ComponentActivity() {
    /** Bluetooth was denied with "don't ask again": only app settings can grant it now. */
    private var permissionsBlocked by mutableStateOf(false)

    /** App settings were opened for Bluetooth; start once the user is back with it allowed. */
    private var awaitingSettings = false

    private val permissions = registerForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions(),
    ) {
        // Notifications are optional: the pointer works without them, so only
        // the Bluetooth trio decides whether to start.
        if (hasBlePermissions()) {
            permissionsBlocked = false
            startPointer()
        } else {
            permissionsBlocked = BLE_PERMISSIONS.any {
                checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED &&
                    !shouldShowRequestPermissionRationale(it)
            }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            val state by PointerRuntime.state.collectAsState()
            val keepScreenOn = state.connected && !state.paused
            DisposableEffect(keepScreenOn) {
                if (keepScreenOn) {
                    window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
                } else {
                    window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
                }
                onDispose {
                    window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
                }
            }
            RemoteScreen(
                permissionsBlocked = permissionsBlocked,
                onRequestPermissions = { requestBlePermissions() },
                onRole = { PointerRuntime.setRole(it) },
                onPairPc = { PointerRuntime.requestPairable(this) },
                onDpi = { PointerRuntime.setDpi(it) },
                onStepDpi = { PointerRuntime.stepDpi(it) },
                onStop = { PointerRuntime.stop() },
                onStart = { startPointer() },
            )
        }
        if (hasBlePermissions()) {
            startPointer()
        }
    }

    override fun onResume() {
        super.onResume()
        // Coming back from app settings with Bluetooth now allowed. Only after
        // our own "Open Settings": onCreate already started the service, and a
        // user Stop must not be undone by returning to the app.
        if (awaitingSettings && hasBlePermissions()) {
            awaitingSettings = false
            permissionsBlocked = false
            startPointer()
        }
    }

    private fun requestBlePermissions() {
        if (permissionsBlocked) {
            openAppSettings()
            return
        }
        permissions.launch(BLE_PERMISSIONS + Manifest.permission.POST_NOTIFICATIONS)
    }

    private fun openAppSettings() {
        val intent = Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS)
            .setData(Uri.fromParts("package", packageName, null))
        try {
            awaitingSettings = true
            startActivity(intent)
        } catch (err: ActivityNotFoundException) {
            Log.w("wear-pointer", "no app settings screen; asking again", err)
            permissions.launch(BLE_PERMISSIONS)
        }
    }

    private fun hasBlePermissions(): Boolean {
        return BLE_PERMISSIONS.all { checkSelfPermission(it) == PackageManager.PERMISSION_GRANTED }
    }

    private fun startPointer() {
        startForegroundService(Intent(this, PointerService::class.java))
    }

    private companion object {
        val BLE_PERMISSIONS = arrayOf(
            Manifest.permission.BLUETOOTH_CONNECT,
            Manifest.permission.BLUETOOTH_ADVERTISE,
            Manifest.permission.BLUETOOTH_SCAN,
        )
    }
}
