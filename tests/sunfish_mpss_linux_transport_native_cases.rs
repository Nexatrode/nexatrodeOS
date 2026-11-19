// Full Native install -> retained SMEM/SMP -> actual loader receipt -> FIFO.
use crate::{
    qcom_glink_fifo::{BootPhase, Receive},
    qcom_glink_qmi::{Config, Event, Session},
    sunfish_mpss_linux_smp2p as smp, sunfish_mpss_linux_transport_native as live,
};
std::thread_local! {static LIFE:std::cell::Cell<smp::Lifecycle>=const{std::cell::Cell::new(smp::Lifecycle::PreAuth)};}
#[cfg(feature = "sunfish-glink-barrier-observe-pair")]
std::thread_local! {static PAIR_TABLE_CHANGE:std::cell::Cell<Option<*mut u64>>=const{std::cell::Cell::new(None)};}
fn observe(g: u64) -> Result<smp::Observation, smp::Error> {
    #[cfg(feature = "sunfish-glink-barrier-observe-pair")]
    if let Some(word) = PAIR_TABLE_CHANGE.take() {
        unsafe { word.write(word.read() ^ 1); }
    }
    Ok(smp::Observation {
        generation: g,
        lifecycle: LIFE.get(),
        power: smp::Power::Active,
    })
}
fn with_native(
    f: impl FnOnce(
        crate::sunfish_mpss_live_domain::NativeGate<Native<'_, fn(usize) -> Result<(), Error>>>,
        &mut crate::LinkPeer,
        *mut u32,
        crate::sunfish_mpss_linux_loader::AuthReturned,
    ),
) {
    let mut registers = Fixture::new();
    let mut shared = super::super::shared_fixture::Fixture::new();
    let data = core::array::from_fn::<_, 32, _>(|i| 0xa3000000 + i as u64 * 4096);
    let mut ram = receive_ram();
    let mut extra = core::array::from_fn::<_, 4, _>(|_| Page([0xa5; 512]));
    let base = shared.smem.as_mut_ptr().cast::<u8>();
    let lock = shared.lock.0.as_mut_ptr().cast::<u32>();
    let mut peer = crate::LinkPeer {
        base,
        lock,
        free: crate::sunfish_mpss_linux_smem::FINAL_FREE,
        rx: None,
    };
    let mut n = super::mpss_postinstall::attach(&mut registers, &mut shared, &data, &mut ram);
    let plan = stage_receive(&mut n, &mut extra);
    n.select(&plan).unwrap();
    n.install_experiment(&plan, 8).unwrap();
    let _receive = n.take_receive_memory().unwrap();
    let prepared = super::mpss_postinstall::finish(&mut n);
    LIFE.set(smp::Lifecycle::PreAuth);
    let mut apcs = 0u32;
    let signal = unsafe {
        crate::sunfish_mpss_linux_smp2p_native::Native::new(
            &mut apcs,
            0x17c0000c,
            observe as fn(u64) -> Result<smp::Observation, smp::Error>,
        )
    }
    .map_err(|(_, e)| e)
    .unwrap();
    n.activate_modem_smp2p(prepared, signal).unwrap();
    assert!(!n.modem_smp2p_report().unwrap().ready_seen);
    crate::sunfish_mpss_linux_loader::tests::boot_handoff::with_auth_returned(|auth| {
        LIFE.set(smp::Lifecycle::AuthReturned);
        f(crate::sunfish_mpss_live_domain::NativeGate::new(n), &mut peer, &mut apcs, auth);
    });
}
fn packet(cmd: u16, id: u16, value: u32) -> Vec<u8> {
    let mut b = vec![0; 8];
    b[..2].copy_from_slice(&cmd.to_le_bytes());
    b[2..4].copy_from_slice(&id.to_le_bytes());
    b[4..8].copy_from_slice(&value.to_le_bytes());
    b
}
fn opening(id: u16) -> Vec<u8> {
    let mut b = packet(2, id, 7);
    b.extend_from_slice(b"IPCRTR\0\0");
    b
}
impl crate::LinkPeer {
    fn push_native(&mut self, b: &[u8]) {
        let desc = crate::sunfish_mpss_linux_smem::ITEMS[1].1;
        let head = u32::from_le_bytes(self.read(desc.offset + 12, 4).try_into().unwrap()) as usize;
        let len = self.rx.unwrap().size;
        let first = b.len().min(len - head);
        self.rx_bytes(head, &b[..first]);
        if first != b.len() {
            self.rx_bytes(0, &b[first..]);
        }
        self.cursor(3, ((head + b.len()) % len) as u32);
    }
    fn take_native_tx(&mut self) -> Vec<u8> {
        let items = crate::sunfish_mpss_linux_smem::ITEMS;
        let d = self.read(items[1].1.offset, 16);
        let tail = u32::from_le_bytes(d[..4].try_into().unwrap()) as usize;
        let head = u32::from_le_bytes(d[4..8].try_into().unwrap()) as usize;
        let bytes = (head + 16384 - tail) % 16384;
        let first = bytes.min(16384 - tail);
        let mut out = self.read(items[2].1.offset + tail, first);
        if first != bytes {
            out.extend_from_slice(&self.read(items[2].1.offset, bytes - first));
        }
        self.cursor(0, head as u32);
        out
    }
}
fn session() -> Session {
    Session::new(
        Config {
            local_node: 1,
            peer_node: 0,
            local_channel: 1,
            remote_channel: 2,
        },
        1,
    )
    .unwrap()
}
#[test]
fn linux_transport_native_keeps_installed_domain_and_uses_original_apcs_writer() {
    with_native(|gate, p, apcs, auth| {
        let before = gate.try_borrow_mut(|n| n.shared_report()).unwrap();
        let mut boot = live::start_live(gate, auth, session(), 19_200_000);
        boot.step(0).unwrap();
        assert_eq!(p.take_native_tx(), packet(0, 1, 1));
        assert_eq!(unsafe { apcs.read() }, 1 << 12);
        p.allocate_rx(4096, 0x80);
        p.push_native(&packet(0, 1, 7));
        p.push_native(&opening(37));
        p.push_native(&packet(1, 1, 1));
        let mut early = false;
        let mut ready = false;
        for now in 1..80 {
            let phase = boot
                .step(now)
                .unwrap_or_else(|e| panic!("{e:?} {:?}", boot.report()));
            let tx = p.take_native_tx();
            let mut at = 0;
            while at < tx.len() {
                let cmd = u16::from_le_bytes(tx[at..at + 2].try_into().unwrap());
                if cmd == 4 && !early {
                    let mut b = packet(5, 37, 1);
                    b.extend_from_slice(&4096u32.to_le_bytes());
                    b.extend_from_slice(&88u32.to_le_bytes());
                    p.push_native(&b);
                    early = true;
                }
                if cmd == 2 {
                    p.push_native(&packet(4, 1, 0));
                }
                at += if cmd == 2 { 16 } else { 8 };
            }
            if phase == BootPhase::Ready {
                ready = true;
                break;
            }
        }
        assert!(ready);
        let mut adapter = boot
            .into_adapter()
            .unwrap_or_else(|b| panic!("{:?}", b.report()));
        adapter.start_control(1).unwrap();
        assert_eq!(
            adapter.poll_receive(1),
            Ok(Receive::Session(Event::RemoteIntents))
        );
        assert_eq!(adapter.configuration().remote_channel, 37);
        assert_eq!(adapter.with_io(|io| io.with_native(|n| n.shared_report())), Ok(before));
        assert!(!adapter.with_io(|io| io.gate_quarantined()));
        assert!(adapter.with_io(|io| io.with_native(|n| n.failed && n.installation_completed)).unwrap());
        adapter.with_io(|io| io.with_native(|n| n.retained_receive_domain().unwrap()).unwrap());
        assert!(adapter.with_io(|io| io.with_native(|n| !n.modem_smp2p_report().unwrap().ready_seen)).unwrap());
        drop(adapter);
        assert_eq!(p.free_lock(), 0);
    });
}
#[test]
fn linux_transport_native_domain_loss_stops_before_further_fifo_writes() {
    for mode in 0..3 {
        with_native(|gate, p, _, auth| {
            let (register, original, table, old_table, fault) = gate.try_borrow_mut(|n| {
                let register = unsafe {
                    n.registers[0]
                        .add(0x800 + n.slot.unwrap() * 4)
                        .cast::<u32>()
                };
                let original = unsafe { register.read() };
                let table = n.table_pointers[0];
                let old_table = unsafe { table.read() };
                (register, original, table, old_table, n.fault as *const AtomicBool)
            }).unwrap();
            let mut boot = live::start_live(gate, auth, session(), 19_200_000);
            boot.step(0).unwrap();
            let before = p.read(0x11d000, 0x7c000);
            unsafe {
                match mode {
                    0 => register.write(0),
                    1 => table.write(0),
                    _ => (&*fault).store(true, Ordering::Release),
                }
            }
            assert!(boot.step(1).is_err());
            unsafe {
                register.write(original);
                table.write(old_table);
                (&*fault).store(false, Ordering::Release);
            }
            assert!(boot.step(2).is_err());
            assert_eq!(p.read(0x11d000, 0x7c000), before);
            assert!(boot.with_io(|io| io.gate_quarantined()));
            drop(boot);
        });
    }
}

#[test]
fn linux_transport_native_glink_doorbell_rejects_pre_auth_without_a_store() {
    use smp::{GlinkSignals, Signals};
    LIFE.set(smp::Lifecycle::PreAuth);
    let mut apcs = 0xfeedu32;
    let mut signal = unsafe {
        crate::sunfish_mpss_linux_smp2p_native::Native::new(
            &mut apcs,
            0x17c0000c,
            observe as fn(u64) -> Result<smp::Observation, smp::Error>,
        )
    }
    .map_err(|(_, e)| e)
    .unwrap();
    signal.observe(1).unwrap();
    assert_eq!(signal.notify_glink(), Err(smp::Error::Guard));
    assert_eq!(apcs, 0xfeed);
    assert_eq!(signal.notify(), Err(smp::Error::Consumed));
}

#[test]
fn linux_transport_native_protocol_failure_quarantines_outer_owner_without_reset() {
    with_native(|gate, p, _, auth| {
        let mut boot = live::start_live(gate, auth, session(), 19_200_000);
        boot.step(0).unwrap();
        p.take_native_tx();
        p.allocate_rx(4096, 0);
        p.push_native(&packet(4, 1, 0));
        let before = p.read(0x11d000, 0x7c000);
        assert!(boot.step(1).is_err());
        assert!(boot.with_io(|io| io.gate_quarantined()));
        drop(boot);
        assert_eq!(p.read(0x11d000, 0x7c000), before);
    });
}

#[test]
fn linux_transport_native_glink_guard_diagnostic_preserves_peer_reason() {
    assert_eq!(live::encode_glink_diagnostic(smp::GlinkError::Guard, None), 3);
    assert_eq!(live::encode_glink_diagnostic(smp::GlinkError::Guard, Some(smp::Error::PeerFatal)),
        3 | ((1 + smp::Error::PeerFatal as u32) << 8));
}

#[test]
fn linux_transport_native_guard_captures_peer_fatal_before_quarantine() {
    use crate::qcom_glink_fifo::Io;
    with_native(|gate, peer, _, auth| {
        let mut boot = live::start_live(gate, auth, session(), 19_200_000);
        boot.step(0).unwrap();
        peer.peer_smp(true);
        assert!(boot.step(1).is_err());
        let detail = boot.with_io(|io| io.diagnostic());
        assert_eq!(detail, live::encode_glink_diagnostic(smp::GlinkError::Guard, Some(smp::Error::PeerFatal)));
        assert!(boot.with_io(|io| io.gate_quarantined()));
    });
}

#[cfg(feature = "sunfish-native-startup-mpss-runtime")]
#[test]
fn linux_transport_native_peer_fatal_preserves_locked_reason_snapshot() {
    use crate::qcom_glink_fifo::Io;
    with_native(|gate, peer, _, auth| {
        // Publish a valid empty global SMEM partition before the first poll.
        let toc = 0x1ff000 + 32 + 48;
        let part = 0x10000;
        for (at, value) in [(toc, part as u32), (toc + 4, 0x4000),
            (toc + 12, 0xfffefffe), (toc + 16, 32), (part, 0x54525024),
            (part + 4, 0xfffefffe), (part + 8, 0x4000),
            (part + 12, 32), (part + 16, 0x4000)] { peer.word(at, value); }
        let mut boot = live::start_live(gate, auth, session(), 19_200_000);
        boot.step(0).unwrap();
        // Firmware publishes a reason and then asserts its fatal flag.
        peer.word(part + 32, (421u32 << 16) | 0xa5a5);
        peer.word(part + 36, 8);
        peer.word(part + 40, 0);
        peer.word(part + 48, u32::from_le_bytes(*b"DOG\0"));
        peer.word(part + 52, 0);
        peer.word(part + 12, 56);
        peer.peer_smp(true);
        assert!(boot.step(1).is_err());
        let evidence = boot.with_io(|io| io.crash_evidence()).unwrap();
        assert_eq!(crate::qcom_modem_crash::evidence_changed(evidence.before, evidence.after), Some(true));
        assert_eq!(&evidence.after.unwrap().unwrap().raw[..4], b"DOG\0");
    });
}

#[cfg(feature = "sunfish-glink-combined-cursor-sample")]
#[test]
fn linux_transport_native_cursor_snapshot_rejects_each_retained_page_and_stays_quarantined() {
    use crate::qcom_glink_fifo::Io;
    for page in 0..8 {
        with_native(|gate, peer, _, auth| {
            let (word, original) = gate.try_borrow_mut(|n| {
                let extra = n.root_extra.unwrap().0;
                let pages = [n.table_pointers[0], n.table_pointers[1], extra[0],
                    n.table_pointers[2], extra[1], n.receive_table.unwrap().0,
                    extra[2], n.table_pointers[3]];
                let word = unsafe { pages[page].add(511) };
                (word, unsafe { word.read() })
            }).unwrap();
            let mut boot = live::start_live(gate, auth, session(), 19_200_000);
            boot.step(0).unwrap();
            assert!(boot.with_io(|io| io.cursor_snapshot()).is_ok());
            let before = peer.read(0x11d000, 0x7c000);
            unsafe { word.write(original ^ 1); }
            assert!(boot.with_io(|io| io.cursor_snapshot()).is_err(), "page {page}");
            unsafe { word.write(original); }
            assert!(boot.with_io(|io| io.cursor_snapshot()).is_err());
            assert!(boot.with_io(|io| io.write_tx(0, b"12345678")).is_err());
            assert!(boot.with_io(|io| io.gate_quarantined()));
            assert_eq!(peer.read(0x11d000, 0x7c000), before);
            assert_eq!(peer.free_lock(), 0);
        });
    }
}

#[cfg(feature = "sunfish-glink-barrier-observe-pair")]
#[test]
fn linux_transport_native_barrier_pair_retains_domain_and_peer_checks() {
    use crate::qcom_glink_fifo::{Barrier, Io};
    for mode in 0..6 {
        with_native(|gate, peer, apcs, auth| {
            let (word, old, fault) = gate.try_borrow_mut(|n| {
                let word = unsafe { n.table_pointers[0].add(511) };
                (word, unsafe { word.read() }, n.fault as *const AtomicBool)
            }).unwrap();
            let mut boot = live::start_live(gate, auth, session(), 19_200_000);
            boot.step(0).unwrap();
            // The full runtime configuration uses the instrumented domain
            // wrapper; the base configuration exercises the ordinary wrapper.
            #[cfg(feature = "sunfish-tftp-barrier-domain-observer")]
            boot.with_io(|io| { io.owner_timing_clock(|| 1); io.owner_timing_enable(true); });
            peer.allocate_rx(4096, 0);
            let door = unsafe { apcs.read() };
            match mode {
                1 => peer.peer_smp(true),
                2 => LIFE.set(smp::Lifecycle::PreAuth),
                3 => unsafe { word.write(old ^ 1); },
                4 => PAIR_TABLE_CHANGE.set(Some(word)),
                5 => unsafe { (&*fault).store(true, Ordering::Release); },
                _ => {},
            }
            let before = peer.read(0x11d000, 0x7c000);
            let result = boot.with_io(|io| io.barrier_observe(Barrier::Acquire));
            if mode == 0 {
                let observation = result.unwrap();
                assert_eq!(observation.generation, 1);
                assert!(observation.ready && !observation.fatal);
                assert_eq!(boot.with_io(|io| io.barrier_observe(Barrier::Release)), Ok(observation));
            } else {
                assert!(result.is_err(), "mode {mode}");
                assert!(PAIR_TABLE_CHANGE.get().is_none());
                // Restore the changed input: failed ownership is never revived.
                unsafe { word.write(old); (&*fault).store(false, Ordering::Release); }
                LIFE.set(smp::Lifecycle::AuthReturned);
                assert!(boot.with_io(|io| io.barrier_observe(Barrier::Release)).is_err());
                assert!(boot.with_io(|io| io.write_tx(0, b"12345678")).is_err());
                assert!(boot.with_io(|io| io.gate_quarantined()));
            }
            assert!(peer.read(0x11d000, 0x7c000) == before, "shared memory changed in mode {mode}");
            assert_eq!(unsafe { apcs.read() }, door);
            assert_eq!(peer.free_lock(), 0);
        });
    }
}

/// Compare the existing boundary with a hypothetical paired read-only action.
/// This models ordering only; it does not install the candidate in the runtime.
#[cfg(all(feature = "sunfish-glink-combined-cursor-sample", feature = "sunfish-glink-barrier-observe-pair"))]
#[test]
fn status_cursor_pair_can_read_after_mid_pair_domain_corruption() {
    use crate::qcom_glink_fifo::Io;
    for pair in [false, true] {
        with_native(|gate, peer, _, auth| {
            let (word, original) = gate.try_borrow_mut(|n| {
                let word = unsafe { n.table_pointers[0].add(511) };
                (word, unsafe { word.read() })
            }).unwrap();
            let mut boot = live::start_live(gate, auth, session(), 19_200_000);
            boot.step(0).unwrap();
            peer.allocate_rx(4096, 0);
            let before = peer.read(0x11d000, 0x7c000);
            let attempted_cursor = std::cell::Cell::new(false);
            PAIR_TABLE_CHANGE.set(Some(word));
            if pair {
                let result = boot.with_io(|io| io.with_native(|native| native.with_glink(|c| {
                    c.glink_poll()?;
                    attempted_cursor.set(true);
                    c.glink_cursor_snapshot().map(|_| ())
                })));
                assert!(matches!(result, Ok(Err(_))));
                assert!(attempted_cursor.get(), "paired path attempted cursor before its post-check");
            } else {
                let result = boot.with_io(|io| io.observe());
                if result.is_ok() {
                    attempted_cursor.set(true);
                    let _ = boot.with_io(|io| io.cursor_snapshot());
                }
                assert!(result.is_err());
                assert!(!attempted_cursor.get(), "existing post-check rejects before cursor read");
            }
            assert!(PAIR_TABLE_CHANGE.get().is_none());
            unsafe { word.write(original); }
            assert!(boot.with_io(|io| io.write_tx(0, b"12345678")).is_err());
            assert!(boot.with_io(|io| io.gate_quarantined()));
            assert_eq!(peer.read(0x11d000, 0x7c000), before);
            assert_eq!(peer.free_lock(), 0);
        });
    }
}
