#!/usr/bin/env python3
"""The native Haskell merge exposes only the TCP page and protected endpoint."""
import subprocess
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BASE = ROOT / "experiments/ghc-microkit/haskell_probe/haskell.system"


class HaskellTcpMergeTest(unittest.TestCase):
    def test_capability_boundary(self):
        with tempfile.TemporaryDirectory() as tmp:
            work = Path(tmp)
            network = work / "net.system"
            network.write_text("""<system>
              <memory_region name="net_regs" size="0x1000" phys_addr="0xa000000"/>
              <protection_domain name="timer_driver" priority="102">
                <program_image path="timer_driver.elf"/>
              </protection_domain>
              <protection_domain name="dhcp_client" priority="97">
                <program_image path="dhcp_client.elf"/>
                <map mr="net_regs" vaddr="0x20000000" perms="rw"/>
              </protection_domain>
              <channel>
                <end pd="timer_driver" id="1"/>
                <end pd="dhcp_client" id="2" notify="false" pp="true"/>
              </channel>
            </system>""")
            output = work / "merged.system"
            subprocess.run([sys.executable, str(ROOT / "tests/merge_network_system.py"),
                            "--haskell-tcp", "--base", str(BASE),
                            "--network", str(network), "--output", str(output)],
                           check=True, capture_output=True, text=True)
            root = ET.parse(output).getroot()
            client = next(pd for pd in root.findall("protection_domain")
                          if pd.get("name") == "client")
            dhcp = next(pd for pd in root.findall("protection_domain")
                        if pd.get("name") == "dhcp_client")
            self.assertEqual([m.get("mr") for m in client.findall("map")],
                             ["haskell_tcp_io"])
            self.assertIn("net_regs", [m.get("mr") for m in dhcp.findall("map")])
            self.assertIn("haskell_tcp_io", [m.get("mr") for m in dhcp.findall("map")])
            self.assertEqual(len([mr for mr in root.findall("memory_region")
                                  if mr.get("name") == "haskell_tcp_io"]), 1)
            endpoints = [tuple((end.get("pd"), end.get("id"), end.get("pp"))
                               for end in channel.findall("end"))
                         for channel in root.findall("channel")]
            self.assertIn((("client", "2", "true"),
                           ("dhcp_client", "4", None)), endpoints)


if __name__ == "__main__":
    unittest.main()
