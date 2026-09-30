#!/usr/bin/env python3
"""Unit test: merge_network_system shares an existing timer_driver PD."""
import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

from merge_network_system import main as merge_main


BASE = """<?xml version="1.0" encoding="UTF-8"?>
<system>
    <memory_region name="messages_net_tx" size="0x1000" />
    <protection_domain name="timer_driver" priority="254" passive="true">
        <program_image path="timer_driver.elf" />
        <irq irq="30" id="0" />
    </protection_domain>
    <protection_domain name="gpu_client" priority="1">
        <program_image path="gpu_client.elf" />
    </protection_domain>
    <protection_domain name="messages_app" priority="100">
        <program_image path="rust_messages_app.elf" />
        <map mr="messages_net_tx" vaddr="0x4b000000" perms="rw" setvar_vaddr="messages_net_tx" />
    </protection_domain>
    <channel>
        <end pd="gpu_client" id="1" pp="true" />
        <end pd="timer_driver" id="1" />
    </channel>
</system>
"""

NETWORK = """<?xml version="1.0" encoding="UTF-8"?>
<system>
    <memory_region name="net_rx_data" size="0x1000" />
    <protection_domain name="timer_driver" priority="102" passive="true">
        <program_image path="timer_driver.elf" />
        <irq irq="30" id="0" trigger="level" />
    </protection_domain>
    <protection_domain name="dhcp_client" priority="97">
        <program_image path="dhcp_client.elf" />
        <map mr="net_rx_data" vaddr="0x20000000" perms="rw" />
    </protection_domain>
    <channel>
        <end pd="timer_driver" id="1" />
        <end pd="dhcp_client" id="2" notify="false" pp="true" />
    </channel>
</system>
"""


class MergeNetworkTimerShareTest(unittest.TestCase):
    def test_shares_existing_timer_driver(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            base = root / "base.system"
            network = root / "net.system"
            output = root / "merged.system"
            base.write_text(BASE)
            network.write_text(NETWORK)

            import sys
            sys.argv = [
                "merge_network_system.py",
                "--base", str(base),
                "--network", str(network),
                "--output", str(output),
            ]
            merge_main()

            tree = ET.parse(output)
            timers = [
                pd for pd in tree.getroot().findall("protection_domain")
                if pd.get("name") == "timer_driver"
            ]
            self.assertEqual(len(timers), 1)
            self.assertEqual(timers[0].get("priority"), "254")

            timer_ends = []
            for channel in tree.getroot().findall("channel"):
                ends = list(channel.findall("end"))
                if any(end.get("pd") == "timer_driver" for end in ends):
                    timer_ends.append(ends)
            # gpu_client keeps id=1; dhcp_client gets a fresh timer-side id.
            timer_ids = {
                end.get("id")
                for ends in timer_ends
                for end in ends
                if end.get("pd") == "timer_driver"
            }
            self.assertEqual(timer_ids, {"1", "2"})

            dhcp = next(
                pd for pd in tree.getroot().findall("protection_domain")
                if pd.get("name") == "dhcp_client"
            )
            self.assertEqual(dhcp.get("priority"), "101")


if __name__ == "__main__":
    unittest.main()
