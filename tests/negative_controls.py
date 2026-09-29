#!/usr/bin/env python3
"""Prove the isolation test rejects a leaked mapping and an unrelated fault.

Builds disposable copies of the experiment. Never changes the normal image,
system description, SDK, or UTM bundle. Run ./dev setup first.
"""
from pathlib import Path
import shutil
import subprocess
import tempfile
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]


def check_control(name, mutate, required_failure):
    with tempfile.TemporaryDirectory(prefix="sel4-fault-control-") as temporary:
        checkout = Path(temporary)
        for filename in ["dev", "toolchain.lock.json", "system.system"]:
            shutil.copy2(ROOT / filename, checkout / filename)
        (checkout / "tests").mkdir()
        for filename in ["merge_storage_system.py", "storage_meta.py"]:
            shutil.copy2(ROOT / "tests" / filename, checkout / "tests" / filename)
        shutil.copytree(ROOT / "src", checkout / "src")
        shutil.copytree(ROOT / "rust", checkout / "rust")
        (checkout / ".cache").symlink_to(ROOT / ".cache", target_is_directory=True)
        mutate(checkout)
        result = subprocess.run([str(checkout / "dev"), "test", "--timeout", "5"],
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                text=True, timeout=30)
        logs = ROOT / "build/negative-controls"
        logs.mkdir(parents=True, exist_ok=True)
        (logs / f"{name}.log").write_text(result.stdout)
        serial_path = checkout / "build/serial.log"
        serial = serial_path.read_text() if serial_path.exists() else ""
        # A compiler failure, timeout, or missing tool is not a successful control.
        # Use the guest trace for markers: the host error also lists missing
        # expected strings, including the success marker, in its diagnostic.
        if (result.returncode == 0 or required_failure not in serial or
                "Error: Boot test failed" not in result.stdout or
                "ISOLATION COMPLETE:" in serial):
            raise RuntimeError(f"Control {name} did not fail as intended; see {logs / (name + '.log')}")
        print(f"PASS: {name} was rejected by the boot test")


def leak_mapping(checkout):
    system = checkout / "system.system"
    tree = ET.parse(system)
    crasher = tree.find(".//protection_domain[@name='crasher']")
    if crasher is None:
        raise RuntimeError("Could not locate crasher PD")
    ET.SubElement(crasher, "map", mr="server_private", vaddr="0x40000000", perms="rw")
    tree.write(system, encoding="unicode")


def wrong_address(checkout):
    source = checkout / "rust/src/crasher.rs"
    original = source.read_text()
    old = "const SERVER_PRIVATE_ADDRESS: usize = 0x4000_0000;"
    if original.count(old) != 1:
        raise RuntimeError("Could not locate faulting store address")
    source.write_text(original.replace(old, "const SERVER_PRIVATE_ADDRESS: usize = 0x4000_1000;"))


if __name__ == "__main__":
    check_control("leaked-writable-mapping", leak_mapping,
                  "FAIL: forbidden write unexpectedly succeeded")
    check_control("wrong-fault-address", wrong_address,
                  "FAIL: unexpected child fault; isolation test aborted")
    print("Both negative controls passed; the normal build and UTM image were untouched.")
