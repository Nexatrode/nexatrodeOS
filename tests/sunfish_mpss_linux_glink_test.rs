// Retained native SMEM plus an actual loader AUTH receipt, in CPU RAM only.
include!("sunfish_mpss_linux_boot_test.rs");
#[cfg(not(feature = "sunfish-native-startup-mpss-runtime"))]
#[path = "../rust/src/qcom_modem_status.rs"]
pub mod qcom_modem_status;
#[path = "../rust/src/sunfish_ipa_gsi_profile.rs"]
#[allow(dead_code)]
pub mod sunfish_ipa_gsi_profile;
#[path = "sunfish_ipa_shared_fixture.rs"]
mod sunfish_ipa_shared_fixture;
#[path = "../rust/src/sunfish_ipa_shared_memory.rs"]
#[allow(dead_code)]
pub mod sunfish_ipa_shared_memory;
#[path = "../rust/src/sunfish_ipa_smem.rs"]
pub mod sunfish_ipa_smem;
#[path = "../rust/src/sunfish_ipa_smem_native.rs"]
pub mod sunfish_ipa_smem_native;
#[path = "../rust/src/sunfish_mpss_linux_smem.rs"]
pub mod sunfish_mpss_linux_smem;
#[path = "../rust/src/sunfish_mpss_linux_smem_native.rs"]
pub mod sunfish_mpss_linux_smem_native;
#[path = "../rust/src/sunfish_mpss_linux_smp2p.rs"]
pub mod sunfish_mpss_linux_smp2p;
#[path = "../rust/src/sunfish_mpss_linux_smp2p_native.rs"]
pub mod sunfish_mpss_linux_smp2p_native;
#[path = "../rust/src/sunfish_smmu_lpae.rs"]
pub mod sunfish_smmu_lpae;
use sunfish_mpss_linux_smp2p as smp_link;
std::thread_local! {
    static GLINK_LIFE:std::cell::Cell<(u64,smp_link::Lifecycle,smp_link::Power)>=const{
        std::cell::Cell::new((1,smp_link::Lifecycle::PreAuth,smp_link::Power::Active))};
    static GLINK_FAULT:std::cell::Cell<u8>=const{std::cell::Cell::new(0)};
}
fn link_observe(_: u64) -> Result<smp_link::Observation, smp_link::Error> {
    let (generation, lifecycle, power) = GLINK_LIFE.get();
    Ok(smp_link::Observation {
        generation,
        lifecycle,
        power,
    })
}
fn link_cold(g: u64) -> Result<(), sunfish_mpss_linux_smem::Error> {
    if g == 1 && GLINK_LIFE.get().1 == smp_link::Lifecycle::PreAuth {
        Ok(())
    } else {
        Err(sunfish_mpss_linux_smem::Error::Guard)
    }
}
struct LinkPayload<P>(P);
impl<P: sunfish_mpss_linux_smem::Payload> sunfish_mpss_linux_smem::Payload for LinkPayload<P> {
    fn guard(&mut self, g: u64) -> Result<(), sunfish_mpss_linux_smem::Error> {
        self.0.guard(g)
    }
    fn store(
        &mut self,
        r: &sunfish_mpss_linux_smem::Store,
    ) -> Result<(), sunfish_mpss_linux_smem::Error> {
        self.0.store(r)
    }
}
impl<P: smp_link::SmpIo> smp_link::SmpIo for LinkPayload<P> {
    fn claim_smp2p(&mut self) -> Result<(), smp_link::Error> {
        self.0.claim_smp2p()
    }
    fn smp_store(&mut self, r: &smp_link::Write) -> Result<(), smp_link::Error> {
        self.0.smp_store(r)
    }
}
impl<P: smp_link::GlinkPayload> smp_link::GlinkPayload for LinkPayload<P> {
    fn claim_glink(&mut self) -> Result<(), smp_link::GlinkError> {
        self.0.claim_glink()
    }
    fn bind_glink_rx(&mut self, r: &smp_link::GlinkRx) -> Result<(), smp_link::GlinkError> {
        self.0.bind_glink_rx(r)
    }
    fn glink_descriptor(&mut self, w: usize) -> Result<u32, smp_link::GlinkError> {
        self.0.glink_descriptor(w)
    }
    fn glink_store(&mut self, r: &smp_link::GlinkWrite<'_>) -> Result<(), smp_link::GlinkError> {
        let mode = GLINK_FAULT.replace(0);
        if mode == 1 {
            return Err(smp_link::GlinkError::Access);
        }
        if mode != 4 {
            self.0.glink_store(r)?;
        }
        assert_ne!(mode, 3, "uncertain native GLINK store");
        if mode == 2 {
            return Err(smp_link::GlinkError::Access);
        }
        Ok(())
    }
}
type Link<'a> = smp_link::Controller<
    LinkWindow<'a>,
    LinkPayload<
        sunfish_mpss_linux_smem_native::Native<
            'a,
            fn(u64) -> Result<(), sunfish_mpss_linux_smem::Error>,
        >,
    >,
    sunfish_mpss_linux_smp2p_native::Platform<'a>,
>;
// CPU RAM lacks the TCSR hardware's "claim only if free" semantics. Retain
// the actual native window and emulate only that lock-register operation.
struct LinkWindow<'a> {
    native: sunfish_ipa_smem_native::Window<'a>,
    lock: *mut u32,
}
impl qcom_modem_status::Read for LinkWindow<'_> {
    fn read8(&self, at: usize) -> u8 {
        self.native.read8(at)
    }
    fn read_le32(&self, at: usize) -> u32 {
        self.native.read_le32(at)
    }
}
unsafe impl sunfish_ipa_smem::Io for LinkWindow<'_> {
    fn barrier(&mut self) {
        self.native.barrier()
    }
    fn lock_read(&mut self) -> u32 {
        self.native.lock_read()
    }
    fn lock_write(&mut self, v: u32) {
        if v == 0 || unsafe { self.lock.read() } <= 1 {
            self.native.lock_write(v)
        }
    }
    fn store_metadata(&mut self, at: usize, v: u32) -> Result<(), sunfish_ipa_smem::StoreError> {
        self.native.store_metadata(at, v)
    }
}
struct LinkPeer {
    base: *mut u8,
    lock: *mut u32,
    free: usize,
    rx: Option<qcom_modem_status::Item>,
}
impl LinkPeer {
    fn word(&mut self, at: usize, v: u32) {
        assert!(at % 4 == 0 && at + 4 <= 0x200000);
        assert_eq!(unsafe { self.lock.read() }, 0);
        unsafe { self.base.add(at).cast::<u32>().write(v.to_le()) }
    }
    fn read(&self, at: usize, n: usize) -> Vec<u8> {
        assert!(at + n <= 0x200000);
        unsafe { std::slice::from_raw_parts(self.base.add(at), n) }.to_vec()
    }
    fn allocate_rx(&mut self, bytes: usize, pad: usize) {
        assert!(bytes % 8 == 0 && pad % 8 == 0 && bytes >= 8);
        if pad != 0 {
            self.add(590, pad);
        }
        self.rx = Some(self.add(480, bytes));
    }
    fn add(&mut self, id: u16, bytes: usize) -> qcom_modem_status::Item {
        let at = 0x11d000 + self.free;
        self.word(at, (u32::from(id) << 16) | 0xa5a5);
        self.word(at + 4, bytes as u32);
        self.word(at + 8, 0);
        self.free += 16 + bytes;
        self.word(0x11d00c, self.free as u32);
        qcom_modem_status::Item {
            offset: at + 16,
            size: bytes,
        }
    }
    fn peer_smp(&mut self, fatal: bool) {
        let i = self.add(435, 344);
        let mut data = [0u8; 340];
        data[..4].copy_from_slice(&0x504d5324u32.to_le_bytes());
        data[4] = 2;
        data[5] = 1;
        data[8] = 1;
        data[12] = 16;
        data[14] = 1;
        data[20..32].copy_from_slice(b"slave-kernel");
        data[36] = u8::from(fatal);
        assert_eq!(unsafe { self.lock.read() }, 0);
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), self.base.add(i.offset), 340) };
    }
    fn rx_bytes(&mut self, at: usize, data: &[u8]) {
        let i = self.rx.unwrap();
        assert!(at + data.len() <= i.size);
        assert_eq!(unsafe { self.lock.read() }, 0);
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), self.base.add(i.offset + at), data.len())
        }
    }
    fn cursor(&mut self, word: usize, v: u32) {
        assert!(word == 0 || word == 3);
        self.word(sunfish_mpss_linux_smem::ITEMS[1].1.offset + word * 4, v);
    }
    fn corrupt_owned_cursor(&mut self, word: usize, v: u32) {
        assert!(word == 1 || word == 2 || word == 4);
        self.word(sunfish_mpss_linux_smem::ITEMS[1].1.offset + word * 4, v);
    }
    fn free_lock(&self) -> u32 {
        unsafe { self.lock.read() }
    }
    fn set_foreign_lock(&mut self) {
        unsafe { self.lock.write(2) }
    }
}
fn with_link_memory(f: impl FnOnce(&mut Link<'_>, &mut LinkPeer)) {
    GLINK_LIFE.set((1, smp_link::Lifecycle::PreAuth, smp_link::Power::Active));
    GLINK_FAULT.set(0);
    let mut mem = sunfish_ipa_shared_fixture::Fixture::new();
    let mut apcs = 0u32;
    let base = mem.smem.as_mut_ptr().cast::<u8>();
    let lock = mem.lock.0.as_mut_ptr().cast::<u32>();
    // Exclusive aligned host allocations model the exact retained native
    // views. Peer helpers model remote publications between serialized calls.
    let (window, payload, signal) = unsafe {
        let window = sunfish_ipa_smem_native::Window::new_with_modem_pages(
            base,
            0x86000000,
            base.add(0x11d000).cast(),
            0x8611d000,
            lock,
            0x1f43000,
            base.add(0x11f000).cast(),
            0x8611f000,
        )
        .unwrap();
        let payload = sunfish_mpss_linux_smem_native::Native::new(
            base,
            0x86000000,
            base.add(0x11d000).cast(),
            0x8611d000,
            base.add(0x11f000).cast(),
            0x8611f000,
            link_cold as fn(u64) -> Result<(), sunfish_mpss_linux_smem::Error>,
        )
        .map_err(|(_, e)| e)
        .unwrap();
        let signal = sunfish_mpss_linux_smp2p_native::Native::new(
            &mut apcs,
            0x17c0000c,
            link_observe as fn(u64) -> Result<smp_link::Observation, smp_link::Error>,
        )
        .map_err(|(_, e)| e)
        .unwrap();
        (window, payload, signal)
    };
    let mut ipa = sunfish_ipa_smem::Owner::new(LinkWindow {
        native: window,
        lock,
    });
    ipa.allocate_once().unwrap();
    let mut ext = sunfish_mpss_linux_smem::Extension::new(ipa, LinkPayload(payload), 1);
    for _ in 0..32 {
        if ext.step().unwrap() == sunfish_mpss_linux_smem::Phase::Prepared {
            break;
        }
    }
    let prepared = ext.take_prepared().unwrap();
    let mut link = Link::new(ext, prepared, signal);
    link.initialize().unwrap();
    let mut peer = LinkPeer {
        base,
        lock,
        free: sunfish_mpss_linux_smem::FINAL_FREE,
        rx: None,
    };
    f(&mut link, &mut peer);
}
