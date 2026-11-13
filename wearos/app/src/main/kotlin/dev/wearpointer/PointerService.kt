package dev.wearpointer

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.graphics.drawable.Icon
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.IBinder
import android.util.Log

/**
 * Keeps sampling and the GATT server alive after the screen dims.
 * Stop is explicit: the notification action and [PointerRuntime.stop] both
 * tear the runtime down. [START_NOT_STICKY] so a user stop is not undone by
 * the system. `onDestroy` is not guaranteed if the process is killed; the
 * fallback is `adb shell am force-stop dev.wearpointer`.
 */
class PointerService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            PointerRuntime.teardown()
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
            return START_NOT_STICKY
        }
        return try {
            startForeground(
                NOTIFICATION_ID,
                notification(),
                ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE,
            )
            PointerRuntime.ensureStarted(this)
            START_NOT_STICKY
        } catch (err: Exception) {
            Log.e(TAG, "foreground service failed; not sampling", err)
            PointerRuntime.teardown()
            stopSelf()
            START_NOT_STICKY
        }
    }

    override fun onDestroy() {
        PointerRuntime.teardown()
        super.onDestroy()
    }

    private fun notification(): Notification {
        val manager = getSystemService(NotificationManager::class.java)
        val channel = NotificationChannel(CHANNEL, "wear-pointer", NotificationManager.IMPORTANCE_LOW)
        manager.createNotificationChannel(channel)
        val stop = android.app.PendingIntent.getService(
            this,
            0,
            Intent(this, PointerService::class.java).setAction(ACTION_STOP),
            android.app.PendingIntent.FLAG_IMMUTABLE or android.app.PendingIntent.FLAG_UPDATE_CURRENT,
        )
        return Notification.Builder(this, CHANNEL)
            .setContentTitle("wear-pointer")
            .setContentText("Air mouse")
            .setSmallIcon(android.R.drawable.stat_sys_data_bluetooth)
            .setOngoing(true)
            .addAction(
                Notification.Action.Builder(
                    Icon.createWithResource(this, android.R.drawable.ic_menu_close_clear_cancel),
                    "Stop",
                    stop,
                ).build(),
            )
            .build()
    }

    companion object {
        const val ACTION_STOP = "dev.wearpointer.STOP"
        private const val CHANNEL = "wear-pointer"
        private const val NOTIFICATION_ID = 1
        private const val TAG = "wear-pointer"
    }
}
