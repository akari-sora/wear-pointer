package dev.wearpointer.ble

import android.annotation.SuppressLint
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothClass
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothHidDevice
import android.bluetooth.BluetoothHidDeviceAppQosSettings
import android.bluetooth.BluetoothHidDeviceAppSdpSettings
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.Handler
import android.os.SystemClock
import android.util.Log
import dev.wearpointer.proto.Packets
import java.util.concurrent.Executor

/**
 * Standard Bluetooth HID Mouse transport (`BluetoothProfile.HID_DEVICE`).
 *
 * Zero desktop receiver required: pairs directly with Windows, macOS, Linux, iPadOS, or Android
 * as a standard 3-button + wheel Bluetooth mouse. All sensor-to-HID math runs on-watch inside
 * [HidMotionEngine], and [HidReportScheduler] coalesces `< 7 ms` or rate-limited motion reports
 * without ever coalescing or dropping discrete button transitions.
 */
class HidMouseTransport(
    private val context: Context,
    private val handler: Handler,
    private val listener: LinkListener,
    initialMode: Int = Packets.MODE_ABSOLUTE,
    initialDpi: Int = Packets.DEFAULT_DPI,
) : PointerTransport {
    private val engine = HidMotionEngine(initialMode = initialMode, initialDpi = initialDpi)
    private val scheduler = HidReportScheduler()
    private val handlerExecutor = Executor { command -> onHandler { command.run() } }
    private val flushRunnable = Runnable { flushScheduledReports() }
    private val postBondConnectRunnable = Runnable { tryAutoConnectHost(pluggedDevice = null) }

    private var proxy: BluetoothHidDevice? = null
    @Volatile private var hostDevice: BluetoothDevice? = null
    @Volatile private var appRegistered = false
    private var running = false
    private var radioOn = false
    private var receiverRegistered = false
    private var triedQosFallback = false
    private var registeredWithQos = false

    override val ready: Boolean
        get() = appRegistered && hostDevice != null

    private val radioReceiver = object : BroadcastReceiver() {
        override fun onReceive(ctx: Context, intent: Intent) {
            when (intent.action) {
                BluetoothAdapter.ACTION_STATE_CHANGED -> {
                    val state = intent.getIntExtra(BluetoothAdapter.EXTRA_STATE, BluetoothAdapter.ERROR)
                    onHandler {
                        when (state) {
                            BluetoothAdapter.STATE_OFF, BluetoothAdapter.STATE_TURNING_OFF -> {
                                radioOn = false
                                tearDownHid()
                                listener.onConnectionChanged(false)
                                listener.onStatus("Bluetooth is off")
                            }
                            BluetoothAdapter.STATE_ON -> {
                                radioOn = true
                                if (running) openHidProfile()
                            }
                        }
                    }
                }
                BluetoothDevice.ACTION_BOND_STATE_CHANGED -> {
                    val bondState = intent.getIntExtra(BluetoothDevice.EXTRA_BOND_STATE, BluetoothDevice.ERROR)
                    if (bondState != BluetoothDevice.BOND_BONDED) return
                    val device = intent.getParcelableExtra(
                        BluetoothDevice.EXTRA_DEVICE,
                        BluetoothDevice::class.java,
                    ) ?: return
                    onHandler {
                        if (!running || !appRegistered || hostDevice != null || isLikelyPhoneOrAudio(device)) {
                            return@onHandler
                        }
                        // Give the newly paired PC 1.5s to open the HID L2CAP channel first;
                        // step in only if it hasn't connected yet.
                        handler.removeCallbacks(postBondConnectRunnable)
                        handler.postDelayed(postBondConnectRunnable, POST_BOND_CONNECT_DELAY_MS)
                    }
                }
            }
        }
    }

    private val serviceListener = object : BluetoothProfile.ServiceListener {
        override fun onServiceConnected(profile: Int, rawProxy: BluetoothProfile) {
            if (profile != BluetoothProfile.HID_DEVICE) return
            val hid = rawProxy as? BluetoothHidDevice ?: return
            onHandler {
                if (!running || !radioOn) {
                    closeProxyOnly(hid)
                    return@onHandler
                }
                if (proxy != null && proxy !== hid) {
                    closeProxyOnly(proxy)
                }
                proxy = hid
                triedQosFallback = false
                registerHidApp(hid, useQos = true)
            }
        }

        override fun onServiceDisconnected(profile: Int) {
            if (profile != BluetoothProfile.HID_DEVICE) return
            onHandler {
                val wasReady = ready
                proxy = null
                appRegistered = false
                hostDevice = null
                resetLinkState()
                if (wasReady) listener.onConnectionChanged(false)
                if (running && radioOn) {
                    listener.onStatus("HID service reset")
                    openHidProfile()
                }
            }
        }
    }

    private val hidCallback = object : BluetoothHidDevice.Callback() {
        @SuppressLint("MissingPermission")
        override fun onAppStatusChanged(pluggedDevice: BluetoothDevice?, registered: Boolean) {
            if (!running) return
            if (!registered) {
                val hid = proxy
                if (radioOn && registeredWithQos && !triedQosFallback && hid != null) {
                    Log.w(TAG, "OEM rejected HID QoS asynchronously; retrying without QoS")
                    triedQosFallback = true
                    registerHidApp(hid, useQos = false)
                    return
                }
                val wasReady = ready
                appRegistered = false
                hostDevice = null
                resetLinkState()
                if (wasReady) listener.onConnectionChanged(false)
                if (radioOn) listener.onStatus("HID unavailable")
                return
            }

            appRegistered = true
            Log.i(TAG, "BluetoothHidDevice registered (qos=$registeredWithQos)")

            val hid = proxy ?: return
            val alreadyConnected = runCatching { hid.connectedDevices }
                .getOrDefault(emptyList())
                .firstOrNull { !isLikelyPhoneOrAudio(it) }
                ?: runCatching { hid.connectedDevices }.getOrDefault(emptyList()).firstOrNull()

            if (alreadyConnected != null) {
                adoptConnectedHost(alreadyConnected)
                return
            }

            listener.onStatus("HID ready — pair PC")
            tryAutoConnectHost(pluggedDevice)
        }

        @SuppressLint("MissingPermission")
        override fun onConnectionStateChanged(device: BluetoothDevice, state: Int) {
            Log.i(TAG, "HID host ${device.address} state=$state")
            if (!running) return
            when (state) {
                BluetoothProfile.STATE_CONNECTED -> adoptConnectedHost(device)
                BluetoothProfile.STATE_CONNECTING -> {
                    if (hostDevice == null) {
                        listener.onStatus("Connecting")
                    }
                }
                BluetoothProfile.STATE_DISCONNECTED -> {
                    if (hostDevice == device || hostDevice == null) {
                        val wasReady = ready
                        if (hostDevice == device) {
                            hostDevice = null
                        }
                        val remaining = runCatching { proxy?.connectedDevices }
                            .getOrNull()
                            ?.firstOrNull()
                        if (remaining != null) {
                            hostDevice = remaining
                        } else {
                            resetLinkState()
                            if (wasReady) listener.onConnectionChanged(false)
                            if (radioOn && appRegistered) {
                                listener.onStatus("HID ready — pair PC")
                            }
                        }
                    }
                }
            }
        }

        @SuppressLint("MissingPermission")
        override fun onGetReport(device: BluetoothDevice, type: Byte, id: Byte, bufferSize: Int) {
            val hid = proxy ?: return
            try {
                if (type == BluetoothHidDevice.REPORT_TYPE_INPUT &&
                    (id == 0.toByte() || id == HidMotionEngine.HID_REPORT_ID.toByte())
                ) {
                    hid.replyReport(
                        device,
                        BluetoothHidDevice.REPORT_TYPE_INPUT,
                        HidMotionEngine.HID_REPORT_ID.toByte(),
                        engine.currentReportBytes(),
                    )
                } else {
                    hid.reportError(device, BluetoothHidDevice.ERROR_RSP_UNSUPPORTED_REQ)
                }
            } catch (err: SecurityException) {
                Log.w(TAG, "onGetReport permission error", err)
            }
        }

        override fun onSetProtocol(device: BluetoothDevice, protocol: Byte) {
            Log.i(TAG, "HID protocol=$protocol for ${device.address}")
        }

        override fun onVirtualCableUnplug(device: BluetoothDevice) {
            Log.i(TAG, "HID virtual cable unplug for ${device.address}")
            if (hostDevice == device) {
                val wasReady = ready
                hostDevice = null
                resetLinkState()
                if (wasReady) listener.onConnectionChanged(false)
                if (running && radioOn && appRegistered) {
                    listener.onStatus("HID ready — pair PC")
                }
            }
        }
    }

    @SuppressLint("MissingPermission")
    override fun start() {
        if (running) return
        running = true
        registerRadioReceiver()
        val adapter = bluetoothAdapter()
        if (adapter == null || !adapter.isEnabled) {
            radioOn = false
            listener.onStatus("Bluetooth is off")
            return
        }
        radioOn = true
        openHidProfile()
    }

    override fun requestReconnect() {
        onHandler {
            if (!running || !radioOn) return@onHandler
            if (proxy == null) {
                openHidProfile()
            } else if (appRegistered && hostDevice == null) {
                tryAutoConnectHost(pluggedDevice = null)
            }
        }
    }

    /** Called on [handler] by `PointerRuntime`. */
    override fun send(packet: ByteArray, reliable: Boolean): Boolean {
        if (!ready) return false
        val reports = engine.process(packet)
        if (reports.isEmpty()) return true
        val nowMs = SystemClock.uptimeMillis()
        var nextDelay: Long? = null
        for (report in reports) {
            nextDelay = scheduler.submit(report, nowMs, ::transmitReport)
        }
        scheduleFlush(nextDelay)
        return true
    }

    override fun takeCounters(): SendCounters = scheduler.takeCounters()

    @SuppressLint("MissingPermission")
    override fun dropLinkForTest(reason: String) {
        onHandler {
            Log.w(TAG, "dropping HID link: $reason")
            val active = hostDevice
            hostDevice = null
            resetLinkState()
            if (active != null) {
                runCatching { proxy?.disconnect(active) }
            }
            listener.onLinkFailed(reason)
            listener.onConnectionChanged(false)
        }
    }

    override fun stop() {
        onHandler {
            running = false
            radioOn = false
            unregisterRadioReceiver()
            tearDownHid()
        }
    }

    private fun openHidProfile() {
        if (proxy != null) return
        val adapter = bluetoothAdapter()
        if (adapter == null || !adapter.isEnabled) {
            radioOn = false
            listener.onStatus("Bluetooth is off")
            return
        }
        listener.onStatus("Starting BT HID")
        val ok = runCatching {
            adapter.getProfileProxy(context, serviceListener, BluetoothProfile.HID_DEVICE)
        }.getOrElse { err ->
            Log.e(TAG, "getProfileProxy(HID_DEVICE) failed", err)
            false
        }
        if (!ok) {
            listener.onStatus("BT HID unsupported")
        }
    }

    @SuppressLint("MissingPermission")
    private fun registerHidApp(hid: BluetoothHidDevice, useQos: Boolean) {
        val sdp = BluetoothHidDeviceAppSdpSettings(
            SDP_NAME,
            SDP_DESCRIPTION,
            SDP_PROVIDER,
            BluetoothHidDevice.SUBCLASS1_MOUSE,
            HidMotionEngine.MOUSE_REPORT_DESCRIPTOR,
        )
        val outQos = if (useQos) {
            BluetoothHidDeviceAppQosSettings(
                BluetoothHidDeviceAppQosSettings.SERVICE_BEST_EFFORT,
                QOS_TOKEN_RATE,
                QOS_TOKEN_BUCKET_SIZE,
                QOS_PEAK_BANDWIDTH,
                QOS_LATENCY_US,
                BluetoothHidDeviceAppQosSettings.MAX,
            )
        } else {
            null
        }
        registeredWithQos = useQos
        val accepted = try {
            hid.registerApp(sdp, null, outQos, handlerExecutor, hidCallback)
        } catch (err: Exception) {
            Log.w(TAG, "registerApp(useQos=$useQos) threw", err)
            false
        }
        if (!accepted) {
            if (useQos && !triedQosFallback) {
                Log.w(TAG, "registerApp with QoS rejected; falling back to null QoS")
                triedQosFallback = true
                registerHidApp(hid, useQos = false)
            } else {
                listener.onStatus("HID register failed")
            }
        }
    }

    @SuppressLint("MissingPermission")
    private fun adoptConnectedHost(device: BluetoothDevice) {
        val wasConnected = hostDevice != null
        hostDevice = device
        resetLinkState()
        if (!wasConnected) {
            listener.onConnectionChanged(true)
        }
        listener.onStatus("Connected")
    }

    @SuppressLint("MissingPermission")
    private fun tryAutoConnectHost(pluggedDevice: BluetoothDevice?) {
        val hid = proxy ?: return
        if (!appRegistered || hostDevice != null) return

        if (pluggedDevice != null &&
            pluggedDevice.bondState == BluetoothDevice.BOND_BONDED &&
            !isLikelyPhoneOrAudio(pluggedDevice)
        ) {
            val ok = runCatching { hid.connect(pluggedDevice) }.getOrDefault(false)
            Log.i(TAG, "auto-connect plugged host ${pluggedDevice.address}: $ok")
            if (ok) return
        }

        val bonded = runCatching { bluetoothAdapter()?.bondedDevices }
            .getOrNull()
            .orEmpty()
            .filter { !isLikelyPhoneOrAudio(it) }
            .sortedByDescending { isComputerClass(it) }

        for (candidate in bonded) {
            val state = runCatching { hid.getConnectionState(candidate) }
                .getOrDefault(BluetoothProfile.STATE_DISCONNECTED)
            if (state == BluetoothProfile.STATE_CONNECTED) {
                adoptConnectedHost(candidate)
                return
            }
            if (state == BluetoothProfile.STATE_DISCONNECTED) {
                val ok = runCatching { hid.connect(candidate) }.getOrDefault(false)
                Log.i(TAG, "auto-connect bonded host ${candidate.address}: $ok")
                if (ok) return
            }
        }
    }

    @SuppressLint("MissingPermission")
    private fun isLikelyPhoneOrAudio(device: BluetoothDevice): Boolean {
        val major = runCatching { device.bluetoothClass?.majorDeviceClass }.getOrNull() ?: return false
        return major == BluetoothClass.Device.Major.PHONE ||
            major == BluetoothClass.Device.Major.AUDIO_VIDEO ||
            major == BluetoothClass.Device.Major.WEARABLE ||
            major == BluetoothClass.Device.Major.HEALTH
    }

    @SuppressLint("MissingPermission")
    private fun isComputerClass(device: BluetoothDevice): Boolean {
        val major = runCatching { device.bluetoothClass?.majorDeviceClass }.getOrNull() ?: return false
        return major == BluetoothClass.Device.Major.COMPUTER
    }

    @SuppressLint("MissingPermission")
    private fun transmitReport(report: HidReport): Boolean {
        val hid = proxy ?: return false
        val target = hostDevice ?: return false
        return try {
            hid.sendReport(target, HidMotionEngine.HID_REPORT_ID, report.toBytes())
        } catch (err: SecurityException) {
            Log.w(TAG, "sendReport permission error", err)
            false
        } catch (err: Exception) {
            Log.w(TAG, "sendReport failed", err)
            false
        }
    }

    private fun flushScheduledReports() {
        if (!ready) {
            scheduler.clear()
            return
        }
        val nextDelay = scheduler.flushDue(SystemClock.uptimeMillis(), ::transmitReport)
        scheduleFlush(nextDelay)
    }

    private fun scheduleFlush(delayMs: Long?) {
        handler.removeCallbacks(flushRunnable)
        if (delayMs != null && ready) {
            handler.postDelayed(flushRunnable, delayMs)
        }
    }

    private fun resetLinkState() {
        handler.removeCallbacks(flushRunnable)
        handler.removeCallbacks(postBondConnectRunnable)
        scheduler.clear()
        engine.resetLink()
    }

    @SuppressLint("MissingPermission")
    private fun tearDownHid() {
        val hid = proxy
        val active = hostDevice
        // Release any held button before tearing down the HID app so the host never keeps a stuck drag.
        if (hid != null && active != null && engine.heldButtons != 0) {
            runCatching {
                hid.sendReport(active, HidMotionEngine.HID_REPORT_ID, HidReport(0, 0, 0, 0).toBytes())
            }
        }
        hostDevice = null
        appRegistered = false
        resetLinkState()
        if (hid != null) {
            runCatching { hid.unregisterApp() }
                .onFailure { Log.w(TAG, "unregisterApp", it) }
            closeProxyOnly(hid)
        }
        proxy = null
    }

    private fun closeProxyOnly(hid: BluetoothHidDevice?) {
        if (hid == null) return
        runCatching {
            bluetoothAdapter()?.closeProfileProxy(BluetoothProfile.HID_DEVICE, hid)
        }.onFailure {
            Log.w(TAG, "closeProfileProxy", it)
        }
    }

    private fun bluetoothAdapter(): BluetoothAdapter? =
        context.getSystemService(BluetoothManager::class.java)?.adapter

    private fun onHandler(block: () -> Unit) {
        if (handler.looper.isCurrentThread) block() else handler.post(block)
    }

    private fun registerRadioReceiver() {
        if (receiverRegistered) return
        val filter = IntentFilter().apply {
            addAction(BluetoothAdapter.ACTION_STATE_CHANGED)
            addAction(BluetoothDevice.ACTION_BOND_STATE_CHANGED)
        }
        context.registerReceiver(radioReceiver, filter, Context.RECEIVER_NOT_EXPORTED)
        receiverRegistered = true
    }

    private fun unregisterRadioReceiver() {
        if (!receiverRegistered) return
        try {
            context.unregisterReceiver(radioReceiver)
        } catch (err: IllegalArgumentException) {
            Log.w(TAG, "receiver already unregistered", err)
        }
        receiverRegistered = false
    }

    private companion object {
        const val TAG = "wear-pointer"
        const val SDP_NAME = "Wear Pointer"
        const val SDP_DESCRIPTION = "Spatial BLE & HID Mouse"
        const val SDP_PROVIDER = "WearPointer"
        const val QOS_TOKEN_RATE = 800
        const val QOS_TOKEN_BUCKET_SIZE = 9
        const val QOS_PEAK_BANDWIDTH = 0
        const val QOS_LATENCY_US = 11_250
        const val POST_BOND_CONNECT_DELAY_MS = 1_500L
    }
}
