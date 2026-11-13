package dev.wearpointer

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import dev.wearpointer.ui.RemoteScreen

class MainActivity : ComponentActivity() {
    private val permissions = registerForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions(),
    ) { granted ->
        if (granted.values.all { it }) {
            startPointer()
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            RemoteScreen(
                onRequestPermissions = { requestBlePermissions() },
                onRole = { PointerRuntime.setRole(it) },
            )
        }
        if (hasBlePermissions()) {
            startPointer()
        }
    }

    private fun requestBlePermissions() {
        permissions.launch(
            arrayOf(
                Manifest.permission.BLUETOOTH_CONNECT,
                Manifest.permission.BLUETOOTH_ADVERTISE,
                Manifest.permission.BLUETOOTH_SCAN,
                Manifest.permission.POST_NOTIFICATIONS,
            ),
        )
    }

    private fun hasBlePermissions(): Boolean {
        return listOf(
            Manifest.permission.BLUETOOTH_CONNECT,
            Manifest.permission.BLUETOOTH_ADVERTISE,
            Manifest.permission.BLUETOOTH_SCAN,
        ).all { checkSelfPermission(it) == PackageManager.PERMISSION_GRANTED }
    }

    private fun startPointer() {
        startForegroundService(Intent(this, PointerService::class.java))
    }
}
