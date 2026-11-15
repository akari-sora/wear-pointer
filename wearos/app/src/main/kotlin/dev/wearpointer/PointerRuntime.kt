package dev.wearpointer

import android.annotation.SuppressLint
import android.app.Activity
import android.bluetooth.BluetoothAdapter
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.ApplicationInfo
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
import android.provider.Settings
import android.util.Log
import dev.wearpointer.ble.HidMouseTransport
import dev.wearpointer.ble.LinkListener
import dev.wearpointer.ble.PointerTransport
import dev.wearpointer.ble.SendCounters
import dev.wearpointer.ble.WatchCentral
import dev.wearpointer.ble.WatchPeripheral
import dev.wearpointer.motion.AimTracker
import dev.wearpointer.motion.SensorGapMonitor
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
    enum class Role { WATCH_ADVERTISES, STANDARD_HID, DESKTOP_ADVERTISES }

    /**
     * Pointing model. [ABSOLUTE] is WatchPointer's laser (cursor = where the
     * 12 o'clock edge points). [AIR] turns wrist motion into cursor motion with
     * acceleration, so it never needs a Center. [TOUCH] makes the pad a
     * touchpad. The desktop does the math; the watch only says which one.
     */
    enum class Mode(val wire: Int, val label: String) {
        ABSOLUTE(Packets.MODE_ABSOLUTE, "Absolute"),
        AIR(Packets.MODE_AIR, "Air mouse"),
        TOUCH(Packets.MODE_TOUCH, "Touchpad"),
        ;

        fun next(): Mode = entries[(ordinal + 1) % entries.size]
    }

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
        val aimLeftCdeg: Int = 0,
        val aimUpCdeg: Int = 0,
        val dpi: Int = Packets.DEFAULT_DPI,
        val mode: Mode = Mode.ABSOLUTE,
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
    private var samples = 0
    private var dropped = 0
    private val gapMonitor = SensorGapMonitor(SENSOR_GAP_NS)
    private var windowStartNs = 0L
    private var lastUiAimNs = 0L
    private var sensorHzX10 = 0
    private var infoTicks = 0
    private var pendingScroll = 0.0
    private var statsRequested = false
    private var debugReceiver: BroadcastReceiver? = null

    // Touchpad state, written from the UI thread, read on the sensor thread.
    @Volatile private var touchXCentiMm = 0
    @Volatile private var touchYCentiMm = 0
    @Volatile private var touchDown = false

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
        val dpi = loadDpi(app)
        val mode = loadMode(app)
        _state.update { it.copy(role = role, dpi = dpi, mode = mode, bluetoothGranted = hasPermissions(app)) }
        handler?.post {
            startSensors()
            openTransport(role)
        }
        registerDebugReceiver(app)
        started = true
    }

    /**
     * Debuggable builds only. `adb shell am broadcast -a dev.wearpointer.DEBUG_DROP_LINK`
     * runs the same drop path as a failed notify, so Stage 0 and the emulator
     * harness can check that the desktop releases a held drag.
     */
    private fun registerDebugReceiver(context: Context) {
        if (debugReceiver != null) return
        if (context.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE == 0) return
        val receiver = object : BroadcastReceiver() {
            override fun onReceive(ctx: Context, intent: Intent) {
                handler?.post { transport?.dropLinkForTest("debug drop") }
            }
        }
        context.registerReceiver(receiver, IntentFilter(ACTION_DEBUG_DROP_LINK), Context.RECEIVER_EXPORTED)
        debugReceiver = receiver
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
            if (started) {
                openTransport(role)
            } else {
                _state.update { it.copy(status = "Stopped") }
            }
        }
    }

    /**
     * Makes the watch Bluetooth-discoverable for 120s and nudges [HidMouseTransport]
     * to reconnect to any already-bonded desktop host.
     */
    @SuppressLint("MissingPermission")
    fun requestPairable(context: Context) {
        handler?.post {
            transport?.requestReconnect()
        }
        val discoverable = Intent(BluetoothAdapter.ACTION_REQUEST_DISCOVERABLE).apply {
            putExtra(BluetoothAdapter.EXTRA_DISCOVERABLE_DURATION, DISCOVERABLE_DURATION_SEC)
            if (context !is Activity) {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
        }
        try {
            context.startActivity(discoverable)
            onStatus("Discoverable for 2 min")
        } catch (err: Exception) {
            Log.w(TAG, "ACTION_REQUEST_DISCOVERABLE unavailable; opening BT settings", err)
            runCatching {
                val btSettings = Intent(Settings.ACTION_BLUETOOTH_SETTINGS).apply {
                    if (context !is Activity) {
                        addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                    }
                }
                context.startActivity(btSettings)
            }.onFailure {
                onStatus("Pair watch in PC Bluetooth")
            }
        }
    }

    fun setDpi(requestedDpi: Int) {
        val clamped = ((requestedDpi + Packets.DPI_STEP / 2) / Packets.DPI_STEP * Packets.DPI_STEP)
            .coerceIn(Packets.MIN_DPI, Packets.MAX_DPI)
        appContext?.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            ?.edit()
            ?.putInt(KEY_DPI, clamped)
            ?.apply()
        _state.update { it.copy(dpi = clamped) }
        Log.i(TAG, "dpi=$clamped")
        handler?.post {
            if (transport?.ready == true) {
                sendSensitivity(clamped)
            }
        }
    }

    fun setMode(mode: Mode) {
        appContext?.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            ?.edit()
            ?.putString(KEY_MODE, mode.name)
            ?.apply()
        _state.update { it.copy(mode = mode) }
        Log.i(TAG, "mode=${mode.name}")
        handler?.post {
            touchDown = false
            if (transport?.ready == true) sendMode(mode)
        }
    }

    /** Touchpad finger travel since the last call, in millimetres. UI thread. */
    fun touchMove(dxMm: Float, dyMm: Float) {
        touchXCentiMm += kotlin.math.round(dxMm * 100f).toInt()
        touchYCentiMm += kotlin.math.round(dyMm * 100f).toInt()
    }

    fun touchFinger(down: Boolean) {
        touchDown = down
    }

    fun stepDpi(steps: Int) {
        if (steps == 0) return
        setDpi(_state.value.dpi + steps * Packets.DPI_STEP)
    }

    /**
     * Pause: no aim is built or sent, the wake lock is released, the sensor
     * listener stays so the heading keeps integrating while the CPU is awake.
     * The aim waiting in the queue is dropped by the Pause packet.
     *
     * Resume: if [gapMonitor] saw a hole in the gyro timestamps while paused,
     * the heading is rebased with a Center after the Resume packet. A hole
     * that only shows up once a late FIFO is delivered after resume is caught
     * by the active-path check in [onSensorChanged].
     */
    fun togglePause() {
        handler?.post {
            paused = !paused
            if (paused) {
                releaseWakeLock()
                send(Packets.pause(0, nowUs(), true), reliable = true)
                _state.update { it.copy(paused = true, status = "Paused — tracks while awake") }
                return@post
            }
            holdWakeLock()
            send(Packets.pause(0, nowUs(), false), reliable = true)
            if (gapMonitor.onResume()) {
                rebase(nowUs())
            } else {
                _state.update { it.copy(paused = false, status = if (it.connected) "Connected" else it.status) }
            }
            _state.update { it.copy(paused = false) }
        }
    }

    /**
     * New calibration base on both ends. The queue drops the aim built on the
     * old one. Without a link only the watch rebases; the desktop has no base
     * of its own and maps the next aim it receives.
     */
    private fun rebase(timestampUs: Long) {
        tracker.center()
        _state.update { it.copy(aimLeftCdeg = 0, aimUpCdeg = 0) }
        if (transport?.ready != true) return
        // Relative modes have no base to lose: the desktop drops the one-off
        // jump in the aim angles, and a Center would warp the cursor.
        if (_state.value.mode != Mode.ABSOLUTE) return
        send(Packets.center(0, timestampUs), reliable = true)
        Log.i(TAG, "Sensor gap — recentered")
        _state.update { it.copy(status = "Sensor gap — recentered; press Center if the cursor is off") }
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
        debugReceiver?.let { receiver ->
            runCatching { appContext?.unregisterReceiver(receiver) }
        }
        debugReceiver = null
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
        samples = 0
        lastUiAimNs = 0L
        gapMonitor.reset()
        _state.update {
            it.copy(
                status = "Stopped",
                connected = false,
                paused = false,
                samplesPerSec = 0,
                sentPerSec = 0,
                droppedPerSec = 0,
                aimLeftCdeg = 0,
                aimUpCdeg = 0,
            )
        }
    }

    fun center() {
        handler?.post {
            tracker.center()
            // Center is a calibration boundary: the queue drops aims still waiting.
            send(Packets.center(0, nowUs()), reliable = true)
            _state.update {
                it.copy(
                    aimLeftCdeg = 0,
                    aimUpCdeg = 0,
                    status = if (it.connected && it.status.startsWith("Sensor gap")) "Connected" else it.status,
                )
            }
        }
    }

    fun rightClick() {
        val down = SystemClock.elapsedRealtimeNanos() - 120_000_000L
        handler?.post {
            send(Packets.rightClick(0, nanosToUs(down)), reliable = true)
        }
    }

    fun leftClick(downNs: Long) {
        handler?.post {
            send(Packets.leftClick(0, nanosToUs(downNs)), reliable = true)
        }
    }

    fun drag(down: Boolean, eventNs: Long) {
        handler?.post {
            send(Packets.drag(0, nanosToUs(eventNs), down), reliable = true)
        }
    }

    fun scrollBy(pixels: Float) {
        handler?.post {
            pendingScroll += pixels.toDouble()
            val whole = pendingScroll.toInt().coerceIn(Short.MIN_VALUE.toInt(), Short.MAX_VALUE.toInt())
            if (whole == 0) return@post
            pendingScroll -= whole
            send(Packets.scroll(0, nowUs(), whole), reliable = false)
        }
    }

    @SuppressLint("MissingPermission")
    private fun openTransport(role: Role) {
        val context = appContext ?: return
        if (transport != null) return
        if (!hasPermissions(context)) {
            _state.update { it.copy(bluetoothGranted = false, status = "Bluetooth permission needed") }
            return
        }
        _state.update { it.copy(bluetoothGranted = true, status = "Starting radio") }
        val cur = _state.value
        val next: PointerTransport = when (role) {
            Role.WATCH_ADVERTISES -> WatchPeripheral(context, handler ?: return, this)
            Role.STANDARD_HID -> HidMouseTransport(
                context = context,
                handler = handler ?: return,
                listener = this,
                initialMode = cur.mode.wire,
                initialDpi = cur.dpi,
            )
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
                val hole = gapMonitor.onSample(event.timestamp, paused)
                tracker.push(event.timestamp, gravity, event.values)
                samples++
                if (hole) rebase(nanosToUs(event.timestamp))
                val left = tracker.leftCdeg()
                val up = tracker.upCdeg()
                if (!paused && transport?.ready == true) {
                    // Touch mode reuses the 100 Hz sensor clock as its cadence, so
                    // the desktop's silence rules (drag release, reconnect) work
                    // unchanged while a finger rests on the pad.
                    val packet = if (_state.value.mode == Mode.TOUCH) {
                        Packets.touch(0, nanosToUs(event.timestamp), touchXCentiMm, touchYCentiMm, touchDown)
                    } else {
                        Packets.aim(0, nanosToUs(event.timestamp), left, up)
                    }
                    send(packet, reliable = false)
                }
                if (event.timestamp - lastUiAimNs >= UI_AIM_INTERVAL_NS) {
                    val cur = _state.value
                    if (kotlin.math.abs(left - cur.aimLeftCdeg) >= 8 || kotlin.math.abs(up - cur.aimUpCdeg) >= 8) {
                        lastUiAimNs = event.timestamp
                        _state.update { it.copy(aimLeftCdeg = left, aimUpCdeg = up) }
                    }
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
                    0,
                    nowUs(),
                    samples,
                    counts.completed,
                    counts.rejected,
                    windowMs,
                ),
                reliable = true,
            )
            infoTicks++
            if (infoTicks == 1 || infoTicks % 60 == 0) {
                sendInfo()
                sendSensitivity(_state.value.dpi)
                sendMode(_state.value.mode)
            }
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
                "rejected=${counts.rejected} coalesced=${counts.coalesced} notReady=$dropped " +
                "gyroGapsOver50ms=${gapMonitor.takeSmallGaps()} windowMs=$windowMs",
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
            Packets.info(0, nowUs(), pct, charging, sensorHzX10, Build.MODEL ?: "watch"),
            reliable = true,
        )
    }

    private fun sendSensitivity(dpi: Int) {
        send(
            Packets.sensitivity(0, nowUs(), dpi),
            reliable = true,
        )
    }

    private fun sendMode(mode: Mode) {
        send(Packets.mode(0, nowUs(), mode.wire), reliable = true)
    }

    private fun send(packet: ByteArray, reliable: Boolean) {
        val transport = transport
        if (transport == null || !transport.ready) {
            dropped++
            return
        }
        if (!transport.send(packet, reliable)) dropped++
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
            infoTicks = 0
            sendInfo()
            sendSensitivity(_state.value.dpi)
            sendMode(_state.value.mode)
            if (paused) send(Packets.pause(0, nowUs(), true), reliable = true)
        }
        _state.update { it.copy(connected = connected) }
    }

    override fun onLinkFailed(reason: String) {
        Log.w(TAG, reason)
        val update = {
            _state.update { it.copy(connected = false, status = "Link dropped — $reason") }
        }
        val handler = handler
        if (handler == null || handler.looper.isCurrentThread) update() else handler.post(update)
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
                status = "Watch cannot advertise BLE. Switch to BT HID.",
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
            // Checked again on the handler: ensureStarted posts the first
            // openTransport, and a second GATT server would advertise the
            // service twice.
            handler?.post { if (transport == null) openTransport(_state.value.role) }
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

    private fun loadDpi(context: Context): Int {
        val saved = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .getInt(KEY_DPI, Packets.DEFAULT_DPI)
        return saved.coerceIn(Packets.MIN_DPI, Packets.MAX_DPI)
    }

    private fun loadMode(context: Context): Mode {
        val name = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString(KEY_MODE, null)
        return runCatching { Mode.valueOf(name ?: "") }.getOrDefault(Mode.ABSOLUTE)
    }

    private const val ACTION_DEBUG_DROP_LINK = "dev.wearpointer.DEBUG_DROP_LINK"
    private const val DISCOVERABLE_DURATION_SEC = 120
    private const val PREFS = "wear-pointer"
    private const val KEY_ROLE = "role"
    private const val KEY_DPI = "dpi"
    private const val KEY_MODE = "mode"
    private const val SAMPLE_US = 10_000
    private const val UI_AIM_INTERVAL_NS = 33_000_000L
    private const val WAKE_MS = 10 * 60 * 1000L
    /** Longer than this between gyro samples means events were dropped while asleep. */
    private const val SENSOR_GAP_NS = 500_000_000L
    private const val TAG = "wear-pointer"
}
