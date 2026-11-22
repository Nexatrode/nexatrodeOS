"""Add the timer notification path to the interactive 16 KiB image."""

import sys
import xml.etree.ElementTree as ET


def add_frame_wake(tree, budget=None):
    system = tree.getroot()
    page = ET.Element("memory_region", name="frame_wake", size="0x4000", page_size="0x4000")
    system.insert(0, page)
    channel = ET.SubElement(system, "channel")
    for name, addr, perms, symbol in (
        ("slint_notes", "0x45000000", "rw", "slint_frame_wake_request"),
        ("touch_service", "0x47000000", "r", "touch_frame_wake_request"),
    ):
        pd = system.find(f"protection_domain[@name='{name}']")
        if pd is None:
            raise ValueError(f"missing protection domain {name}")
        if budget is not None:
            pd.set("budget", str(int(budget)))
        ET.SubElement(pd, "map", mr="frame_wake", vaddr=addr, perms=perms, setvar_vaddr=symbol)
        ET.SubElement(channel, "end", pd=name, id="1")
    ET.indent(tree)


if __name__ == "__main__":
    tree = ET.parse(sys.argv[1])
    add_frame_wake(tree, *sys.argv[3:])
    tree.write(sys.argv[2], encoding="utf-8", xml_declaration=True)
