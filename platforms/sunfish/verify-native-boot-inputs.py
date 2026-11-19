#!/usr/bin/env python3
"""Host-only integration check of private generated native boot inputs.

Uses the real public Rust provider, RAM writes/sync and a full routed TFTP
RRQ/DATA/ACK transfer. No ADB/Fastboot/SCM/MMIO or network operations occur.
The compiled binary contains private inputs; all build output stays under
the selected private directory. Does not establish native device startup.
"""
from pathlib import Path
import argparse
import hashlib
import json
import os
import subprocess

ROOT = Path(__file__).resolve().parents[2]
HARNESS = r'''
mod generated;
use personal_os_rust_server::{qcom_modem_storage::PARTITIONS, qcom_qrtr_tftp::*, qcom_tftp};
fn main() {
    let input = generated::inputs(1);
    let sources = input.efs;
    let wlan = input.wlan;
    let mut shadows = sources.map(|s| vec![0xa5; s.len()]);
    let mut prepared = input.prepare(generated::DEVICE_BINDING,
        shadows.each_mut().map(|s| &mut s[..])).expect("private input preparation");
    assert_eq!(prepared.storage.persistence(),
        personal_os_rust_server::qcom_modem_handlers::Persistence::RamShadow);
    for (i, partition) in PARTITIONS.into_iter().enumerate() {
        let offset = sources[i].len() as u64 - 512;
        let mut read = [0; 512];
        prepared.storage.read(partition, offset, &mut read).unwrap();
        assert_eq!(&read, &sources[i][offset as usize..]);
        let changed = read.map(|b| b ^ 0x5a);
        prepared.storage.write(partition, offset, &changed).unwrap();
        prepared.storage.flush(partition).unwrap();
        prepared.storage.read(partition, offset, &mut read).unwrap();
        assert_eq!(read, changed);
        assert_ne!(&read, &sources[i][offset as usize..]);
    }
    let mut server = Server::new(1, 0, 0x5000, 0x5100, &[], prepared.wlan).unwrap();
    let peer = Peer { node: 0, port: 0x7001 };
    let mut incoming = IncomingRoute { destination_node: 1, destination_port: 0x5000, source: peer };
    let mut packet = [0; qcom_tftp::MAX_REQUEST_BYTES];
    let mut dispatch = server.receive(incoming,
        b"\0\x01/readonly/firmware/image/wlanmdsp.mbn\0octet\0", &mut packet);
    let mut received = 0;
    let mut expected_block = 1u16;
    let mut packets = 0;
    loop {
        match dispatch {
            Dispatch::Send { route, bytes } => {
                assert_eq!(route.source_node, 1);
                assert_eq!(route.destination, peer);
                assert_eq!(&packet[..2], &[0, qcom_tftp::OP_DATA as u8]);
                assert_eq!(u16::from_be_bytes([packet[2], packet[3]]), expected_block);
                let end = received + bytes - 4;
                assert_eq!(&packet[4..bytes], &wlan[received..end]);
                received = end;
                packets += 1;
                incoming.destination_port = route.source_port;
                let [hi, lo] = expected_block.to_be_bytes();
                dispatch = server.receive(incoming, &[0, qcom_tftp::OP_ACK as u8, hi, lo], &mut packet);
                expected_block = expected_block.wrapping_add(1);
            }
            Dispatch::Closed { reason: CloseReason::TransferComplete, .. } => break,
            other => panic!("unexpected TFTP dispatch: {other:?}"),
        }
        assert!(packets <= 8000);
    }
    assert_eq!(received, wlan.len());
    assert_eq!(server.active_transfers(), 0);
    println!("Verified RAM read/write/sync for four EFS shadows and full {}-byte TFTP transfer ({} DATA packets).", received, packets);
}
'''


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inputs", type=Path, required=True)
    args = parser.parse_args()
    os.umask(0o077)
    inputs = args.inputs.resolve()
    if inputs.stat().st_mode & 0o077:
        parser.error("input directory must be private (mode 0700)")
    manifest = json.loads((inputs / "manifest.json").read_text())
    paths = [inputs / name for name in manifest["files"]]
    paths.append(inputs / "private_boot_inputs.rs")
    for path in paths:
        if path.parent != inputs or path.stat().st_mode & 0o077:
            parser.error("all input files must be private and inside the selected directory")
        expected = manifest["module_sha256"] if path.name.endswith(".rs") else manifest["files"][path.name]["sha256"]
        if hashlib.sha256(path.read_bytes()).hexdigest() != expected:
            parser.error("prepared input hash mismatch: " + path.name)
    checks = inputs / "host-check"
    checks.mkdir(exist_ok=True, mode=0o700)
    (checks / "src").mkdir(exist_ok=True, mode=0o700)
    (checks / "src/main.rs").write_text(HARNESS)
    # Keep the generated include paths intact and use this module verbatim.
    (checks / "src/generated.rs").write_bytes((inputs / "private_boot_inputs.rs").read_bytes())
    (checks / "Cargo.toml").write_text(
        '[package]\nname = "sunfish-private-input-host-check"\nversion = "0.1.0"\nedition = "2024"\n'
        '[dependencies]\npersonal-os-rust-server = { path = %s, features = ["sunfish-native-wifi-private-inputs"] }\n' % json.dumps(str(ROOT / "rust")))
    result = subprocess.run(["cargo", "run", "--offline", "--manifest-path", str(checks / "Cargo.toml"),
                             "--target-dir", str(checks / "target")], cwd=checks, capture_output=True, text=True)
    (checks / "cargo-check.log").write_text(result.stdout + result.stderr)
    if result.returncode:
        print(result.stderr[-6000:])
        raise SystemExit(result.returncode)
    # Independently recheck disk sources after the real provider writes/syncs.
    for path in paths:
        expected = manifest["module_sha256"] if path.name.endswith(".rs") else manifest["files"][path.name]["sha256"]
        if hashlib.sha256(path.read_bytes()).hexdigest() != expected:
            raise SystemExit("private source changed during host check")
    report = {"result": "passed", "scope": "host CPU providers and in-memory routed TFTP; no hardware",
              "output": result.stdout.strip(), "source_manifest_sha256": hashlib.sha256((inputs / "manifest.json").read_bytes()).hexdigest(),
              "source_files_unchanged": True, "device_tested": False}
    (checks / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    print(result.stdout.strip())
    print("Private source files unchanged; no device operation.")


if __name__ == "__main__":
    main()
