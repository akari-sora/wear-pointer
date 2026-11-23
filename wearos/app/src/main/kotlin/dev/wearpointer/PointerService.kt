package dev.wearpointer

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.IBinder
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.wear.ongoing.OngoingActivity
import androidx.wear.ongoing.Status

/**
 * Keeps sampling and the GATT server alive after the screen dims.
 * Stop is explicit: the notification action and [PointerRuntime.stop] both
 * tear the runtime down. [START_NOT_STICKY] so a user stop is not undone by
 * the system. `onDestroy` is not guaranteed if the process is killed; the
 * fallback is `adb shell am force-stop dev.wearpointer`.
 *
 * While the service runs, the pointer session is a Wear OS Ongoing Activity:
 * the watch face shows its icon and the launcher lists it under Recents, and
 * a tap on either returns to [MainActivity]. Both go away with the service.
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
                ongoingNotification(),
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

    private fun ongoingNotification(): android.app.Notification {
        val manager = getSystemService(NotificationManager::class.java)
        val channel = NotificationChannel(CHANNEL, "wear-pointer", NotificationManager.IMPORTANCE_LOW)
        manager.createNotificationChannel(channel)
        val immutable = PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        val stop = PendingIntent.getService(
            this,
            0,
            Intent(this, PointerService::class.java).setAction(ACTION_STOP),
            immutable,
        )
        val open = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
            immutable,
        )
        val builder = NotificationCompat.Builder(this, CHANNEL)
            .setContentTitle("Wear Pointer")
            .setContentText("Air mouse running")
            .setSmallIcon(R.drawable.ic_stat_pointer)
            .setContentIntent(open)
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setOngoing(true)
            .addAction(android.R.drawable.ic_menu_close_clear_cancel, "Stop", stop)
        // Adds the Ongoing Activity data to the builder; the notification must be built after this.
        OngoingActivity.Builder(applicationContext, NOTIFICATION_ID, builder)
            .setStaticIcon(R.drawable.ic_stat_pointer)
            .setTouchIntent(open)
            .setStatus(Status.Builder().addTemplate("Air mouse running").build())
            .build()
            .apply(applicationContext)
        return builder.build()
    }

    companion object {
        const val ACTION_STOP = "dev.wearpointer.STOP"
        private const val CHANNEL = "wear-pointer"
        private const val NOTIFICATION_ID = 1
        private const val TAG = "wear-pointer"
    }
}
