package dev.wearpointer.ble

import android.annotation.SuppressLint
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
import android.content.Context
import android.os.ParcelUuid
import android.util.Log
import dev.wearpointer.proto.Packets
import java.util.concurrent.ConcurrentHashMap

/**
 * Watch as GATT peripheral. The desktop scans, connects, and subscribes.
 * This is the Stage 1 path. If the Watch7 radio cannot advertise, [LinkListener]
 * hears [LinkListener.onPeripheralUnsupported] and the UI can switch roles.
 */
class WatchPeripheral(
    private val context: Context,
    private val listener: LinkListener,
) : PointerTransport {
    private val subscribed = ConcurrentHashMap.newKeySet<BluetoothDevice>()
    private var server: BluetoothGattServer? = null
    private var advertiser: BluetoothLeAdvertiser? = null
    private var pointerChar: BluetoothGattCharacteristic? = null
    private var running = false

    override val ready: Boolean
        get() = subscribed.isNotEmpty()

    private val advertiseCallback = object : AdvertiseCallback() {
        override fun onStartSuccess(settingsInEffect: AdvertiseSettings) {
            Log.i(TAG, "advertising")
            listener.onStatus("Advertising")
        }

        override fun onStartFailure(errorCode: Int) {
            val reason = "advertise failed ($errorCode)"
            Log.w(TAG, reason)
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
                listener.onConnectionChanged(ready)
                if (running && !ready) listener.onStatus("Advertising")
            }
        }

        override fun onServiceAdded(status: Int, service: BluetoothGattService) {
            if (status != BluetoothGatt.GATT_SUCCESS) {
                listener.onStatus("GATT service failed ($status)")
                return
            }
            startAdvertising()
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
            if (characteristic.uuid == Packets.CONTROL_UUID && value?.firstOrNull()?.toInt() == Packets.CONTROL_REQUEST_STATS) {
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
        val manager = context.getSystemService(BluetoothManager::class.java)
        val adapter = manager?.adapter
        if (adapter == null || !adapter.isEnabled) {
            listener.onStatus("Bluetooth is off")
            running = false
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
            running = false
            return
        }
        this.advertiser = advertiser

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
            running = false
            return
        }
        this.server = server
        server.addService(service)
    }

    @SuppressLint("MissingPermission")
    private fun startAdvertising() {
        val advertiser = advertiser ?: return
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
        val server = server ?: return false
        val characteristic = pointerChar ?: return false
        val devices = subscribed.toList()
        if (devices.isEmpty()) return false
        var accepted = false
        for (device in devices) {
            val status = server.notifyCharacteristicChanged(device, characteristic, reliable, packet)
            if (status == BluetoothStatusCodes.SUCCESS) accepted = true
            else Log.w(TAG, "notify status $status")
        }
        return accepted
    }

    @SuppressLint("MissingPermission")
    override fun stop() {
        running = false
        subscribed.clear()
        try {
            advertiser?.stopAdvertising(advertiseCallback)
        } catch (err: Exception) {
            Log.w(TAG, "stopAdvertising", err)
        }
        server?.close()
        server = null
        advertiser = null
        pointerChar = null
    }

    private companion object {
        const val TAG = "wear-pointer"
    }
}
