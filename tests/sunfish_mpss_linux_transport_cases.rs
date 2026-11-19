use super::*;
use qcom_glink_fifo::{BootIo, BootPhase, BootTransport};
use std::cell::Cell;
#[derive(Clone)]
struct Bus {
    fake: Fake,
    length: Rc<Cell<Option<usize>>>,
    claimed: Rc<Cell<bool>>,
}
impl Bus {
    fn new(length: Option<usize>) -> Self {
        let fake = Fake::new(0, 0);
        {
            let mut m = fake.0.borrow_mut();
            m.words = [0; 8];
            m.rx = vec![0x5a; length.unwrap_or(32768)];
        }
        Self {
            fake,
            length: Rc::new(Cell::new(length)),
            claimed: Rc::new(Cell::new(false)),
        }
    }
    fn push(&self, b: &[u8]) {
        let len = self.length.get().unwrap();
        let mut m = self.fake.0.borrow_mut();
        let at = m.words[3] as usize;
        assert_eq!(b.len() % 8, 0);
        for (n, v) in b.iter().enumerate() {
            m.rx[(at + n) % len] = *v;
        }
        m.words[3] = ((at + b.len()) % len) as u32;
    }
}
impl BootIo for Bus {
    fn fault_boot(&mut self) {}
    fn claim_boot(&mut self) -> Result<(), Fault> {
        if self.claimed.replace(true) {
            Err(Fault)
        } else {
            Ok(())
        }
    }
}
impl Io for Bus {
    fn observe(&mut self) -> Result<Observation, Fault> {
        self.fake.observe()
    }
    fn descriptor(&mut self, n: usize) -> Result<u32, Fault> {
        self.fake.descriptor(n)
    }
    fn rx_length(&mut self) -> Result<Option<usize>, Fault> {
        Ok(self.length.get())
    }
    fn read_rx(&mut self, at: usize, out: &mut [u8]) -> Result<(), Fault> {
        self.fake.op("read_rx", Op::ReadRx(at, out.len()))?;
        let mut m = self.fake.0.borrow_mut();
        out.copy_from_slice(&m.rx[at..at + out.len()]);
        m.read_rx_calls += 1;
        if m.mutate_rx && m.read_rx_calls > 1 && !out.is_empty() {
            out[0] ^= 1;
        }
        Ok(())
    }
    fn read_tx(&mut self, at: usize, out: &mut [u8]) -> Result<(), Fault> {
        self.fake.read_tx(at, out)
    }
    fn write_tx(&mut self, at: usize, data: &[u8]) -> Result<(), Fault> {
        self.fake.write_tx(at, data)
    }
    fn store_tx_head(&mut self, v: u32) -> Result<(), Fault> {
        self.fake.store_tx_head(v)
    }
    fn store_rx_tail(&mut self, v: u32) -> Result<(), Fault> {
        self.fake.store_rx_tail(v)
    }
    fn barrier(&mut self, k: Barrier) -> Result<(), Fault> {
        self.fake.barrier(k)
    }
    fn notify_peer(&mut self) -> Result<(), Fault> {
        self.fake.notify_peer()
    }
}
fn packet(cmd: u16, id: u16, value: u32) -> Vec<u8> {
    let mut b = vec![0; 8];
    b[..2].copy_from_slice(&cmd.to_le_bytes());
    b[2..4].copy_from_slice(&id.to_le_bytes());
    b[4..8].copy_from_slice(&value.to_le_bytes());
    b
}
fn open(id: u16, name: &[u8]) -> Vec<u8> {
    let mut b = packet(2, id, (name.len() + 1) as u32);
    b.extend_from_slice(name);
    b.push(0);
    b.resize((b.len() + 7) & !7, 0);
    b
}
fn prepared() -> Session {
    let mut s = Session::new(
        Config {
            local_node: 1,
            peer_node: 0,
            local_channel: 1,
            remote_channel: 2,
        },
        1,
    )
    .unwrap();
    s.register(1, Service::RemoteStorage, 0x4000).unwrap();
    s.prepare_service_available(1, Service::RemoteStorage)
        .unwrap();
    s
}
fn run_open(bus: &Bus, remote_first: bool, features: u32) -> (BootTransport<Bus>, Vec<Vec<u8>>) {
    let mut boot = BootTransport::new(bus.clone(), prepared(), 1, 19_200_000);
    let mut offered = false;
    let mut remote_open = false;
    let mut early = false;
    let mut records = vec![];
    for now in 0..100 {
        let phase = boot
            .step(now)
            .unwrap_or_else(|e| panic!("{e:?} {:?}", boot.report()));
        let tx = bus.fake.take_tx();
        let mut at = 0;
        while at < tx.len() {
            let cmd = u16::from_le_bytes(tx[at..at + 2].try_into().unwrap());
            let id = u16::from_le_bytes(tx[at + 2..at + 4].try_into().unwrap());
            let value = u32::from_le_bytes(tx[at + 4..at + 8].try_into().unwrap());
            match cmd {
                0 => {
                    if !offered {
                        bus.push(&packet(0, 1, features));
                        bus.push(&open(9, b"sys_mon"));
                        if remote_first {
                            bus.push(&open(37, b"IPCRTR"));
                            remote_open = true;
                        }
                        offered = true;
                    }
                    bus.push(&packet(1, 1, value & features));
                }
                1 => assert_eq!((id, value), (1, features & 1)),
                2 => {
                    assert_eq!(&tx[at..at + 16], open(1, b"IPCRTR"));
                    if !remote_open {
                        bus.push(&open(37, b"IPCRTR"));
                        remote_open = true;
                    }
                    bus.push(&packet(4, 1, 0));
                }
                4 => {
                    assert_eq!((id, value), (37, 0));
                    if remote_first && !early {
                        let mut intent = packet(5, 37, 1);
                        intent.extend_from_slice(&4096u32.to_le_bytes());
                        intent.extend_from_slice(&88u32.to_le_bytes());
                        bus.push(&intent);
                        early = true;
                    }
                }
                _ => panic!("unexpected boot command {cmd}"),
            }
            let n = if cmd == 2 { 16 } else { 8 };
            records.push(tx[at..at + n].to_vec());
            at += n;
        }
        if phase == BootPhase::Ready {
            return (boot, records);
        }
    }
    panic!("opening stalled {:?}", boot.report());
}
#[test]
fn linux_transport_opens_either_direction_binds_actual_ids_and_preserves_services() {
    for first in [false, true] {
        for features in [7, 0] {
            for len in [4096, 16384, 32768] {
                let bus = Bus::new(Some(len));
                let (boot, _) = run_open(&bus, first, features);
                assert_eq!(boot.report().features, features & 1);
                assert_eq!(boot.report().other_channels, 1);
                let prior = bus.fake.0.borrow().words;
                let mut a = boot
                    .into_adapter()
                    .unwrap_or_else(|b| panic!("{:?}", b.report()));
                assert_eq!(
                    bus.fake.0.borrow().words,
                    prior,
                    "handoff must not reset cursors"
                );
                assert_eq!(a.configuration().remote_channel, 37);
                assert_eq!(
                    a.endpoint(Service::RemoteStorage).unwrap().address.port,
                    0x4000
                );
                a.start_control(1).unwrap();
                if first {
                    assert_eq!(
                        a.poll_receive(1),
                        Ok(Receive::Session(Event::RemoteIntents))
                    );
                }
                bus.push(&open(48, b"unconfigured"));
                assert_eq!(a.poll_receive(1), Ok(Receive::Control));
                assert!(
                    bus.fake.take_tx().is_empty(),
                    "no ACK for an unconfigured consumer"
                );
                assert_eq!(a.reset_after_transport_reset(2), Err(Error::Negotiation));
            }
        }
    }
}

#[test]
fn receive_done_reuse_accepts_only_an_outstanding_intent_with_or_without_feature_bit() {
    for features in [0, 1] {
        let bus = Bus::new(Some(4096));
        let (boot, _) = run_open(&bus, true, features);
        let mut adapter = boot.into_adapter().unwrap_or_else(|b| panic!("{:?}", b.report()));
        adapter.start_control(1).unwrap();
        assert_eq!(adapter.poll_receive(1), Ok(Receive::Session(Event::RemoteIntents)));
        let mut sent = false;
        for _ in 0..16 {
            if adapter.publish_next(1) == Ok(Transmit::Session(Published::Data {
                remote_intent: 88, last_fragment: true,
            })) { sent = true; break; }
        }
        assert!(sent, "expected a completed transmission on the peer intent");
        bus.fake.take_tx();
        bus.push(&packet(14, 37, 88));
        assert_eq!(adapter.poll_receive(1), Ok(Receive::Session(Event::TransmitDone)));
        // A duplicate cannot complete the same transmission twice.
        bus.push(&packet(14, 37, 88));
        assert!(adapter.poll_receive(1).is_err());
    }
}

#[test]
fn late_open_diagnostic_distinguishes_bad_length_from_rejected_checked_record() {
    for bad_length in [true, false] {
        let bus = Bus::new(Some(4096));
        let (boot, _) = run_open(&bus, true, 0);
        let mut adapter = boot.into_adapter().unwrap_or_else(|b| panic!("{:?}", b.report()));
        assert_eq!(adapter.poll_receive(1), Ok(Receive::Session(Event::RemoteIntents)));
        if bad_length {
            bus.push(&packet(2, 52, 1));
        } else {
            // IPCRTR was already opened during the handshake.
            bus.push(&open(52, b"IPCRTR"));
        }
        let tail_before = bus.fake.0.borrow().words[2];
        assert_eq!(adapter.poll_receive(1), Err(Error::Negotiation));
        assert_eq!(bus.fake.0.borrow().words[2], tail_before, "rejected OPEN must remain unconsumed");
        let info = adapter.open_diagnostic().unwrap();
        assert_eq!(info.stage, if bad_length { 1 } else { 2 });
        assert_eq!(info.channel, 52);
        assert_eq!(info.advertised_length, if bad_length { 1 } else { 7 });
        assert_eq!(info.reason, if bad_length { 1 } else { 1 << 7 });
        assert_eq!(info.remote_channel, 37);
        assert_eq!(info.other_channels, 1);
        if bad_length { assert_eq!(info.name_len, 0); }
        else { assert_eq!(&info.name[..info.name_len], b"IPCRTR\0"); }
    }
}

#[test]
fn late_unconfigured_channels_pass_old_limit_and_stop_at_new_bound() {
    let bus = Bus::new(Some(4096));
    let (boot, _) = run_open(&bus, true, 0);
    let mut adapter = boot.into_adapter().unwrap_or_else(|b| panic!("{:?}", b.report()));
    assert_eq!(adapter.poll_receive(1), Ok(Receive::Session(Event::RemoteIntents)));
    // sys_mon already occupies one slot. None of these channels is accepted
    // as a consumer or acknowledged by our endpoint.
    for n in 0..63u16 {
        let name = format!("DATA{n}");
        bus.push(&open(100 + n, name.as_bytes()));
        assert_eq!(adapter.poll_receive(1), Ok(Receive::Control), "channel {n}");
        assert!(bus.fake.take_tx().is_empty());
    }
    let old_tail = bus.fake.0.borrow().words[2];
    bus.push(&open(200, b"ONE_TOO_MANY"));
    assert_eq!(adapter.poll_receive(1), Err(Error::Negotiation));
    assert_eq!(bus.fake.0.borrow().words[2], old_tail);
    let info = adapter.open_diagnostic().unwrap();
    assert_eq!(info.other_channels, 64);
    assert_eq!(info.reason, 0x100);
}
#[test]
fn linux_transport_sends_version_before_peer_rx_allocation() {
    let bus = Bus::new(None);
    let mut boot = BootTransport::new(bus.clone(), prepared(), 1, 19_200_000);
    assert_eq!(boot.step(0), Ok(BootPhase::Negotiating));
    assert_eq!(bus.fake.take_tx(), packet(0, 1, 1));
    assert_eq!(boot.step(1), Ok(BootPhase::Negotiating));
    assert!(bus.fake.take_tx().is_empty());
    bus.length.set(Some(32768));
    bus.push(&packet(0, 1, 7));
    assert!(boot.step(2).is_ok());
}
#[test]
fn linux_transport_peer_version_requires_a_later_step_to_send_reply() {
    let bus = Bus::new(Some(4096));
    let mut boot = BootTransport::new(bus.clone(), prepared(), 1, 19_200_000);
    boot.step(0).unwrap();
    assert_eq!(bus.fake.take_tx(), packet(0, 1, 1));
    bus.push(&packet(0, 1, 1));

    // A single IRQ-driven step consumes VERSION but only queues its ACK.
    boot.step(1).unwrap();
    assert_eq!(boot.report().rx_records, 1);
    assert!(bus.fake.take_tx().is_empty());

    // No peer event is supplied here: the pending ACK needs local work.
    boot.step(2).unwrap();
    assert_eq!(bus.fake.take_tx(), packet(1, 1, 1));
}
#[test]
fn linux_transport_drive_sends_queued_version_ack_without_a_second_irq() {
    use qcom_glink_fifo::BootDrive;
    let bus = Bus::new(Some(4096));
    let mut boot = BootTransport::new(bus.clone(), prepared(), 1, 19_200_000);
    assert_eq!(boot.drive(0, 16), Ok(BootDrive::Idle));
    assert_eq!(bus.fake.take_tx(), packet(0, 1, 1));
    bus.push(&packet(0, 1, 1));
    assert_eq!(boot.drive(1, 16), Ok(BootDrive::Idle));
    assert_eq!(boot.report().rx_records, 1);
    assert_eq!(bus.fake.take_tx(), packet(1, 1, 1));
}

#[test]
fn linux_transport_drive_reports_exhaustion_without_hiding_pending_work() {
    use qcom_glink_fifo::BootDrive;
    let bus = Bus::new(Some(4096));
    let mut boot = BootTransport::new(bus.clone(), prepared(), 1, 19_200_000);
    assert_eq!(boot.drive(0, 0), Ok(BootDrive::BudgetExhausted));
    assert!(bus.fake.take_tx().is_empty());
    assert_eq!(boot.drive(0, 1), Ok(BootDrive::BudgetExhausted));
    assert_eq!(bus.fake.take_tx(), packet(0, 1, 1));
    assert_eq!(boot.drive(1, 16), Ok(BootDrive::Idle));
}

#[test]
fn linux_transport_drive_handles_coalesced_peer_records_until_ready() {
    use qcom_glink_fifo::BootDrive;
    let bus = Bus::new(Some(4096));
    let mut boot = BootTransport::new(bus.clone(), prepared(), 1, 19_200_000);
    assert_eq!(boot.drive(0, 32), Ok(BootDrive::Idle));
    assert_eq!(bus.fake.take_tx(), packet(0, 1, 1));

    // These peer records arrive under one notification.
    bus.push(&packet(0, 1, 1));
    bus.push(&packet(1, 1, 1));
    bus.push(&open(37, b"IPCRTR"));
    assert_eq!(boot.drive(1, 32), Ok(BootDrive::Idle));
    let tx = bus.fake.take_tx();
    assert!(tx.windows(8).any(|window| window == packet(1, 1, 1)));
    assert!(tx.windows(16).any(|window| window == open(1, b"IPCRTR")));

    bus.push(&packet(4, 1, 0));
    assert_eq!(boot.drive(2, 32), Ok(BootDrive::Ready));
    assert_eq!(boot.report().remote_channel, Some(37));
    assert_eq!(bus.fake.take_tx(), packet(4, 37, 0));
}
#[test]
fn linux_transport_rejects_bad_open_wrong_ack_zero_version_or_torn_record() {
    for mode in 0..5 {
        let bus = Bus::new(Some(4096));
        let mut boot = BootTransport::new(bus.clone(), prepared(), 1, 19_200_000);
        boot.step(0).unwrap();
        bus.fake.take_tx();
        match mode {
            0 => bus.push(&packet(1, 0, 0)),
            1 => bus.push(&packet(4, 1, 0)),
            2 => {
                let mut o = open(37, b"IPCRTR");
                o[14] = b'X';
                bus.push(&o);
            }
            3 => {
                bus.push(&packet(0, 1, 7));
                bus.fake.0.borrow_mut().mutate_rx = true;
            }
            _ => bus.push(&packet(2, 37, 0xffff)),
        }
        assert!(boot.step(1).is_err());
        let before = bus.fake.log();
        assert!(boot.step(2).is_err());
        assert_eq!(bus.fake.log(), before);
        assert!(boot.into_adapter().is_err());
    }
}
#[test]
fn linux_transport_quarantines_write_uncertainty_notify_unwind_and_generation_loss() {
    for mode in 0..5 {
        let bus = Bus::new(Some(4096));
        {
            let mut m = bus.fake.0.borrow_mut();
            match mode {
                0 => m.corrupt_readback = true,
                1 => m.fail_after_store = true,
                2 => m.panic_on_notify = true,
                3 => m.revoke_after_write = true,
                _ => m.observation.generation = 2,
            }
        }
        let mut boot = BootTransport::new(bus.clone(), prepared(), 1, 19_200_000);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| boot.step(0)));
        if mode == 2 {
            assert!(result.is_err());
        } else {
            assert!(result.unwrap().is_err());
        }
        let before = bus.fake.log();
        assert!(boot.step(1).is_err());
        assert_eq!(bus.fake.log(), before);
        assert_eq!(boot.report().phase, BootPhase::Failed);
    }
}
#[test]
fn linux_transport_bounds_wait_and_rejects_clock_regression_and_changed_rx() {
    let bus = Bus::new(None);
    let mut b = BootTransport::new(bus.clone(), prepared(), 1, 19_200_000);
    b.step(0).unwrap();
    assert_eq!(b.step(96_000_000), Err(Error::Timeout));
    let bus = Bus::new(Some(4096));
    let mut b = BootTransport::new(bus.clone(), prepared(), 1, 19_200_000);
    b.step(100).unwrap();
    assert_eq!(b.step(99), Err(Error::Timeout));
    let bus = Bus::new(Some(4096));
    let mut b = BootTransport::new(bus.clone(), prepared(), 1, 19_200_000);
    b.step(0).unwrap();
    bus.length.set(Some(8192));
    assert_eq!(b.step(1), Err(Error::IdentityChanged));
}

#[test]
fn linux_transport_matches_original_linux_control_and_counteroffer_bytes() {
    let dir =
        std::env::var("SUNFISH_GLK_TRANSPORT_C").expect("original Linux control fixture directory");
    let read = |name: &str| std::fs::read(format!("{dir}/{name}.bin")).unwrap();
    for first in [false, true] {
        for feature in [7, 0] {
            let bus = Bus::new(Some(4096));
            let (_, records) = run_open(&bus, first, feature);
            assert_eq!(records.len(), 4);
            assert_eq!(records[0], read("version-1"));
            assert_eq!(
                records[1],
                read(if feature == 7 { "ack-1" } else { "ack-0" })
            );
            assert_eq!(
                records[2],
                read(if first {
                    "open-ack-37"
                } else {
                    "open-ipcrtr-1"
                })
            );
            assert_eq!(
                records[3],
                read(if first {
                    "open-ipcrtr-1"
                } else {
                    "open-ack-37"
                })
            );
        }
    }
    let bus = Bus::new(Some(4096));
    let mut b = BootTransport::new(bus.clone(), prepared(), 1, 19_200_000);
    b.step(0).unwrap();
    bus.fake.take_tx();
    bus.push(&packet(1, 1, 0));
    b.step(1).unwrap();
    b.step(2).unwrap();
    assert_eq!(bus.fake.take_tx(), read("counteroffer-0"));
    bus.push(&packet(0, 1, 7));
    b.step(3).unwrap();
    b.step(4).unwrap();
    assert_eq!(bus.fake.take_tx(), read("ack-0"));
    assert_eq!(b.report().features, 0);
}

#[test]
fn linux_transport_preserves_one_pending_reply_across_fifo_backpressure() {
    let bus = Bus::new(Some(4096));
    let mut b = BootTransport::new(bus.clone(), prepared(), 1, 19_200_000);
    b.step(0).unwrap();
    // Keep the peer TX tail at zero while it requests bounded VERSION replies.
    // Each request is consumed once, including the reply that fills the FIFO.
    for n in 0..2045 {
        bus.push(&packet(0, 1, 7));
        b.step(1 + n * 2).unwrap();
        b.step(2 + n * 2).unwrap();
    }
    let before = bus.fake.0.borrow().words;
    bus.push(&packet(0, 1, 7));
    b.step(5000).unwrap();
    b.step(5001).unwrap();
    let blocked = bus.fake.0.borrow().words;
    assert_eq!(blocked[1], 16376);
    assert_eq!(blocked[2], (before[2] + 8) % 4096);
    let tx = bus.fake.take_tx();
    assert_eq!(&tx[tx.len() - 8..], packet(13, 0, 0));
    let received = b.report().rx_records;
    b.step(5002).unwrap();
    assert_eq!(bus.fake.take_tx(), packet(1, 1, 1));
    assert_eq!(b.report().rx_records, received);
}

#[test]
fn linux_transport_dynamic_rx_wrap_delivers_qrtr_after_negotiation() {
    for len in [4096, 32768] {
        let bus = Bus::new(Some(len));
        let (b, _) = run_open(&bus, true, 7);
        let mut a = b
            .into_adapter()
            .unwrap_or_else(|b| panic!("{:?}", b.report()));
        a.start_control(1).unwrap();
        assert_eq!(
            a.poll_receive(1),
            Ok(Receive::Session(Event::RemoteIntents))
        );
        // Advance by real READ_NOTIF records, exercising cursor bookkeeping
        // rather than rewriting the host-owned cursor behind the adapter.
        loop {
            let tail = bus.fake.0.borrow().words[2] as usize;
            if tail == len - 8 {
                break;
            }
            bus.push(&packet(13, 0, 0));
            assert_eq!(a.poll_receive(1), Ok(Receive::ReadNotification));
            bus.fake.take_tx();
        }
        let id = a.offer_receive(1).unwrap();
        assert!(matches!(
            a.publish_next(1),
            Ok(Transmit::Session(Published::ReceiveIntent(_)))
        ));
        bus.fake.take_tx();
        let qrtr: Vec<u8> = [
            1u32,
            2,
            0,
            0xffff_fffe,
            0,
            20,
            0xffff_ffff,
            0xffff_fffe,
            2,
            0,
            0,
            0,
            0,
        ]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
        let mut frame = packet(9, 37, id);
        frame.extend_from_slice(&(qrtr.len() as u32).to_le_bytes());
        frame.extend_from_slice(&0u32.to_le_bytes());
        frame.extend_from_slice(&qrtr);
        frame.resize((frame.len() + 7) & !7, 0);
        bus.push(&frame);
        assert!(matches!(
            a.poll_receive(1),
            Ok(Receive::Session(Event::QrtrControl(
                crate::qcom_qrtr_control::Event::Hello { .. }
            )))
        ));
        assert!(a.control_status().peer_hello);
    }
}
