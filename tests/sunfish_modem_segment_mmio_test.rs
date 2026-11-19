//! Exercise the actual byte-volatile adapter with canaries around one page.
extern crate self as personal_os_rust_server;
#[path = "../rust/src/pixel_modem_segment_memory.rs"]
mod pixel_modem_segment_memory;
#[path = "../rust/src/qcom_mdt.rs"]
mod qcom_mdt;
#[path = "../rust/src/qcom_modem_segment.rs"]
mod qcom_modem_segment;
#[path = "../rust/src/qcom_pas_init.rs"]
mod qcom_pas_init;
#[path = "../rust/src/qcom_pas_mem_setup.rs"]
mod qcom_pas_mem_setup;
#[path = "../rust/src/qcom_pas_metadata.rs"]
mod qcom_pas_metadata;

#[test]
fn actual_adapter_rejects_every_guard_write_and_stays_within_window() {
    use qcom_modem_segment::{DATA_END, DATA_START, Memory, WINDOW_BYTES};
    let mut ram = [0x5a; WINDOW_BYTES + 2];
    let mut memory =
        unsafe { pixel_modem_segment_memory::SegmentMemory::new(ram.as_mut_ptr().add(1)) };
    for at in [
        DATA_START,
        DATA_START + 4095,
        DATA_START + 4096,
        DATA_END - 1,
    ] {
        memory.write_byte(at, 0xa5);
        memory.sync();
        assert_eq!(memory.read_byte(at), 0xa5);
        for (i, byte) in ram.iter().enumerate() {
            assert_eq!(*byte, if i == at + 1 { 0xa5 } else { 0x5a });
        }
        memory.write_byte(at, 0x5a);
    }
    for at in [
        0,
        DATA_START - 1,
        DATA_END,
        WINDOW_BYTES - 1,
        WINDOW_BYTES,
        usize::MAX,
    ] {
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| memory.write_byte(at, 0)))
                .is_err()
        );
    }
    for at in [0, DATA_START - 1, DATA_END, WINDOW_BYTES - 1] {
        assert_eq!(memory.read_byte(at), 0x5a);
    }
    for at in [WINDOW_BYTES, usize::MAX] {
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| memory.read_byte(at)))
                .is_err()
        );
    }
    assert_eq!(ram, [0x5a; WINDOW_BYTES + 2]);
}
#[test]
fn installed_segment_crosses_pages_and_zeroes_bss_through_real_adapter_after_fake_setup() {
    use qcom_modem_segment::{DATA_END, DATA_START, FILE_BYTES, WINDOW_BYTES};
    let mdt = std::fs::read(std::env::var("SUNFISH_MODEM_MDT_PATH").unwrap()).unwrap();
    let source = std::fs::read(std::env::var("SUNFISH_MODEM_B14_PATH").unwrap()).unwrap();
    assert_eq!(source.len(), FILE_BYTES);
    let mut ram = [0x5a; WINDOW_BYTES + 2];
    let mut memory =
        unsafe { pixel_modem_segment_memory::SegmentMemory::new(ram.as_mut_ptr().add(1)) };
    let mut s = qcom_modem_segment::SegmentOnce::after_setup(
        qcom_pas_mem_setup::tests::completed_setup(),
        &mdt,
        &source,
    )
    .unwrap();
    s.run(&mut memory, |_| {}).unwrap();
    assert!(s.report().passed());
    assert_eq!(
        &ram[1 + DATA_START..1 + DATA_START + FILE_BYTES],
        source.as_slice()
    );
    assert!(
        ram[1 + DATA_START + FILE_BYTES..1 + DATA_END]
            .iter()
            .all(|v| *v == 0)
    );
    assert!(ram[..1 + DATA_START].iter().all(|v| *v == 0x5a));
    assert!(ram[1 + DATA_END..].iter().all(|v| *v == 0x5a));
}
