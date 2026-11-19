#!/usr/bin/env python3
"""Stage private, device-bound native inputs; never talks to a phone.

The caller supplies the independently checked phone serial and device binding.
Payloads and the generated include module stay in a new private directory.
This prepares CPU-side EFS/TFTP providers, not modem/PAS/DMA readiness.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import tempfile
import uuid

PARTITIONS = (("modemst1", 2097152), ("modemst2", 2097152),
              ("fsg", 2097152), ("fsc", 131072))
WLAN_BYTES = 3751004
WLAN_SHA256 = "a2ee6c86b82b42140e39964900cb9cc10ca263085a7125002fdb9c448df94f2e"


def digest(value: object) -> str:
    if not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{64}", value) or value == "0" * 64:
        raise ValueError("invalid SHA-256 binding")
    return value


def verified_bytes(path: Path, size: int, expected: str) -> bytes:
    if path.stat().st_size != size:
        raise ValueError("input size mismatch: " + path.name)
    data = path.read_bytes()
    if len(data) != size or hashlib.sha256(data).hexdigest() != digest(expected):
        raise ValueError("input digest mismatch: " + path.name)
    return data


def rust_digest(value: str) -> str:
    return "[" + ", ".join("0x" + value[i:i+2] for i in range(0, 64, 2)) + "]"


def prepare(manifest_path: Path, wlan_path: Path, serial: str,
            device_binding: str, output: Path) -> dict:
    manifest_bytes = manifest_path.read_bytes()
    manifest = json.loads(manifest_bytes)
    binding = digest(device_binding)
    if not re.fullmatch(r"[A-Za-z0-9_-]{1,64}", serial) or manifest.get("phone_serial") != serial:
        raise ValueError("phone serial does not match selected EFS snapshot")
    if manifest.get("device_binding_sha256") != binding:
        raise ValueError("device binding does not match selected EFS snapshot")
    # This is provenance from the successful Linux run, not the future native
    # boot's generation or proof that these snapshots are the latest on disk.
    boot_id = str(uuid.UUID(manifest["linux_boot_id"]))
    if manifest.get("storage_mode") != "RAM shadow, no persistent writes":
        raise ValueError("unsupported EFS persistence policy")
    records = manifest.get("sources")
    if not isinstance(records, list) or len(records) != 4:
        raise ValueError("exactly four ordered EFS sources required")
    payloads = []
    for record, (name, size) in zip(records, PARTITIONS, strict=True):
        if not isinstance(record, dict) or record.get("partition") != name or record.get("bytes") != size:
            raise ValueError("EFS partition order or geometry mismatch")
        payloads.append(verified_bytes(Path(record["path"]), size, record["sha256"]))
    wlan = verified_bytes(wlan_path, WLAN_BYTES, WLAN_SHA256)
    # All hashes are checked before creating the output or copying any input.
    output = output.resolve()
    if output.exists():
        raise ValueError("output must be a new directory")
    output.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix=".native-inputs-", dir=output.parent))
    try:
        files = {}
        for (name, _), payload in zip(PARTITIONS, payloads, strict=True):
            files[name + ".bin"] = payload
        files["wlanmdsp.mbn"] = wlan
        for name, payload in files.items():
            (staging / name).write_bytes(payload)
            (staging / name).chmod(0o600)

        module = [
            "// Generated PRIVATE device inputs. Do not commit or redistribute.",
            "// Requires sunfish-native-wifi-private-inputs; CPU providers only.",
            "use personal_os_rust_server::qcom_modem_handlers::Disk;",
            "use personal_os_rust_server::qcom_modem_services::Partition;",
            "use personal_os_rust_server::qcom_modem_storage::{Binding, Configuration};",
            "use personal_os_rust_server::sunfish_private_boot_inputs::Inputs;",
            "pub const DEVICE_BINDING: [u8; 32] = " + rust_digest(binding) + ";",
            "pub fn inputs(generation: u64) -> Inputs<'static> {",
            "    Inputs {",
            "        configuration: Configuration { generation, device: DEVICE_BINDING, bindings: [",
        ]
        variants = ("ModemFs1", "ModemFs2", "ModemFsg", "ModemFsc")
        for i, (record, (_, size), variant) in enumerate(zip(records, PARTITIONS, variants, strict=True)):
            module.append("            Binding { partition: Partition::%s, object: %d, source: %s, disk: Disk { bytes: %d, writable: true, durable: false } }," %
                          (variant, i + 1, rust_digest(record["sha256"]), size))
        module += ["        ] },", "        efs: ["]
        for name, _ in PARTITIONS:
            module.append("            include_bytes!(%s)," % json.dumps(str(output / (name + ".bin")), ensure_ascii=False))
        module += ["        ],", "        wlan: include_bytes!(%s)," % json.dumps(str(output / "wlanmdsp.mbn"), ensure_ascii=False),
                   "    }", "}", ""]
        (staging / "private_boot_inputs.rs").write_text("\n".join(module))
        (staging / "private_boot_inputs.rs").chmod(0o600)
        result = {
            "purpose": "Owner's private Sunfish native boot inputs; no redistribution",
            "phone_serial": serial, "device_binding_sha256": binding,
            "snapshot_linux_boot_id": boot_id,
            "source_manifest_sha256": hashlib.sha256(manifest_bytes).hexdigest(),
            "storage_mode": "RAM shadow, no persistent writes",
            "files": {name: {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
                      for name, data in files.items()},
            "module_sha256": hashlib.sha256((staging / "private_boot_inputs.rs").read_bytes()).hexdigest(),
            "limitations": ["No phone identity detection by this generator or Rust provider",
                            "Separate temporary-boot serial/product verification required",
                            "Not SCM pool ownership, native modem startup, or DMA evidence"],
        }
        (staging / "manifest.json").write_text(json.dumps(result, indent=2) + "\n")
        (staging / "manifest.json").chmod(0o600)
        # Refuse replacing an existing directory; even an empty destination is
        # not silently overwritten after the initial existence check.
        if output.exists():
            raise ValueError("output appeared while preparing inputs")
        staging.rename(output)
        return result
    finally:
        if staging.exists():
            shutil.rmtree(staging)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--efs-manifest", required=True, type=Path)
    parser.add_argument("--wlan", required=True, type=Path)
    parser.add_argument("--serial", required=True)
    parser.add_argument("--device-binding-sha256", required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    try:
        result = prepare(args.efs_manifest, args.wlan, args.serial,
                         args.device_binding_sha256, args.output)
    except (OSError, ValueError, KeyError, TypeError) as error:
        parser.exit(1, "Native input preparation failed: " + str(error) + "\n")
    print("Prepared private inputs: %d verified payloads; no device operation" % len(result["files"]))


if __name__ == "__main__":
    main()
