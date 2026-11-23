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
import androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen
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

    /** Optional: the pointer runs either way, so the answer is not acted on. */
    private val notifications = registerForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) {}

    override fun onCreate(savedInstanceState: Bundle?) {
        // Wear OS branded launch: 48dp app icon on black, then Theme.WearPointer.
        installSplashScreen()
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
                // Swipe right on a permission notice leaves the app.
                onExit = { finish() },
            )
        }
        if (hasBlePermissions()) {
            startPointer()
            // Bluetooth was granted earlier (for example before an update), so the
            // combined request never ran. Without notifications the system hides
            // the session's Ongoing Activity; ask once, on its own.
            if (savedInstanceState == null &&
                checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
            ) {
                notifications.launch(Manifest.permission.POST_NOTIFICATIONS)
            }
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
