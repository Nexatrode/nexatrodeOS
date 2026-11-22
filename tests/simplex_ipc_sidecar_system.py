#!/usr/bin/env python3
"""Add an isolated SimpleX IPC service probe beside the normal Messages PD."""

from __future__ import annotations

import argparse
import xml.etree.ElementTree as ET
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    tree = ET.parse(args.base)
    root = tree.getroot()
    names = {pd.get("name") for pd in root.findall("protection_domain")}
    if "messages_app" not in names:
        raise SystemExit("base system must keep the normal messages_app PD")
    if "simplex_agent_probe" in names or "simplex_ipc_client_probe" in names:
        raise SystemExit("base system already contains a SimpleX sidecar probe")

    memory_region = ET.Element("memory_region", {
        "name": "simplex_ipc_page", "size": "0x1000", "page_size": "0x1000"
    })
    first_domain = next((index for index, child in enumerate(root)
                         if child.tag == "protection_domain"), len(root))
    root.insert(first_domain, memory_region)

    service = ET.SubElement(root, "protection_domain", {
        "name": "simplex_agent_probe", "priority": "101", "passive": "true",
        "stack_size": "0x4000"
    })
    ET.SubElement(service, "program_image", {"path": "simplex_ipc_sidecar_service.elf"})
    ET.SubElement(service, "map", {
        "mr": "simplex_ipc_page", "vaddr": "0x4a000000", "perms": "rw",
        "cached": "false", "setvar_vaddr": "simplex_ipc_page"
    })

    client = ET.SubElement(root, "protection_domain", {
        "name": "simplex_ipc_client_probe", "priority": "100", "stack_size": "0x4000"
    })
    ET.SubElement(client, "program_image", {"path": "simplex_ipc_sidecar_client.elf"})
    ET.SubElement(client, "map", {
        "mr": "simplex_ipc_page", "vaddr": "0x4a000000", "perms": "rw",
        "cached": "false", "setvar_vaddr": "simplex_ipc_page"
    })

    channel = ET.SubElement(root, "channel")
    ET.SubElement(channel, "end", {"pd": "simplex_agent_probe", "id": "0"})
    ET.SubElement(channel, "end", {
        "pd": "simplex_ipc_client_probe", "id": "0", "pp": "true"
    })

    args.output.parent.mkdir(parents=True, exist_ok=True)
    ET.indent(tree, space="    ")
    tree.write(args.output, encoding="utf-8", xml_declaration=True)

    # The Messages app is the UI-facing peer. The test client is an additional
    # probe PD; ensure the merge neither replaces Messages nor grants the Agent
    # any of its view, vault-transfer, network, or UI input memory.
    messages = root.find("protection_domain[@name='messages_app']")
    agent = root.find("protection_domain[@name='simplex_agent_probe']")
    assert messages is not None and agent is not None
    assert messages.find("program_image").get("path") == "rust_messages_app.elf"
    agent_mrs = {mapping.get("mr") for mapping in agent.findall("map")}
    assert agent_mrs == {"simplex_ipc_page"}, agent_mrs


if __name__ == "__main__":
    main()
