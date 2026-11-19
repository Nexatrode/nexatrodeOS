use super::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use sunfish_mpss_linux_power::{self as p, Control, OnRequest};
fn db(maps: [Vec<u16>; 3]) -> Vec<u8> {
    let mut b = vec![0u8; 144 + 96 + 192];
    b[4..8].copy_from_slice(&qcom_cmd_db::MAGIC.to_le_bytes());
    let half = |b: &mut [u8], at: usize, v: u16| b[at..at + 2].copy_from_slice(&v.to_le_bytes());
    half(&mut b, 8, 3);
    half(&mut b, 12, 96);
    half(&mut b, 14, 4);
    half(&mut b, 16, 0x100);
    for (i, (name, address)) in [
        ("mx.lvl", 0x30010u32),
        ("cx.lvl", 0x30000),
        ("mss.lvl", 0x30060),
        ("xo.lvl", 0x30090),
    ]
    .into_iter()
    .enumerate()
    {
        let at = 144 + i * 24;
        b[at..at + name.len()].copy_from_slice(name.as_bytes());
        b[at + 16..at + 20].copy_from_slice(&address.to_le_bytes());
        if i < 3 {
            half(&mut b, at + 20, (maps[i].len() * 2) as u16);
            half(&mut b, at + 22, (i * 64) as u16);
            for (n, v) in maps[i].iter().enumerate() {
                half(&mut b, 144 + 96 + i * 64 + n * 2, *v);
            }
        }
    }
    b
}
fn plan() -> p::Plan {
    p::Plan::from_database(
        &qcom_cmd_db::Database::parse(&db([
            vec![0, 48, 128, 256, 384, 416, 480, 0],
            vec![0, 48, 128, 256, 384, 416, 480, 512],
            vec![0, 48, 64, 128, 192, 256, 384, 416, 480, 0],
        ]))
        .unwrap(),
    )
    .unwrap()
}
#[derive(Default)]
struct Effects {
    checks: usize,
    pre_auth: Vec<bool>,
    on: usize,
    fail_on: bool,
    deny: Option<usize>,
    panic_on: bool,
    dropped: usize,
    words: Option<[u32; 17]>,
}
struct Coordinator {
    e: Rc<RefCell<Effects>>,
    started: Rc<Cell<bool>>,
}
impl Drop for Coordinator {
    fn drop(&mut self) {
        self.e.borrow_mut().dropped += 1;
    }
}
impl Control for Coordinator {
    fn check(&mut self, g: u64, pre: bool) -> Result<(), p::Error> {
        assert_eq!(g, 91);
        let mut e = self.e.borrow_mut();
        e.checks += 1;
        e.pre_auth.push(pre);
        if e.deny == Some(e.checks) {
            Err(p::Error::Guard)
        } else {
            Ok(())
        }
    }
    fn load_state_on(&mut self, r: &OnRequest) -> Result<(), p::Error> {
        let mut e = self.e.borrow_mut();
        e.on += 1;
        assert_eq!(e.on, 1);
        assert_eq!(
            r.message(),
            "{class: image, res: load_state, name: modem, val: on}"
        );
        e.words = Some(r.words());
        assert!(!e.panic_on, "uncertain load state");
        if e.fail_on {
            return Err(p::Error::Aop);
        }
        self.started.set(true);
        Ok(())
    }
}
type Hold = p::HoldOnce<Coordinator>;
fn setup() -> (Hold, Fake, Rc<RefCell<Effects>>) {
    let e = Rc::new(RefCell::new(Effects::default()));
    let started = Rc::new(Cell::new(false));
    let mut io = Fake::new();
    io.started = Some(started.clone());
    (
        Hold::new(
            Coordinator {
                e: e.clone(),
                started,
            },
            91,
            plan(),
        ),
        io,
        e,
    )
}
fn stopped(h: &mut Hold, io: &mut Fake, e: &Rc<RefCell<Effects>>) {
    let n = io.requests.len();
    let c = e.borrow().checks;
    assert!(h.enable(io, 8, || {}, |_| {}).is_err());
    assert!(h.verify(io, 8, || {}).is_err());
    assert_eq!(n, io.requests.len());
    assert_eq!(c, e.borrow().checks);
}
#[test]
fn mpss_plan_matches_pinned_c_raw_levels_max_corner_and_xo_without_regulator_offset() {
    let r = plan().rails();
    assert_eq!(
        r.map(|r| (
            r.resource().address(),
            r.selector(),
            r.raw_level(),
            r.enable_corner()
        )),
        [
            (0x30010, 6, 480, 1),
            (0x30000, 7, 512, 1),
            (0x30060, 8, 480, 1),
            (0x30090, 3, 0, 0)
        ]
    );
    if let Some(path) = std::env::var_os("SUNFISH_MPSS_POWER_C_LEVELS") {
        let text = std::fs::read_to_string(path).unwrap();
        for line in text.lines() {
            let v: Vec<u16> = line
                .split_whitespace()
                .map(|s| s.parse().unwrap())
                .collect();
            let n = v[0] as usize;
            let map = v[1..1 + n].to_vec();
            let d = db([map.clone(), map.clone(), map]);
            let plan = p::Plan::from_database(&qcom_cmd_db::Database::parse(&d).unwrap()).unwrap();
            for rail in &plan.rails()[..3] {
                assert_eq!(
                    (rail.selector(), rail.raw_level(), rail.enable_corner()),
                    (u32::from(v[n + 1]), v[n + 2], v[n + 3] as u8)
                );
            }
        }
    }
}
#[test]
fn mpss_rejects_bad_addresses_empty_unpowered_oversized_or_odd_level_maps() {
    for map in [vec![], vec![0], vec![0, 0], vec![0; 33]] {
        let d = db([map, vec![0, 48], vec![0, 48]]);
        assert!(p::Plan::from_database(&qcom_cmd_db::Database::parse(&d).unwrap()).is_err());
    }
    for mode in 0..3 {
        let mut d = db(core::array::from_fn(|_| vec![0, 48]));
        let at = 144;
        match mode {
            0 => d[at + 16..at + 20].copy_from_slice(&0x30014u32.to_le_bytes()),
            1 => d[at + 16..at + 20].copy_from_slice(&0x50010u32.to_le_bytes()),
            _ => d[at + 20..at + 22].copy_from_slice(&3u16.to_le_bytes()),
        }
        assert!(p::Plan::from_database(&qcom_cmd_db::Database::parse(&d).unwrap()).is_err());
    }
}
#[test]
fn mpss_real_transaction_engine_applies_parent_before_child_then_mss_xo_and_retains() {
    let (mut h, mut io, e) = setup();
    let mut progress = vec![];
    h.enable(&mut io, 8, || {}, |r| progress.push(r)).unwrap();
    assert_eq!(progress.len(), 5);
    assert!(
        progress
            .iter()
            .all(|r| r.error.is_none() && r.phase != p::Phase::Failed)
    );
    let r = h.report();
    assert_eq!(r.phase, p::Phase::Held);
    assert_eq!((r.attempted, r.acknowledged, r.reads), (15, 15, 40));
    assert!(r.aop_consumed);
    let writes: Vec<_> = io.requests.iter().filter(|r| r.2).copied().collect();
    assert_eq!(
        writes,
        [
            (0x30010, 6, true),
            (0x30000, 7, true),
            (0x30060, 8, true),
            (0x30090, 3, true)
        ]
    );
    assert_eq!(io.rails, [7, 8, 6, 3]);
    assert!(e.borrow().pre_auth.iter().all(|p| *p));
    h.verify(&mut io, 8, || {}).unwrap();
    assert!(e.borrow().pre_auth.iter().any(|p| !*p));
    assert_eq!(io.requests.iter().filter(|r| r.2).count(), 4);
    if let Some(path) = std::env::var_os("SUNFISH_MPSS_POWER_C_PACKET") {
        let expected = std::fs::read(path).unwrap();
        let actual: Vec<u8> = e
            .borrow()
            .words
            .unwrap()
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        assert_eq!(actual, expected);
    }
    drop(h);
    assert_eq!(e.borrow().dropped, 0);
}
#[test]
fn mpss_aop_guard_and_invalid_budget_stop_before_rail_effects() {
    for mode in 0..3 {
        let (mut h, mut io, e) = setup();
        if mode == 0 {
            e.borrow_mut().fail_on = true;
        }
        if mode == 1 {
            e.borrow_mut().deny = Some(1);
        }
        assert!(
            h.enable(&mut io, if mode == 2 { 0 } else { 8 }, || {}, |_| {})
                .is_err()
        );
        assert!(io.requests.is_empty());
        stopped(&mut h, &mut io, &e);
    }
}
#[test]
fn mpss_every_read_and_write_timeout_is_consumed_with_no_later_request() {
    for n in 0..44 {
        let (mut h, mut io, e) = setup();
        io.timeout_at = Some(n);
        assert!(h.enable(&mut io, 8, || {}, |_| {}).is_err());
        assert_eq!(io.requests.len(), n + 1);
        stopped(&mut h, &mut io, &e);
    }
}
#[test]
fn mpss_lost_readback_bank_identity_or_runtime_floor_revokes_hold() {
    let (mut h, mut io, e) = setup();
    io.suppress = true;
    assert_eq!(h.enable(&mut io, 8, || {}, |_| {}), Err(p::Error::Readback));
    assert_eq!(io.requests.iter().filter(|r| r.2).count(), 1);
    stopped(&mut h, &mut io, &e);
    for kind in 0..3 {
        let (mut h, mut io, e) = setup();
        h.enable(&mut io, 8, || {}, |_| {}).unwrap();
        match kind {
            0 => io.rails[0] = 0,
            1 => io.words[(0xd1c + 0x2a0) / 4] = 1,
            _ => {
                let n = e.borrow().checks;
                e.borrow_mut().deny = Some(n + 1);
            }
        }
        assert!(h.verify(&mut io, 8, || {}).is_err());
        stopped(&mut h, &mut io, &e);
    }
}
#[test]
fn mpss_unwind_during_load_notification_retains_and_never_retries() {
    let (mut h, mut io, e) = setup();
    e.borrow_mut().panic_on = true;
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| h.enable(
            &mut io,
            8,
            || {},
            |_| {}
        )))
        .is_err()
    );
    assert_eq!(h.report().phase, p::Phase::Failed);
    stopped(&mut h, &mut io, &e);
    drop(h);
    assert_eq!(e.borrow().dropped, 0);
}
#[test]
fn mpss_composed_secure_lease_verifies_both_holds_without_unvoting() {
    let (mut h, mut io, _) = setup();
    io.started = None;
    let mut scm = sunfish_scm_linux_power::HoldOnce::new();
    scm.enable(&mut io, metadata(), 8, || {}, |_| {}).unwrap();
    h.enable(&mut io, 8, || {}, |_| {}).unwrap();
    let before = io.requests.iter().filter(|r| r.2).count();
    {
        let mut resources = p::Resources::new(&mut h, &scm, &mut io, 8).unwrap();
        for _ in 0..3 {
            let lease = resources.acquire().unwrap();
            resources.release(lease).unwrap();
        }
    }
    assert_eq!(before, io.requests.iter().filter(|r| r.2).count());
    assert_eq!(h.report().phase, p::Phase::Held);
    let mut resources = p::Resources::new(&mut h, &scm, &mut io, 8).unwrap();
    let lease = resources.acquire().unwrap();
    drop(resources);
    drop(lease);
    assert!(sunfish_scm_linux_power::HeldResources::new(&scm, &mut io, 8).is_err());
    let mut resources = p::Resources::new(&mut h, &scm, &mut io, 8).unwrap();
    assert!(resources.acquire().is_err());
}
#[test]
fn mpss_either_lost_hold_blocks_composed_secure_lease_before_monitor() {
    for kind in 0..2 {
        let (mut h, mut io, _) = setup();
        io.started = None;
        let mut scm = sunfish_scm_linux_power::HoldOnce::new();
        scm.enable(&mut io, metadata(), 8, || {}, |_| {}).unwrap();
        h.enable(&mut io, 8, || {}, |_| {}).unwrap();
        if kind == 0 {
            io.values[2] = 0x20000000;
        } else {
            io.rails[1] = 0;
        }
        let mut resources = p::Resources::new(&mut h, &scm, &mut io, 8).unwrap();
        assert!(resources.acquire().is_err());
        assert!(resources.acquire().is_err());
        drop(resources);
        assert_eq!(h.report().phase, p::Phase::Failed);
    }
}

#[repr(C, align(4096))]
struct Page([u32; 1024]);
struct Peer {
    ram: *mut u32,
    consume: bool,
    packets: Rc<RefCell<Vec<Vec<u8>>>>,
}
impl sunfish_mpss_linux_power_native::Gate for Peer {
    fn check(&mut self, g: u64, _: bool) -> Result<(), p::Error> {
        assert_eq!(g, 91);
        // Explicit host peer simulation. These writes are never in a native
        // image or evidence of real firmware, power, reset or IRQ delivery.
        unsafe {
            let r = self.ram;
            if r.add(9).read_volatile() == qcom_qmp::QMP_UP {
                r.add(10).write_volatile(qcom_qmp::QMP_UP);
                r.add(5).write_volatile(qcom_qmp::QMP_UP);
            }
            if r.add(11).read_volatile() == qcom_qmp::QMP_UP {
                r.add(12).write_volatile(qcom_qmp::QMP_UP);
            }
            if r.add(0x128 / 4).read_volatile() == 64 {
                let packet = (0..17)
                    .flat_map(|n| r.add(0x128 / 4 + n).read_volatile().to_le_bytes())
                    .collect();
                if self.packets.borrow().is_empty() {
                    self.packets.borrow_mut().push(packet);
                }
                if self.consume {
                    r.add(0x128 / 4).write_volatile(0);
                }
            }
        }
        Ok(())
    }
    fn wait(&mut self) -> Result<(), p::Error> {
        Ok(())
    }
}
#[test]
fn mpss_native_aop_uses_linux_64_bytes_and_only_own_descriptor_payload_doorbell() {
    for consume in [false, true] {
        let mut ram = Box::new(Page([0xeeeeeeee; 1024]));
        let mut apcs = Box::new(Page([0; 1024]));
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
        ram.0[0xc0 / 4] = 0;
        ram.0[0x128 / 4] = 0;
        let before = ram.0;
        let packets = Rc::new(RefCell::new(vec![]));
        let ptr = ram.0.as_mut_ptr();
        let native = unsafe {
            sunfish_mpss_linux_power_native::Native::new(
                ptr,
                0xc300000,
                apcs.0.as_mut_ptr(),
                0x17c00000,
                4,
                Peer {
                    ram: ptr,
                    consume,
                    packets: packets.clone(),
                },
            )
        }
        .map_err(|(_, e)| e)
        .unwrap();
        let mut h = p::HoldOnce::new(native, 91, plan());
        let mut io = Fake::new();
        let result = h.enable(&mut io, 8, || {}, |_| {});
        assert_eq!(result.is_ok(), consume);
        let report = h.control().report();
        assert_eq!(report.failed, !consume);
        assert_eq!(
            report.phase,
            if consume {
                sunfish_mpss_linux_power_native::Phase::Held
            } else {
                sunfish_mpss_linux_power_native::Phase::Consume
            }
        );
        assert_eq!((report.stores, report.notifications), (21, 3));
        assert_eq!(apcs.0[3], 1);
        assert!(apcs.0.iter().enumerate().all(|(n, v)| n == 3 || *v == 0));
        assert_eq!(packets.borrow().len(), 1);
        assert_eq!(ram.0[0x128 / 4], if consume { 0 } else { 64 });
        assert_eq!(ram.0[11], qcom_qmp::QMP_UP);
        for (n, (a, b)) in ram.0.iter().zip(before).enumerate() {
            if ![4, 5, 6, 9, 10, 11, 12].contains(&n) && !(0x128 / 4..0x128 / 4 + 17).contains(&n) {
                assert_eq!(*a, b, "foreign RAM word {n:x}");
            }
        }
        if let Some(path) = std::env::var_os("SUNFISH_MPSS_POWER_C_PACKET") {
            assert_eq!(packets.borrow()[0], std::fs::read(path).unwrap());
        }
        if !consume {
            assert!(io.requests.is_empty());
            let after = ram.0;
            assert!(h.enable(&mut io, 8, || {}, |_| {}).is_err());
            assert_eq!(ram.0, after);
        }
    }
}
