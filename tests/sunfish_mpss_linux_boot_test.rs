// Actual loader/native RMTFS/QRTR owner composition in CPU RAM only.
include!("sunfish_linux_rmtfs_server_test.rs");
#[path = "../rust/src/qcom_mdt.rs"]
pub mod qcom_mdt;
#[path = "../rust/src/qcom_mdt_packed.rs"]
pub mod qcom_mdt_packed;
#[path = "../rust/src/sunfish_mpss_linux_boot.rs"]
pub mod sunfish_mpss_linux_boot;
#[path = "../rust/src/sunfish_mpss_linux_loader.rs"]
pub mod sunfish_mpss_linux_loader;

#[repr(C, align(4096))]
struct BootPage([u8; 4096]);
fn boot_native_io<G: sunfish_linux_rmtfs_native::Gate, S: sunfish_linux_rmtfs_native::Smc>(
    gate: G,
    smc: S,
    m: sunfish_linux_rmtfs::Mappings,
    buffer: &mut [BootPage],
    scratch: &mut [BootPage],
) -> sunfish_linux_rmtfs_native::Native<G, S> {
    assert_eq!(buffer.len(), 514);
    assert_eq!(scratch.len(), 3);
    assert_eq!(
        m.buffer.virtual_address,
        buffer.as_mut_ptr().wrapping_add(1) as u64
    );
    assert_eq!(
        m.scratch.virtual_address,
        scratch.as_mut_ptr().wrapping_add(1) as u64
    );
    // Host-only fixture constructor outside the forbid-unsafe loader module.
    // The scoped caller retains these exact aligned, exclusive CPU allocations
    // until its Boot callback ends; no address here refers to real hardware.
    unsafe { sunfish_linux_rmtfs_native::Native::new(gate, smc, m) }
        .unwrap_or_else(|_| panic!("native mappings"))
}
