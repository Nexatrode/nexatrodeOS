#!/usr/bin/env python3
"""Configure the wallet's trusted clock and a merged UTM guest's HVF poller.

The wallet already has its entropy channel. This tool refuses to overwrite an
existing endpoint or to connect protection domains with invalid priorities.
"""
import argparse
import xml.etree.ElementTree as ET
from pathlib import Path


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("input", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    tree = ET.parse(args.input)
    root = tree.getroot()
    domains = {pd.get("name"): pd for pd in root.findall("protection_domain")}
    clock = domains.get("clock")
    wallet = domains.get("wallet_app")
    if clock is None or wallet is None:
        raise SystemExit("expected clock and wallet_app protection domains")
    if int(clock.get("priority", "0"), 0) <= int(wallet.get("priority", "0"), 0):
        raise SystemExit("clock priority must exceed wallet priority for protected calls")
    wanted = {("clock", "5"), ("wallet_app", "5")}
    have_clock_channel = False
    for channel in root.findall("channel"):
        ends = channel.findall("end")
        endpoints = {(end.get("pd"), end.get("id")) for end in ends}
        if endpoints == wanted and len(ends) == 2 and not have_clock_channel:
            if all(end.get("notify") == "false" and
                   end.get("pp", "false") == ("true" if end.get("pd") == "wallet_app" else "false")
                   for end in ends):
                have_clock_channel = True
                continue
        if wanted & endpoints:
            raise SystemExit("a wallet HTTPS clock endpoint is already in use")
    if not any({("entropy_rng", "3"), ("wallet_app", "4")} <=
               {(end.get("pd"), end.get("id")) for end in channel.findall("end")}
               for channel in root.findall("channel")):
        raise SystemExit("wallet entropy channel is missing")
    if not have_clock_channel:
        channel = ET.SubElement(root, "channel")
        ET.SubElement(channel, "end", {"pd": "clock", "id": "5", "notify": "false"})
        ET.SubElement(channel, "end", {"pd": "wallet_app", "id": "5", "pp": "true", "notify": "false"})
    poller = domains.get("timer_poller")
    if poller is not None and poller.find("program_image[@path='timer_poller.elf']") is not None:
        # Wallet socket readiness is polled at 1 ms. A poller replenished every
        # 10 ms delayed most callbacks even when data was already available.
        poller.set("budget", "100")
        poller.set("period", "1000")
    ET.indent(tree, space="    ")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    tree.write(args.output, encoding="utf-8", xml_declaration=True)


if __name__ == "__main__":
    main()
