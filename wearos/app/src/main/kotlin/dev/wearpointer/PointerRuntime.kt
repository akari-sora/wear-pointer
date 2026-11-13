package dev.wearpointer

import android.annotation.SuppressLint
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.os.BatteryManager
import android.os.Build
import android.os.Handler
import android.os.HandlerThread
import android.os.PowerManager
import android.os.SystemClock
import android.util.Log
import dev.wearpointer.ble.LinkListener
import dev.wearpointer.ble.PointerTransport
import dev.wearpointer.ble.SendCounters
import dev.wearpointer.ble.WatchCentral
import dev.wearpointer.ble.WatchPeripheral
import dev.wearpointer.motion.AimTracker
import dev.wearpointer.proto.Packets
import dev.wearpointer.proto.nanosToUs
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update

/**
 * Sensors, aim integration, and the active Bluetooth role.
 *
 * Sampling is requested at 100 Hz. The desktop cursor loop runs at 240 Hz.
 * Both rates are WatchPointer's. What the radio actually delivers is what
 * Stage 0 measures.
 */
object PointerRuntime : SensorEventListener, LinkListener {
    enum class Role { WATCH_ADVERTISES, DESKTOP_ADVERTISES }

    data class UiState(
        val role: Role = Role.WATCH_ADVERTISES,
        val status: String = "Starting",
        val connected: Boolean = false,
        val paused: Boolean = false,
        val peripheralUnsupported: Boolean = false,
        val samplesPerSec: Int = 0,
        val sentPerSec: Int = 0,
        val droppedPerSec: Int = 0,
        val bluetoothGranted: Boolean = false,
    )

    private val _state = MutableStateFlow(UiState())
    val state: StateFlow<UiState> = _state.asStateFlow()

    private var appContext: Context? = null
    private var sensorManager: SensorManager? = null
    private var thread: HandlerThread? = null
    private var handler: Handler? = null
    private var wakeLock: PowerManager.WakeLock? = null
    private var transport: PointerTransport? = null
    private val tracker = AimTracker()
    private val gravity = FloatArray(3)
    private var haveGravity = false
    private var started = false
    private var paused = false
    private var sequence = 0
    private var samples = 0
    private var dropped = 0
    private var windowStartNs = 0L
    private var sensorHzX10 = 0
    private var infoTicks = 0
    private var pendingScroll = 0.0
    private var statsRequested = false

    @Synchronized
    fun ensureStarted(context: Context) {
        if (started) {
            refreshPermissionFlag(context)
            return
        }
        val app = context.applicationContext
        appContext = app
        val thread = HandlerThread("wear-pointer").also { it.start() }
        this.thread = thread
        handler = Handler(thread.looper)
        sensorManager = app.getSystemService(SensorManager::class.java)
        holdWakeLock()
        val role = loadRole(app)
        _state.update { it.copy(role = role, bluetoothGranted = hasPermissions(app)) }
        handler?.post {
            startSensors()
            openTransport(role)
        }
        started = true
    }

    fun setRole(role: Role) {
        val app = appContext ?: return
        app.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .edit()
            .putString(KEY_ROLE, role.name)
            .apply()
        _state.update { it.copy(role = role, peripheralUnsupported = false, connected = false) }
        handler?.post {
            transport?.stop()
            transport = null
            sequence = 0
            openTransport(role)
        }
    }

    fun togglePause() {
        handler?.post {
            paused = !paused
            if (paused) releaseWakeLock() else holdWakeLock()
            _state.update { it.copy(paused = paused) }
            send(Packets.pause(nextSequence(), nowUs(), paused), reliable = true)
        }
    }

    /**
     * Unregisters sensors, stops advertising, closes the GATT server, releases
     * the wake lock, and stops the service. Idempotent.
     * If this process is killed without running this path:
     * `adb shell am force-stop dev.wearpointer`
     */
    @Synchronized
    fun stop() {
        val context = appContext
        teardown()
        context?.stopService(Intent(context, PointerService::class.java))
    }

    @Synchronized
    fun teardown() {
        if (!started && transport == null && wakeLock == null && thread == null) {
            _state.update { it.copy(status = "Stopped", connected = false, paused = false) }
            return
        }
        started = false
        paused = false
        sensorManager?.unregisterListener(this)
        try {
            transport?.stop()
        } catch (err: Exception) {
            Log.w(TAG, "transport stop", err)
        }
        transport = null
        releaseWakeLock()
        wakeLock = null
        thread?.quitSafely()
        thread = null
        handler = null
        sequence = 0
        samples = 0
        _state.update {
            it.copy(
                status = "Stopped",
                connected = false,
                paused = false,
                samplesPerSec = 0,
                sentPerSec = 0,
                droppedPerSec = 0,
            )
        }
    }

    fun center() {
        handler?.post {
            tracker.center()
            send(Packets.center(nextSequence(), nowUs()), reliable = true)
        }
    }

    fun rightClick() {
        val down = SystemClock.elapsedRealtimeNanos() - 120_000_000L
        handler?.post {
            send(Packets.rightClick(nextSequence(), nanosToUs(down)), reliable = true)
        }
    }

    fun leftClick(downNs: Long) {
        handler?.post {
            send(Packets.leftClick(nextSequence(), nanosToUs(downNs)), reliable = true)
        }
    }

    fun drag(down: Boolean, eventNs: Long) {
        handler?.post {
            send(Packets.drag(nextSequence(), nanosToUs(eventNs), down), reliable = true)
        }
    }

    fun scrollBy(pixels: Float) {
        handler?.post {
            pendingScroll += pixels.toDouble()
            val whole = pendingScroll.toInt().coerceIn(Short.MIN_VALUE.toInt(), Short.MAX_VALUE.toInt())
            if (whole == 0) return@post
            pendingScroll -= whole
            send(Packets.scroll(nextSequence(), nowUs(), whole), reliable = false)
        }
    }

    @SuppressLint("MissingPermission")
    private fun openTransport(role: Role) {
        val context = appContext ?: return
        if (!hasPermissions(context)) {
            _state.update { it.copy(bluetoothGranted = false, status = "Bluetooth permission needed") }
            return
        }
        _state.update { it.copy(bluetoothGranted = true, status = "Starting radio") }
        val next: PointerTransport = when (role) {
            Role.WATCH_ADVERTISES -> WatchPeripheral(context, this)
            Role.DESKTOP_ADVERTISES -> WatchCentral(context, handler ?: return, this)
        }
        transport = next
        try {
            next.start()
        } catch (err: SecurityException) {
            Log.e(TAG, "bluetooth start", err)
            _state.update { it.copy(status = "Bluetooth permission denied") }
        }
    }

    private fun startSensors() {
        val sensors = sensorManager ?: return
        val gyro = sensors.getDefaultSensor(Sensor.TYPE_GYROSCOPE)
        val gravitySensor = sensors.getDefaultSensor(Sensor.TYPE_GRAVITY)
        if (gyro == null || gravitySensor == null) {
            _state.update { it.copy(status = "Gyroscope or gravity sensor missing") }
            return
        }
        val handler = handler ?: return
        sensors.registerListener(this, gyro, SAMPLE_US, 0, handler)
        sensors.registerListener(this, gravitySensor, SAMPLE_US, 0, handler)
        Log.i(TAG, "sensors registered at ${1_000_000 / SAMPLE_US} Hz request")
    }

    override fun onSensorChanged(event: SensorEvent) {
        if (!started) return
        when (event.sensor.type) {
            Sensor.TYPE_GRAVITY -> {
                gravity[0] = event.values[0]
                gravity[1] = event.values[1]
                gravity[2] = event.values[2]
                haveGravity = true
            }
            Sensor.TYPE_GYROSCOPE -> {
                if (!haveGravity) return
                tracker.push(event.timestamp, gravity, event.values)
                samples++
                if (!paused && transport?.ready == true) {
                    send(
                        Packets.aim(nextSequence(), nanosToUs(event.timestamp), tracker.leftCdeg(), tracker.upCdeg()),
                        reliable = false,
                    )
                }
                flushWindow(event.timestamp)
            }
        }
    }

    override fun onAccuracyChanged(sensor: Sensor?, accuracy: Int) = Unit

    private fun flushWindow(nowNs: Long) {
        if (windowStartNs == 0L) windowStartNs = nowNs
        val elapsed = nowNs - windowStartNs
        if (elapsed < 1_000_000_000L && !statsRequested) return
        val windowMs = (elapsed / 1_000_000L).toInt().coerceIn(1, 65_535)
        sensorHzX10 = (samples * 10_000L / (elapsed / 1_000_000L).coerceAtLeast(1)).toInt().coerceIn(0, 65_535)
        val counts = transport?.takeCounters() ?: SendCounters()
        if (transport?.ready == true) {
            send(
                Packets.stats(
                    nextSequence(),
                    nowUs(),
                    samples,
                    counts.completed,
                    counts.rejected,
                    windowMs,
                ),
                reliable = true,
            )
            infoTicks++
            if (infoTicks == 1 || infoTicks % 60 == 0) sendInfo()
        }
        if (!paused) holdWakeLock()
        _state.update {
            it.copy(
                samplesPerSec = samples,
                sentPerSec = counts.completed,
                droppedPerSec = counts.rejected + dropped,
            )
        }
        Log.i(
            TAG,
            "samples=$samples accepted=${counts.accepted} completed=${counts.completed} " +
                "rejected=${counts.rejected} coalesced=${counts.coalesced} notReady=$dropped windowMs=$windowMs",
        )
        samples = 0
        dropped = 0
        windowStartNs = nowNs
        statsRequested = false
    }

    private fun sendInfo() {
        val context = appContext ?: return
        val battery = context.registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED))
        val level = battery?.getIntExtra(BatteryManager.EXTRA_LEVEL, -1) ?: -1
        val scale = battery?.getIntExtra(BatteryManager.EXTRA_SCALE, 100) ?: 100
        val pct = if (level >= 0 && scale > 0) level * 100 / scale else 255
        val status = battery?.getIntExtra(BatteryManager.EXTRA_STATUS, -1) ?: -1
        val charging = status == BatteryManager.BATTERY_STATUS_CHARGING ||
            status == BatteryManager.BATTERY_STATUS_FULL
        send(
            Packets.info(nextSequence(), nowUs(), pct, charging, sensorHzX10, Build.MODEL ?: "watch"),
            reliable = true,
        )
    }

    private fun send(packet: ByteArray, reliable: Boolean) {
        val transport = transport
        if (transport == null || !transport.ready) {
            dropped++
            return
        }
        if (!transport.send(packet, reliable)) dropped++
    }

    private fun nextSequence(): Int {
        val current = sequence
        sequence = (sequence + 1) and 0xFFFF
        return current
    }

    private fun nowUs(): Long = nanosToUs(SystemClock.elapsedRealtimeNanos())

    override fun onConnectionChanged(connected: Boolean) {
        val handler = handler
        if (handler == null || handler.looper.isCurrentThread) {
            applyConnection(connected)
        } else {
            handler.post { applyConnection(connected) }
        }
    }

    private fun applyConnection(connected: Boolean) {
        if (connected) {
            sequence = 0
            infoTicks = 0
            sendInfo()
        }
        _state.update { it.copy(connected = connected) }
    }

    override fun onStatus(text: String) {
        Log.i(TAG, text)
        _state.update { it.copy(status = text) }
    }

    override fun onPeripheralUnsupported(reason: String) {
        Log.w(TAG, "peripheral unsupported: $reason")
        _state.update {
            it.copy(
                peripheralUnsupported = true,
                status = "Watch cannot advertise. Switch to desktop advertiser.",
            )
        }
    }

    override fun onStatsRequested() {
        handler?.post { statsRequested = true }
    }

    @SuppressLint("WakelockTimeout")
    private fun holdWakeLock() {
        if (!started) return
        val context = appContext ?: return
        val power = context.getSystemService(PowerManager::class.java) ?: return
        val lock = wakeLock ?: power.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "wear-pointer:sensors").also {
            it.setReferenceCounted(false)
            wakeLock = it
        }
        if (lock.isHeld) lock.release()
        lock.acquire(WAKE_MS)
    }

    private fun releaseWakeLock() {
        val lock = wakeLock ?: return
        if (lock.isHeld) lock.release()
    }

    private fun refreshPermissionFlag(context: Context) {
        val granted = hasPermissions(context)
        _state.update { it.copy(bluetoothGranted = granted) }
        if (granted && transport == null) {
            handler?.post { openTransport(_state.value.role) }
        }
    }

    private fun hasPermissions(context: Context): Boolean {
        val needed = arrayOf(
            android.Manifest.permission.BLUETOOTH_CONNECT,
            android.Manifest.permission.BLUETOOTH_ADVERTISE,
            android.Manifest.permission.BLUETOOTH_SCAN,
        )
        return needed.all {
            context.checkSelfPermission(it) == PackageManager.PERMISSION_GRANTED
        }
    }

    private fun loadRole(context: Context): Role {
        val name = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString(KEY_ROLE, null)
        return runCatching { Role.valueOf(name ?: "") }.getOrDefault(Role.WATCH_ADVERTISES)
    }

    private const val PREFS = "wear-pointer"
    private const val KEY_ROLE = "role"
    private const val SAMPLE_US = 10_000
    private const val WAKE_MS = 10 * 60 * 1000L
    private const val TAG = "wear-pointer"
}
