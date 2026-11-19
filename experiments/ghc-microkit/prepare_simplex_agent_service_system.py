#!/usr/bin/env python3
"""Add a dedicated SimpleX Agent PD beside the normal Messages UI PD."""

from __future__ import annotations

import argparse
import xml.etree.ElementTree as ET
from pathlib import Path


def add_channel(root: ET.Element, left_pd: str, left_id: int,
                right_pd: str, right_id: int, *, pp_left: bool = False,
                pp_right: bool = False, notify_left: bool = False,
                notify_right: bool = False) -> None:
    used = {
        (end.get("pd"), int(end.get("id", "-1"), 0))
        for end in root.findall("channel/end")
    }
    if (left_pd, left_id) in used or (right_pd, right_id) in used:
        raise SystemExit(f"channel endpoint already used: {left_pd}:{left_id} or {right_pd}:{right_id}")
    channel = ET.SubElement(root, "channel")
    ET.SubElement(channel, "end", {
        "pd": left_pd, "id": str(left_id), "notify": str(notify_left).lower(),
        **({"pp": "true"} if pp_left else {}),
    })
    ET.SubElement(channel, "end", {
        "pd": right_pd, "id": str(right_id), "notify": str(notify_right).lower(),
        **({"pp": "true"} if pp_right else {}),
    })


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    tree = ET.parse(args.base)
    root = tree.getroot()
    names = {pd.get("name") for pd in root.findall("protection_domain")}
    if "messages_app" not in names or "vault" not in names:
        raise SystemExit("base system must contain Messages and vault protection domains")
    if "simplex_agent" in names:
        raise SystemExit("base system already contains simplex_agent")

    if root.find("memory_region[@name='simplex_agent_transfer']") is not None:
        raise SystemExit("base system already contains simplex_agent_transfer")
    ET.SubElement(root, "memory_region", {
        "name": "simplex_agent_transfer", "size": "0x1000",
        "page_size": "0x1000",
    })
    vault = root.find("protection_domain[@name='vault']")
    assert vault is not None
    ET.SubElement(vault, "map", {
        "mr": "simplex_agent_transfer", "vaddr": "0x4c_000_000",
        "perms": "rw", "cached": "false",
        "setvar_vaddr": "simplex_agent_transfer",
    })

    agent = ET.SubElement(root, "protection_domain", {
        "name": "simplex_agent", "priority": "100", "passive": "true",
        "stack_size": "0x100000",
    })
    ET.SubElement(agent, "program_image", {"path": "agent-runtime.elf"})
    # AgentStore file I/O gets its own transfer page so SQLCipher activity
    # cannot overwrite the Messages UI snapshot or the Messages app's vault
    # operations. Vault access remains a separate endpoint (vault:7).
    ET.SubElement(agent, "map", {
        "mr": "simplex_agent_transfer", "vaddr": "0x44_000_000", "perms": "rw",
        "cached": "false", "setvar_vaddr": "blob_transfer",
    })
    add_channel(root, "vault", 7, "simplex_agent", 0, pp_right=True)
    add_channel(root, "clock", 4, "simplex_agent", 2, pp_right=True)
    add_channel(root, "entropy_rng", 2, "simplex_agent", 3, pp_right=True)

    args.output.parent.mkdir(parents=True, exist_ok=True)
    ET.indent(tree, space="    ")
    tree.write(args.output, encoding="UTF-8", xml_declaration=True)

    # Guard the central isolation property: retain the normal Messages image,
    # and keep all UI, network-transmit, and view mappings out of the Agent PD.
    messages = root.find("protection_domain[@name='messages_app']")
    assert messages is not None
    assert messages.find("program_image").get("path") == "rust_messages_app.elf"
    agent_maps = {mapping.get("mr") for mapping in agent.findall("map")}
    assert agent_maps == {"simplex_agent_transfer"}, agent_maps


if __name__ == "__main__":
    main()
