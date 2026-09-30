#!/usr/bin/env python3
"""Merge the sDDF GPU demo domains into the regular Personal OS system."""
import argparse
from pathlib import Path
import xml.etree.ElementTree as ET


def parse(path):
    parser = ET.XMLParser(target=ET.TreeBuilder(insert_comments=True))
    return ET.parse(path, parser=parser)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", type=Path, required=True)
    parser.add_argument("--graphics", type=Path, required=True)
    parser.add_argument("--input", type=Path,
                        help="optional VirtIO keyboard/tablet driver component system")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    base_tree = parse(args.base)
    graphics_root = parse(args.graphics).getroot()
    root = base_tree.getroot()
    memory_regions = {node.get("name") for node in root.findall("memory_region")}
    for region in graphics_root.findall("memory_region"):
        name = region.get("name", "")
        if name in memory_regions:
            raise RuntimeError(f"duplicate memory region while merging graphics: {name}")
        root.append(region)
        memory_regions.add(name)

    pd_names = {pd.get("name") for pd in root.findall(".//protection_domain")}
    client_renamed = False
    for component in graphics_root.findall("protection_domain"):
        name = component.get("name", "")
        if name == "client":
            name = "gpu_client"
            component.set("name", name)
            image = component.find("program_image")
            if image is None or image.get("path") != "client.elf":
                raise RuntimeError("GPU sample client must use client.elf")
            image.set("path", "gpu_client.elf")
            client_renamed = True
        if name in pd_names:
            raise RuntimeError(f"duplicate protection domain while merging graphics: {name}")
        root.append(component)
        pd_names.add(name)
    if not client_renamed:
        raise RuntimeError("graphics system has no client protection domain")

    for channel in graphics_root.findall("channel"):
        for endpoint in channel.findall("end"):
            if endpoint.get("pd") == "client":
                endpoint.set("pd", "gpu_client")
        root.append(channel)

    if args.input is not None:
        input_root = parse(args.input).getroot()
        for region in input_root.findall("memory_region"):
            name = region.get("name", "")
            if name in memory_regions:
                raise RuntimeError(f"duplicate memory region while merging input: {name}")
            root.append(region)
            memory_regions.add(name)
        for component in input_root.findall("protection_domain"):
            name = component.get("name", "")
            if name in pd_names:
                raise RuntimeError(f"duplicate protection domain while merging input: {name}")
            root.append(component)
            pd_names.add(name)
        if "ui_focus" in memory_regions:
            gpu_client = next(
                (pd for pd in root.findall("protection_domain")
                 if pd.get("name") == "gpu_client"),
                None,
            )
            if gpu_client is None:
                raise RuntimeError("input focus region requires the graphics gpu_client domain")
            if any(mapping.get("mr") == "ui_focus" for mapping in gpu_client.findall("map")):
                raise RuntimeError("graphics client already maps the input focus region")
            gpu_client.append(ET.Element("map", {
                "mr": "ui_focus",
                "vaddr": "0x42_000_000",
                "perms": "r",
                "cached": "false",
                "setvar_vaddr": "ui_focus",
            }))
        if "ui_pointer" in memory_regions:
            gpu_client = next(
                (pd for pd in root.findall("protection_domain")
                 if pd.get("name") == "gpu_client"),
                None,
            )
            if gpu_client is None:
                raise RuntimeError("input pointer region requires the graphics gpu_client domain")
            if any(mapping.get("mr") == "ui_pointer" for mapping in gpu_client.findall("map")):
                raise RuntimeError("graphics client already maps the input pointer region")
            gpu_client.append(ET.Element("map", {
                "mr": "ui_pointer",
                "vaddr": "0x47_000_000",
                "perms": "r",
                "cached": "false",
                "setvar_vaddr": "ui_pointer",
            }))
        app_view_mappings = {
            "notes_app": ("notes_view", "0x43_000_000", "app_view"),
            "contacts_app": ("contacts_view", "0x43_000_000", "app_view"),
            "calculator_app": ("calculator_view", "0x43_000_000", "calculator_view"),
            "calendar_app": ("calendar_view", "0x43_000_000", "calendar_view"),
            "messages_app": ("messages_view", "0x43_000_000", "messages_view"),
            "settings_app": ("settings_view", "0x43_000_000", "settings_view"),
            "phone_app": ("phone_view", "0x43_000_000", "phone_view"),
            "clock": ("clock_view", "0x50100000", "clock_view"),
        }
        for app_name, (region_name, vaddr, setvar) in app_view_mappings.items():
            if region_name not in memory_regions:
                continue
            app = next(
                (pd for pd in root.findall("protection_domain")
                 if pd.get("name") == app_name),
                None,
            )
            if app is None:
                raise RuntimeError(f"app view region {region_name} requires {app_name}")
            app_mapping = next(
                (mapping for mapping in app.findall("map")
                 if mapping.get("mr") == region_name),
                None,
            )
            if app_mapping is None:
                app.append(ET.Element("map", {
                    "mr": region_name,
                    "vaddr": vaddr,
                    "perms": "rw",
                    "cached": "false",
                    "setvar_vaddr": setvar,
                }))
            elif (app_mapping.get("vaddr") != vaddr or
                  app_mapping.get("perms") != "rw" or
                  app_mapping.get("cached") != "false" or
                  app_mapping.get("setvar_vaddr") != setvar):
                raise RuntimeError(f"{app_name} has an incompatible {region_name} map")
            if gpu_client is None:
                raise RuntimeError(f"app view region {region_name} requires the graphics client")
            gpu_vaddr = {
                "notes_app": "0x43_000_000",
                "contacts_app": "0x44_000_000",
                "calculator_app": "0x45_000_000",
                "clock": "0x46_000_000",
                "calendar_app": "0x48_000_000",
                "messages_app": "0x49_000_000",
                "settings_app": "0x4a_000_000",
                "phone_app": "0x4b_000_000",
            }[app_name]
            gpu_client.append(ET.Element("map", {
                "mr": region_name,
                "vaddr": gpu_vaddr,
                "perms": "r",
                "cached": "false",
                "setvar_vaddr": {
                    "notes_app": "notes_view",
                    "contacts_app": "contacts_view",
                    "calculator_app": "calculator_view",
                    "clock": "clock_view",
                    "calendar_app": "calendar_view",
                    "messages_app": "messages_view",
                    "settings_app": "settings_view",
                    "phone_app": "phone_view",
                }[app_name],
            }))
        for channel in input_root.findall("channel"):
            for endpoint in channel.findall("end"):
                if endpoint.get("pd") not in pd_names:
                    raise RuntimeError(
                        f"input channel references missing protection domain: {endpoint.get('pd')}"
                    )
            root.append(channel)

    endpoint_ids = set()
    for mapping in root.findall(".//map"):
        if mapping.get("mr") not in memory_regions:
            raise RuntimeError(f"mapping references missing memory region: {mapping.get('mr')}")
    for channel in root.findall("channel"):
        for endpoint in channel.findall("end"):
            if endpoint.get("pd") not in pd_names:
                raise RuntimeError(f"channel references missing protection domain: {endpoint.get('pd')}")
            endpoint_key = (endpoint.get("pd"), endpoint.get("id"))
            if endpoint_key in endpoint_ids:
                raise RuntimeError(f"protection-domain channel endpoint is reused: {endpoint_key}")
            endpoint_ids.add(endpoint_key)

    args.output.parent.mkdir(parents=True, exist_ok=True)
    base_tree.write(args.output, encoding="UTF-8", xml_declaration=True)


if __name__ == "__main__":
    main()
