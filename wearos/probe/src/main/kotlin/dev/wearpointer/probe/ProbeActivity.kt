package dev.wearpointer.probe

import android.annotation.SuppressLint
import android.app.Activity
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCallback
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattDescriptor
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanFilter
import android.bluetooth.le.ScanResult
import android.bluetooth.le.ScanSettings
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.ParcelUuid
import android.os.SystemClock
import android.util.Log
import android.view.WindowManager
import android.widget.TextView
import java.util.UUID

/**
 * Stand-in for the desktop receiver on a second emulator. It scans for the
 * pointer service, subscribes to notifications, and logs each one as
 *
 *     rx <elapsedRealtimeNanos> <hex>
 *
 * under tag `wp-probe`, plus `connected` / `subscribed` / `disconnected`.
 * `receiver/crates/core/examples/replay.rs` feeds that log through the real
 * desktop session. `am broadcast -a dev.wearpointer.probe.DROP` disconnects,
 * after which the probe scans and reconnects.
 *
 * Like the desktop receiver (`SILENCE_TIMEOUT` in crates/bluetooth), the probe
 * disconnects when nothing has arrived for 3 s while subscribed: the watch
 * cannot always end a link the central opened.
 */
@SuppressLint("MissingPermission", "SetTextI18n")
class ProbeActivity : Activity() {
    private val main = Handler(Looper.getMainLooper())
    private var gatt: BluetoothGatt? = null
    private var scanning = false
    private var received = 0L
    private var subscribed = false
    @Volatile private var lastRxMs = 0L
    private val watchdog = object : Runnable {
        override fun run() {
            val g = gatt
            if (g != null && subscribed && SystemClock.elapsedRealtime() - lastRxMs > SILENCE_MS) {
                Log.i(TAG, "silent for ${SILENCE_MS} ms, disconnecting")
                subscribed = false
                g.disconnect()
            }
            main.postDelayed(this, 250)
        }
    }
    private lateinit var label: TextView

    private val dropReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            Log.i(TAG, "drop requested")
            gatt?.disconnect()
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        label = TextView(this).apply { text = "probe: starting"; textSize = 20f }
        setContentView(label)
        registerReceiver(dropReceiver, IntentFilter(ACTION_DROP), Context.RECEIVER_EXPORTED)
        startScan()
        main.post(watchdog)
    }

    override fun onDestroy() {
        main.removeCallbacks(watchdog)
        unregisterReceiver(dropReceiver)
        stopScan()
        gatt?.close()
        gatt = null
        super.onDestroy()
    }

    private val scanCallback = object : ScanCallback() {
        override fun onScanResult(callbackType: Int, result: ScanResult) {
            if (gatt != null) return
            stopScan()
            Log.i(TAG, "found ${result.device.address}")
            gatt = result.device.connectGatt(this@ProbeActivity, false, gattCallback, BluetoothDevice.TRANSPORT_LE)
        }

        override fun onScanFailed(errorCode: Int) {
            Log.w(TAG, "scan failed $errorCode")
            scanning = false
            main.postDelayed({ startScan() }, 1_000)
        }
    }

    private val gattCallback = object : BluetoothGattCallback() {
        override fun onConnectionStateChange(g: BluetoothGatt, status: Int, newState: Int) {
            if (newState == BluetoothProfile.STATE_CONNECTED) {
                Log.i(TAG, "connected status=$status")
                g.requestConnectionPriority(BluetoothGatt.CONNECTION_PRIORITY_HIGH)
                g.discoverServices()
                show("connected")
            } else if (newState == BluetoothProfile.STATE_DISCONNECTED) {
                Log.i(TAG, "disconnected status=$status")
                g.close()
                main.post {
                    subscribed = false
                    if (gatt === g) gatt = null
                    show("disconnected, scanning")
                    main.postDelayed({ startScan() }, 500)
                }
            }
        }

        override fun onServicesDiscovered(g: BluetoothGatt, status: Int) {
            val pointer = g.getService(SERVICE_UUID)?.getCharacteristic(POINTER_UUID)
            if (status != BluetoothGatt.GATT_SUCCESS || pointer == null) {
                Log.w(TAG, "service discovery status=$status pointer=${pointer != null}")
                g.disconnect()
                return
            }
            g.setCharacteristicNotification(pointer, true)
            val cccd = pointer.getDescriptor(CCCD_UUID)
            g.writeDescriptor(cccd, BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE)
        }

        override fun onDescriptorWrite(g: BluetoothGatt, descriptor: BluetoothGattDescriptor, status: Int) {
            Log.i(TAG, "subscribed status=$status")
            main.post {
                lastRxMs = SystemClock.elapsedRealtime()
                subscribed = true
            }
            show("subscribed")
        }

        override fun onCharacteristicChanged(
            g: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
            value: ByteArray,
        ) {
            val now = SystemClock.elapsedRealtimeNanos()
            lastRxMs = now / 1_000_000
            Log.i(TAG, "rx $now ${value.joinToString("") { "%02x".format(it) }}")
            received++
            if (received % 100 == 0L) show("rx $received")
        }
    }

    private fun startScan() {
        if (scanning || gatt != null) return
        val scanner = getSystemService(BluetoothManager::class.java)?.adapter?.bluetoothLeScanner
        if (scanner == null) {
            Log.w(TAG, "no scanner yet")
            main.postDelayed({ startScan() }, 1_000)
            return
        }
        val filter = ScanFilter.Builder().setServiceUuid(ParcelUuid(SERVICE_UUID)).build()
        val settings = ScanSettings.Builder().setScanMode(ScanSettings.SCAN_MODE_LOW_LATENCY).build()
        scanner.startScan(listOf(filter), settings, scanCallback)
        scanning = true
        Log.i(TAG, "scanning")
        show("scanning")
    }

    private fun stopScan() {
        if (!scanning) return
        getSystemService(BluetoothManager::class.java)?.adapter?.bluetoothLeScanner?.stopScan(scanCallback)
        scanning = false
    }

    private fun show(text: String) {
        main.post { label.text = "probe: $text" }
    }

    private companion object {
        const val TAG = "wp-probe"
        const val SILENCE_MS = 3_000L
        const val ACTION_DROP = "dev.wearpointer.probe.DROP"
        val SERVICE_UUID: UUID = UUID.fromString("7a1e0001-4c3b-4e2a-9f10-6d5c4b3a2910")
        val POINTER_UUID: UUID = UUID.fromString("7a1e0002-4c3b-4e2a-9f10-6d5c4b3a2910")
        val CCCD_UUID: UUID = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb")
    }
}
