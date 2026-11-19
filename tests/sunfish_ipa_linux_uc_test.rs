#![allow(dead_code)]
#[path = "../rust/src/sunfish_ipa_linux_irq.rs"]
mod sunfish_ipa_linux_irq;
#[path = "../rust/src/sunfish_ipa_linux_uc.rs"]
mod sunfish_ipa_linux_uc;
use sunfish_ipa_linux_irq::{self as irq, Read, Shared};
use sunfish_ipa_linux_uc::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Op {
    Guard,
    Read(Read),
    Write(usize, u32),
    Byte(Shared),
    Ack,
}
struct Fake {
    enabled: u32,
    pending: u32,
    bytes: [u8; 128],
    ops: Vec<Op>,
    fail: Option<usize>,
    inject_on_byte: bool,
    endless: bool,
    lose_enable: bool,
    fail_mask: bool,
    suppress_mask: bool,
}
impl Fake {
    fn new(pending: u32) -> Self {
        let mut bytes = [0xa5; 128];
        bytes[12] = 1;
        bytes[20] = 2;
        Self {
            enabled: irq::ENABLED,
            pending,
            bytes,
            ops: vec![],
            fail: None,
            inject_on_byte: false,
            endless: false,
            lose_enable: false,
            fail_mask: false,
            suppress_mask: false,
        }
    }
    fn op(&mut self, op: Op) -> Result<(), Error> {
        self.ops.push(op);
        if self.fail == Some(self.ops.len() - 1) {
            Err(Error::Io)
        } else {
            Ok(())
        }
    }
}
impl Io for Fake {
    fn guard(&mut self) -> Result<(), Error> {
        self.op(Op::Guard)
    }
    fn read(&mut self, r: Read) -> Result<u32, Error> {
        self.op(Op::Read(r))?;
        Ok(match r {
            Read::Enable => {
                if self.lose_enable {
                    self.enabled ^ 0x8000
                } else {
                    self.enabled
                }
            }
            Read::Status => {
                if self.endless {
                    self.pending | irq::RESPONSE_MASK
                } else {
                    self.pending
                }
            }
        })
    }
    fn write(&mut self, w: &Write) -> Result<(), Error> {
        // The effect happens before the error, modelling uncertain stores.
        match w.offset() {
            CLEAR_OFFSET => {
                assert_ne!(w.value(), 0);
                self.pending &= !w.value();
            }
            irq::ENABLE_OFFSET => {
                assert_eq!(w.value(), 0);
                if !self.suppress_mask {
                    self.enabled = 0;
                }
            }
            _ => panic!("unapproved MMIO offset"),
        }
        let result = self.op(Op::Write(w.offset(), w.value()));
        if w.offset() == irq::ENABLE_OFFSET && self.fail_mask {
            Err(Error::Io)
        } else {
            result
        }
    }
    fn shared(&mut self, f: Shared) -> Result<u8, Error> {
        self.op(Op::Byte(f))?;
        if self.inject_on_byte && f == Shared::Response {
            self.inject_on_byte = false;
            self.pending |= irq::RESPONSE_MASK;
        }
        Ok(self.bytes[f.offset()])
    }
    fn acknowledge(&mut self) -> Result<(), Error> {
        self.op(Op::Ack)
    }
}
struct Arm {
    enabled: u32,
}
impl irq::Io for Arm {
    fn guard(&mut self, _: u32) -> Result<(), irq::Error> {
        Ok(())
    }
    fn read(&mut self, r: Read) -> Result<u32, irq::Error> {
        Ok(match r {
            Read::Enable => self.enabled,
            Read::Status => 0,
        })
    }
    fn write(&mut self, w: &irq::Write) -> Result<(), irq::Error> {
        self.enabled = w.value();
        Ok(())
    }
    fn bind(&mut self) -> Result<(), irq::Error> {
        Ok(())
    }
    fn shared(&mut self, _: Shared) -> Result<u8, irq::Error> {
        panic!("arming must not read UC bytes")
    }
}
fn armed() -> (irq::ArmOnce, Runtime) {
    let mut arm = irq::ArmOnce::new();
    let r = arm.run(&mut Arm { enabled: 0 }, |_| {});
    assert!(r.error.is_none());
    let runtime = Runtime::after_arming(&mut arm).unwrap();
    (arm, runtime)
}
fn no_retry(runtime: &mut Runtime, io: &mut Fake) {
    let r = runtime.report();
    let count = io.ops.len();
    assert_eq!(runtime.notification(io), Err(Error::Quarantined));
    runtime.external_fault(io);
    assert_eq!(io.ops.len(), count);
    assert_eq!(runtime.report(), r);
}

#[test]
fn only_a_successful_unconsumed_arm_can_transfer_once() {
    assert_eq!(
        Runtime::after_arming(&mut irq::ArmOnce::new()).err(),
        Some(Error::NotArmed)
    );
    let (mut arm, _) = armed();
    assert_eq!(Runtime::after_arming(&mut arm).err(), Some(Error::NotArmed));
    let mut io = Arm { enabled: 0 };
    assert_eq!(arm.notification(&mut io).error, irq::Error::AlreadyStopped);
    assert_eq!(io.enabled, 0);
    let mut arm = irq::ArmOnce::new();
    assert_eq!(
        arm.run(&mut Arm { enabled: 1 }, |_| {}).error,
        Some(irq::Error::Readback)
    );
    assert_eq!(Runtime::after_arming(&mut arm).err(), Some(Error::NotArmed));
}
#[test]
fn clear_each_source_before_defined_byte_then_clear_disabled_then_ack() {
    let (_, mut runtime) = armed();
    let mut io = Fake::new(irq::ENABLED | 0x10080);
    let before = io.bytes;
    runtime.notification(&mut io).unwrap();
    let effects: Vec<_> = io
        .ops
        .iter()
        .copied()
        .filter(|op| matches!(op, Op::Write(..) | Op::Byte(..) | Op::Ack))
        .collect();
    assert_eq!(
        effects,
        vec![
            Op::Write(0x3010, 4),
            Op::Byte(Shared::Event),
            Op::Write(0x3010, 8),
            Op::Byte(Shared::Response),
            Op::Write(0x3010, 0x10080),
            Op::Ack
        ]
    );
    let r = runtime.report();
    assert_eq!((r.events, r.responses, r.init_observed), (1, 1, 1));
    assert_eq!(
        (
            r.clear_submitted,
            r.clear_completed,
            r.acknowledge_completed
        ),
        (3, 3, 1)
    );
    assert_eq!((r.error, r.containment_error), (None, None));
    assert_eq!(io.bytes, before);
    assert_eq!(io.enabled, irq::ENABLED);
}
#[test]
fn new_indication_arriving_during_shared_read_is_not_lost() {
    let (_, mut runtime) = armed();
    let mut io = Fake::new(8);
    io.inject_on_byte = true;
    runtime.notification(&mut io).unwrap();
    assert_eq!(
        (runtime.report().responses, runtime.report().init_observed),
        (2, 2)
    );
    assert_eq!(runtime.report().status_reads, 3);
    assert_eq!(io.ops.iter().filter(|op| **op == Op::Ack).count(), 1);
}
#[test]
fn subsequent_notifications_keep_handler_active_and_preserve_evidence() {
    let (_, mut runtime) = armed();
    let mut io = Fake::new(8);
    runtime.notification(&mut io).unwrap();
    io.pending = 4;
    io.bytes[20] = 2;
    runtime.notification(&mut io).unwrap();
    let r = runtime.report();
    assert_eq!(
        (
            r.notifications,
            r.events,
            r.init_observed,
            r.acknowledge_completed
        ),
        (2, 1, 1, 2)
    );
    assert_eq!(r.last_response, Some(1));
    assert!(!r.mask_submitted);
}
#[test]
fn disabled_only_or_spurious_irq_does_not_read_uc_payload() {
    for pending in [0, 0x10080, 1 << 31] {
        let (_, mut runtime) = armed();
        let mut io = Fake::new(pending);
        runtime.notification(&mut io).unwrap();
        assert!(!io.ops.iter().any(|op| matches!(op, Op::Byte(_))));
        assert_eq!(runtime.report().clear_completed, u32::from(pending != 0));
        assert_eq!(runtime.report().acknowledge_completed, 1);
    }
}
#[test]
fn unsupported_codes_are_recorded_without_inventing_initialization() {
    for value in [0, 2, 3, 0xff] {
        let (_, mut runtime) = armed();
        let mut io = Fake::new(irq::ENABLED);
        io.bytes[12] = value;
        io.bytes[20] = if value == 2 { 0 } else { value };
        runtime.notification(&mut io).unwrap();
        let r = runtime.report();
        assert_eq!(r.init_observed, 0);
        assert_eq!((r.unsupported_events, r.unsupported_responses), (1, 1));
        assert_eq!(r.acknowledge_completed, 1);
    }
}
#[test]
fn uc_error_is_terminal_with_no_response_read_or_irq_ack() {
    let (_, mut runtime) = armed();
    let mut io = Fake::new(irq::ENABLED);
    io.bytes[20] = 1;
    assert_eq!(
        runtime.notification(&mut io),
        Err(Error::MicrocontrollerError)
    );
    let r = runtime.report();
    assert_eq!((r.events, r.responses, r.init_observed), (1, 0, 0));
    assert_eq!(r.last_event, Some(1));
    assert!(r.mask_completed);
    assert_eq!(r.acknowledge_submitted, 0);
    assert_eq!(io.pending, 8);
    no_retry(&mut runtime, &mut io);
}
#[test]
fn continuous_enabled_interrupts_are_bounded_and_not_acknowledged() {
    let (_, mut runtime) = armed();
    let mut io = Fake::new(8);
    io.endless = true;
    assert_eq!(runtime.notification(&mut io), Err(Error::DrainLimit));
    let r = runtime.report();
    assert_eq!(r.responses, MAX_DRAIN_ROUNDS);
    assert_eq!(r.status_reads, MAX_DRAIN_ROUNDS + 1);
    assert_eq!(r.acknowledge_submitted, 0);
    assert!(r.mask_completed);
    no_retry(&mut runtime, &mut io);
}
#[test]
fn every_uncertain_io_boundary_quarantines_without_replay() {
    let (_, mut reference) = armed();
    let mut baseline = Fake::new(irq::ENABLED | 0x10080);
    reference.notification(&mut baseline).unwrap();
    for index in 0..baseline.ops.len() {
        let (_, mut runtime) = armed();
        let mut io = Fake::new(irq::ENABLED | 0x10080);
        io.fail = Some(index);
        assert_eq!(
            runtime.notification(&mut io),
            Err(Error::Io),
            "operation {index}"
        );
        let r = runtime.report();
        assert_eq!(r.error, Some(Error::Io));
        assert!(r.mask_submitted);
        assert_eq!(r.acknowledge_completed, 0);
        assert_eq!(
            io.ops
                .iter()
                .filter(|op| **op == Op::Write(irq::ENABLE_OFFSET, 0))
                .count(),
            1
        );
        no_retry(&mut runtime, &mut io);
    }
}
#[test]
fn changed_enable_is_contained_without_clears_shared_reads_or_ack() {
    let (_, mut runtime) = armed();
    let mut io = Fake::new(8);
    io.lose_enable = true;
    assert_eq!(runtime.notification(&mut io), Err(Error::EnableChanged));
    assert_eq!(runtime.report().clear_submitted, 0);
    assert!(!io.ops.iter().any(|op| matches!(op, Op::Byte(_) | Op::Ack)));
    no_retry(&mut runtime, &mut io);
}
#[test]
fn containment_failure_retains_original_error_and_is_not_retried() {
    for fail_store in [false, true] {
        let (_, mut runtime) = armed();
        let mut io = Fake::new(8);
        io.fail = Some(0);
        io.fail_mask = fail_store;
        io.suppress_mask = !fail_store;
        assert_eq!(runtime.notification(&mut io), Err(Error::Io));
        let r = runtime.report();
        assert_eq!(r.error, Some(Error::Io));
        assert_eq!(
            r.containment_error,
            Some(if fail_store {
                Error::Io
            } else {
                Error::EnableChanged
            })
        );
        no_retry(&mut runtime, &mut io);
    }
}
#[test]
fn co_delivered_fault_masks_without_clearing_reading_shared_or_acknowledging() {
    let (_, mut runtime) = armed();
    let mut io = Fake::new(irq::ENABLED);
    runtime.external_fault(&mut io);
    assert_eq!(
        io.ops,
        vec![Op::Write(irq::ENABLE_OFFSET, 0), Op::Read(Read::Enable)]
    );
    assert_eq!(io.pending, irq::ENABLED);
    assert_eq!(runtime.report().error, Some(Error::ExternalFault));
    no_retry(&mut runtime, &mut io);
}
