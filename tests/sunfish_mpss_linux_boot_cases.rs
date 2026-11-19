use crate::BootPage as Page;
#[cfg(mpss_auth_root_tests)]
pub(crate) fn with_auth_boundary(
    mut request: impl FnMut(&Request),
    receipt: impl FnOnce(AuthReturned),
) {
    struct Hook<'a, F> {
        inner: &'a mut Power,
        request: F,
    }
    impl<F: FnMut(&Request)> Backend for Hook<'_, F> {
        type Lease = usize;
        fn guard(&mut self, g: Guard) -> Result<(), Error> {
            self.inner.guard(g)
        }
        fn acquire(&mut self) -> Result<usize, Error> {
            self.inner.acquire()
        }
        fn release(&mut self, l: usize) -> Result<(), Error> {
            self.inner.release(l)
        }
        fn invoke(&mut self, r: &Request) -> Result<[u64; 8], Error> {
            if r.kind() == Kind::AuthAndReset {
                (self.request)(r);
            }
            self.inner.invoke(r)
        }
    }
    with_boot(|b, p, _, _, _| {
        until_auth(b, p);
        b.step(&mut Hook {
            inner: p,
            request: &mut request,
        })
        .unwrap();
        receipt(b.take_auth_returned().unwrap());
    });
}
#[cfg(mpss_glink_transport_tests)]
pub(crate) fn with_auth_returned(f: impl FnOnce(AuthReturned)) {
    with_boot(|b, p, _, _, _| {
        until_auth(b, p);
        b.step(p).unwrap();
        f(b.take_auth_returned().unwrap());
    });
}
#[cfg(feature = "sunfish-linux-mpss-glink")]
mod glink_memory {
    use super::*;
    include!("sunfish_mpss_linux_glink_cases.rs");
}
use crate::{
    qcom_modem_handlers::Fault, qcom_modem_ram_storage::RamShadow, qcom_modem_storage as disk,
    sunfish_linux_rmtfs as r, sunfish_linux_rmtfs_native as native,
    sunfish_linux_rmtfs_server as server, sunfish_mpss_linux_boot as boot,
};
struct Life {
    o: r::Observation,
    events: Vec<&'static str>,
    assignments: usize,
    auth: usize,
    refuse: bool,
    unwind: bool,
    stay_stopped: bool,
    revoke: bool,
    assign_error: bool,
}
#[derive(Clone)]
struct Gate(Rc<RefCell<Life>>);
impl native::Gate for Gate {
    fn observe(&mut self) -> Result<r::Observation, Fault> {
        Ok(self.0.borrow().o)
    }
}
impl native::AuthGate for Gate {
    fn submit_mpss_auth(&mut self, g: u64, q: &Request) -> Result<(), Fault> {
        let mut s = self.0.borrow_mut();
        assert_eq!(g, 1);
        assert!(s.o.modem_stopped);
        assert_eq!(q.kind(), Kind::AuthAndReset);
        assert_eq!(q.words(), &[0x42000205, 1, 4, 0, 0, 0, 0, 0]);
        s.events.push("reset-may-run");
        s.auth += 1;
        s.o.modem_stopped = s.stay_stopped;
        s.o.initial_hlos = false;
        if s.revoke {
            s.o.identity += 1;
        }
        assert!(!s.unwind, "uncertain reset-lifecycle transition");
        if s.refuse {
            Err(Fault::Uncertain)
        } else {
            Ok(())
        }
    }
}
struct Assign(Rc<RefCell<Life>>);
impl native::Smc for Assign {
    fn invoke(&mut self, _: &r::Call) -> Result<[u64; 8], Fault> {
        let mut s = self.0.borrow_mut();
        assert!(s.o.modem_stopped);
        s.assignments += 1;
        let mut a = [0; 8];
        a[1] = u64::from(s.assign_error);
        Ok(a)
    }
}
type Io = native::Native<Gate, Assign>;
type Boot<'a, 'b> = boot::Boot<'a, 'b, Cpu, Io>;
struct Power {
    pas: Platform,
    life: Rc<RefCell<Life>>,
    restore_stopped_after_auth: bool,
}
impl Backend for Power {
    type Lease = usize;
    fn guard(&mut self, g: Guard) -> Result<(), Error> {
        assert_eq!(g.generation, 1);
        // Reuse the original CPU-only SCM resource/error fixture, adapting
        // only its synthetic generation. This is not a native power permit.
        self.pas.guard(Guard {
            generation: 77,
            ..g
        })
    }
    fn acquire(&mut self) -> Result<usize, Error> {
        self.pas.acquire()
    }
    fn release(&mut self, l: usize) -> Result<(), Error> {
        self.pas.release(l)?;
        if self.restore_stopped_after_auth && self.pas.0.borrow().authenticated {
            self.life.borrow_mut().o.modem_stopped = true;
        }
        Ok(())
    }
    fn invoke(&mut self, r: &Request) -> Result<[u64; 8], Error> {
        if r.kind() == Kind::AuthAndReset {
            let mut s = self.life.borrow_mut();
            assert!(!s.o.modem_stopped);
            assert_eq!(s.auth, 1);
            s.events.push("actual-auth-invoke");
        }
        self.pas.invoke(r)
    }
}
fn with_boot(
    f: impl FnOnce(&mut Boot<'_, '_>, &mut Power, &Rc<RefCell<Life>>, &Rc<RefCell<State>>, &mut [Page]),
) {
    let firmware = fixture();
    let mut buffer: Vec<Page> = (0..514).map(|_| Page([0xa5; 4096])).collect();
    let mut scratch: Vec<Page> = (0..3).map(|_| Page([0xa5; 4096])).collect();
    let make = |va, pa, bytes| r::Mapping {
        virtual_address: va,
        physical_address: pa,
        bytes,
        writable: true,
        executable: false,
        cached: false,
    };
    let b = buffer.as_mut_ptr().wrapping_add(1).cast::<u8>();
    let maps = r::Mappings {
        buffer: make(b as u64, r::BUFFER_PA, r::BUFFER_BYTES),
        scratch: make(
            scratch.as_mut_ptr().wrapping_add(1) as u64,
            META_PA + 0x10000,
            4096,
        ),
    };
    let life = Rc::new(RefCell::new(Life {
        o: r::Observation {
            generation: 1,
            identity: 88,
            ready: true,
            mappings: maps,
            modem_stopped: true,
            initial_hlos: true,
        },
        events: vec![],
        assignments: 0,
        auth: 0,
        refuse: false,
        unwind: false,
        stay_stopped: false,
        revoke: false,
        assign_error: false,
    }));
    let io = crate::boot_native_io(
        Gate(life.clone()),
        Assign(life.clone()),
        maps,
        &mut buffer,
        &mut scratch,
    );
    let mut pool = r::Pool::attach(1, maps, io);
    for _ in 0..512 {
        pool.prepare_step().unwrap();
    }
    pool.share().unwrap();
    let source = core::array::from_fn::<_, 4, _>(|i| vec![i as u8 + 1; 8192]);
    let mut dest = core::array::from_fn::<_, 4, _>(|_| vec![0; 8192]);
    let c = disk::Configuration {
        generation: 1,
        device: [3; 32],
        bindings: core::array::from_fn(|i| disk::Binding {
            partition: disk::PARTITIONS[i],
            object: i as u64 + 1,
            source: digest(&source[i]),
            disk: crate::qcom_modem_handlers::Disk {
                bytes: 8192,
                writable: true,
                durable: false,
            },
        }),
    };
    let [a, b0, c0, d] = &mut dest;
    let ram = RamShadow::new(
        c,
        source.each_ref().map(|s| s.as_slice()),
        [a, b0, c0, d].map(|b| b.as_mut_slice()),
    )
    .unwrap();
    let resources = r::Resources::attach(pool, ram.into_storage().unwrap());
    let server = server::Server::attach(
        server::Configuration {
            generation: 1,
            local: crate::qcom_qmi_router::Address {
                node: 1,
                port: 0x4000,
            },
            peer_node: 0,
        },
        resources,
    );
    let state = Rc::new(RefCell::new(State {
        metadata: vec![0xa5; METADATA_BYTES],
        target: vec![0xa5; SMALL],
        ..State::default()
    }));
    let loader = Loader::new(
        1,
        image(&firmware),
        Cpu {
            state: state.clone(),
            pa: META_PA,
        },
    )
    .ok()
    .unwrap();
    let mut owner = boot::Boot::new(1, loader, server);
    let mut power = Power {
        pas: Platform(state.clone()),
        life: life.clone(),
        restore_stopped_after_auth: false,
    };
    f(&mut owner, &mut power, &life, &state, &mut buffer[1..513]);
    assert!(
        buffer
            .first()
            .unwrap()
            .0
            .iter()
            .chain(buffer.last().unwrap().0.iter())
            .all(|v| *v == 0xa5)
    );
    assert!(
        scratch
            .first()
            .unwrap()
            .0
            .iter()
            .chain(scratch.last().unwrap().0.iter())
            .all(|v| *v == 0xa5)
    );
}
fn until_auth(b: &mut Boot<'_, '_>, p: &mut Power) {
    for _ in 0..1000 {
        if b.loader_report().phase == Phase::AuthAndReset {
            return;
        }
        b.step(p).unwrap();
    }
    panic!("bounded load");
}
fn no_replay(b: &mut Boot<'_, '_>, p: &mut Power, s: &Rc<RefCell<State>>) {
    let effects = (s.borrow().calls.len(), s.borrow().memory_accesses);
    assert!(b.step(p).is_err());
    assert!(b.server().is_err());
    assert!(b.take_auth_returned().is_err());
    assert_eq!(
        (s.borrow().calls.len(), s.borrow().memory_accesses),
        effects
    );
    assert_eq!(s.borrow().dropped, 0, "target owner stays retained");
}
#[test]
fn auth_handoff_external_quarantine_retains_and_stops_before_and_after_auth() {
    for after_auth in [false, true] {
        with_boot(|b, p, _, s, _| {
            until_auth(b, p);
            if after_auth {
                b.step(p).unwrap();
            }
            let calls = s.borrow().calls.len();
            b.quarantine();
            assert_eq!(b.report().phase, boot::Phase::Failed);
            assert_eq!(b.report().error, Some(boot::Error::Resource));
            no_replay(b, p, s);
            assert_eq!(s.borrow().calls.len(), calls);
        });
    }
}
#[test]
fn auth_handoff_uses_actual_shared_pool_and_native_gate_before_auth() {
    with_boot(|b, p, l, s, _| {
        until_auth(b, p);
        assert!(l.borrow().o.modem_stopped);
        assert_eq!(l.borrow().auth, 0);
        assert_eq!(b.step(p), Ok(Phase::AuthReturned));
        assert_eq!(b.report().phase, boot::Phase::AuthReturned);
        assert!(b.report().auth_attempted);
        assert_eq!(l.borrow().events, ["reset-may-run", "actual-auth-invoke"]);
        assert!(!l.borrow().o.modem_stopped);
        assert_eq!(l.borrow().assignments, 1);
        assert!(b.server().is_ok());
        assert_eq!(b.take_auth_returned().unwrap().generation(), 1);
        assert_eq!(s.borrow().acquisitions, 3);
        assert_eq!(s.borrow().releases, 3);
    });
}
#[test]
fn auth_handoff_bad_both_statuses_transport_and_release_retain_running_unknown() {
    for how in 0..4 {
        with_boot(|b, p, l, s, _| {
            until_auth(b, p);
            if how < 3 {
                s.borrow_mut().bad = Some((5, how));
            } else {
                s.borrow_mut().deny_release = true;
            }
            assert!(b.step(p).is_err());
            assert_eq!(l.borrow().auth, 1);
            assert!(!l.borrow().o.modem_stopped);
            assert_eq!(b.report().phase, boot::Phase::Failed);
            no_replay(b, p, s);
        });
    }
}
#[test]
fn auth_handoff_gate_failure_stale_stopped_identity_and_unwind_prevent_secure_call() {
    for how in 0..4 {
        with_boot(|b, p, l, s, _| {
            until_auth(b, p);
            {
                let mut o = l.borrow_mut();
                match how {
                    0 => o.refuse = true,
                    1 => o.stay_stopped = true,
                    2 => o.revoke = true,
                    _ => o.unwind = true,
                }
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| b.step(p)));
            if how == 3 {
                assert!(result.is_err());
            } else {
                assert!(
                    result
                        .expect("a rejected gate must return, not panic")
                        .is_err()
                );
            }
            assert_eq!(l.borrow().auth, 1);
            assert_eq!(
                s.borrow().calls.len(),
                5,
                "no actual AUTH after rejected lifecycle"
            );
            assert_eq!(b.report().phase, boot::Phase::Failed);
            no_replay(b, p, s);
        });
    }
}
#[test]
fn auth_handoff_monitor_unwind_or_final_guard_error_cannot_forge_success() {
    for unwind in [true, false] {
        with_boot(|b, p, l, s, _| {
            until_auth(b, p);
            if unwind {
                s.borrow_mut().panic_call = Some(5);
            } else {
                // Initial tick guard, post-acquire guard, then final post-call guard.
                let next = s.borrow().guards + 3;
                s.borrow_mut().deny_guard = Some(next);
            }
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| b.step(p)));
            assert!(r.is_err() || r.unwrap().is_err());
            assert_eq!(l.borrow().auth, 1);
            assert!(b.take_auth_returned().is_err());
            no_replay(b, p, s);
        });
    }
}
#[test]
fn auth_handoff_storage_lifecycle_loss_stops_before_any_auth_transition() {
    with_boot(|b, p, l, s, _| {
        until_auth(b, p);
        l.borrow_mut().o.generation += 1;
        assert!(b.step(p).is_err());
        assert_eq!(l.borrow().auth, 0);
        assert!(l.borrow().o.modem_stopped);
        assert_eq!(s.borrow().calls.len(), 5);
        no_replay(b, p, s);
    });
}

#[test]
fn auth_handoff_requires_stopped_provenance_throughout_loading_and_rejects_early_service() {
    with_boot(|b, p, l, s, _| {
        b.step(p).unwrap();
        let memory = s.borrow().memory_accesses;
        l.borrow_mut().o.modem_stopped = false;
        assert!(b.step(p).is_err());
        assert_eq!(s.borrow().memory_accesses, memory);
        assert_eq!(s.borrow().calls.len(), 0);
        assert_eq!(l.borrow().auth, 0);
        no_replay(b, p, s);
    });
    with_boot(|b, p, l, s, _| {
        assert!(b.server().is_err());
        assert_eq!(l.borrow().auth, 0);
        assert_eq!(s.borrow().memory_accesses, 0);
        no_replay(b, p, s);
    });
}

#[test]
fn auth_handoff_final_runtime_lifecycle_is_required_before_returning_receipt() {
    with_boot(|b, p, l, s, _| {
        until_auth(b, p);
        p.restore_stopped_after_auth = true;
        assert!(b.step(p).is_err());
        assert_eq!(l.borrow().auth, 1);
        assert!(s.borrow().authenticated);
        assert!(b.take_auth_returned().is_err());
        no_replay(b, p, s);
    });
}

#[test]
fn auth_handoff_native_adapter_requires_completed_successful_assignment_itself() {
    use boot::AuthIo;
    for mode in 0..5 {
        with_boot(|b, p, l, _, _| {
            let mut buffer: Vec<Page> = (0..514).map(|_| Page([0; 4096])).collect();
            let mut scratch: Vec<Page> = (0..3).map(|_| Page([0; 4096])).collect();
            let mut o = l.borrow().o;
            o.mappings.buffer.virtual_address = buffer.as_mut_ptr().wrapping_add(1) as u64;
            o.mappings.scratch.virtual_address = scratch.as_mut_ptr().wrapping_add(1) as u64;
            let extra = Rc::new(RefCell::new(Life {
                o,
                events: vec![],
                assignments: 0,
                auth: 0,
                refuse: false,
                unwind: false,
                stay_stopped: false,
                revoke: false,
                assign_error: mode == 2,
            }));
            let mut io = crate::boot_native_io(
                Gate(extra.clone()),
                Assign(extra.clone()),
                o.mappings,
                &mut buffer,
                &mut scratch,
            );
            let q = r::Call::new(o.mappings).unwrap();
            if mode > 0 {
                r::Io::stage(&mut io, &q).unwrap();
            }
            if mode > 1 {
                r::Io::invoke(&mut io, &q).unwrap();
            }
            if mode == 2 {
                assert!(r::Io::complete(&mut io).is_err());
            }
            if mode == 4 {
                r::Io::complete(&mut io).unwrap();
            }
            struct Extra<'a> {
                p: &'a mut Power,
                io: &'a mut Io,
                mode: usize,
            }
            impl Backend for Extra<'_> {
                type Lease = usize;
                fn guard(&mut self, g: Guard) -> Result<(), Error> {
                    self.p.guard(g)
                }
                fn acquire(&mut self) -> Result<usize, Error> {
                    self.p.acquire()
                }
                fn release(&mut self, l: usize) -> Result<(), Error> {
                    self.p.release(l)
                }
                fn invoke(&mut self, r: &Request) -> Result<[u64; 8], Error> {
                    if r.kind() == Kind::AuthAndReset {
                        assert_eq!(self.io.submit_mpss_auth(1, r).is_ok(), self.mode == 4);
                        assert!(
                            self.io.submit_mpss_auth(1, r).is_err(),
                            "native transition is one-shot"
                        );
                    }
                    self.p.invoke(r)
                }
            }
            let mut delegate = Extra {
                p,
                io: &mut io,
                mode,
            };
            for _ in 0..1000 {
                if b.step(&mut delegate).unwrap() == Phase::AuthReturned {
                    break;
                }
            }
            assert_eq!(b.report().phase, boot::Phase::AuthReturned);
            assert_eq!(extra.borrow().auth, usize::from(mode == 4));
        });
    }
}

fn drive_service(b: &mut Boot<'_, '_>, t: &mut crate::LoRig) {
    for _ in 0..4096 {
        let n = t.owner.notified(8, &mut t.irqs).unwrap();
        let p = t
            .owner
            .dispatch_linux_rmtfs_step(b.server().unwrap(), &mut t.irqs)
            .unwrap();
        let sent = t.drain();
        if n.received == 0
            && n.published == 0
            && p == crate::qcom_modem_owner::ServiceProgress::Idle
            && !sent
        {
            return;
        }
    }
    panic!("bounded service dispatch");
}
fn query(
    b: &mut Boot<'_, '_>,
    t: &mut crate::LoRig,
    id: u16,
    txn: u16,
    fields: &[(u8, &[u8])],
) -> Vec<u8> {
    t.enqueue(&crate::lo_packet(
        1,
        9,
        0x4000,
        true,
        &crate::lo_message(0, id, txn, fields),
    ));
    drive_service(b, t);
    let packet = t
        .host
        .iter()
        .find(|p| {
            crate::lo_word(p, 12) == 0x4000
                && u16::from_le_bytes(p[33..35].try_into().unwrap()) == txn
        })
        .unwrap();
    let q = crate::qcom_qmi::Message::decode(&packet[32..32 + crate::lo_word(packet, 20) as usize])
        .unwrap();
    assert_eq!(q.get(2), Some([0, 0, 0, 0].as_slice()));
    q.get(0x10).unwrap_or(&[]).to_vec()
}
#[test]
fn auth_handoff_retained_native_pool_serves_real_qrtr_storage_after_auth_without_global_ready() {
    with_boot(|b, p, l, _, ram| {
        until_auth(b, p);
        b.step(p).unwrap();
        let receipt = b.take_auth_returned().unwrap();
        assert_eq!(receipt.generation(), 1);
        assert!(!l.borrow().o.modem_stopped);
        let mut t = crate::LoRig::new();
        t.recycle_intents = true;
        t.owner
            .activate_linux_rmtfs(b.server().unwrap(), &mut t.irqs)
            .unwrap();
        drive_service(b, &mut t);
        let h = query(b, &mut t, 1, 700, &[(1, b"/boot/modem_fs1")]);
        let pa = query(b, &mut t, 4, 701, &[(1, &h), (2, &4096u32.to_le_bytes())]);
        assert_eq!(u64::from_le_bytes(pa.try_into().unwrap()), r::BUFFER_PA);
        let mut v = vec![1];
        for x in [0u32, r::BUFFER_PA as u32, 8] {
            v.extend(x.to_le_bytes());
        }
        query(b, &mut t, 3, 702, &[(1, &h), (2, &[0]), (3, &v), (4, &[0])]);
        assert!(ram[0].0.iter().all(|v| *v == 1));
        ram[0].0.fill(0x67); // simulated peer publication into the shared buffer
        query(b, &mut t, 3, 703, &[(1, &h), (2, &[1]), (3, &v), (4, &[0])]);
        ram[0].0.fill(0);
        query(b, &mut t, 3, 704, &[(1, &h), (2, &[0]), (3, &v), (4, &[0])]);
        assert!(ram[0].0.iter().all(|v| *v == 0x67));
        assert_eq!(l.borrow().assignments, 1);
        assert_eq!(l.borrow().auth, 1);
        assert_eq!(b.report().phase, boot::Phase::AuthReturned);
    });
}
