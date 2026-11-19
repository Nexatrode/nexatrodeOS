//! The actual adapter stages all installed segments into host RAM. Secure
//! completion is synthetic; this test cannot establish hardware acceptance.
extern crate self as personal_os_rust_server;
#[path = "../rust/src/pixel_modem_image_memory.rs"]
mod pixel_modem_image_memory;
#[path = "../rust/src/qcom_mdt.rs"]
mod qcom_mdt;
#[path = "../rust/src/qcom_modem_image.rs"]
mod qcom_modem_image;
#[path = "../rust/src/qcom_pas_init.rs"]
mod qcom_pas_init;
#[path = "../rust/src/qcom_pas_mem_setup.rs"]
mod qcom_pas_mem_setup;
#[path = "../rust/src/qcom_pas_metadata.rs"]
mod qcom_pas_metadata;
#[path = "../rust/src/qcom_pas_auth.rs"]
mod qcom_pas_auth;

#[test]
fn full_installed_image_uses_aligned_words_exact_bss_and_gap_snapshot_with_canaries() {
    use qcom_modem_image::{
        FILE_BYTES, GAP_BYTES, ImageOnce, LOAD_MASK, MEMORY_BYTES, Memory, POOL_BYTES, Plan,
        SNAPSHOT_BYTES, Source, ZERO_BYTES,
    };
    let directory = std::path::PathBuf::from(std::env::var("SUNFISH_MODEM_IMAGE_DIR").unwrap());
    let mdt = std::fs::read(std::env::var("SUNFISH_MODEM_MDT_PATH").unwrap()).unwrap();
    let files: Vec<_> = (2..=30)
        .map(|i| std::fs::read(directory.join(format!("modem.b{i:02}"))).unwrap())
        .collect();
    let sources: Vec<_> = files
        .iter()
        .enumerate()
        .map(|(n, b)| Source {
            index: n as u16 + 2,
            bytes: b,
        })
        .collect();
    let mut bad = sources.clone();
    bad[1].index = bad[0].index;
    assert!(Plan::new(&mdt, &bad).is_err());
    let mut bad = sources.clone();
    bad[0].bytes = &bad[0].bytes[..bad[0].bytes.len() - 1];
    assert!(Plan::new(&mdt, &bad).is_err());
    assert!(Plan::new(&mdt, &sources[..28]).is_err());
    let plan = Plan::new(&mdt, &sources).unwrap();
    let mut ram = vec![0xa5a5a5a5u32; POOL_BYTES / 4 + 2];
    let mut snapshot = vec![0x5au8; SNAPSHOT_BYTES + 2];
    let mut memory = unsafe {
        pixel_modem_image_memory::ImageMemory::new(
            ram.as_mut_ptr().cast::<u8>().add(4),
            snapshot.as_mut_ptr().add(1),
        )
    };
    for at in [1, POOL_BYTES - 3, POOL_BYTES, usize::MAX] {
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| memory.read_word(at)))
                .is_err()
        );
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| memory.write_word(at, 0)))
                .is_err()
        );
    }
    for at in [POOL_BYTES, usize::MAX] {
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| memory.read_byte(at)))
                .is_err()
        );
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| memory.write_byte(at, 0)))
                .is_err()
        );
    }
    for at in [SNAPSHOT_BYTES, usize::MAX] {
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| memory.snapshot_read(at)))
                .is_err()
        );
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| memory.snapshot_write(at, 0)))
                .is_err()
        );
    }
    let mut s = ImageOnce::after_setup(qcom_pas_mem_setup::tests::completed_setup(), plan).unwrap();
    s.run(&mut memory, |_| {}).unwrap();
    let r = s.report();
    assert!(r.passed());
    assert_eq!(r.loaded_mask, LOAD_MASK);
    assert_eq!(r.file_writes, FILE_BYTES as u32);
    assert_eq!(r.zero_writes, ZERO_BYTES as u32);
    assert_eq!(r.checked_bytes, [MEMORY_BYTES as u32; 2]);
    assert_eq!(r.gap_checked, GAP_BYTES as u32);
    assert_eq!(ram[0], 0xa5a5a5a5);
    assert_eq!(*ram.last().unwrap(), 0xa5a5a5a5);
    assert_eq!(snapshot[0], 0x5a);
    assert_eq!(*snapshot.last().unwrap(), 0x5a);
    // Use the real complete byte-staging session to authorize the next core
    // state machine. Secure return values remain synthetic in this host test.
    let prepared=qcom_pas_metadata::Prepared { metadata_physical:0x80001000,
        metadata_length:9100,program_headers:31,loadable_segments:29,
        linked_start:0x8b000000,linked_end:0x94800000 };
    let consumed=qcom_pas_mem_setup::tests::consumed();
    let mut auth=qcom_pas_auth::AuthOnce::after_image(s,prepared,&consumed).unwrap();
    struct Secure { calls:Vec<[u64;8]> }
    impl qcom_pas_init::Transport for Secure {
        fn invoke(&mut self,r:&qcom_pas_init::Request)->Result<[u64;8],u64> {
            self.calls.push(*r.words());
            Ok([0,u64::from(r.words()[0]==0x42000601),0,0,0,0,0,0])
        }
    }
    let mut secure=Secure {calls:vec![]};
    auth.check(&mut secure,|_|{}).unwrap();
    auth.authenticate(&mut secure,&consumed,|_|{}).unwrap();
    assert!(auth.report().passed());
    assert_eq!(secure.calls,vec![[0x42000601,1,0x02000205,0,0,0,0,0],
        [0x42000601,1,0x02000206,0,0,0,0,0],[0x42000205,1,4,0,0,0,0,0]]);
    assert!(auth.authenticate(&mut secure,&consumed,|_|{}).is_err());
    assert_eq!(secure.calls.len(),3);
}
