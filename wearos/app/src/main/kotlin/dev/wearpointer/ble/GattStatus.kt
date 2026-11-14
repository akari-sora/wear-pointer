package dev.wearpointer.ble

import android.bluetooth.BluetoothStatusCodes

/**
 * Classifies synchronous status codes from `notifyCharacteristicChanged` and
 * `writeCharacteristic` (API 33+).
 *
 * Only "busy" is retried: the stack did not take the bytes and will take them
 * once its current operation completes. Everything else (not connected, not
 * allowed, missing permission, radio off, unknown) will not fix itself in a
 * few milliseconds, so the link is dropped and the desktop releases buttons.
 */
object GattStatus {
    fun isRetryable(status: Int): Boolean =
        status == BluetoothStatusCodes.ERROR_GATT_WRITE_REQUEST_BUSY
}
