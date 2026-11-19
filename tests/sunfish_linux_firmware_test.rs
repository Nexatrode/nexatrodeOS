#![allow(dead_code)] // Shared legacy PAS/metadata APIs have separate hardware-lifecycle suites.
#[path = "../rust/src/qcom_mdt.rs"]
mod qcom_mdt;
#[path = "../rust/src/qcom_mdt_packed.rs"]
mod qcom_mdt_packed;
#[path = "../rust/src/qcom_modem_image.rs"]
mod qcom_modem_image;
#[path = "../rust/src/qcom_pas_init.rs"]
mod qcom_pas_init;
#[path = "../rust/src/qcom_pas_mem_setup.rs"]
mod qcom_pas_mem_setup;
#[path = "../rust/src/qcom_pas_metadata.rs"]
mod qcom_pas_metadata;
#[path = "../rust/src/sunfish_ipa_fws.rs"]
mod sunfish_ipa_fws;
#[path = "../rust/src/sunfish_linux_firmware.rs"]
mod sunfish_linux_firmware;
use qcom_mdt_packed::{Error, Packed};

fn word(b: &mut [u8], at: usize, n: u32) {
    b[at..at + 4].copy_from_slice(&n.to_le_bytes());
}
fn ph(b: &mut [u8], i: usize, t: u32, off: u32, pa: u32, file: u32, mem: u32, flags: u32) {
    let at = 52 + i * 32;
    for (d, v) in [
        (0, t),
        (4, off),
        (12, pa),
        (16, file),
        (20, mem),
        (24, flags),
    ] {
        word(b, at + d, v);
    }
}
fn fixture() -> Vec<u8> {
    let mut b = vec![0; 0x204];
    b[..7].copy_from_slice(b"\x7fELF\x01\x01\x01");
    b[18..20].copy_from_slice(&164u16.to_le_bytes());
    word(&mut b, 20, 1);
    word(&mut b, 28, 52);
    b[40..42].copy_from_slice(&52u16.to_le_bytes());
    b[42..44].copy_from_slice(&32u16.to_le_bytes());
    b[44..46].copy_from_slice(&4u16.to_le_bytes());
    ph(&mut b, 0, 0, 0, 0, 180, 0, 7 << 24);
    ph(&mut b, 1, 0, 0x100, 0x5000, 8, 8, 2 << 24);
    ph(&mut b, 2, 1, 0x200, 0x1000, 4, 16, 1 << 27);
    ph(&mut b, 3, 1, u32::MAX, 0x3000, 0, 32, 1 << 27);
    b[0x100..0x108].copy_from_slice(b"hashdata");
    b[0x200..].copy_from_slice(b"data");
    b
}

#[test]
fn compact_metadata_excludes_file_padding_and_does_not_rebase_elf_offsets() {
    let input = fixture();
    let p = Packed::parse(&input).unwrap();
    let mut out = [0xa5; 256];
    assert_eq!(p.metadata_parts(), (&input[..180], &input[0x100..0x108]));
    assert_eq!(p.copy_metadata(&mut out), Ok(188));
    assert_eq!(&out[..180], &input[..180]);
    assert_eq!(&out[180..188], b"hashdata");
    assert!(out[188..].iter().all(|b| *b == 0xa5));
    // Compact PAS bytes are not passed back to the packed-file parser.
    assert!(matches!(
        Packed::parse(&out[..188]),
        Err(Error::SplitOrTruncated)
    ));
    assert_eq!(qcom_mdt::parse_mdt(&out[..188]).unwrap(), *p.image());
}
#[test]
fn short_output_remains_unchanged() {
    let b = fixture();
    let p = Packed::parse(&b).unwrap();
    let mut out = [0xa5; 187];
    assert_eq!(p.copy_metadata(&mut out), Err(Error::OutputTooSmall));
    assert_eq!(out, [0xa5; 187]);
}
#[test]
fn bss_beyond_eof_is_empty_source_with_full_memory_extent() {
    let b = fixture();
    let p = Packed::parse(&b).unwrap();
    let seg = p.segments().collect::<Vec<_>>();
    assert_eq!(seg.len(), 2);
    assert_eq!(seg[0].bytes, b"data");
    assert!(seg[1].bytes.is_empty());
    assert_eq!(seg[1].descriptor.memory_size, 32);
    assert_eq!(seg[1].descriptor.index, 3);
    assert_eq!(p.image().memory_extent(), 0x3000);
}
#[test]
fn any_nonempty_range_outside_file_including_nonload_hash_is_rejected() {
    for i in 1..4 {
        let mut b = fixture();
        word(&mut b, 52 + i * 32 + 4, u32::MAX);
        word(&mut b, 52 + i * 32 + 16, 1);
        assert!(matches!(Packed::parse(&b), Err(Error::SplitOrTruncated)));
    }
    let b = fixture();
    assert!(Packed::parse(&b[..0x203]).is_err());
}
#[test]
fn bad_or_ambiguous_metadata_and_section_tables_are_rejected() {
    let mut b = fixture();
    word(&mut b, 52, 1);
    assert!(Packed::parse(&b).is_err());
    let mut b = fixture();
    word(&mut b, 52 + 16, 179);
    assert!(matches!(Packed::parse(&b), Err(Error::MetadataHeader)));
    let mut b = fixture();
    word(&mut b, 52 + 32 + 24, 0);
    assert!(matches!(Packed::parse(&b), Err(Error::MissingHash)));
    let mut b = fixture();
    word(&mut b, 52 + 3 * 32 + 24, 2 << 24);
    assert!(matches!(Packed::parse(&b), Err(Error::AmbiguousHash)));
    let mut b = fixture();
    b[48..50].copy_from_slice(&1u16.to_le_bytes());
    assert!(matches!(Packed::parse(&b), Err(Error::SectionHeaders)));
    let mut b = fixture();
    b[46..48].copy_from_slice(&40u16.to_le_bytes());
    b[48..50].copy_from_slice(&1u16.to_le_bytes());
    word(&mut b, 32, u32::MAX);
    assert!(matches!(Packed::parse(&b), Err(Error::SectionHeaders)));
}
#[test]
fn overlapping_targets_are_rejected_before_staging() {
    let mut b = fixture();
    word(&mut b, 52 + 3 * 32 + 12, 0x1008);
    assert!(matches!(Packed::parse(&b), Err(Error::Overlap)));
    assert!(matches!(
        sunfish_linux_firmware::Ipa::parse(&b),
        Err(sunfish_linux_firmware::Error::Packed(Error::Overlap))
    ));
}
struct Cpu(Vec<u8>, usize);
impl sunfish_ipa_fws::StagingMemory for Cpu {
    fn write(&mut self, at: u32, b: &[u8]) {
        self.0[at as usize..at as usize + b.len()].copy_from_slice(b);
    }
    fn zero(&mut self, at: u32, n: u32) {
        self.0[at as usize..(at + n) as usize].fill(0);
    }
    fn sync(&mut self) {
        self.1 += 1;
    }
}
#[test]
fn packed_adapter_runs_real_ipa_staging_path_preserving_gaps_and_zeroing_bss_only() {
    let b = fixture();
    let image = sunfish_linux_firmware::Ipa::parse(&b).unwrap();
    assert_eq!(image.plan().placement_count(), 2);
    assert_eq!(image.plan().mem_setup.unwrap().bytes, 0x3000);
    let mut cpu = Cpu(vec![0xa5; 0x10000], 0);
    let report = image.stage_after_pas_init(&mut cpu).unwrap();
    assert_eq!(
        (
            report.segments_staged,
            report.file_bytes_staged,
            report.bss_bytes_zeroed
        ),
        (2, 4, 44)
    );
    assert_eq!(&cpu.0[..4], b"data");
    assert!(cpu.0[4..16].iter().all(|b| *b == 0));
    assert!(cpu.0[16..0x2000].iter().all(|b| *b == 0xa5));
    assert!(cpu.0[0x2000..0x2020].iter().all(|b| *b == 0));
    assert!(cpu.0[0x2020..].iter().all(|b| *b == 0xa5));
    assert_eq!(cpu.1, 1);
    let mut metadata = [0; 256];
    assert_eq!(image.packed().copy_metadata(&mut metadata), Ok(188));
}
#[test]
fn ipa_region_overflow_and_wrong_modem_shape_do_not_produce_plans() {
    let mut b = fixture();
    word(&mut b, 52 + 3 * 32 + 12, 0x20000);
    assert!(matches!(
        sunfish_linux_firmware::Ipa::parse(&b),
        Err(sunfish_linux_firmware::Error::Ipa(
            sunfish_ipa_fws::Error::RelocationExtent
        ))
    ));
    let b = fixture();
    let p = Packed::parse(&b).unwrap();
    assert!(matches!(
        sunfish_linux_firmware::modem_plan(&p),
        Err(sunfish_linux_firmware::Error::Modem(
            qcom_modem_image::Error::SourcePlan
        ))
    ));
}
#[test]
fn malformed_inputs_never_panic() {
    let b = fixture();
    for n in 0..b.len() {
        let _ = Packed::parse(&b[..n]);
    }
    for at in [28, 32, 42, 44, 46, 48, 52, 68, 84, 100, 116, 132, 148, 164] {
        let mut changed = b.clone();
        changed[at..at + 4].fill(0xff);
        let _ = Packed::parse(&changed);
    }
}

#[test]
fn exact_header_plus_hash_input_uses_linux_compact_hash_shortcut() {
    let mut b = fixture();
    word(&mut b, 52 + 32 + 4, 0x80); // Declared range fits; actual packed hash follows header.
    word(&mut b, 52 + 2 * 32 + 16, 0); // BSS-only loads need no file bytes.
    b.truncate(188);
    b[180..188].copy_from_slice(b"hashdata");
    let p = Packed::parse(&b).unwrap();
    assert_eq!(p.metadata_parts().1, b"hashdata");
    let mut out = [0; 188];
    assert_eq!(p.copy_metadata(&mut out), Ok(188));
    assert_eq!(out.as_slice(), b);
}
