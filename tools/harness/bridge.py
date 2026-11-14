"""Bridge a kernel virtual HCI controller (/dev/vhci) to netsim's controller.

bumble-hci-bridge opens the host side first, so the kernel's HCI Reset on the
new hciN arrives before the controller side is connected and is dropped
(kernel: "Opcode 0x0c03 failed: -110"). This opens the controller first and
wires both directions before the event loop can deliver the first packet.

usage: python bridge.py tcp-client:127.0.0.1:<netsim hci.port>
"""

import asyncio
import sys

from bumble.transport import open_transport


async def main(controller_spec: str) -> None:
    async with await open_transport(controller_spec) as (ctrl_source, ctrl_sink):
        async with await open_transport("vhci") as (host_source, host_sink):
            host_source.set_packet_sink(ctrl_sink)
            ctrl_source.set_packet_sink(host_sink)
            print("bridge up", flush=True)
            await asyncio.get_running_loop().create_future()


if __name__ == "__main__":
    asyncio.run(main(sys.argv[1]))
