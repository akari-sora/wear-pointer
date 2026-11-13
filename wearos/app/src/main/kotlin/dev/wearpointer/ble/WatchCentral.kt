package dev.wearpointer.ble

import android.annotation.SuppressLint
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCallback
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattDescriptor
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.bluetooth.BluetoothStatusCodes
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanFilter
import android.bluetooth.le.ScanResult
import android.bluetooth.le.ScanSettings
import android.content.Context
import android.os.Handler
import android.os.ParcelUuid
import android.util.Log
import dev.wearpointer.proto.Packets
import java.util.ArrayDeque

/**
 * Watch as GATT central. Used when the watch cannot advertise. The desktop
 * must be the peripheral (WatchPointer's direction). That desktop role is
 * still a stub; this side is ready for it.
 *
 * Writes are serialized. A newer aim sample replaces one that has not been
 * written yet. Clicks stay queued.
 */
class WatchCentral(
    private val context: Context,
    private val handler: Handler,
    private val listener: LinkListener,
) : PointerTransport {
    private var gatt: BluetoothGatt? = null
    private var pointerChar: BluetoothGattCharacteristic? = null
    private var scanning = false
    private var running = false
    private var writing = false
    private val reliableQueue = ArrayDeque<ByteArray>()
    private var pendingUnreliable: ByteArray? = null
    private var wireSequence = 0
    private var connected = false

    override val ready: Boolean
        get() = connected && pointerChar != null

    private val scanCallback = object : ScanCallback() {
        override fun onScanResult(callbackType: Int, result: ScanResult) {
            if (!running || gatt != null) return
            stopScan()
            listener.onStatus("Connecting")
            connect(result.device)
        }

        override fun onScanFailed(errorCode: Int) {
            listener.onStatus("scan failed ($errorCode)")
        }
    }

    private val gattCallback = object : BluetoothGattCallback() {
        @SuppressLint("MissingPermission")
        override fun onConnectionStateChange(gatt: BluetoothGatt, status: Int, newState: Int) {
            handler.post {
                if (newState == BluetoothProfile.STATE_CONNECTED) {
                    Log.i(TAG, "connected ${gatt.device.address}")
                    gatt.requestConnectionPriority(BluetoothGatt.CONNECTION_PRIORITY_HIGH)
                    gatt.discoverServices()
                } else {
                    Log.i(TAG, "disconnected status=$status")
                    tearDownGatt()
                    listener.onConnectionChanged(false)
                    if (running) {
                        listener.onStatus("Scanning for desktop")
                        startScan()
                    }
                }
            }
        }

        @SuppressLint("MissingPermission")
        override fun onServicesDiscovered(gatt: BluetoothGatt, status: Int) {
            handler.post {
                if (status != BluetoothGatt.GATT_SUCCESS) {
                    listener.onStatus("service discovery failed ($status)")
                    return@post
                }
                val service = gatt.getService(Packets.SERVICE_UUID)
                val pointer = service?.getCharacteristic(Packets.POINTER_UUID)
                if (pointer == null) {
                    listener.onStatus("desktop is missing the pointer characteristic")
                    return@post
                }
                pointerChar = pointer
                val control = service.getCharacteristic(Packets.CONTROL_UUID)
                if (control != null) {
                    gatt.setCharacteristicNotification(control, true)
                    val cccd = control.getDescriptor(Packets.CCCD_UUID)
                    if (cccd != null) {
                        gatt.writeDescriptor(cccd, BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE)
                    }
                }
                connected = true
                listener.onConnectionChanged(true)
                listener.onStatus("Connected")
                pump()
            }
        }

        override fun onCharacteristicWrite(
            gatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
            status: Int,
        ) {
            handler.post {
                writing = false
                if (status != BluetoothGatt.GATT_SUCCESS) {
                    Log.w(TAG, "write status $status")
                }
                pump()
            }
        }
    }

    @SuppressLint("MissingPermission")
    override fun start() {
        if (running) return
        running = true
        val adapter = context.getSystemService(BluetoothManager::class.java)?.adapter
        if (adapter == null || !adapter.isEnabled) {
            listener.onStatus("Bluetooth is off")
            running = false
            return
        }
        listener.onStatus("Scanning for desktop")
        startScan()
    }

    @SuppressLint("MissingPermission")
    private fun startScan() {
        if (scanning) return
        val scanner = context.getSystemService(BluetoothManager::class.java)
            ?.adapter
            ?.bluetoothLeScanner
        if (scanner == null) {
            listener.onStatus("BLE scanner unavailable")
            return
        }
        val filter = ScanFilter.Builder().setServiceUuid(ParcelUuid(Packets.SERVICE_UUID)).build()
        val settings = ScanSettings.Builder().setScanMode(ScanSettings.SCAN_MODE_LOW_LATENCY).build()
        scanner.startScan(listOf(filter), settings, scanCallback)
        scanning = true
    }

    @SuppressLint("MissingPermission")
    private fun stopScan() {
        if (!scanning) return
        context.getSystemService(BluetoothManager::class.java)
            ?.adapter
            ?.bluetoothLeScanner
            ?.stopScan(scanCallback)
        scanning = false
    }

    @SuppressLint("MissingPermission")
    private fun connect(device: BluetoothDevice) {
        gatt = device.connectGatt(context, false, gattCallback, BluetoothDevice.TRANSPORT_LE)
    }

    override fun send(packet: ByteArray, reliable: Boolean): Boolean {
        if (!ready) return false
        // Fallback path only (desktop advertises). Sequence is stamped in writeNow.
        // It does not share OutboundQueue's FIFO / Center rules; Stage 1 does not use this.
        if (writing) {
            if (reliable) reliableQueue.add(packet) else pendingUnreliable = packet
            return true
        }
        return writeNow(packet, reliable)
    }

    @SuppressLint("MissingPermission")
    private fun writeNow(packet: ByteArray, reliable: Boolean): Boolean {
        val gatt = gatt ?: return false
        val characteristic = pointerChar ?: return false
        val type = if (reliable) {
            BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT
        } else {
            BluetoothGattCharacteristic.WRITE_TYPE_NO_RESPONSE
        }
        Packets.stampSequence(packet, wireSequence)
        val status = gatt.writeCharacteristic(characteristic, packet, type)
        if (status == BluetoothStatusCodes.SUCCESS) {
            wireSequence = (wireSequence + 1) and 0xFFFF
        }
        writing = status == BluetoothStatusCodes.SUCCESS
        return writing
    }

    private fun pump() {
        if (writing) return
        val next = reliableQueue.pollFirst()
        if (next != null) {
            writeNow(next, true)
            return
        }
        val aim = pendingUnreliable
        pendingUnreliable = null
        if (aim != null) writeNow(aim, false)
    }

    @SuppressLint("MissingPermission")
    private fun tearDownGatt() {
        connected = false
        writing = false
        pointerChar = null
        reliableQueue.clear()
        pendingUnreliable = null
        wireSequence = 0
        gatt?.close()
        gatt = null
    }

    override fun stop() {
        running = false
        stopScan()
        tearDownGatt()
    }

    private companion object {
        const val TAG = "wear-pointer"
    }
}
