extern crate self as personal_os_rust_server;
include!("sunfish_ipa_linux_receive_dispatch_test.rs");
#[path = "../rust/src/pixel_ipa_receive_policy.rs"]
mod policy;
// Only the split function is exercised here; these types satisfy the other
// independent policy functions without modeling an SMP2P device.
#[allow(dead_code)]
pub mod sunfish_mpss_linux_smp2p {
    #[derive(PartialEq)]
    pub enum Poll { Observed }
    #[derive(Clone, Copy)]
    pub struct Peer { pub slave_kernel: Option<u32> }
    pub struct Report { pub ready_seen: bool, pub peer: Option<Peer> }
}
#[allow(dead_code)]
#[path = "../rust/src/pixel_mpss_auth_irq.rs"]
mod auth_irq_policy;
struct Capture {
    io: RxIo,
    states: Vec<GuardState>,
}
impl Io<fixture::Ram, RxData> for Capture {
    fn ticks(&mut self) -> u64 {
        Io::ticks(&mut self.io)
    }
    fn guard(
        &mut self,
        r: &Pool<fixture::Ram>,
        d: &DataPool<RxData>,
        s: GuardState,
    ) -> Result<(), Error> {
        self.states.push(s);
        Io::guard(&mut self.io, r, d, s)
    }
    fn read(&mut self, r: Read) -> Result<u32, Error> {
        Io::read(&mut self.io, r)
    }
    fn write(&mut self, r: &Request) -> Result<(), Error> {
        Io::write(&mut self.io, r)
    }
    fn acknowledge_start(&mut self) -> Result<(), Error> {
        Io::acknowledge_start(&mut self.io)
    }
}
fn witnesses() -> (Vec<GuardState>, Vec<Word>) {
    let (owned, mut once, io) = rx_prepared();
    let mut io = Capture { io, states: vec![] };
    once.begin(
        &owned.owners.0,
        &owned.owners.1,
        &owned.owners.2,
        &owned.memory,
        &owned.tables,
        &mut io,
    )
    .unwrap();
    let now = io.io.base.now;
    once.notified(&mut io, now).unwrap();
    assert_eq!(once.report().phase, Phase::Armed);
    (io.states, io.io.base.writes)
}
fn key(w: Word) -> (u8, usize, u32) {
    ((w.space == Space::Gsi) as u8, w.offset, w.value)
}
#[test]
fn nativepolicy_startup_accepts_exact_original_c_writes_and_only_fixed_containment() {
    let (_, words) = witnesses();
    let golden = std::fs::read_to_string(env!("SUNFISH_RECEIVE_STARTUP_C")).unwrap();
    let mut expected = std::collections::BTreeSet::new();
    for row in golden.lines() {
        let p: Vec<_> = row.split_whitespace().collect();
        expected.insert((
            (p[0] == "gsi") as u8,
            usize::from_str_radix(p[1], 16).unwrap(),
            u32::from_str_radix(p[2], 16).unwrap(),
        ));
    }
    assert_eq!(words.len(), 27);
    for w in words {
        assert!(expected.contains(&key(w)));
        assert!(policy::startup(w));
    }
    expected.insert((1, 0x1f088, 0));
    expected.insert((0, 0x300c, 0));
    let mut values: Vec<_> = expected.iter().map(|(_, _, v)| *v).collect();
    values.extend([0, 1, 2, 3, 4, 8, 0x44, 0x4c, 0x4d, u32::MAX]);
    values.sort();
    values.dedup();
    for space in [Space::Ipa, Space::Gsi] {
        let pages = if space == Space::Ipa {
            vec![0, 0x3000]
        } else {
            vec![0xc000, 0x1c000, 0x1d000, 0x1e000, 0x1f000]
        };
        for page in pages {
            for offset in (page..page + 4096).step_by(4) {
                for value in &values {
                    let w = Word {
                        space,
                        offset,
                        value: *value,
                    };
                    assert_eq!(policy::startup(w), expected.contains(&key(w)), "{w:?}");
                    assert!(!policy::startup(Word {
                        offset: offset + 1,
                        ..w
                    }));
                }
            }
        }
    }
    for &(space, offset, value) in &expected {
        for bit in 0..32 {
            let w = Word {
                space: if space == 1 { Space::Gsi } else { Space::Ipa },
                offset,
                value: value ^ (1 << bit),
            };
            assert_eq!(policy::startup(w), expected.contains(&key(w)));
        }
    }
}
#[test]
fn nativepolicy_runtime_doorbells_never_leave_the_two_owned_rings() {
    for (register, base) in [
        (rt::Register::EventDoorbell, 0x80008000u32),
        (rt::Register::ChannelDoorbell, 0x80009000u32),
    ] {
        for delta in 0..0x1020u32 {
            let value = base - 16 + delta;
            assert_eq!(
                policy::receive(register, value),
                value >= base && value < base + 4096 && value % 16 == 0
            );
        }
        for value in [0, u32::MAX, 0x80003000, 0x80004000, 0x80200000] {
            assert!(!policy::receive(register, value));
        }
    }
    for value in 0..256 {
        assert_eq!(
            policy::receive(rt::Register::IeobMask, value),
            matches!(value, 1 | 3)
        );
        assert_eq!(policy::receive(rt::Register::IeobClear, value), value == 2);
    }
}
#[test]
fn nativepolicy_sealed_transition_state_controls_config_rx_state_and_aggregation() {
    let (states, _) = witnesses();
    assert!(states.iter().any(|s| s.rx_running() && !s.route_set()));
    assert!(states.iter().any(|s| s.route_set()));
    let cold = sunfish_ipa_linux_config::Snapshot {
        config: core::array::from_fn(|n| 0x11220000 + n as u32),
        guards: [0x08090811, 0x415, 0, 0, 0, 0xe4046b59],
    };
    for s in states {
        let mut actual = cold;
        actual.guards[3] = s.ipa_mask();
        actual.guards[4] = 1;
        assert!(policy::config(cold, actual, s));
        for n in 0..9 {
            for bit in 0..32 {
                let mut bad = actual;
                bad.config[n] ^= 1 << bit;
                assert!(!policy::config(cold, bad, s));
                let f = policy::config_failure(cold, bad, s);
                assert_eq!((f.stage, f.space, f.observed, f.expected, f.mask),
                           (3, 0, bad.config[n], cold.config[n], u32::MAX));
                assert_eq!(f.offset, sunfish_ipa_linux_config::PROFILE[n].offset as u32);
            }
        }
        for n in 0..6 {
            for bit in 0..32 {
                let mut bad = actual;
                bad.guards[n] ^= 1 << bit;
                let permitted = n == 2
                    && bit == 8
                    && s.rx_running()
                    && s.ipa_mask() == 0x400c
                    && s.suspend_mask() == 256;
                assert_eq!(
                    policy::config(cold, bad, s),
                    permitted,
                    "guard {n}/{bit}, {s:?}"
                );
                if !permitted {
                    let f = policy::config_failure(cold, bad, s);
                    assert_eq!(f.stage, 3);
                    assert_ne!(f.observed & f.mask, f.expected & f.mask);
                }
            }
        }
        for n in [0, 1, 2, 3, 4, 5] {
            let mut forged = cold;
            forged.guards[n] ^= 1;
            assert!(!policy::config(forged, actual, s));
        }
        for observed in 0..16 {
            let expected = if s.rx_running() {
                observed == 2
            } else if s.gsi_channel_mask() == 4 {
                matches!(observed, 1 | 2)
            } else {
                observed == 1
            };
            assert_eq!(policy::rx_state(s, observed), expected);
        }
    }
}

#[test]
fn nativepolicy_guard_failure_keeps_the_first_failure_even_after_later_faults() {
    let first = policy::GuardFailure { stage: 7, space: 1, offset: 0x1234, observed: 8, expected: 0, mask: 15 };
    let second = policy::GuardFailure { stage: 9, ..first };
    let mut saved = None;
    policy::first_guard_failure(&mut saved, first);
    policy::first_guard_failure(&mut saved, second);
    assert_eq!(saved, Some(first));
}

#[test]
fn nativepolicy_config_trace_reports_all_mismatches_from_existing_snapshots() {
    let expected = sunfish_ipa_linux_config::Snapshot {
        config: [0, 1, 0x30000000, 3, 4, 5, 6, 7, 8], guards: [0; 6],
    };
    let mut actual = expected;
    actual.config[2] = 0;
    actual.config[7] ^= 1;
    let trace = policy::config_trace(expected, actual, Some(0x30000000), 17);
    assert_eq!(trace.actual, actual.config);
    assert_eq!(trace.mismatch, (1 << 2) | (1 << 7));
    assert_eq!((trace.clkon_baseline, trace.last_good_clkon, trace.successful_snapshots),
               (0x30000000, 0x30000000, 17));
    assert_eq!(policy::config_trace(expected, expected, None, 0).last_good_clkon, u32::MAX);
}

#[test]
fn nativepolicy_reference_pair_requires_ready_running_route_and_every_other_check() {
    let (states, _) = witnesses();
    let expected = sunfish_ipa_linux_config::Snapshot {
        config: [0, 0x400, 0x30000000, 0xf81f8, 8, 12, 0xf0, 0, 0x100],
        guards: [0x08090811, 0x415, 0, 0, 0, 0xe4046b59],
    };
    for state in states {
        let mut pair = expected;
        pair.guards[3] = state.ipa_mask();
        pair.guards[4] = 1;
        pair.config[2] = 0;
        pair.config[4] = 0x78;
        assert!(!policy::post_modem_config(expected, pair, state, false));
        let eligible = state.rx_running() && state.route_set();
        assert_eq!(policy::post_modem_config(expected, pair, state, true), eligible);
        if !eligible { continue; }
        assert!(!policy::config(expected, pair, state), "strict mode remains unchanged");
        for index in 0..pair.config.len() {
            for bit in 0..32 {
                let mut bad = pair;
                bad.config[index] ^= 1 << bit;
                assert!(!policy::post_modem_config(expected, bad, state, true), "config {index}/{bit}");
            }
        }
        for index in 0..pair.guards.len() {
            for bit in 0..32 {
                let mut bad = pair;
                bad.guards[index] ^= 1 << bit;
                let allowed = index == 2 && bit == 8 && state.ipa_mask() == 0x400c && state.suspend_mask() == 0x100;
                assert_eq!(policy::post_modem_config(expected, bad, state, true), allowed, "guard {index}/{bit}");
            }
        }
        let mut forged = expected;
        forged.config[2] ^= 1;
        assert!(!policy::post_modem_config(forged, pair, state, true));
        forged = expected;
        forged.config[4] ^= 1;
        assert!(!policy::post_modem_config(forged, pair, state, true));
    }
}
#[test]
fn nativepolicy_any_foreign_or_co_delivered_fault_badge_has_priority() {
    for badge in 0..256 {
        let expected = match badge {
            4 => dispatch::Notice::Ipa,
            8 => dispatch::Notice::Gsi,
            12 => dispatch::Notice::Both,
            _ => dispatch::Notice::Fault,
        };
        assert_eq!(policy::notice(badge), expected);
    }
    for bit in 4..64 {
        for base in [4, 8, 12] {
            assert_eq!(policy::notice((1 << bit) | base), dispatch::Notice::Fault);
        }
    }
}

#[test]
fn nativepolicy_auth_split_forwards_glink_to_ipa_as_a_fault() {
    // Model the current polled receive path using both actual policies. This
    // is a source-level reproduction, not proof the device delivered GLINK.
    for ipa in [0, 4, 8, 12] {
        let (mpss, remaining) = auth_irq_policy::split((1 << 5) | (1 << 4) | ipa);
        assert_eq!(mpss, 1 << 5);
        assert_eq!(remaining, (1 << 4) | ipa);
        assert_eq!(policy::notice(remaining), dispatch::Notice::Fault);
    }
}
