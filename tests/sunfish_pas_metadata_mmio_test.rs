//! Real byte-volatile adapter against bounded canary RAM, not hardware.
extern crate self as personal_os_rust_server;
#[path = "../rust/src/pixel_pas_metadata.rs"]
mod pixel_pas_metadata;
#[path = "../rust/src/qcom_mdt.rs"]
mod qcom_mdt;
#[path = "../rust/src/qcom_pas_metadata.rs"]
mod qcom_pas_metadata;

#[test]
fn real_adapter_writes_only_one_requested_byte_and_reads_the_window_boundaries() {
    use qcom_pas_metadata::{MAPPING_SIZE, Memory};
    let mut ram = [0x5au8; MAPPING_SIZE + 2];
    let mut io = unsafe { pixel_pas_metadata::Buffer::new(ram.as_mut_ptr().wrapping_add(1)) };
    for offset in [0, 4095, 4096, MAPPING_SIZE - 1] {
        io.write_byte(offset, 0xa5);
        io.sync();
        assert_eq!(io.read_byte(offset), 0xa5);
        for (i, byte) in ram.iter().enumerate() {
            assert_eq!(*byte, if i == offset + 1 { 0xa5 } else { 0x5a });
        }
        io.write_byte(offset, 0x5a);
    }
}

#[test]
fn pinned_installed_mdt_stages_through_the_real_adapter_and_parser() {
    let data = std::fs::read(
        std::env::var("SUNFISH_MODEM_MDT_PATH").expect("builder must pin installed MDT fixture"),
    )
    .unwrap();
    let mut ram = [0u8; qcom_pas_metadata::MAPPING_SIZE];
    let mut io = unsafe { pixel_pas_metadata::Buffer::new(ram.as_mut_ptr()) };
    let r = qcom_pas_metadata::prepare(&mut io, 0x80080000, &data).unwrap();
    assert_eq!(r.metadata_length, 9100);
    assert_eq!((r.program_headers, r.loadable_segments), (31, 29));
    assert_eq!((r.linked_start, r.linked_end), (0x8b000000, 0x94800000));
}

#[test]
fn actual_adapter_inspects_zeroed_installed_payload_and_both_damaged_guards_without_writes() {
    use qcom_pas_metadata::{MAPPING_SIZE, PAGE, inspect, prepare};
    let data = std::fs::read(std::env::var("SUNFISH_MODEM_MDT_PATH").unwrap()).unwrap();
    let mut ram = [0u8; MAPPING_SIZE + 2];
    ram[0] = 0x5a;
    ram[MAPPING_SIZE + 1] = 0x5a;
    let mut io = unsafe { pixel_pas_metadata::Buffer::new(ram.as_mut_ptr().wrapping_add(1)) };
    prepare(&mut io, 0x80080000, &data).unwrap();
    ram[1 + PAGE..1 + PAGE + data.len()].fill(0);
    ram[1] = 0;
    ram[MAPPING_SIZE] = 0;
    let before = ram;
    let r = inspect(&mut io, 0x80080000, &data).unwrap();
    assert_eq!(r.payload_zero, 9100);
    assert_eq!(
        r.payload_diff,
        data.iter().filter(|v| **v != 0).count() as u32
    );
    assert_eq!(
        (
            r.low_guard_bad,
            r.high_guard_bad,
            r.padding_bad,
            r.unstable_bytes
        ),
        (1, 1, 0, 0)
    );
    assert_eq!(r.first_diff, Some(0));
    assert_eq!(r.actual_words, [0, 0]);
    assert_eq!(r.expected_words[0], 0x464c457f);
    assert_ne!(r.actual_parse_error, 0);
    assert_eq!(ram, before);
}
