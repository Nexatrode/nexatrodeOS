//! Exercise the actual byte-volatile adapter with canaries around one page.
extern crate self as personal_os_rust_server;
#[path = "../rust/src/pixel_modem_data_page.rs"]
mod pixel_modem_data_page;
#[path = "../rust/src/qcom_mdt.rs"]
mod qcom_mdt;
#[path = "../rust/src/qcom_modem_data.rs"]
mod qcom_modem_data;
#[path = "../rust/src/qcom_pas_init.rs"]
mod qcom_pas_init;
#[path = "../rust/src/qcom_pas_mem_setup.rs"]
mod qcom_pas_mem_setup;
#[path = "../rust/src/qcom_pas_metadata.rs"]
mod qcom_pas_metadata;

#[test]
fn byte_writes_preserve_canaries_and_every_other_byte() {
    use qcom_modem_data::{PAGE_BYTES, Page};
    let mut ram = [0x5a; PAGE_BYTES + 2];
    let mut page = unsafe { pixel_modem_data_page::DataPage::new(ram.as_mut_ptr().add(1)) };
    for at in [0, 44, 45, PAGE_BYTES - 1] {
        page.write_byte(at, 0xa5);
        page.sync();
        assert_eq!(page.read_byte(at), 0xa5);
        for (i, byte) in ram.iter().enumerate() {
            assert_eq!(*byte, if i == at + 1 { 0xa5 } else { 0x5a });
        }
        page.write_byte(at, 0x5a);
    }
    for at in [PAGE_BYTES, usize::MAX] {
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| page.read_byte(at))).is_err()
        );
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| page.write_byte(at, 0)))
                .is_err()
        );
    }
    assert_eq!(ram, [0x5a; PAGE_BYTES + 2]);
}

#[test]
fn installed_non_executable_segment_copies_through_real_adapter_after_fake_setup() {
    let mdt = std::fs::read(std::env::var("SUNFISH_MODEM_MDT_PATH").unwrap()).unwrap();
    let source = std::fs::read(std::env::var("SUNFISH_MODEM_B23_PATH").unwrap()).unwrap();
    assert_eq!(source.len(), 45);
    let mut ram = [0x5a; qcom_modem_data::PAGE_BYTES + 2];
    let mut page = unsafe { pixel_modem_data_page::DataPage::new(ram.as_mut_ptr().add(1)) };
    // The secure-world side here is synthetic; only the actual data plan,
    // byte adapter and pinned installed input are exercised on host RAM.
    let mut stage = qcom_modem_data::StageOnce::after_setup(
        qcom_pas_mem_setup::tests::completed_setup(),
        &mdt,
        &source,
    )
    .unwrap();
    stage.run(&mut page, |_| {}).unwrap();
    assert!(stage.report().passed());
    assert_eq!(&ram[1..46], source.as_slice());
    assert_eq!(ram[0], 0x5a);
    assert!(ram[46..].iter().all(|v| *v == 0x5a));
}
