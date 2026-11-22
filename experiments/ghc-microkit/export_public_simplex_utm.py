#!/usr/bin/env python3
"""Export the public SimpleX Messages guest as a UTM virtual machine.

The account is the encrypted disk copied from
~/Library/Application Support/NexatrodeOS/simplex/disk.img on the first
export. Later exports refresh the loader and leave that UTM disk in place.
The guest resolves relays through QEMU's built-in DNS at 10.0.2.3. A host
helper types the Mac clipboard into the guest and reads copies from the
guest serial port.
"""

from __future__ import annotations

import plistlib
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BUNDLE = ROOT / "dist" / "NexatrodeOS-Messages.utm"
LOADER = ROOT / "build" / "guest-simplex-public" / "qemu-image" / "loader.img"
LIBRARY_DISK = Path.home() / "Library/Application Support/NexatrodeOS/simplex/disk.img"
DISK_ID = "C4E8A917-2B61-4D5E-8F03-91AB6C7D8E9F"
VM_ID = "B7E4A1C2-6D58-4F0A-9C33-1A2B3C4D5E6F"


def main() -> None:
    if not LOADER.is_file():
        raise SystemExit(f"Messages loader is missing: {LOADER}")
    if BUNDLE.exists() and not (BUNDLE / ".nexatrode-messages-export").exists():
        raise SystemExit(f"Refusing to overwrite an unrelated bundle: {BUNDLE}")

    data = BUNDLE / "Data"
    data.mkdir(parents=True, exist_ok=True)
    shutil.copy2(LOADER, data / "loader.img.new")
    (data / "loader.img.new").replace(data / "loader.img")

    disk = data / "personal-os-disk.img"
    if not disk.exists():
        if not LIBRARY_DISK.is_file():
            raise SystemExit(f"Saved SimpleX disk is missing: {LIBRARY_DISK}")
        shutil.copy2(LIBRARY_DISK, disk.with_name(disk.name + ".new"))
        disk.with_name(disk.name + ".new").replace(disk)

    loader = (data / "loader.img").resolve()
    profile = {
        "Backend": "QEMU",
        "ConfigurationVersion": 4,
        "Information": {
            "Name": "NexatrodeOS Messages",
            "UUID": VM_ID,
            "IconCustom": False,
            "Notes": (
                "Saved NexatrodeOS SimpleX profile. QEMU user networking reaches "
                "public relays through the built-in DNS at 10.0.2.3. Mac typing "
                "goes into the message and invitation fields. Copy and paste use "
                "nexatrode_clipboard.py. Built by export_public_simplex_utm.py."
            ),
        },
        "System": {
            "Architecture": "aarch64",
            "Target": "virt",
            "CPU": "cortex-a53",
            "CPUFlagsAdd": [],
            "CPUFlagsRemove": [],
            "CPUCount": 1,
            "ForceMulticore": False,
            "MemorySize": 2048,
            "JITCacheSize": 128,
        },
        "QEMU": {
            "DebugLog": True,
            "UEFIBoot": False,
            "RNGDevice": False,
            "BalloonDevice": False,
            "TPMDevice": False,
            "Hypervisor": False,
            "TSO": False,
            "RTCLocalTime": False,
            "PS2Controller": False,
            "MachinePropertyOverride": "virtualization=on,gic-version=2",
            "AdditionalArguments": [
                "-global virtio-mmio.force-legacy=false",
                f"-device loader,file={loader},addr=0x70000000,cpu-num=0",
                f"-device virtio-blk-device,drive=drive{DISK_ID},bus=virtio-mmio-bus.1",
                "-device virtio-rng-device,bus=virtio-mmio-bus.8",
                "-global virtio-gpu-device.edid=off",
                "-global virtio-gpu-device.blob=off",
                "-global virtio-gpu-device.max_outputs=1",
                "-global virtio-gpu-device.indirect_desc=off",
                "-global virtio-gpu-device.event_idx=off",
                "-device virtio-keyboard-device,indirect_desc=off,event_idx=off",
                "-device virtio-tablet-device,indirect_desc=off,event_idx=off",
                "-device virtio-net-device,netdev=net0,bus=virtio-mmio-bus.0",
                "-netdev user,id=net0",
                # Host-side tablet taps for Messages proof (unread clear,
                # delete, archive) without taking over the Mac display.
                "-qmp unix:B7E4A1C2-6D58-4F0A-9C33-1A2B3C4D5E6F.qmp,server,nowait",
            ],
        },
        "Input": {"UsbBusSupport": "Disabled", "UsbSharing": False, "MaximumUsbShare": 0},
        "Sharing": {
            "DirectoryShareMode": "None",
            "DirectoryShareReadOnly": True,
            "ClipboardSharing": False,
        },
        "Display": [{
            "DownscalingFilter": "Linear",
            "DynamicResolution": False,
            "Hardware": "virtio-gpu-device",
            "NativeResolution": True,
            "UpscalingFilter": "Nearest",
        }],
        "Network": [],
        "Sound": [],
        "Drive": [{
            "Identifier": DISK_ID,
            "ImageName": "personal-os-disk.img",
            "ImageType": "Disk",
            "Interface": "None",
            "InterfaceVersion": 1,
            "ReadOnly": False,
        }],
        "Serial": [{
            "Mode": "Ptty",
            "Target": "Auto",
        }],
    }
    updated = BUNDLE / "config.plist.new"
    with updated.open("wb") as stream:
        plistlib.dump(profile, stream)
    updated.replace(BUNDLE / "config.plist")
    (BUNDLE / ".nexatrode-messages-export").write_text(
        "Generated by export_public_simplex_utm.py\n"
    )
    reload_utm_configuration()
    print(f"UTM bundle: {BUNDLE}")


def reload_utm_configuration() -> None:
    """UTM keeps the serial mode in memory. A plist edit is ignored until reload."""
    script = """
tell application "UTM"
  set vm to first virtual machine whose id is "B7E4A1C2-6D58-4F0A-9C33-1A2B3C4D5E6F"
  if status of vm is stopped then
    reload configuration vm
    return "reloaded"
  end if
  return "running"
end tell
"""
    result = subprocess.run(["osascript", "-e", script], capture_output=True, text=True)
    if result.returncode != 0:
        print(result.stderr.strip())
        return
    if result.stdout.strip() == "reloaded":
        print("UTM reloaded the Messages configuration")
    else:
        print("UTM is still running this VM, so it keeps the previous serial until the next stop")


if __name__ == "__main__":
    main()
