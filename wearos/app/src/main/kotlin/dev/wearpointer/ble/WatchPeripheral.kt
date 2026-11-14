package dev.wearpointer.ble

import android.annotation.SuppressLint
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattDescriptor
import android.bluetooth.BluetoothGattServer
import android.bluetooth.BluetoothGattServerCallback
import android.bluetooth.BluetoothGattService
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.bluetooth.BluetoothStatusCodes
import android.bluetooth.le.AdvertiseCallback
import android.bluetooth.le.AdvertiseData
import android.bluetooth.le.AdvertiseSettings
import android.bluetooth.le.BluetoothLeAdvertiser
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.Handler
import android.os.ParcelUuid
import android.util.Log
import dev.wearpointer.proto.Packets
import java.util.concurrent.ConcurrentHashMap

/**
 * Watch as GATT peripheral. The pointer characteristic is notify-only.
 * [notifyCharacteristicChanged]'s confirm flag stays false so the stack sends
 * a notification, which is what the CCCD bit-0 subscription enables.
 * [OutboundQueue] waits for [onNotificationSent] before the next packet.
 *
 * Threading: queue state, the in-flight bookkeeping, retries, and the
 * timeout all run on [handler] (the runtime thread that also builds packets).
 * Binder callbacks only answer the GATT request and post to [handler], so a
 * completion cannot race the `notifyCharacteristicChanged` call it belongs to.
 *
 * Advertising is restarted on disconnect, and the GATT server is torn down
 * and rebuilt when the adapter goes off and back on. A status string is not
 * evidence that advertising started.
 */
class WatchPeripheral(
    private val context: Context,
    private val handler: Handler,
    private val listener: LinkListener,
) : PointerTransport {
    private val subscribed = ConcurrentHashMap.newKeySet<BluetoothDevice>()
    private val queue = OutboundQueue()

    /** Devices that accepted the packet in flight and owe a callback. */
    private val awaiting = HashSet<BluetoothDevice>()
    private var flightOk = true
    private var retryPacket: ByteArray? = null
    private val notifyTimeout = Runnable { onNotifyTimeout() }
    private val retryRunnable = Runnable { retryNow() }
    private var server: BluetoothGattServer? = null
    private var advertiser: BluetoothLeAdvertiser? = null
    private var pointerChar: BluetoothGattCharacteristic? = null
    private var running = false
    private var radioOn = false
    private var receiverRegistered = false

    override val ready: Boolean
        get() = subscribed.isNotEmpty()

    private val radioReceiver = object : BroadcastReceiver() {
        override fun onReceive(ctx: Context, intent: Intent) {
            if (intent.action != BluetoothAdapter.ACTION_STATE_CHANGED) return
            val state = intent.getIntExtra(BluetoothAdapter.EXTRA_STATE, BluetoothAdapter.ERROR)
            onHandler {
                when (state) {
                    BluetoothAdapter.STATE_OFF, BluetoothAdapter.STATE_TURNING_OFF -> {
                        radioOn = false
                        tearDownRadio()
                        listener.onConnectionChanged(false)
                        listener.onStatus("Bluetooth is off")
                    }
                    BluetoothAdapter.STATE_ON -> {
                        radioOn = true
                        if (running) openServer()
                    }
                }
            }
        }
    }

    private val advertiseCallback = object : AdvertiseCallback() {
        override fun onStartSuccess(settingsInEffect: AdvertiseSettings) {
            Log.i(TAG, "advertising started")
            listener.onStatus("Advertising")
        }

        override fun onStartFailure(errorCode: Int) {
            val reason = "advertise failed ($errorCode)"
            Log.w(TAG, reason)
            if (errorCode == ADVERTISE_FAILED_ALREADY_STARTED) return
            if (errorCode == ADVERTISE_FAILED_FEATURE_UNSUPPORTED) {
                listener.onPeripheralUnsupported(reason)
            } else {
                listener.onStatus(reason)
            }
        }
    }

    private val gattCallback = object : BluetoothGattServerCallback() {
        override fun onConnectionStateChange(device: BluetoothDevice, status: Int, newState: Int) {
            Log.i(TAG, "connection ${device.address} state=$newState status=$status")
            if (newState != BluetoothProfile.STATE_DISCONNECTED) return
            onHandler {
                subscribed.remove(device)
                forgetDevice(device)
                listener.onConnectionChanged(ready)
                if (running && radioOn && subscribed.isEmpty()) restartAdvertising()
            }
        }

        override fun onServiceAdded(status: Int, service: BluetoothGattService) {
            onHandler {
                if (status != BluetoothGatt.GATT_SUCCESS) {
                    listener.onStatus("GATT service failed ($status)")
                } else {
                    restartAdvertising()
                }
            }
        }

        override fun onNotificationSent(device: BluetoothDevice, status: Int) {
            onHandler { onSent(device, status) }
        }

        override fun onCharacteristicReadRequest(
            device: BluetoothDevice,
            requestId: Int,
            offset: Int,
            characteristic: BluetoothGattCharacteristic,
        ) {
            server?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, byteArrayOf())
        }

        override fun onCharacteristicWriteRequest(
            device: BluetoothDevice,
            requestId: Int,
            characteristic: BluetoothGattCharacteristic,
            preparedWrite: Boolean,
            responseNeeded: Boolean,
            offset: Int,
            value: ByteArray?,
        ) {
            if (characteristic.uuid == Packets.CONTROL_UUID &&
                value?.firstOrNull()?.toInt() == Packets.CONTROL_REQUEST_STATS
            ) {
                listener.onStatsRequested()
            }
            if (responseNeeded) {
                server?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, value)
            }
        }

        override fun onDescriptorWriteRequest(
            device: BluetoothDevice,
            requestId: Int,
            descriptor: BluetoothGattDescriptor,
            preparedWrite: Boolean,
            responseNeeded: Boolean,
            offset: Int,
            value: ByteArray?,
        ) {
            if (responseNeeded) {
                server?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, value)
            }
            if (descriptor.uuid != Packets.CCCD_UUID) return
            // Bit 0 is notifications. Bit 1 would be indications, which this
            // characteristic does not send.
            val enable = value != null && value.isNotEmpty() && (value[0].toInt() and 0x01) != 0
            onHandler {
                val wasReady = ready
                if (enable) subscribed.add(device) else subscribed.remove(device)
                // A new subscription is a new link for the receiver: sequence starts at 0.
                if (!wasReady && ready) resetFlight()
                if (!enable) forgetDevice(device)
                Log.i(TAG, "notify ${if (enable) "on" else "off"} for ${device.address}")
                listener.onConnectionChanged(ready)
                if (ready) listener.onStatus("Connected")
            }
        }
    }

    @SuppressLint("MissingPermission")
    override fun start() {
        if (running) return
        running = true
        registerRadioReceiver()
        val adapter = context.getSystemService(BluetoothManager::class.java)?.adapter
        if (adapter == null || !adapter.isEnabled) {
            radioOn = false
            listener.onStatus("Bluetooth is off")
            return
        }
        radioOn = true
        openServer()
    }

    @SuppressLint("MissingPermission")
    private fun openServer() {
        val manager = context.getSystemService(BluetoothManager::class.java) ?: return
        val adapter = manager.adapter ?: return
        if (!adapter.isEnabled) {
            radioOn = false
            listener.onStatus("Bluetooth is off")
            return
        }
        try {
            adapter.name = "wear-pointer"
        } catch (err: SecurityException) {
            Log.w(TAG, "adapter name unchanged", err)
        }
        val advertiser = adapter.bluetoothLeAdvertiser
        if (advertiser == null) {
            listener.onPeripheralUnsupported("this radio has no BLE advertiser")
            return
        }
        this.advertiser = advertiser
        tearDownServerOnly()

        val pointer = BluetoothGattCharacteristic(
            Packets.POINTER_UUID,
            BluetoothGattCharacteristic.PROPERTY_READ or BluetoothGattCharacteristic.PROPERTY_NOTIFY,
            BluetoothGattCharacteristic.PERMISSION_READ,
        )
        pointer.addDescriptor(
            BluetoothGattDescriptor(
                Packets.CCCD_UUID,
                BluetoothGattDescriptor.PERMISSION_READ or BluetoothGattDescriptor.PERMISSION_WRITE,
            ),
        )
        val control = BluetoothGattCharacteristic(
            Packets.CONTROL_UUID,
            BluetoothGattCharacteristic.PROPERTY_WRITE or BluetoothGattCharacteristic.PROPERTY_WRITE_NO_RESPONSE,
            BluetoothGattCharacteristic.PERMISSION_WRITE,
        )
        val service = BluetoothGattService(Packets.SERVICE_UUID, BluetoothGattService.SERVICE_TYPE_PRIMARY)
        service.addCharacteristic(pointer)
        service.addCharacteristic(control)
        pointerChar = pointer

        val server = manager.openGattServer(context, gattCallback)
        if (server == null) {
            listener.onPeripheralUnsupported("openGattServer returned null")
            return
        }
        this.server = server
        server.addService(service)
    }

    @SuppressLint("MissingPermission")
    private fun restartAdvertising() {
        val advertiser = advertiser ?: return
        try {
            advertiser.stopAdvertising(advertiseCallback)
        } catch (err: Exception) {
            Log.w(TAG, "stopAdvertising before restart", err)
        }
        val settings = AdvertiseSettings.Builder()
            .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_LOW_LATENCY)
            .setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_HIGH)
            .setConnectable(true)
            .build()
        val data = AdvertiseData.Builder()
            .addServiceUuid(ParcelUuid(Packets.SERVICE_UUID))
            .setIncludeDeviceName(false)
            .build()
        val scanResponse = AdvertiseData.Builder()
            .setIncludeDeviceName(true)
            .build()
        advertiser.startAdvertising(settings, data, scanResponse, advertiseCallback)
    }

    /** Called on [handler] by the runtime. */
    override fun send(packet: ByteArray, reliable: Boolean): Boolean {
        if (!ready) return false
        dispatch(queue.enqueue(packet))
        return true
    }

    private fun dispatch(step: OutboundQueue.Transmit) {
        when (step) {
            is OutboundQueue.Transmit.Send -> transmit(step.packet)
            is OutboundQueue.Transmit.Retry -> {
                retryPacket = step.packet
                handler.removeCallbacks(retryRunnable)
                handler.postDelayed(retryRunnable, step.delayMs)
            }
            OutboundQueue.Transmit.DropLink -> dropLink("notify failed")
            OutboundQueue.Transmit.Idle -> Unit
        }
    }

    private fun retryNow() {
        val packet = retryPacket ?: return
        retryPacket = null
        transmit(packet)
    }

    @SuppressLint("MissingPermission")
    private fun transmit(packet: ByteArray) {
        val server = server
        val characteristic = pointerChar
        val devices = subscribed.toList()
        if (server == null || characteristic == null || devices.isEmpty()) {
            dispatch(queue.onApiRejected(retryable = false))
            return
        }
        awaiting.clear()
        flightOk = true
        var allBusy = true
        for (device in devices) {
            // confirm = false: notification, matching PROPERTY_NOTIFY and CCCD bit 0.
            val status = server.notifyCharacteristicChanged(device, characteristic, false, packet)
            if (status == BluetoothStatusCodes.SUCCESS) {
                awaiting.add(device)
            } else {
                Log.w(TAG, "notify status $status for ${device.address}")
                if (!GattStatus.isRetryable(status)) allBusy = false
            }
        }
        if (awaiting.isEmpty()) {
            handler.removeCallbacks(notifyTimeout)
            dispatch(queue.onApiRejected(retryable = allBusy))
            return
        }
        // With several centrals, one that refused this packet sees a sequence
        // gap. Stage 1 expects a single receiver.
        queue.markAccepted()
        handler.removeCallbacks(notifyTimeout)
        handler.postDelayed(notifyTimeout, NOTIFY_TIMEOUT_MS)
    }

    private fun onSent(device: BluetoothDevice, status: Int) {
        // A callback for a packet abandoned with a dropped link, or from a
        // device that did not accept this packet, is ignored.
        if (!awaiting.remove(device)) return
        flightOk = flightOk && status == BluetoothGatt.GATT_SUCCESS
        if (awaiting.isNotEmpty()) return
        handler.removeCallbacks(notifyTimeout)
        dispatch(queue.onFinished(flightOk))
    }

    private fun onNotifyTimeout() {
        if (awaiting.isEmpty()) return
        awaiting.clear()
        if (queue.onTimeout() is OutboundQueue.Transmit.DropLink) {
            dropLink("notify callback timed out")
        }
    }

    /**
     * [device] left (disconnect or unsubscribe). With nobody left, the link is
     * gone and the queue restarts. Otherwise its owed callback will not come:
     * delivery to it is unknown, so the flight finishes as a failure.
     */
    private fun forgetDevice(device: BluetoothDevice) {
        if (subscribed.isEmpty()) {
            resetFlight()
            return
        }
        if (!awaiting.remove(device)) return
        flightOk = false
        if (awaiting.isNotEmpty()) return
        handler.removeCallbacks(notifyTimeout)
        dispatch(queue.onFinished(flightOk))
    }

    /** Forget the packet in flight, any pending retry, and the queue. */
    private fun resetFlight() {
        handler.removeCallbacks(notifyTimeout)
        handler.removeCallbacks(retryRunnable)
        retryPacket = null
        awaiting.clear()
        queue.clear()
    }

    /**
     * End the link so the desktop sees a disconnect and releases buttons.
     *
     * `cancelConnection` alone does not end a link the central opened: on an
     * API 35 Wear emulator the stack logs `bta_gatts_cancel_open: failed` and
     * the central stays connected and subscribed, while this side has stopped
     * sending. Closing the GATT server releases this app's hold on the link,
     * so the server is closed and rebuilt; [onServiceAdded] restarts
     * advertising. Even if the link survived, the watch is silent, so the
     * desktop's 300 ms no-aim rule still releases a held drag.
     */
    @SuppressLint("MissingPermission")
    private fun dropLink(reason: String) {
        Log.w(TAG, "dropping link: $reason")
        val devices = subscribed.toList()
        subscribed.clear()
        resetFlight()
        for (device in devices) {
            try {
                server?.cancelConnection(device)
            } catch (err: Exception) {
                Log.w(TAG, "cancelConnection", err)
            }
        }
        tearDownServerOnly()
        listener.onLinkFailed(reason)
        listener.onConnectionChanged(false)
        if (running && radioOn) openServer()
    }

    override fun dropLinkForTest(reason: String) {
        onHandler { dropLink(reason) }
    }

    override fun takeCounters(): SendCounters {
        val raw = queue.takeCounters()
        return SendCounters(raw.accepted, raw.completed, raw.rejected, raw.coalesced)
    }

    @SuppressLint("MissingPermission")
    override fun stop() {
        onHandler {
            running = false
            radioOn = false
            unregisterRadioReceiver()
            tearDownRadio()
        }
    }

    @SuppressLint("MissingPermission")
    private fun tearDownRadio() {
        resetFlight()
        subscribed.clear()
        try {
            advertiser?.stopAdvertising(advertiseCallback)
        } catch (err: Exception) {
            Log.w(TAG, "stopAdvertising", err)
        }
        advertiser = null
        tearDownServerOnly()
    }

    private fun tearDownServerOnly() {
        server?.close()
        server = null
        pointerChar = null
    }

    private fun onHandler(block: () -> Unit) {
        if (handler.looper.isCurrentThread) block() else handler.post(block)
    }

    private fun registerRadioReceiver() {
        if (receiverRegistered) return
        val filter = IntentFilter(BluetoothAdapter.ACTION_STATE_CHANGED)
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
        const val NOTIFY_TIMEOUT_MS = 1_000L
    }
}
