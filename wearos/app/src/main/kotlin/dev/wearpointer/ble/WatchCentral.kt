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

/**
 * Watch as GATT central. Used when the watch cannot advertise. The desktop
 * must be the peripheral (WatchPointer's direction). That desktop role is
 * still a stub; this side is ready for it.
 *
 * Writes go through the same [OutboundQueue] as the peripheral: one write in
 * flight, FIFO, sequence stamped at hand-off, Center/Pause drop the waiting
 * aim, busy is retried, other failures drop the link. Everything runs on
 * [handler]; GATT callbacks post to it.
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
    private val queue = OutboundQueue()
    private var retryPacket: ByteArray? = null
    private val writeTimeout = Runnable { onWriteTimeout() }
    private val retryRunnable = Runnable { retryNow() }
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
                queue.clear()
                connected = true
                listener.onConnectionChanged(true)
                listener.onStatus("Connected")
            }
        }

        override fun onCharacteristicWrite(
            gatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
            status: Int,
        ) {
            if (characteristic.uuid != Packets.POINTER_UUID) return
            handler.post {
                if (!queue.isBusy()) return@post
                if (status != BluetoothGatt.GATT_SUCCESS) {
                    Log.w(TAG, "write status $status")
                }
                handler.removeCallbacks(writeTimeout)
                dispatch(queue.onFinished(status == BluetoothGatt.GATT_SUCCESS))
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

    /** Called on [handler] by the runtime. */
    override fun send(packet: ByteArray, reliable: Boolean): Boolean {
        if (!ready) return false
        dispatch(queue.enqueue(packet))
        return true
    }

    private fun dispatch(step: OutboundQueue.Transmit) {
        when (step) {
            is OutboundQueue.Transmit.Send -> writeNow(step.packet)
            is OutboundQueue.Transmit.Retry -> {
                retryPacket = step.packet
                handler.removeCallbacks(retryRunnable)
                handler.postDelayed(retryRunnable, step.delayMs)
            }
            OutboundQueue.Transmit.DropLink -> dropLink("write failed")
            OutboundQueue.Transmit.Idle -> Unit
        }
    }

    private fun retryNow() {
        val packet = retryPacket ?: return
        retryPacket = null
        writeNow(packet)
    }

    @SuppressLint("MissingPermission")
    private fun writeNow(packet: ByteArray) {
        val gatt = gatt
        val characteristic = pointerChar
        if (gatt == null || characteristic == null) {
            dispatch(queue.onApiRejected(retryable = false))
            return
        }
        // Aim may be lost without harm; discrete events ask for a response.
        val type = if (packet[1].toInt() == Packets.KIND_AIM) {
            BluetoothGattCharacteristic.WRITE_TYPE_NO_RESPONSE
        } else {
            BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT
        }
        val status = gatt.writeCharacteristic(characteristic, packet, type)
        if (status == BluetoothStatusCodes.SUCCESS) {
            queue.markAccepted()
            handler.removeCallbacks(writeTimeout)
            handler.postDelayed(writeTimeout, WRITE_TIMEOUT_MS)
        } else {
            Log.w(TAG, "write rejected $status")
            dispatch(queue.onApiRejected(retryable = GattStatus.isRetryable(status)))
        }
    }

    private fun onWriteTimeout() {
        if (queue.onTimeout() is OutboundQueue.Transmit.DropLink) dropLink("write callback timed out")
    }

    @SuppressLint("MissingPermission")
    private fun dropLink(reason: String) {
        Log.w(TAG, reason)
        listener.onLinkFailed(reason)
        resetFlight()
        connected = false
        // onConnectionStateChange(DISCONNECTED) tears down and rescans.
        gatt?.disconnect()
    }

    private fun resetFlight() {
        handler.removeCallbacks(writeTimeout)
        handler.removeCallbacks(retryRunnable)
        retryPacket = null
        queue.clear()
    }

    override fun dropLinkForTest(reason: String) {
        handler.post { dropLink(reason) }
    }

    override fun takeCounters(): SendCounters {
        val raw = queue.takeCounters()
        return SendCounters(raw.accepted, raw.completed, raw.rejected, raw.coalesced)
    }

    @SuppressLint("MissingPermission")
    private fun tearDownGatt() {
        connected = false
        pointerChar = null
        resetFlight()
        gatt?.close()
        gatt = null
    }

    override fun stop() {
        val block = {
            running = false
            stopScan()
            tearDownGatt()
        }
        if (handler.looper.isCurrentThread) block() else handler.post(block)
    }

    private companion object {
        const val TAG = "wear-pointer"
        const val WRITE_TIMEOUT_MS = 1_000L
    }
}
