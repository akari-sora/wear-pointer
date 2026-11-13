package dev.wearpointer

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.IBinder
import android.util.Log

/**
 * Keeps sampling and the GATT server alive after the screen dims.
 * Wear OS may still suspend the process when the watch leaves the wrist.
 * Stage 0 should keep the app open and the display awake.
 */
class PointerService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val notification = notification()
        try {
            startForeground(
                NOTIFICATION_ID,
                notification,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE,
            )
        } catch (err: Exception) {
            Log.w(TAG, "foreground service not started", err)
        }
        PointerRuntime.ensureStarted(this)
        return START_STICKY
    }

    private fun notification(): Notification {
        val manager = getSystemService(NotificationManager::class.java)
        val channel = NotificationChannel(CHANNEL, "wear-pointer", NotificationManager.IMPORTANCE_LOW)
        manager.createNotificationChannel(channel)
        return Notification.Builder(this, CHANNEL)
            .setContentTitle("wear-pointer")
            .setContentText("Air mouse")
            .setSmallIcon(android.R.drawable.stat_sys_data_bluetooth)
            .setOngoing(true)
            .build()
    }

    private companion object {
        const val CHANNEL = "wear-pointer"
        const val NOTIFICATION_ID = 1
        const val TAG = "wear-pointer"
    }
}
