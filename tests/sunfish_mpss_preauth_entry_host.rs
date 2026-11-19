//! Exact pre-auth entry, native QMP/shared-memory adapters; host-only outer
//! health/factory, peer and RSC model. Each scenario runs in a fresh process.
#![allow(dead_code)]
extern crate self as personal_os_rust_server;
extern crate self as sel4_microkit;
include!(env!("SUNFISH_MPSS_PREAUTH_MODULES"));
include!(env!("SUNFISH_MPSS_PREAUTH_RSC_FIXTURE"));
#[path = "../rust/src/pixel_mpss_preauth.rs"]
mod pixel_mpss_preauth;
#[path = "sunfish_ipa_shared_fixture.rs"]
mod sunfish_ipa_shared_fixture;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
static RAM_VIEW: AtomicUsize = AtomicUsize::new(0);
static APCS_VIEW: AtomicUsize = AtomicUsize::new(0);
static BAD_PA: AtomicBool = AtomicBool::new(false);
static CONSUME: AtomicBool = AtomicBool::new(true);
pub struct Symbol(*mut u32);
impl Symbol {
    pub fn as_ptr(self) -> *mut u32 {
        self.0
    }
}
pub fn symbol(s: &str) -> Symbol {
    Symbol(match s {
        "mpss_aop_msgram" => RAM_VIEW.load(Ordering::SeqCst),
        "mpss_apcs" => APCS_VIEW.load(Ordering::SeqCst),
        _ => panic!("unknown symbol"),
    } as *mut u32)
}
pub fn physical(s: &str) -> &'static usize {
    match s {
        "mpss_aop_msgram_paddr" => &0xc300000,
        "mpss_apcs_paddr" => {
            if BAD_PA.load(Ordering::SeqCst) {
                &0x17c01000
            } else {
                &0x17c00000
            }
        }
        _ => panic!("unknown physical symbol"),
    }
}
#[macro_export]
macro_rules! memory_region_symbol {
    ($name:ident:*mut u32) => {
        $crate::symbol(stringify!($name))
    };
}
#[macro_export]
macro_rules! var {
    ($name:ident:usize=0) => {
        $crate::physical(stringify!($name))
    };
}
#[derive(Default)]
struct Display {
    rows: RefCell<BTreeMap<usize, String>>,
    metrics: RefCell<BTreeMap<String, u32>>,
}
impl Display {
    fn clear(&self) {
        self.rows.borrow_mut().clear();
        self.metrics.borrow_mut().clear();
    }
    fn replace_line(&self, n: usize, s: &str) {
        self.rows.borrow_mut().insert(n, s.into());
    }
    fn hex(&self, _: usize, s: &str, v: u32) {
        self.metrics.borrow_mut().insert(s.into(), v);
    }
    fn flush(&self) {}
}
mod pixel_rpmh_bus {
    pub type Controller = super::Fake;
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
            self.error = e;
            self.phase = Phase::Failed;
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
        s.healthy
    }
    pub fn poll_pending(s: &mut Startup) {
        s.polls += 1;
        if s.polls == s.fail_poll {
            s.healthy = false;
        }
    }
}
mod pixel_smmu_install {
    use super::*;
    static CHECKS: AtomicUsize = AtomicUsize::new(0);
    pub static FAIL_AT: AtomicUsize = AtomicUsize::new(usize::MAX);
    pub fn preboot_faults_clear() -> bool {
        let count = CHECKS.fetch_add(1, Ordering::SeqCst) + 1;
        let r = RAM_VIEW.load(Ordering::SeqCst) as *mut u32;
        if !r.is_null() {
            unsafe {
                if r.add(9).read_volatile() == qcom_qmp::QMP_UP {
                    r.add(10).write_volatile(qcom_qmp::QMP_UP);
                    r.add(5).write_volatile(qcom_qmp::QMP_UP);
                }
                if r.add(11).read_volatile() == qcom_qmp::QMP_UP {
                    r.add(12).write_volatile(qcom_qmp::QMP_UP);
                }
                if CONSUME.load(Ordering::SeqCst) && r.add(0x128 / 4).read_volatile() == 64 {
                    r.add(0x128 / 4).write_volatile(0);
                }
            }
        }
        count != FAIL_AT.load(Ordering::SeqCst)
    }
    pub struct Probe {
        pub memory: sunfish_ipa_shared_memory::SharedMemory<'static>,
        pub quiet: bool,
    }
    impl Probe {
        pub fn retained_quiet(&self) -> bool {
            self.quiet
        }
        pub fn activate_modem_smp2p(
            &mut self,
            r: sunfish_mpss_linux_smem::Prepared,
            s: sunfish_mpss_linux_smp2p_native::Platform<'static>,
        ) -> Result<(), sunfish_ipa_shared_memory::Error> {
            self.memory.activate_smp2p(r, s)
        }
        pub fn modem_smp2p_report(&self) -> Option<sunfish_mpss_linux_smp2p::Report> {
            self.memory.smp2p_report()
        }
    }
}
struct Startup {
    _modem_power: Option<pixel_mpss_preauth::Power>,
    _firmware_rsc: Option<pixel_rpmh_bus::Controller>,
    _scm_hold: sunfish_scm_linux_power::HoldOnce,
    _domain: Option<pixel_smmu_install::Probe>,
    _linux_rmtfs: Option<pixel_linux_rmtfs::Factory>,
    _modem_smem_prepared: Option<sunfish_mpss_linux_smem::Prepared>,
    healthy: bool,
    polls: usize,
    fail_poll: usize,
}
#[repr(C, align(4096))]
struct Page([u32; 1024]);
#[repr(C, align(4096))]
struct Db([u8; 0x3000]);
fn cold(g: u64) -> Result<(), sunfish_mpss_linux_smem::Error> {
    assert_eq!(g, 1);
    Ok(())
}
fn main() {
    let case = std::env::args().nth(1).unwrap();
    let ram = Box::leak(Box::new(Page([0; 1024])));
    let apcs = Box::leak(Box::new(Page([0; 1024])));
    let down = qcom_qmp::QMP_DOWN;
    ram.0[..16].copy_from_slice(&[
        qcom_qmp::QMP_MAGIC,
        1,
        0,
        qcom_qmp::QMP_UP,
        down,
        down,
        down,
        0x68,
        0xc0,
        down,
        down,
        down,
        down,
        0x2d8,
        0x128,
        0,
    ]);
    RAM_VIEW.store(ram.0.as_mut_ptr() as usize, Ordering::SeqCst);
    APCS_VIEW.store(apcs.0.as_mut_ptr() as usize, Ordering::SeqCst);
    let db = Box::leak(Box::new(Db([0; 0x3000])));
    db.0[12..16].copy_from_slice(&0x85f20000u32.to_le_bytes());
    db.0[16..20].copy_from_slice(&0x1078u32.to_le_bytes());
    let data = &mut db.0[4096..];
    data[4..8].copy_from_slice(&qcom_cmd_db::MAGIC.to_le_bytes());
    data[8..10].copy_from_slice(&3u16.to_le_bytes());
    data[12..14].copy_from_slice(&96u16.to_le_bytes());
    data[14..16].copy_from_slice(&4u16.to_le_bytes());
    for (i, (name, address, n)) in [
        ("mx.lvl", 0x30010u32, 7usize),
        ("cx.lvl", 0x30000, 8),
        ("mss.lvl", 0x30060, 9),
        ("xo.lvl", 0x30090, 0),
    ]
    .into_iter()
    .enumerate()
    {
        let at = 144 + i * 24;
        data[at..at + name.len()].copy_from_slice(name.as_bytes());
        data[at + 16..at + 20].copy_from_slice(&address.to_le_bytes());
        data[at + 20..at + 22].copy_from_slice(&(n as u16 * 2).to_le_bytes());
        data[at + 22..at + 24].copy_from_slice(&(i as u16 * 64).to_le_bytes());
        for j in 1..n {
            let at = 144 + 96 + i * 64 + j * 2;
            data[at..at + 2].copy_from_slice(&(j as u16 * 48).to_le_bytes());
        }
    }
    let dict = db.0.as_ptr().cast::<u32>();
    let dbp = unsafe { dict.add(4096 / 4) };
    let f = Box::leak(Box::new(sunfish_ipa_shared_fixture::Fixture::new()));
    let base = f.smem.as_mut_ptr().cast::<u8>();
    let window = unsafe {
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
    let mut memory = unsafe {
        sunfish_ipa_shared_memory::SharedMemory::new(
            window,
            f.imem.as_ptr().cast(),
            0x146a8000,
            8192,
        )
    }
    .unwrap();
    memory.prepare().unwrap();
    memory.attach_modem(1, cold).unwrap();
    for _ in 0..64 {
        if memory.modem_report().unwrap().phase == sunfish_mpss_linux_smem::Phase::Prepared {
            break;
        }
        memory.modem_step().unwrap();
    }
    let receipt = memory.take_modem_prepared().unwrap();
    let previous = memory.report();
    let mut io = Fake::new();
    let mut scm = sunfish_scm_linux_power::HoldOnce::new();
    scm.enable(&mut io, metadata(), 8, || {}, |_| {}).unwrap();
    let before = io.requests.len();
    let mut state = Startup {
        _modem_power: None,
        _firmware_rsc: Some(io),
        _scm_hold: scm,
        _domain: Some(pixel_smmu_install::Probe {
            memory,
            quiet: true,
        }),
        _linux_rmtfs: Some(pixel_linux_rmtfs::Factory {
            phase: pixel_linux_rmtfs::Phase::Prepared,
            error: 0,
        }),
        _modem_smem_prepared: Some(receipt),
        healthy: true,
        polls: 0,
        fail_poll: usize::MAX,
    };
    let d = Display::default();
    match case.as_str() {
        "success" | "duplicate" | "late-fault" => {}
        "earlier-error" => {
            state._linux_rmtfs.as_mut().unwrap().fault(0x55);
            d.replace_line(1, "EARLIER PROVIDER STOP");
        }
        "no-receipt" => {
            state._modem_smem_prepared = None;
            d.replace_line(1, "EARLIER SMEM STOP");
        }
        "rx-not-ready" => state.healthy = false,
        "domain-not-quiet" => state._domain.as_mut().unwrap().quiet = false,
        "cold-lost" => pixel_linux_rmtfs::invalidate(),
        "wrong-physical" => BAD_PA.store(true, Ordering::SeqCst),
        "wrong-db" => db.0[12] = 1,
        "lost-irq-after-power" => state.fail_poll = 1,
        "lost-irq-after-smp" => state.fail_poll = 2,
        "lost-irq-final" => state.fail_poll = 3,
        "aop-timeout" => CONSUME.store(false, Ordering::SeqCst),
        "rail-timeout" => state._firmware_rsc.as_mut().unwrap().timeout_at = Some(before + 8),
        "guard-during-aop" => pixel_smmu_install::FAIL_AT.store(12, Ordering::SeqCst),
        _ => panic!("unknown case"),
    }
    pixel_mpss_preauth::start(&d, &mut state, dict, dbp);
    if case == "earlier-error" || case == "no-receipt" {
        assert!(!pixel_mpss_preauth::started());
        pixel_mpss_preauth::fault(&mut state, 0x99);
        assert!(!pixel_mpss_preauth::started());
        assert!(d.rows.borrow()[&1].starts_with("EARLIER"));
        assert!(state._modem_power.is_none());
        assert_eq!(state._firmware_rsc.as_ref().unwrap().requests.len(), before);
        return;
    }
    let success = matches!(case.as_str(), "success" | "duplicate" | "late-fault");
    assert_eq!(d.metrics.borrow()["NATIVE ERROR"] == 0, success, "{case}");
    if success {
        assert_eq!(d.rows.borrow()[&1], "PREAUTH PROVIDERS PASS");
        let r = state
            ._domain
            .as_ref()
            .unwrap()
            .modem_smp2p_report()
            .unwrap();
        assert_eq!(
            (r.submitted, r.completed, r.kicks_sent, r.kicks_done),
            (16, 16, 2, 2)
        );
        assert!(!r.ready_seen);
        assert!(state._modem_smem_prepared.is_none());
        assert_eq!(
            state._modem_power.as_ref().unwrap().report().phase,
            sunfish_mpss_linux_power::Phase::Held
        );
        assert_eq!(state._firmware_rsc.as_ref().unwrap().rails, [7, 8, 6, 3]);
        assert_eq!(state._domain.as_ref().unwrap().memory.report(), previous);
        assert_eq!(state.polls, 3);
        if case == "duplicate" {
            let n = state._firmware_rsc.as_ref().unwrap().requests.len();
            pixel_mpss_preauth::start(&d, &mut state, dict, dbp);
            assert_eq!(state._firmware_rsc.as_ref().unwrap().requests.len(), n);
        }
        if case == "late-fault" {
            state._linux_rmtfs.as_mut().unwrap().fault(0x55);
            pixel_mpss_preauth::fault(&mut state, 0x99);
            pixel_mpss_preauth::show(&d, &state);
            assert_eq!(d.metrics.borrow()["NATIVE ERROR"], 0x99);
            assert_eq!(state._linux_rmtfs.as_ref().unwrap().error, 0x55);
        }
    } else {
        assert_eq!(d.rows.borrow()[&1], "PREAUTH PROVIDERS STOP");
        assert_eq!(
            state._linux_rmtfs.as_ref().unwrap().phase,
            pixel_linux_rmtfs::Phase::Failed
        );
        let n = state._firmware_rsc.as_ref().unwrap().requests.len();
        pixel_mpss_preauth::start(&d, &mut state, dict, dbp);
        assert_eq!(state._firmware_rsc.as_ref().unwrap().requests.len(), n);
    }
    println!(
        "PASS {case}: actual pre-auth entry and native owned QMP/SMEM paths; outer services/health/RSC are host models"
    );
}
