package dev.wearpointer.ble

/**
 * Byte pipe for protocol v1 packets. The two implementations are the two
 * Bluetooth roles. The packet codec does not change between them.
 *
 * [WatchPeripheral] is Stage 1: the watch advertises and notifies.
 * [WatchCentral] is the fallback: the desktop advertises and the watch writes.
 * See docs/fallback-transport.md.
 */
data class SendCounters(
    val accepted: Int = 0,
    val completed: Int = 0,
    val rejected: Int = 0,
    val coalesced: Int = 0,
)

interface PointerTransport {
    val ready: Boolean
    fun start()
    fun stop()

    /**
     * @param reliable clicks, drags, and other discrete events. Aim may be
     * replaced by a newer sample while a notification is in flight. This flag
     * is not Android's indication `confirm` bit.
     * @return true when the packet was queued for a subscribed central.
     */
    fun send(packet: ByteArray, reliable: Boolean): Boolean

    /** Counts since the previous call: API accept, callback success, callback/API failure, coalesced aims. */
    fun takeCounters(): SendCounters = SendCounters()
}

interface LinkListener {
    fun onConnectionChanged(connected: Boolean)
    fun onStatus(text: String)
    fun onPeripheralUnsupported(reason: String)
    fun onStatsRequested()
}
