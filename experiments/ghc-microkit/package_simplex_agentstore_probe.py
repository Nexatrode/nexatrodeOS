#!/usr/bin/env python3
"""Package the SimpleX AgentStore probe with the QEMU sDDF timer service."""

from __future__ import annotations

import argparse
import shutil
import subprocess
import xml.etree.ElementTree as ET
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-system", type=Path, required=True)
    parser.add_argument("--agent-elf", type=Path, required=True)
    parser.add_argument("--agent-pd", default="messages_app")
    ipc_group = parser.add_mutually_exclusive_group()
    ipc_group.add_argument("--simplex-ipc-client-elf", type=Path)
    ipc_group.add_argument("--messages-agent-elf", type=Path)
    parser.add_argument("--timer-elf", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--microkit-tool", type=Path, required=True)
    parser.add_argument("--extra-search-path", type=Path, action="append", default=[])
    parser.add_argument("--board", default="qemu_virt_aarch64")
    parser.add_argument("--config", default="debug")
    parser.add_argument("--objcopy", type=Path, required=True)
    args = parser.parse_args()

    args.output.mkdir(parents=True, exist_ok=True)
    tree = ET.parse(args.base_system)
    root = tree.getroot()

    timer_pd = root.find("protection_domain[@name='timer_driver']")
    agent_pd = root.find(f"protection_domain[@name='{args.agent_pd}']")
    if agent_pd is None:
        raise SystemExit(f"base system has no {args.agent_pd} PD")
    program = agent_pd.find("program_image")
    if program is None:
        raise SystemExit(f"{args.agent_pd} has no program_image")
    program.set("path", "agentstore_probe.elf")
    agent_pd.set("stack_size", "0x100000")
    if args.agent_pd == "messages_app":
        for mapping in list(agent_pd.findall("map")):
            if mapping.get("setvar_vaddr") in {"messages_view", "messages_net_tx"}:
                agent_pd.remove(mapping)

    if args.simplex_ipc_client_elf is not None:
        if root.find("memory_region[@name='simplex_ipc_page']") is not None:
            raise SystemExit("base system already contains simplex_ipc_page")
        ET.SubElement(root, "memory_region", {
            "name": "simplex_ipc_page", "size": "0x1000", "page_size": "0x1000"
        })
        ET.SubElement(agent_pd, "map", {
            "mr": "simplex_ipc_page", "vaddr": "0x4a000000", "perms": "rw",
            "cached": "false", "setvar_vaddr": "simplex_ipc_page"
        })
        client = ET.SubElement(root, "protection_domain", {
            "name": "simplex_agent_rpc_probe", "priority": "99",
            "stack_size": "0x4000"
        })
        ET.SubElement(client, "program_image", {"path": "simplex_agent_rpc_client.elf"})
        ET.SubElement(client, "map", {
            "mr": "simplex_ipc_page", "vaddr": "0x4a000000", "perms": "rw",
            "cached": "false", "setvar_vaddr": "simplex_ipc_page"
        })
        channel = ET.SubElement(root, "channel")
        ET.SubElement(channel, "end", {
            "pd": args.agent_pd, "id": "6", "notify": "true"
        })
        ET.SubElement(channel, "end", {
            "pd": "simplex_agent_rpc_probe", "id": "0", "notify": "true"
        })

    if args.messages_agent_elf is not None:
        messages_pd = root.find("protection_domain[@name='messages_app']")
        if messages_pd is None:
            raise SystemExit("base system has no messages_app PD")
        messages_program = messages_pd.find("program_image")
        if messages_program is None:
            raise SystemExit("messages_app has no program_image")
        if root.find("memory_region[@name='simplex_ipc_page']") is not None:
            raise SystemExit("base system already contains simplex_ipc_page")
        messages_program.set("path", "rust_messages_agent_app.elf")
        ET.SubElement(root, "memory_region", {
            "name": "simplex_ipc_page", "size": "0x1000", "page_size": "0x1000"
        })
        for pd in (agent_pd, messages_pd):
            ET.SubElement(pd, "map", {
                "mr": "simplex_ipc_page", "vaddr": "0x4a000000", "perms": "rw",
                "cached": "false", "setvar_vaddr": "simplex_ipc_page"
            })
        channel = ET.SubElement(root, "channel")
        ET.SubElement(channel, "end", {
            "pd": args.agent_pd, "id": "5", "notify": "true"
        })
        ET.SubElement(channel, "end", {
            "pd": "messages_app", "id": "4", "notify": "true"
        })

    if timer_pd is None:
        timer_pd = ET.Element(
            "protection_domain",
            {"name": "timer_driver", "priority": "253", "passive": "true"},
        )
        ET.SubElement(timer_pd, "program_image", {"path": "timer_driver.elf"})
        ET.SubElement(
            timer_pd,
            "irq",
            {"irq": "30", "id": "0", "trigger": "level"},
        )
        root.insert(0, timer_pd)
    else:
        program = timer_pd.find("program_image")
        if program is None:
            program = ET.SubElement(timer_pd, "program_image")
        program.set("path", "timer_driver.elf")

    timer_channel = ET.SubElement(root, "channel")
    used_timer_ids = {
        int(endpoint.get("id"), 0)
        for endpoint in root.findall("channel/end")
        if endpoint.get("pd") == "timer_driver"
    }
    timer_id = 1
    while timer_id in used_timer_ids:
        timer_id += 1
    ET.SubElement(timer_channel, "end", {"pd": "timer_driver", "id": str(timer_id)})
    ET.SubElement(
        timer_channel,
        "end",
        {"pd": args.agent_pd, "id": "4", "notify": "false", "pp": "true"},
    )

    system_path = args.output / "system.system"
    ET.indent(tree, space="    ")
    tree.write(system_path, encoding="utf-8", xml_declaration=True)

    # struct timer_client_config is 5-byte "sDDF\x06" magic plus uint8_t id.
    timer_config = args.output / "timer_client_config.data"
    timer_config.write_bytes(b"sDDF\x06\x04")

    agent_image = args.output / "agentstore_probe.elf"
    subprocess.run(
        [
            str(args.objcopy),
            "--update-section",
            f".timer_client_config={timer_config}",
            str(args.agent_elf),
            str(agent_image),
        ],
        check=True,
    )
    shutil.copy2(args.timer_elf, args.output / "timer_driver.elf")
    if args.simplex_ipc_client_elf is not None:
        shutil.copy2(args.simplex_ipc_client_elf,
                     args.output / "simplex_agent_rpc_client.elf")
    if args.messages_agent_elf is not None:
        shutil.copy2(args.messages_agent_elf,
                     args.output / "rust_messages_agent_app.elf")

    subprocess.run(
        [
            str(args.microkit_tool),
            str(system_path),
            "--search-path",
            str(args.output),
            *(str(path) for path in args.extra_search_path),
            "--board",
            args.board,
            "--config",
            args.config,
            "-o",
            str(args.output / "loader.img"),
            "-r",
            str(args.output / "report.txt"),
        ],
        check=True,
    )


if __name__ == "__main__":
    main()
