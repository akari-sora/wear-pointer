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
import android.os.Looper
import android.os.ParcelUuid
import android.util.Log
import dev.wearpointer.proto.Packets
import java.util.concurrent.ConcurrentHashMap

/**
 * Watch as GATT peripheral. The pointer characteristic is notify-only.
 * [notifyCharacteristicChanged]'s confirm flag stays false so the stack sends
 * a notification, which is what the CCCD bit-0 subscription enables.
 * [OutboundQueue] waits for [onNotificationSent] before the next packet.
 * Aim coalesces; clicks and drags do not.
 *
 * Advertising stops while a central is connected. It is started again on
 * disconnect, and the GATT server is torn down and rebuilt when the adapter
 * goes off and back on. A status string is not evidence that advertising started.
 */
class WatchPeripheral(
    private val context: Context,
    private val listener: LinkListener,
) : PointerTransport {
    private val subscribed = ConcurrentHashMap.newKeySet<BluetoothDevice>()
    private val queue = OutboundQueue()
    private val lock = Any()
    private var callbacksLeft = 0
    private var flightOk = true
    private val mainHandler = Handler(Looper.getMainLooper())
    private val notifyTimeout = Runnable { onNotifyTimeout() }
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
            when (intent.getIntExtra(BluetoothAdapter.EXTRA_STATE, BluetoothAdapter.ERROR)) {
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
            if (newState == BluetoothProfile.STATE_DISCONNECTED) {
                subscribed.remove(device)
                synchronized(lock) { queue.clear() }
                listener.onConnectionChanged(ready)
                if (running && radioOn && subscribed.isEmpty()) {
                    restartAdvertising()
                }
            }
        }

        override fun onServiceAdded(status: Int, service: BluetoothGattService) {
            if (status != BluetoothGatt.GATT_SUCCESS) {
                listener.onStatus("GATT service failed ($status)")
                return
            }
            restartAdvertising()
        }

        override fun onNotificationSent(device: BluetoothDevice, status: Int) {
            val step = synchronized(lock) {
                callbacksLeft = (callbacksLeft - 1).coerceAtLeast(0)
                flightOk = flightOk && status == BluetoothGatt.GATT_SUCCESS
                if (callbacksLeft > 0) {
                    OutboundQueue.Transmit.Idle
                } else {
                    mainHandler.removeCallbacks(notifyTimeout)
                    queue.onFinished(flightOk)
                }
            }
            dispatch(step)
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
            if (descriptor.uuid == Packets.CCCD_UUID) {
                // Bit 0 is notifications. Bit 1 would be indications, which this
                // characteristic does not send.
                val enable = value != null && value.isNotEmpty() && (value[0].toInt() and 0x01) != 0
                if (enable) subscribed.add(device) else subscribed.remove(device)
                Log.i(TAG, "notify ${if (enable) "on" else "off"} for ${device.address}")
                listener.onConnectionChanged(ready)
                if (ready) listener.onStatus("Connected")
            }
            if (responseNeeded) {
                server?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, value)
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

    @SuppressLint("MissingPermission")
    override fun send(packet: ByteArray, reliable: Boolean): Boolean {
        if (!ready) return false
        val step = synchronized(lock) { queue.enqueue(packet) }
        dispatch(step)
        return true
    }

    @SuppressLint("MissingPermission")
    private fun dispatch(step: OutboundQueue.Transmit) {
        when (step) {
            is OutboundQueue.Transmit.Send -> transmit(step.packet)
            OutboundQueue.Transmit.DropLink -> dropLink("notify failed")
            OutboundQueue.Transmit.Idle -> Unit
        }
    }

    @SuppressLint("MissingPermission")
    private fun transmit(packet: ByteArray) {
        val server = server
        val characteristic = pointerChar
        val devices = subscribed.toList()
        if (server == null || characteristic == null || devices.isEmpty()) {
            val step = synchronized(lock) { queue.onApiRejected() }
            dispatch(step)
            return
        }
        var accepted = 0
        for (device in devices) {
            // confirm = false: notification, matching PROPERTY_NOTIFY and CCCD bit 0.
            val status = server.notifyCharacteristicChanged(device, characteristic, false, packet)
            if (status == BluetoothStatusCodes.SUCCESS) accepted++
            else Log.w(TAG, "notify status $status")
        }
        val step = synchronized(lock) {
            callbacksLeft = accepted
            flightOk = accepted > 0
            if (accepted == 0) {
                mainHandler.removeCallbacks(notifyTimeout)
                queue.onApiRejected()
            } else {
                queue.markAccepted()
                mainHandler.removeCallbacks(notifyTimeout)
                mainHandler.postDelayed(notifyTimeout, NOTIFY_TIMEOUT_MS)
                OutboundQueue.Transmit.Idle
            }
        }
        dispatch(step)
    }

    @SuppressLint("MissingPermission")
    private fun onNotifyTimeout() {
        val step = synchronized(lock) { queue.onTimeout() }
        if (step is OutboundQueue.Transmit.DropLink) {
            dropLink("notify callback timed out")
        }
    }

    @SuppressLint("MissingPermission")
    private fun dropLink(reason: String) {
        Log.w(TAG, reason)
        mainHandler.removeCallbacks(notifyTimeout)
        val devices = subscribed.toList()
        subscribed.clear()
        synchronized(lock) { queue.clear() }
        for (device in devices) {
            try {
                server?.cancelConnection(device)
            } catch (err: Exception) {
                Log.w(TAG, "cancelConnection", err)
            }
        }
        listener.onLinkFailed(reason)
        listener.onConnectionChanged(false)
        if (running && radioOn) restartAdvertising()
    }

    override fun takeCounters(): SendCounters {
        val raw = synchronized(lock) { queue.takeCounters() }
        return SendCounters(raw.accepted, raw.completed, raw.rejected, raw.coalesced)
    }

    @SuppressLint("MissingPermission")
    override fun stop() {
        running = false
        radioOn = false
        unregisterRadioReceiver()
        tearDownRadio()
    }

    @SuppressLint("MissingPermission")
    private fun tearDownRadio() {
        synchronized(lock) { queue.clear() }
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
