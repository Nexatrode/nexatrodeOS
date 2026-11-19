#!/usr/bin/env python3
"""Host-only check of the native packed-MBN adapters with the working Linux files.

All firmware-containing outputs stay in an explicitly private directory. Runs
real Rust parsers and placement code; no device, SCM, target memory or network.
Python independently compares compact metadata and every IPA RAM-stage byte.
"""
from pathlib import Path
import argparse
import hashlib
import json
import os
import struct
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[2]
PIN = "2267e248204e7e051e0f7355298949a6440de579"
ARCHIVE_SHA = "d55c453cea03bc7aeb3381df2538da5423fb926ce6b88dd33eadecbc12a29cb2"
EXPECTED = {
    "ipa_fws.mbn": "7d61b64288248719d14a4b316e9cb83917c11a2c9753dff5475a7a2db95394dd",
    "modem.mbn": "8489e6828dc12ed98e26dec15adb7102a31aafaf7314e249854c737ddba4cdc0",
}
HARNESS = r'''
use personal_os_rust_server::{qcom_mdt_packed::Packed, sunfish_linux_firmware, sunfish_ipa_fws::StagingMemory};
use std::{fs,path::Path};
struct Cpu { bytes: Vec<u8>, syncs: usize }
impl StagingMemory for Cpu {
    fn write(&mut self, at:u32, source:&[u8]) { self.bytes[at as usize..at as usize+source.len()].copy_from_slice(source); }
    fn zero(&mut self, at:u32, n:u32) { self.bytes[at as usize..(at+n) as usize].fill(0); }
    fn sync(&mut self) { self.syncs+=1; }
}
fn main() {
    let args:Vec<_>=std::env::args_os().collect();let dir=Path::new(&args[1]);let out=Path::new(&args[2]);
    let ipa=fs::read(dir.join("ipa_fws.mbn")).unwrap();
    let parsed=sunfish_linux_firmware::Ipa::parse(&ipa).expect("native IPA packed adapter");
    let plan=parsed.plan();assert_eq!((plan.pas_id,plan.physical_base,plan.region_bytes),(15,0x98f80000,0x10000));
    assert_eq!(plan.placement_count(),3);assert_eq!(plan.mem_setup.unwrap().bytes,0x5000);
    let mut metadata=vec![0;parsed.packed().metadata_len()];assert_eq!(parsed.packed().copy_metadata(&mut metadata),Ok(7020));
    fs::write(out.join("ipa_fws.mbn.native-metadata"),metadata).unwrap();
    let mut cpu=Cpu{bytes:vec![0xa5;0x10000],syncs:0};
    // This is an owned CPU Vec. No PAS call or physical target is substituted.
    let r=parsed.stage_after_pas_init(&mut cpu).expect("CPU-only IPA stage");
    assert_eq!((r.segments_staged,r.file_bytes_staged,r.bss_bytes_zeroed),(3,12488,4344));assert_eq!(cpu.syncs,1);
    fs::write(out.join("ipa-staged-ram.bin"),cpu.bytes).unwrap();
    let modem=fs::read(dir.join("modem.mbn")).unwrap();let packed=Packed::parse(&modem).expect("native modem packed adapter");
    assert_eq!((packed.image().program_header_count,packed.image().loadable_segment_count),(31,29));
    assert_eq!((packed.image().linked_min_address,packed.image().memory_extent()),(0x8b000000,0x9800000));
    assert_eq!(packed.segments().map(|s|s.bytes.len()).sum::<usize>(),68393962);
    let bss=packed.segments().find(|s|s.descriptor.index==30).unwrap();assert!(bss.bytes.is_empty());assert_eq!(bss.descriptor.memory_size,0x13e3000);
    let _plan=sunfish_linux_firmware::modem_plan(&packed).expect("existing exact 29-segment modem placement plan");
    let mut metadata=vec![0;packed.metadata_len()];assert_eq!(packed.copy_metadata(&mut metadata),Ok(9100));
    fs::write(out.join("modem.mbn.native-metadata"),metadata).unwrap();
    println!("CPU_ONLY: IPA metadata, three-segment staging/BSS; modem metadata and exact existing 29-segment plan passed. No PAS, modem/WLAN start, target mapping or DMA.");
}
'''


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--firmware-dir", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    args = p.parse_args()
    os.umask(0o077)
    firmware = args.firmware_dir.resolve()
    output = args.output.resolve()
    if not output.is_dir(): output.mkdir(parents=True, mode=0o700)
    if output.stat().st_mode & 0o077: p.error("output must be private (mode 0700)")
    payloads = {}
    for name, sha in EXPECTED.items():
        path = firmware / name
        if not any(parent.stat().st_uid == os.getuid() and not parent.stat().st_mode & 0o077 for parent in path.resolve().parents):
            p.error("firmware must be inside a private owned directory")
        data = path.read_bytes()
        if hashlib.sha256(data).hexdigest() != sha: p.error("wrong reference firmware: " + name)
        payloads[name] = data
    archive = ROOT / ".cache/sunfish-mobian-comparison/downloads/linux-2267e248.tar.gz"
    if hashlib.sha256(archive.read_bytes()).hexdigest() != ARCHIVE_SHA: p.error("wrong pinned kernel archive")
    paths = ["drivers/soc/qcom/mdt_loader.c", "drivers/remoteproc/qcom_q6v5_pas.c",
             "drivers/net/ipa/ipa_main.c", "arch/arm64/boot/dts/qcom/sm7150-google-sunfish.dts"]
    with tarfile.open(archive) as t:
        source = {path:t.extractfile("linux-"+PIN+"/"+path).read() for path in paths}
    for fragment in [b"if (!phdrs[i].p_filesz)", b"memcpy(data + ehdr_size, fw->data + hash_offset, hash_size)", b"memset(ptr + phdr->p_filesz, 0, phdr->p_memsz - phdr->p_filesz)"]:
        if fragment not in source[paths[0]]: p.error("pinned MDT semantics changed")
    (output / "src").mkdir(exist_ok=True, mode=0o700)
    (output / "src/main.rs").write_text(HARNESS)
    (output / "Cargo.toml").write_text('[package]\nname="sunfish-packed-firmware-host-check"\nversion="0.1.0"\nedition="2024"\n[dependencies]\npersonal-os-rust-server={path='+json.dumps(str(ROOT/"rust"))+'}\n')
    native = ["rust/src/qcom_mdt.rs", "rust/src/qcom_mdt_packed.rs", "rust/src/sunfish_linux_firmware.rs",
              "rust/src/sunfish_ipa_fws.rs", "rust/src/qcom_modem_image.rs", "rust/src/qcom_pas_metadata.rs",
              "rust/src/qcom_pas_mem_setup.rs", "rust/src/qcom_pas_init.rs", "rust/src/lib.rs",
              "rust/Cargo.toml", "rust/Cargo.lock", "platforms/sunfish/verify-native-linux-firmware.py"]
    native_hashes = {path:hashlib.sha256((ROOT/path).read_bytes()).hexdigest() for path in native}
    result = subprocess.run(["cargo", "run", "--offline", "--manifest-path", str(output/"Cargo.toml"),
                             "--target-dir", str(output/"target"), "--", str(firmware), str(output)],
                            cwd=output, capture_output=True, text=True)
    (output/"cargo-check.log").write_text(result.stdout + result.stderr)
    if result.returncode:
        print(result.stderr[-6000:]); raise SystemExit(result.returncode)
    records = {}
    for name, data in payloads.items():
        off = struct.unpack_from("<I", data, 28)[0]; size, count = struct.unpack_from("<HH", data, 42)
        assert size == 32
        ph = [struct.unpack_from("<8I", data, off + size*i) for i in range(count)]
        hashed = next(h for h in ph[1:] if h[6] & 0x7000000 == 0x2000000)
        expected_metadata = data[:ph[0][4]] + data[hashed[1]:hashed[1]+hashed[4]]
        actual = (output/(name+".native-metadata")).read_bytes()
        if actual != expected_metadata: raise ValueError("native compact metadata mismatch: " + name)
        load = [h for h in ph if h[0] == 1 and h[6] & 0x7000000 != 0x2000000 and h[5]]
        minimum = min(h[3] for h in load)
        if name == "ipa_fws.mbn":
            expected = bytearray([0xa5])*0x10000
            for h in load:
                at = h[3]-minimum
                if h[4]: expected[at:at+h[4]] = data[h[1]:h[1]+h[4]]
                expected[at+h[4]:at+h[5]] = bytes(h[5]-h[4])
            staged = (output/"ipa-staged-ram.bin").read_bytes()
            if staged != expected: raise ValueError("IPA source, BSS or untouched gap differs")
        records[name] = {"sha256":EXPECTED[name], "metadata_bytes":len(actual),
                         "metadata_sha256":hashlib.sha256(actual).hexdigest(), "loadable_segments":len(load),
                         "loadable_file_bytes":sum(h[4] for h in load), "zero_tail_bytes":sum(h[5]-h[4] for h in load),
                         "metadata_compared_every_byte":True}
        if hashlib.sha256((firmware/name).read_bytes()).hexdigest()!=EXPECTED[name]: raise ValueError("source changed during check")
    if any(hashlib.sha256((ROOT/path).read_bytes()).hexdigest() != sha for path,sha in native_hashes.items()):
        raise ValueError("native adapter source changed during compilation/check")
    report = {"result":"passed", "scope":"host-only private firmware source adapters and IPA staging in a CPU Vec; no PAS or hardware authority",
              "kernel_commit":PIN, "archive_sha256":ARCHIVE_SHA,
              "pinned_source_sha256":{path:hashlib.sha256(s).hexdigest() for path,s in source.items()},
              "native_source_sha256":native_hashes,
              "images":records, "ipa_staged_ram_bytes_compared":0x10000, "device_tested":False,
              "original_firmware_files_unchanged":True, "output":result.stdout.strip()}
    (output/"result.json").write_text(json.dumps(report,indent=2)+"\n")
    print(result.stdout.strip());print("Every compact metadata/IPA RAM byte matches independent oracle; firmware sources unchanged.")


if __name__ == "__main__": main()
