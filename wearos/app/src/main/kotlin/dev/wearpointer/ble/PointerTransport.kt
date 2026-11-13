package dev.wearpointer.ble

/**
 * Byte pipe for protocol v1 packets. The two implementations are the two
 * Bluetooth roles. The packet codec does not change between them.
 *
 * [WatchPeripheral] is Stage 1: the watch advertises and notifies.
 * [WatchCentral] is the fallback: the desktop advertises and the watch writes.
 * See docs/fallback-transport.md.
 */
interface PointerTransport {
    val ready: Boolean
    fun start()
    fun stop()

    /**
     * @param reliable clicks and drags. Aim and scroll may be replaced by a
     * newer sample when the link is busy.
     * @return true when the stack accepted the packet (or queued a reliable one).
     */
    fun send(packet: ByteArray, reliable: Boolean): Boolean
}

interface LinkListener {
    fun onConnectionChanged(connected: Boolean)
    fun onStatus(text: String)
    fun onPeripheralUnsupported(reason: String)
    fun onStatsRequested()
}
