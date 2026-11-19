//! Execute the actual boot-entry module over real retained native RAM views.
//! Fake outer health/IRQ providers are host-only, never device authority.
#![allow(dead_code)]
extern crate self as personal_os_rust_server;
#[path = "../rust/src/pixel_mpss_smem.rs"]
mod pixel_mpss_smem;
#[path = "../rust/src/qcom_modem_status.rs"]
pub mod qcom_modem_status;
#[path = "../rust/src/sunfish_ipa_gsi_profile.rs"]
pub mod sunfish_ipa_gsi_profile;
mod sunfish_ipa_shared_fixture;
#[path = "../rust/src/sunfish_ipa_shared_memory.rs"]
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
#[path = "../rust/src/sunfish_smmu_lpae.rs"]
pub mod sunfish_smmu_lpae;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
use sunfish_ipa_shared_memory::SharedMemory;

#[derive(Default)]
struct Display {
    text: RefCell<BTreeMap<usize, String>>,
    metrics: RefCell<BTreeMap<String, u32>>,
}
impl Display {
    fn clear(&self) {
        self.text.borrow_mut().clear();
        self.metrics.borrow_mut().clear();
    }
    fn replace_line(&self, row: usize, value: &str) {
        self.text.borrow_mut().insert(row, value.to_string());
    }
    fn hex(&self, _row: usize, label: &str, value: u32) {
        self.metrics.borrow_mut().insert(label.to_string(), value);
    }
    fn flush(&self) {}
}
struct Startup {
    _domain: Option<pixel_smmu_install::Probe>,
    _linux_rmtfs: Option<pixel_linux_rmtfs::Factory>,
    _modem_smem_prepared: Option<sunfish_mpss_linux_smem::Prepared>,
    rx_healthy: bool,
    polls: usize,
    fail_poll: usize,
}
mod pixel_linux_rmtfs {
    use super::*;
    static COLD: AtomicBool = AtomicBool::new(true);
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum Phase {
        Prepared,
        Failed,
    }
    pub struct Factory {
        pub phase: Phase,
        pub error: u32,
    }
    impl Factory {
        pub fn phase(&self) -> Phase {
            self.phase
        }
        pub fn fault(&mut self, e: u32) {
            self.phase = Phase::Failed;
            self.error = e;
            COLD.store(false, Ordering::SeqCst);
        }
    }
    pub fn preboot_generation(g: u64) -> bool {
        g == 1 && COLD.load(Ordering::SeqCst)
    }
    pub fn invalidate() {
        COLD.store(false, Ordering::SeqCst);
    }
}
mod pixel_ipa_receive {
    use super::*;
    pub fn healthy(s: &Startup) -> bool {
        s.rx_healthy
    }
    pub fn poll_pending(s: &mut Startup) {
        s.polls += 1;
        if s.polls == s.fail_poll {
            s.rx_healthy = false;
        }
    }
}
mod pixel_smmu_install {
    use super::*;
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    pub static FAIL_AT: AtomicUsize = AtomicUsize::new(usize::MAX);
    pub struct Probe {
        pub memory: SharedMemory<'static>,
        pub quiet: bool,
    }
    pub fn preboot_faults_clear() -> bool {
        CALLS.fetch_add(1, Ordering::SeqCst) + 1 != FAIL_AT.load(Ordering::SeqCst)
    }
    impl Probe {
        pub fn retained_quiet(&self) -> bool {
            self.quiet
        }
        pub fn attach_modem_smem(
            &mut self,
            g: u64,
            check: fn(u64) -> Result<(), sunfish_mpss_linux_smem::Error>,
        ) -> Result<(), sunfish_ipa_shared_memory::Error> {
            self.memory.attach_modem(g, check)
        }
        pub fn modem_smem_report(&self) -> Option<sunfish_mpss_linux_smem::Report> {
            self.memory.modem_report()
        }
        pub fn modem_smem_step(
            &mut self,
        ) -> Result<sunfish_mpss_linux_smem::Phase, sunfish_ipa_shared_memory::Error> {
            self.memory.modem_step()
        }
        pub fn take_modem_smem_prepared(
            &mut self,
        ) -> Result<sunfish_mpss_linux_smem::Prepared, sunfish_ipa_shared_memory::Error> {
            self.memory.take_modem_prepared()
        }
    }
}
fn main() {
    let case = std::env::args()
        .nth(1)
        .expect("one isolated case per process");
    let f = Box::leak(Box::new(sunfish_ipa_shared_fixture::Fixture::new()));
    let base = f.smem.as_mut_ptr().cast::<u8>();
    // Numerical physical identities simulate the declared views in CPU RAM.
    // The actual firmware/kernel/IRQ/SMC interfaces are not linked here.
    let mut memory = unsafe {
        let window = if case == "no-writable-pages" {
            sunfish_ipa_smem_native::Window::new(
                base,
                0x86000000,
                base.add(0x11d000).cast(),
                0x8611d000,
                f.lock.0.as_mut_ptr().cast(),
                0x1f43000,
            )
        } else {
            sunfish_ipa_smem_native::Window::new_with_modem_pages(
                base,
                0x86000000,
                base.add(0x11d000).cast(),
                0x8611d000,
                f.lock.0.as_mut_ptr().cast(),
                0x1f43000,
                base.add(0x11f000).cast(),
                0x8611f000,
            )
        }
        .unwrap();
        SharedMemory::new(window, f.imem.as_ptr().cast(), 0x146a8000, 8192).unwrap()
    };
    memory.prepare().unwrap();
    let original = unsafe { std::slice::from_raw_parts(base, 0x200000) }.to_vec();
    let mut state = Startup {
        _domain: Some(pixel_smmu_install::Probe {
            memory,
            quiet: true,
        }),
        _linux_rmtfs: Some(pixel_linux_rmtfs::Factory {
            phase: pixel_linux_rmtfs::Phase::Prepared,
            error: 0,
        }),
        _modem_smem_prepared: None,
        rx_healthy: true,
        polls: 0,
        fail_poll: usize::MAX,
    };
    let expected = match case.as_str() {
        "success" | "duplicate" | "late-fault" => 0,
        "rx-not-ready" => {
            state.rx_healthy = false;
            2
        }
        "domain-not-quiet" => {
            state._domain.as_mut().unwrap().quiet = false;
            2
        }
        "storage-not-ready" => {
            state._linux_rmtfs.as_mut().unwrap().phase = pixel_linux_rmtfs::Phase::Failed;
            state._linux_rmtfs.as_mut().unwrap().error = 0x55;
            0x55
        }
        "cold-contract-lost" => {
            pixel_linux_rmtfs::invalidate();
            2
        }
        "no-writable-pages" => 3,
        "pending-irq-before-step" => {
            state.fail_poll = 1;
            4
        }
        "pending-irq-after-final-step" => {
            state.fail_poll = 17;
            6
        }
        "fault-during-word-stores" => {
            pixel_smmu_install::FAIL_AT.store(50, Ordering::SeqCst);
            5
        }
        _ => panic!("unknown case"),
    };
    let display = Display::default();
    if case == "storage-not-ready" {
        display.replace_line(1, "HOST RESOURCE STOP");
        display.hex(3, "FACTORY ERROR", 0x55);
    }
    pixel_mpss_smem::prepare(&display, &mut state);
    if case == "storage-not-ready" {
        assert_eq!(display.text.borrow()[&1], "HOST RESOURCE STOP");
        assert_eq!(display.metrics.borrow()["FACTORY ERROR"], 0x55);
        assert_eq!(state._linux_rmtfs.as_ref().unwrap().error, 0x55);
        assert!(!display.metrics.borrow().contains_key("NATIVE ERROR"));
        assert!(
            state
                ._domain
                .as_ref()
                .unwrap()
                .modem_smem_report()
                .is_none()
        );
        assert!(state._modem_smem_prepared.is_none());
        assert_eq!(
            unsafe { std::slice::from_raw_parts(base, 0x200000) },
            original
        );
        println!(
            "PASS storage-not-ready: original provider failure preserved with no extension effects"
        );
        return;
    }
    assert_eq!(display.metrics.borrow()["NATIVE ERROR"], expected, "{case}");
    assert_eq!(state._modem_smem_prepared.is_some(), expected == 0);
    let report = state._domain.as_ref().unwrap().modem_smem_report();
    if expected == 0 {
        assert_eq!(display.text.borrow()[&1], "HOST SMEM PREPARED");
        let r = report.unwrap();
        assert_eq!(
            (
                r.allocations,
                r.submitted,
                r.completed,
                r.zeroed,
                r.verified
            ),
            (3, 4201, 4201, 16756, 16756)
        );
        assert_eq!(state._modem_smem_prepared.as_ref().unwrap().generation(), 1);
        assert_eq!(state.polls, 17);
        if case == "duplicate" {
            pixel_mpss_smem::prepare(&display, &mut state);
            assert_eq!(display.metrics.borrow()["NATIVE ERROR"], 1);
        }
        if case == "late-fault" {
            state._linux_rmtfs.as_mut().unwrap().fault(0x55);
            pixel_mpss_smem::fault(&mut state, 0x99);
            pixel_mpss_smem::show(&display, &state);
            assert_eq!(display.metrics.borrow()["NATIVE ERROR"], 0x99);
            assert_eq!(state._linux_rmtfs.as_ref().unwrap().error, 0x55);
        }
    } else {
        assert_eq!(display.text.borrow()[&1], "HOST SMEM STOP");
        if expected == 2 || expected == 3 {
            assert!(report.is_none());
        }
        if case == "pending-irq-after-final-step" {
            assert_eq!(
                report.unwrap().phase,
                sunfish_mpss_linux_smem::Phase::Prepared
            );
        }
        if expected == 5 {
            let r = report.unwrap();
            assert!(r.submitted > 0 && r.submitted < 4201);
            assert_eq!(r.error, Some(sunfish_mpss_linux_smem::Error::Guard));
        }
    }
    assert_eq!(
        state._domain.as_ref().unwrap().memory.item_physical(),
        Some(0x8611d030)
    );
    let after = unsafe { std::slice::from_raw_parts(base, 0x200000) };
    for i in 0..after.len() {
        if expected == 2 || expected == 3 {
            assert_eq!(after[i], original[i], "unexpected effect at {i:x}");
            continue;
        }
        if (0x11d00c..0x11d010).contains(&i)
            || (0x11f030..0x11f03c).contains(&i)
            || (0x11f198..0x11f1a4).contains(&i)
            || (0x11f1c8..0x11f1d4).contains(&i)
            || sunfish_mpss_linux_smem::ITEMS
                .iter()
                .any(|(_, it)| (it.offset..it.offset + it.size).contains(&i))
        {
            continue;
        }
        assert_eq!(after[i], original[i], "unplanned entry overwrite at {i:x}");
    }
    println!(
        "PASS {case}: actual boot entry + retained native allocation owners in CPU RAM; no device actions"
    );
}
