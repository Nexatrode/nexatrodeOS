include!("sunfish_ipa_linux_receive_runtime_test.rs");
#[cfg(feature = "sunfish-linux-ipa-qmi")]
#[path = "../rust/src/qcom_qmi.rs"]
pub mod qcom_qmi;
#[cfg(feature = "sunfish-linux-ipa-qmi")]
#[path = "../rust/src/qcom_servreg.rs"]
pub mod qcom_servreg;
#[cfg(feature = "sunfish-linux-ipa-qmi")]
#[path = "../rust/src/qcom_modem_services.rs"]
pub mod qcom_modem_services;
#[cfg(feature = "sunfish-linux-ipa-qmi")]
#[path = "../rust/src/qcom_qmi_router.rs"]
pub mod qcom_qmi_router;
#[cfg(feature = "sunfish-linux-ipa-qmi")]
#[path = "../rust/src/qcom_ipa_service.rs"]
pub mod qcom_ipa_service;
#[cfg(feature = "sunfish-linux-ipa-qmi")]
#[path = "../rust/src/sunfish_ipa_linux_qmi.rs"]
pub mod sunfish_ipa_linux_qmi;
// Only this test fixture creates receipts; production receipts are delivered
// by the retained SMP2P controller after authenticated modem startup.
#[cfg(feature = "sunfish-linux-ipa-qmi")]
pub mod sunfish_mpss_linux_smp2p {
    pub struct ReadyObservation(pub u64);
    impl ReadyObservation { pub fn generation(&self) -> u64 { self.0 } }
}
#[path = "../rust/src/sunfish_ipa_linux_irq.rs"]
pub mod sunfish_ipa_linux_irq;
#[path = "../rust/src/sunfish_ipa_linux_receive_dispatch.rs"]
pub mod sunfish_ipa_linux_receive_dispatch;
#[path = "../rust/src/sunfish_ipa_linux_uc.rs"]
pub mod sunfish_ipa_linux_uc;
use sunfish_ipa_linux_irq as irq;
use sunfish_ipa_linux_receive_dispatch as dispatch;
use sunfish_ipa_linux_uc as uc;

struct ArmModel(u32);
impl irq::Io for ArmModel {
    fn guard(&mut self, _: u32) -> Result<(), irq::Error> {
        Ok(())
    }
    fn read(&mut self, r: irq::Read) -> Result<u32, irq::Error> {
        Ok(if r == irq::Read::Enable { self.0 } else { 0 })
    }
    fn write(&mut self, r: &irq::Write) -> Result<(), irq::Error> {
        self.0 = r.value();
        Ok(())
    }
    fn bind(&mut self) -> Result<(), irq::Error> {
        Ok(())
    }
    fn shared(&mut self, _: irq::Shared) -> Result<u8, irq::Error> {
        panic!("no byte read during arm")
    }
}
fn uc_armed() -> uc::Runtime {
    let mut arm = irq::ArmOnce::new();
    let r = arm.run(&mut ArmModel(0), |_| {});
    assert!(r.error.is_none());
    uc::Runtime::after_arming(&mut arm).unwrap()
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Effect {
    Write(usize, u32),
    Byte(irq::Shared, u8),
    Info(u32),
    Ack,
}
struct SharedIo {
    rx: RxIo,
    pending: u32,
    info: u32,
    event: u8,
    response: u8,
    effects: Vec<Effect>,
    inject: bool,
    endless: bool,
}
impl SharedIo {
    fn step(&mut self) -> Result<(), uc::Error> {
        if self.rx.step() {
            Ok(())
        } else {
            Err(uc::Error::Io)
        }
    }
    fn ipa_masks(&self) -> usize {
        self.effects
            .iter()
            .filter(|e| **e == Effect::Write(0x300c, 0))
            .count()
            + self
                .rx
                .base
                .writes
                .iter()
                .filter(|w| w.space == Space::Ipa && w.offset == 0x300c && w.value == 0)
                .count()
    }
    fn gsi_masks(&self) -> usize {
        self.rx
            .base
            .writes
            .iter()
            .filter(|w| w.space == Space::Gsi && w.offset == 0x1f088 && w.value == 0)
            .count()
    }
}
impl Io<fixture::Ram, RxData> for SharedIo {
    fn ticks(&mut self) -> u64 {
        Io::ticks(&mut self.rx)
    }
    fn guard(
        &mut self,
        p: &Pool<fixture::Ram>,
        d: &DataPool<RxData>,
        s: GuardState,
    ) -> Result<(), Error> {
        Io::guard(&mut self.rx, p, d, s)
    }
    fn read(&mut self, r: Read) -> Result<u32, Error> {
        Io::read(&mut self.rx, r)
    }
    fn write(&mut self, r: &Request) -> Result<(), Error> {
        Io::write(&mut self.rx, r)
    }
    fn acknowledge_start(&mut self) -> Result<(), Error> {
        Io::acknowledge_start(&mut self.rx)
    }
}
impl rt::Io<fixture::Ram, RxData> for SharedIo {
    fn acquire(&mut self) -> Result<(), rt::Error> {
        rt::Io::acquire(&mut self.rx)
    }
    fn release(&mut self) -> Result<(), rt::Error> {
        rt::Io::release(&mut self.rx)
    }
    fn write_receive(&mut self, r: &rt::Request) -> Result<(), rt::Error> {
        rt::Io::write_receive(&mut self.rx, r)
    }
    fn acknowledge_receive(&mut self) -> Result<(), rt::Error> {
        rt::Io::acknowledge_receive(&mut self.rx)
    }
}
impl dispatch::Io<fixture::Ram, RxData> for SharedIo {
    fn ipa_read(&mut self, r: irq::Read) -> Result<u32, uc::Error> {
        self.step()?;
        Ok(match r {
            irq::Read::Enable => self.rx.base.ipav(0x300c),
            irq::Read::Status => self.pending | if self.endless { 8 } else { 0 },
        })
    }
    fn ipa_write(&mut self, r: &uc::Write) -> Result<(), uc::Error> {
        self.effects.push(Effect::Write(r.offset(), r.value()));
        match r.offset() {
            0x3010 => {
                self.pending &= !r.value();
                if r.value() & 0x4000 != 0 {
                    self.info = 0;
                }
            }
            0x300c => {
                assert_eq!(r.value(), 0);
                self.rx.base.ipa.insert(0x300c, 0);
            }
            _ => panic!("foreign IPA write"),
        };
        self.step()
    }
    fn uc_byte(&mut self, f: irq::Shared) -> Result<u8, uc::Error> {
        let mask = if f == irq::Shared::Event { 4 } else { 8 };
        assert_eq!(self.pending & mask, 0, "UC byte read before clear");
        let value = if f == irq::Shared::Event {
            self.event
        } else {
            self.response
        };
        self.effects.push(Effect::Byte(f, value));
        if self.inject && f == irq::Shared::Response {
            self.inject = false;
            self.pending |= 8;
        }
        self.step()?;
        Ok(value)
    }
    fn suspend_info(&mut self) -> Result<u32, uc::Error> {
        let v = self.info;
        self.effects.push(Effect::Info(v));
        self.step()?;
        Ok(v)
    }
    fn clear_suspend(&mut self, r: &uc::SuspendWrite) -> Result<(), uc::Error> {
        self.effects
            .push(Effect::Write(uc::SUSPEND_CLEAR, r.value()));
        self.info &= !r.value();
        self.step()
    }
    fn acknowledge_ipa(&mut self) -> Result<(), uc::Error> {
        self.effects.push(Effect::Ack);
        self.step()
    }
}
// The legacy UC-only test uses its own guard and must still reject 0x400c.
impl uc::Io for SharedIo {
    fn guard(&mut self) -> Result<(), uc::Error> {
        self.step()
    }
    fn read(&mut self, r: irq::Read) -> Result<u32, uc::Error> {
        dispatch::Io::ipa_read(self, r)
    }
    fn write(&mut self, r: &uc::Write) -> Result<(), uc::Error> {
        dispatch::Io::ipa_write(self, r)
    }
    fn shared(&mut self, f: irq::Shared) -> Result<u8, uc::Error> {
        dispatch::Io::uc_byte(self, f)
    }
    fn acknowledge(&mut self) -> Result<(), uc::Error> {
        dispatch::Io::acknowledge_ipa(self)
    }
}
type Dispatch = dispatch::Runtime<fixture::Ram, RxData>;
fn shared() -> (Dispatch, SharedIo) {
    let (r, mut io) = rx_armed();
    io.base.writes.clear();
    (
        Dispatch::new(r, uc_armed()),
        SharedIo {
            rx: io,
            pending: 0x1408c,
            info: 0x10100,
            event: 2,
            response: 1,
            effects: vec![],
            inject: false,
            endless: false,
        },
    )
}
#[test]
fn retained_setup_requires_a_successful_live_guard() {
    let (runtime, mut io) = shared();
    assert_eq!(runtime.validate_retained_setup(&mut io), Ok(()));
    let writes = (io.effects.len(), io.rx.writes.len(), io.rx.base.writes.len());
    io.rx.fail = Some(io.rx.operations);
    assert_eq!(runtime.validate_retained_setup(&mut io), Err(dispatch::Error::Startup));
    assert_eq!(writes, (io.effects.len(), io.rx.writes.len(), io.rx.base.writes.len()));
}
#[cfg(feature = "sunfish-linux-ipa-qmi")]
#[test]
fn linux_ap_permit_requires_live_guard_and_binds_receipt_generation() {
    let (runtime, mut io) = shared();
    let zero = sunfish_mpss_linux_smp2p::ReadyObservation(0);
    assert!(runtime.linux_ap_service_permit(&mut io, &zero).is_err());
    let ready = sunfish_mpss_linux_smp2p::ReadyObservation(9);
    let permit = runtime.linux_ap_service_permit(&mut io, &ready).unwrap();
    assert_eq!(permit.check(9), Ok(()));
    assert_eq!(permit.check(8), Err(qcom_ipa_service::Error::StaleGeneration));
    io.rx.fail = Some(io.rx.operations);
    assert!(runtime.linux_ap_service_permit(&mut io, &ready).is_err());
}
fn shared_terminal(r: &mut Dispatch, io: &mut SharedIo) {
    assert!(r.error().is_some());
    assert!(!r.pools().0.table_setup_complete());
    assert!(!r.pools().1.prepared());
    let counts = (
        io.rx.operations,
        io.effects.len(),
        io.rx.writes.len(),
        io.rx.base.writes.len(),
        io.rx.ack,
    );
    assert_eq!(
        r.notification(dispatch::Notice::Both, io, &mut Collect::default()),
        Err(dispatch::Error::Quarantined)
    );
    assert_eq!(
        r.notification(dispatch::Notice::Fault, io, &mut Collect::default()),
        Err(dispatch::Error::Quarantined)
    );
    assert_eq!(
        (
            io.rx.operations,
            io.effects.len(),
            io.rx.writes.len(),
            io.rx.base.writes.len(),
            io.rx.ack
        ),
        counts
    );
}
#[test]
fn sharedirq_actual_owners_simultaneous_sources_match_original_linux_clear_order() {
    let (mut r, mut io) = shared();
    post(&mut io.rx, 0, 0, 0, 7);
    let mut sink = Collect::default();
    assert_eq!(
        r.notification(dispatch::Notice::Both, &mut io, &mut sink),
        Ok(1)
    );
    let rows = io
        .effects
        .iter()
        .map(|e| match e {
            Effect::Write(o, v) => format!("write {o:05x} {v:08x}\n"),
            Effect::Byte(f, v) => format!("byte {} {v}\n", f.offset()),
            Effect::Info(v) => format!("info {v:08x}\n"),
            Effect::Ack => "ack\n".to_string(),
        })
        .collect::<String>();
    assert_eq!(
        rows,
        std::fs::read_to_string(env!("SUNFISH_RECEIVE_DISPATCH_C")).unwrap()
    );
    let (uc, s) = r.ipa_reports();
    assert_eq!(
        (
            uc.events,
            uc.responses,
            uc.init_observed,
            uc.acknowledge_completed
        ),
        (1, 1, 1, 1)
    );
    assert_eq!(
        (
            s.notifications,
            s.info_reads,
            s.last_info,
            s.clears_sent,
            s.clears_done
        ),
        (1, 1, Some(0x10100), 1, 1)
    );
    assert_eq!(
        (r.receive_report().acks_done, io.rx.ack, sink.buffers),
        (1, 1, vec![0])
    );
    assert_eq!((io.ipa_masks(), io.gsi_masks()), (0, 0));
}
#[test]
fn sharedirq_fault_priority_and_gsi_errors_preserve_uc_and_rx_evidence() {
    for fault in [true, false] {
        let (mut r, mut io) = shared();
        post(&mut io.rx, 0, 0, 0, 7);
        if !fault {
            io.rx.base.values.insert(g::GLOBAL_STATUS, 1);
        }
        assert!(
            r.notification(
                if fault {
                    dispatch::Notice::Fault
                } else {
                    dispatch::Notice::Both
                },
                &mut io,
                &mut Collect::default()
            )
            .is_err()
        );
        assert!(io.effects.is_empty());
        assert_eq!((io.pending, io.info), (0x1408c, 0x10100));
        assert_eq!(io.rx.ack, 0);
        assert_eq!((io.ipa_masks(), io.gsi_masks()), (1, 1));
        shared_terminal(&mut r, &mut io);
    }
}
#[test]
fn sharedirq_uc_fatal_stops_before_suspend_or_rx_and_masks_each_engine_once() {
    let (mut r, mut io) = shared();
    post(&mut io.rx, 0, 0, 0, 7);
    io.event = 1;
    assert_eq!(
        r.notification(dispatch::Notice::Both, &mut io, &mut Collect::default()),
        Err(dispatch::Error::Ipa(uc::Error::MicrocontrollerError))
    );
    assert_eq!((io.info, io.pending & 0x4000), (0x10100, 0x4000));
    assert_eq!(io.rx.ack, 0);
    assert!(io.rx.data.0.borrow().reads.is_empty());
    assert_eq!((io.ipa_masks(), io.gsi_masks()), (1, 1));
    assert!(
        !io.effects
            .iter()
            .any(|e| matches!(e, Effect::Info(_) | Effect::Ack))
    );
    shared_terminal(&mut r, &mut io);
}
#[test]
fn sharedirq_every_guard_read_uncertain_write_byte_barrier_and_ack_failure_is_terminal() {
    let (mut r, mut io) = shared();
    post(&mut io.rx, 0, 0, 0, 7);
    r.notification(dispatch::Notice::Both, &mut io, &mut Collect::default())
        .unwrap();
    let n = io.rx.operations;
    for operation in 0..n {
        let (mut r, mut io) = shared();
        post(&mut io.rx, 0, 0, 0, 7);
        io.rx.fail = Some(operation);
        assert!(
            r.notification(dispatch::Notice::Both, &mut io, &mut Collect::default())
                .is_err(),
            "{operation}/{n}"
        );
        assert_eq!(
            (io.ipa_masks(), io.gsi_masks()),
            (1, 1),
            "mask replay {operation}"
        );
        assert_eq!(r.receive_report().acks_done, 0);
        shared_terminal(&mut r, &mut io);
    }
}
#[test]
fn sharedirq_new_uc_indication_is_drained_and_zero_info_never_has_a_clear_request() {
    let (mut r, mut io) = shared();
    io.inject = true;
    io.info = 0;
    assert_eq!(
        r.notification(dispatch::Notice::Ipa, &mut io, &mut Collect::default()),
        Ok(0)
    );
    let (uc, s) = r.ipa_reports();
    assert_eq!(
        (uc.responses, uc.init_observed, uc.acknowledge_completed),
        (2, 2, 1)
    );
    assert_eq!((s.last_info, s.clears_sent), (Some(0), 0));
    assert_eq!(io.rx.ack, 0);
    assert!(
        !io.effects
            .iter()
            .any(|e| matches!(e, Effect::Write(uc::SUSPEND_CLEAR, _)))
    );
}
#[test]
fn sharedirq_legacy_uc_mask_stays_strict_and_combined_mask_loss_is_rejected() {
    let (_, mut io) = shared();
    let mut uc = uc_armed();
    assert_eq!(uc.notification(&mut io), Err(uc::Error::EnableChanged));
    let (mut r, mut io) = shared();
    io.rx.base.ipa.insert(0x300c, 12);
    assert!(
        r.notification(dispatch::Notice::Ipa, &mut io, &mut Collect::default())
            .is_err()
    );
    assert_eq!((io.ipa_masks(), io.gsi_masks()), (1, 1));
    shared_terminal(&mut r, &mut io);
}
#[test]
fn sharedirq_endless_firmware_source_has_a_bounded_drain_and_no_ack() {
    let (mut r, mut io) = shared();
    io.endless = true;
    io.pending = 0;
    assert_eq!(
        r.notification(dispatch::Notice::Ipa, &mut io, &mut Collect::default()),
        Err(dispatch::Error::Ipa(uc::Error::DrainLimit))
    );
    assert_eq!(r.ipa_reports().0.responses, 64);
    assert_eq!(r.ipa_reports().0.acknowledge_completed, 0);
    assert_eq!((io.ipa_masks(), io.gsi_masks()), (1, 1));
    shared_terminal(&mut r, &mut io);
}
#[test]
fn sharedirq_receive_failure_does_not_replay_the_ipa_mask_or_ack_another_irq() {
    let (mut r, mut io) = shared();
    post(&mut io.rx, 0, 0, 0, 7);
    fixture::write_word(&io.rx.base.ram, 0x8004, 1);
    assert!(
        r.notification(dispatch::Notice::Gsi, &mut io, &mut Collect::default())
            .is_err()
    );
    assert!(io.effects.is_empty());
    assert_eq!((io.ipa_masks(), io.gsi_masks()), (1, 1));
    shared_terminal(&mut r, &mut io);
}

#[test]
fn sharedirq_handoff_rejects_actual_failed_uc_or_unarmed_receive_without_io() {
    for bad_uc in [false, true] {
        let (receive, mut rx) = if bad_uc {
            rx_armed()
        } else {
            let (_, once, io) = rx_prepared();
            (Runtime::new(once), io)
        };
        rx.base.writes.clear();
        let mut io = SharedIo {
            rx,
            pending: 0,
            info: 0,
            event: 2,
            response: 1,
            effects: vec![],
            inject: false,
            endless: false,
        };
        let mut uc = uc_armed();
        if bad_uc {
            uc.external_fault(&mut io);
        }
        let count = io.rx.operations;
        let effects = io.effects.len();
        let mut r = Dispatch::new(receive, uc);
        assert_eq!(r.error(), Some(dispatch::Error::Startup));
        assert_eq!((io.rx.operations, io.effects.len()), (count, effects));
        shared_terminal(&mut r, &mut io);
    }
}
