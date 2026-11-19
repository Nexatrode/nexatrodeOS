#![allow(dead_code)]
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
#[path = "../rust/src/qcom_tftp.rs"]
mod qcom_tftp;
#[path = "../rust/src/sunfish_ipa_fws.rs"]
mod sunfish_ipa_fws;
#[path = "../rust/src/sunfish_ipa_linux_loader.rs"]
mod sunfish_ipa_linux_loader;
#[path = "../rust/src/sunfish_ipa_linux_loader_native.rs"]
mod sunfish_ipa_linux_loader_native;
#[path = "../rust/src/sunfish_ipa_linux_pas.rs"]
mod sunfish_ipa_linux_pas;
#[path = "../rust/src/sunfish_ipa_pas.rs"]
mod sunfish_ipa_pas;
#[path = "../rust/src/sunfish_linux_firmware.rs"]
mod sunfish_linux_firmware;
#[path = "../rust/src/sunfish_lpd.rs"]
mod sunfish_lpd;

#[test]
fn native_scalar_views_enforce_bounds_geometry_and_separation() {
    use sunfish_ipa_linux_loader::{Error, Memory};
    use sunfish_ipa_linux_loader_native::Window;
    #[repr(align(4096))]
    struct Metadata([u8; 8192]);
    #[repr(align(4096))]
    struct Target([u8; 65536]);
    let mut m = Box::new(Metadata([0; 8192]));
    let mut t = Box::new(Target([0xa5; 65536]));
    let mp = m.0.as_mut_ptr();
    let tp = t.0.as_mut_ptr();
    for (pages, pa, bytes, alias) in [
        ([0xa0000001, 0xa0001001], 0x98f80000, 65536, false),
        ([0xa0000000, 0xa0002000], 0x98f80000, 65536, false),
        ([0x98f80000, 0x98f81000], 0x98f80000, 65536, false),
        ([0x8611d000, 0x8611e000], 0x98f80000, 65536, false),
        ([0xa0000000, 0xa0001000], 0x98f81000, 65536, false),
        ([0xa0000000, 0xa0001000], 0x98f80000, 4096, false),
        ([0xa0000000, 0xa0001000], 0x98f80000, 65536, true),
    ] {
        assert_eq!(
            unsafe { Window::new(mp, pages, if alias { mp } else { tp }, pa, bytes) }.err(),
            Some(Error::Geometry)
        );
    }
    let mut w =
        unsafe { Window::new(mp, [0xa0000000, 0xa0001000], tp, 0x98f80000, 65536) }.unwrap();
    w.metadata_write(8191, 7).unwrap();
    w.target_write(65535, 9).unwrap();
    assert_eq!(
        (w.metadata_read(8191), w.target_read(65535)),
        (Ok(7), Ok(9))
    );
    assert_eq!(w.metadata_read(8192), Err(Error::Geometry));
    assert_eq!(w.target_write(65536, 1), Err(Error::Geometry));
    w.sync().unwrap();
    drop(w);
    assert!(m.0[..8191].iter().all(|b| *b == 0));
    assert!(t.0[..65535].iter().all(|b| *b == 0xa5));
}

#[test]
#[ignore = "requires the separately authorized private reference image path"]
fn private_reference_runs_through_native_scalar_ram_backend_without_hardware() {
    use sunfish_ipa_linux_loader::{Image, Loader, Phase};
    use sunfish_ipa_linux_loader_native::Window;
    use sunfish_ipa_linux_pas::{LinuxTransport, Resources};
    struct ModelScm;
    impl qcom_pas_init::Transport for ModelScm {
        fn invoke(&mut self, r: &qcom_pas_init::Request) -> Result<[u64; 8], u64> {
            Ok([0, u64::from(r.words()[0] == 0x42000601), 0, 0, 0, 0, 0, 0])
        }
    }
    struct ModelResources(usize);
    impl Resources for ModelResources {
        type Lease = ();
        fn acquire(&mut self) -> Result<(), u64> {
            self.0 += 1;
            Ok(())
        }
        fn release(&mut self, _: ()) -> Result<(), u64> {
            Ok(())
        }
    }
    #[repr(align(4096))]
    struct Meta([u8; 8192]);
    #[repr(align(4096))]
    struct Target([u8; 65536]);
    let bytes = std::fs::read(
        std::env::var("SUNFISH_IPA_TEST_FIRMWARE").expect("private reference path required"),
    )
    .unwrap();
    let image = Image::private_reference(&bytes).unwrap();
    let mut meta = Box::new(Meta([0; 8192]));
    let mut target = Box::new(Target([0xa5; 65536]));
    // RAM-backed stand-ins only; these addresses do not grant hardware authority.
    let window = unsafe {
        Window::new(
            meta.0.as_mut_ptr(),
            [0xa0000000, 0xa0001000],
            target.0.as_mut_ptr(),
            0x98f80000,
            65536,
        )
    }
    .unwrap();
    let mut loader = Loader::new(image, window).unwrap();
    loader.prepare().unwrap();
    let mut scm = ModelScm;
    let mut resources = ModelResources(0);
    loader
        .start(
            &mut LinuxTransport::new(&mut scm, &mut resources),
            |_| Ok(()),
            |_| {},
        )
        .unwrap();
    assert_eq!(loader.report().metadata_length, 7020);
    assert_eq!(resources.0, 3);
    assert_eq!(loader.report().phase, Phase::AuthReturned);
    assert_eq!(
        loader.report().target_writes,
        loader.report().target_verified
    );
    println!(
        "Private reference RAM-model pass: metadata={} staged/verified={} bytes; no SMC or device access",
        loader.report().metadata_length,
        loader.report().target_verified
    );
}
