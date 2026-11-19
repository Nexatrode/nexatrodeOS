//! Bridge the verified ICNSS power sequence to per-resource RPMh vote ledgers.
//!
//! This adapter snapshots and restores the exact WLAN client's vote, encodes
//! only source-matched VRM voltage/enable actions, and sends updates through the
//! shared broker's phase-gated transport. A production caller still needs
//! an authenticated profile, complete per-resource ledgers, a live exclusive
//! lifecycle lease, and a real Apps RSC transport. Clock actions are rejected
//! until a separately verified clock backend is composed.

use crate::qcom_cmd_db::Database;
use crate::qcom_rpmh_vote_aggregate::ClientVote;
use crate::qcom_rpmh_vote_broker::{
    Broker, Commit as BrokerCommit, Error as BrokerError, ExactPhaseTransport, ExecuteError,
};
use crate::qcom_rpmh_vrm::apply_icnss_action;
use crate::qcom_wlan_power::{
    self as power, Action, Io, IoError, Rail, Resource, VerifiedIcNssProfile,
};
use crate::qcom_wlan_rpmh_map::{self, Error as ResourceMapError};

/// ICNSS uses `udelay()` for regulator settle intervals. Keep native waits
/// bounded so an invalid profile cannot monopolize the owner indefinitely.
pub const MAX_SETTLE_DELAY_MICROSECONDS: u32 = 10_000;

/// Bounded delay authority used by the source-ordered WLAN power sequence.
/// Implementations must return only after the requested interval has elapsed,
/// or fail so the sequencer can restore the touched rail prefix.
pub trait Delay {
    fn delay_us(&mut self, microseconds: u32) -> Result<(), IoError>;
}

/// Default adapter for profiles without a delay backend. Zero-length waits are
/// harmless; nonzero waits fail closed instead of being sent to the VRM codec.
pub struct RejectDelay;

impl Delay for RejectDelay {
    fn delay_us(&mut self, microseconds: u32) -> Result<(), IoError> {
        if microseconds == 0 {
            Ok(())
        } else {
            Err(IoError::Rejected)
        }
    }
}

/// Live ICNSS/Apps-RSC ownership gate. `validate` must reject a stale boot,
/// lost owner lease, or changed resource identity on every adapter operation.
pub trait LeaseGate {
    type Lease;

    fn acquire(&mut self, generation: u64) -> Result<Self::Lease, IoError>;
    fn validate(&mut self, lease: &Self::Lease, generation: u64) -> bool;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidProfile,
    ResourceMap(ResourceMapError),
    ClockBackendRequired,
    MissingBroker { rail: u8 },
    UnexpectedBroker { rail: u8 },
    IdentityMismatch { rail: u8 },
    GenerationMismatch { rail: u8 },
    ResourceMismatch { rail: u8 },
    ClientMissing { rail: u8 },
}

/// `BrokeredWlanIo` owns no RPMh authority by itself. Construction requires a
/// profile bound to each broker's resource address, identity, and generation;
/// the gate and transport enforce the live exclusive lease around hardware I/O.
pub struct BrokeredWlanIo<'a, G, T, D = RejectDelay>
where
    G: LeaseGate,
    T: ExactPhaseTransport<G::Lease>,
    D: Delay,
{
    generation: u64,
    client_id: u8,
    brokers: [Option<&'a mut Broker>; power::RAIL_COUNT],
    gate: G,
    transport: &'a mut T,
    delay: D,
}

impl<'a, G, T> BrokeredWlanIo<'a, G, T, RejectDelay>
where
    G: LeaseGate,
    T: ExactPhaseTransport<G::Lease>,
{
    pub fn new(
        profile: &VerifiedIcNssProfile,
        command_db: &Database<'_>,
        client_id: u8,
        brokers: [Option<&'a mut Broker>; power::RAIL_COUNT],
        gate: G,
        transport: &'a mut T,
    ) -> Result<Self, Error> {
        Self::with_delay(
            profile,
            command_db,
            client_id,
            brokers,
            gate,
            transport,
            RejectDelay,
        )
    }
}

impl<'a, G, T, D> BrokeredWlanIo<'a, G, T, D>
where
    G: LeaseGate,
    T: ExactPhaseTransport<G::Lease>,
    D: Delay,
{
    /// Construct the power adapter with an explicit bounded settle-delay
    /// implementation. The lease is checked before and after every wait.
    pub fn with_delay(
        profile: &VerifiedIcNssProfile,
        command_db: &Database<'_>,
        client_id: u8,
        brokers: [Option<&'a mut Broker>; power::RAIL_COUNT],
        gate: G,
        transport: &'a mut T,
        delay: D,
    ) -> Result<Self, Error> {
        let config = profile.config();
        let present = config.present_rails();
        if profile.identity() == 0
            || profile.generation() == 0
            || present & !power::ALL_RAILS_MASK != 0
            || power::verified_sunfish_plan(profile).is_err()
        {
            return Err(Error::InvalidProfile);
        }
        if config.cxo_ref_clock_present() {
            return Err(Error::ClockBackendRequired);
        }
        let resolved_addresses =
            qcom_wlan_rpmh_map::sunfish_rail_addresses(command_db).map_err(Error::ResourceMap)?;

        for index in 0..power::RAIL_COUNT {
            if profile.rail_address(index) != Some(resolved_addresses[index]) {
                return Err(Error::ResourceMismatch { rail: index as u8 });
            }
            let expected = present & (1 << index) != 0;
            match (expected, brokers[index].as_deref()) {
                (false, None) => {}
                (true, None) => return Err(Error::MissingBroker { rail: index as u8 }),
                (false, Some(_)) => return Err(Error::UnexpectedBroker { rail: index as u8 }),
                (true, Some(broker)) => {
                    if broker.identity() != profile.identity() {
                        return Err(Error::IdentityMismatch { rail: index as u8 });
                    }
                    if broker.generation() != profile.generation() {
                        return Err(Error::GenerationMismatch { rail: index as u8 });
                    }
                    if Some(broker.resource_address()) != profile.rail_address(index) {
                        return Err(Error::ResourceMismatch { rail: index as u8 });
                    }
                    if broker.client_vote(client_id).is_none() {
                        return Err(Error::ClientMissing { rail: index as u8 });
                    }
                }
            }
        }

        Ok(Self {
            generation: profile.generation(),
            client_id,
            brokers,
            gate,
            transport,
            delay,
        })
    }

    fn validate_lease(&mut self, lease: &G::Lease) -> Result<(), IoError> {
        if self.gate.validate(lease, self.generation) {
            Ok(())
        } else {
            Err(IoError::Uncertain)
        }
    }

    #[cfg(test)]
    fn test_transport(&self) -> &T {
        self.transport
    }

    fn update_vote(
        &mut self,
        lease: &G::Lease,
        index: usize,
        vote: ClientVote,
    ) -> Result<(), IoError> {
        self.validate_lease(lease)?;
        let broker = self.brokers[index]
            .as_deref_mut()
            .ok_or(IoError::Rejected)?;
        let plan = broker
            .begin_update(self.client_id, vote)
            .map_err(|error| match error {
                BrokerError::NotReady | BrokerError::TicketExhausted => IoError::Uncertain,
                _ => IoError::Rejected,
            })?;
        match broker.execute(plan, lease, self.transport) {
            Ok(BrokerCommit::Committed) => Ok(()),
            Ok(BrokerCommit::Waiting) => Err(IoError::Uncertain),
            Err(ExecuteError::Plan(BrokerError::NotReady)) => Err(IoError::Uncertain),
            Err(ExecuteError::Plan(_)) => Err(IoError::Rejected),
            Err(ExecuteError::Command(_)) => Err(IoError::Rejected),
            Err(ExecuteError::Transport(_)) | Err(ExecuteError::Completion(_)) => {
                Err(IoError::Uncertain)
            }
        }
    }
}

impl<G, T, D> Io for BrokeredWlanIo<'_, G, T, D>
where
    G: LeaseGate,
    T: ExactPhaseTransport<G::Lease>,
    D: Delay,
{
    type Snapshot = ClientVote;
    type Lease = G::Lease;

    fn acquire(&mut self, generation: u64) -> Result<Self::Lease, IoError> {
        if generation != self.generation {
            return Err(IoError::Rejected);
        }
        let lease = self.gate.acquire(generation)?;
        self.validate_lease(&lease)?;
        Ok(lease)
    }

    fn snapshot(
        &mut self,
        lease: &Self::Lease,
        resource: Resource,
    ) -> Result<Self::Snapshot, IoError> {
        self.validate_lease(lease)?;
        let index = rail_index(resource).ok_or(IoError::Rejected)?;
        self.brokers[index]
            .as_deref()
            .and_then(|broker| broker.client_vote(self.client_id))
            .ok_or(IoError::Rejected)
    }

    fn apply(&mut self, lease: &Self::Lease, action: Action) -> Result<(), IoError> {
        self.validate_lease(lease)?;
        if let Action::DelayMicroseconds { microseconds, .. } = action {
            if microseconds > MAX_SETTLE_DELAY_MICROSECONDS {
                return Err(IoError::Rejected);
            }
            self.delay.delay_us(microseconds)?;
            return self.validate_lease(lease);
        }
        let Some(rail) = action_rail(action) else {
            return Err(IoError::Rejected);
        };
        let index = rail_index(Resource::Rail(rail)).ok_or(IoError::Rejected)?;
        let current = self.brokers[index]
            .as_deref()
            .and_then(|broker| broker.client_vote(self.client_id))
            .ok_or(IoError::Rejected)?;
        let updated = apply_icnss_action(current, action).map_err(|_| IoError::Rejected)?;
        self.update_vote(lease, index, updated)
    }

    fn restore(
        &mut self,
        lease: &Self::Lease,
        resource: Resource,
        snapshot: Self::Snapshot,
    ) -> Result<(), IoError> {
        self.validate_lease(lease)?;
        let index = rail_index(resource).ok_or(IoError::Rejected)?;
        self.update_vote(lease, index, snapshot)
    }
}

fn rail_index(resource: Resource) -> Option<usize> {
    let rail = match resource {
        Resource::Rail(rail) => rail,
        Resource::CxoRefClock => return None,
    };
    Some(match rail {
        Rail::CxMx => 0,
        Rail::Xo18 => 1,
        Rail::Rfa13 => 2,
        Rail::Ch1_33 => 3,
        Rail::Ch0_33 => 4,
    })
}

fn action_rail(action: Action) -> Option<Rail> {
    match action {
        Action::SetVoltage { rail, .. }
        | Action::SetLoad { rail, .. }
        | Action::EnableRail(rail) => Some(rail),
        Action::DelayMicroseconds { .. } | Action::EnableCxoRefClock => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qcom_cmd_db::{Database, MAGIC};
    use crate::qcom_rpmh_vote_aggregate::ClientVote as BrokerVote;
    use crate::qcom_rpmh_vote_aggregate::{
        ENABLE_REGISTER, HEADROOM_REGISTER, MODE_REGISTER, VOLTAGE_REGISTER,
    };
    use crate::qcom_rpmh_vote_broker::{AggregatePhase, CompleteVoteSet, Phase as BrokerState};
    use crate::qcom_rpmh_vrm_command::Command;
    use crate::qcom_wlan_power::{
        ALL_RAILS_MASK, Config, PlanView, Sequencer, StartDisposition, verified_sunfish_plan,
    };
    use std::cell::Cell;
    use std::rc::Rc;
    use std::vec::Vec;

    const GENERATION: u64 = 9;
    const IDENTITY: u64 = 0x51;
    const CLIENT: u8 = 1;
    const OTHER: u8 = 2;
    const ADDRESSES: [u32; power::RAIL_COUNT] = [0x30000, 0x30004, 0x30008, 0x3000c, 0x30010];
    const EXPECTED_MV: [u32; power::RAIL_COUNT] = [640, 1800, 1304, 3312, 3312];
    const RESOURCE_KEYS: [&str; power::RAIL_COUNT] =
        ["ldoa9", "ldoc1", "ldoc2", "ldoc11", "ldoc10"];

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct Call {
        address: u32,
        ticket: u64,
        phase: AggregatePhase,
        command_count: usize,
        first_command: Option<Command>,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum TransportError {
        Ambiguous,
    }

    struct FakeTransport {
        calls: Vec<Call>,
        fail_at: Option<usize>,
    }

    impl FakeTransport {
        fn ready() -> Self {
            Self {
                calls: Vec::new(),
                fail_at: None,
            }
        }
    }

    impl ExactPhaseTransport<u64> for FakeTransport {
        type Error = TransportError;

        fn accept_exact_phase(
            &mut self,
            lease: &u64,
            batch: crate::qcom_rpmh_vrm_command::Batch,
        ) -> Result<(), Self::Error> {
            self.calls.push(Call {
                address: batch.resource_address,
                ticket: batch.ticket,
                phase: batch.phase,
                command_count: batch.len(),
                first_command: batch.command(0),
            });
            if *lease != batch.generation || self.fail_at == Some(self.calls.len() - 1) {
                return Err(TransportError::Ambiguous);
            }
            Ok(())
        }
    }

    struct FakeGate {
        generation: u64,
        valid: bool,
    }

    impl LeaseGate for FakeGate {
        type Lease = u64;

        fn acquire(&mut self, generation: u64) -> Result<Self::Lease, IoError> {
            if generation == self.generation && self.valid {
                Ok(generation)
            } else {
                Err(IoError::Rejected)
            }
        }

        fn validate(&mut self, lease: &Self::Lease, generation: u64) -> bool {
            self.valid && *lease == generation && generation == self.generation
        }
    }

    #[derive(Default)]
    struct FakeDelay {
        calls: Vec<u32>,
        fail: bool,
    }

    impl Delay for FakeDelay {
        fn delay_us(&mut self, microseconds: u32) -> Result<(), IoError> {
            self.calls.push(microseconds);
            if self.fail {
                Err(IoError::Uncertain)
            } else {
                Ok(())
            }
        }
    }

    struct SharedGate(Rc<Cell<bool>>);

    impl LeaseGate for SharedGate {
        type Lease = u64;

        fn acquire(&mut self, generation: u64) -> Result<Self::Lease, IoError> {
            if generation == GENERATION && self.0.get() {
                Ok(generation)
            } else {
                Err(IoError::Rejected)
            }
        }

        fn validate(&mut self, lease: &Self::Lease, generation: u64) -> bool {
            *lease == generation && generation == GENERATION && self.0.get()
        }
    }

    struct RevokeDuringDelay {
        live: Rc<Cell<bool>>,
        calls: Vec<u32>,
    }

    impl Delay for RevokeDuringDelay {
        fn delay_us(&mut self, microseconds: u32) -> Result<(), IoError> {
            self.calls.push(microseconds);
            self.live.set(false);
            Ok(())
        }
    }

    fn vote(mask: u8, registers: [u32; 4]) -> BrokerVote {
        BrokerVote {
            set_active: true,
            set_sleep: true,
            valid_mask: mask,
            registers,
        }
    }

    fn make_broker(index: usize, address: u32) -> Broker {
        let initial = vote(
            (1 << MODE_REGISTER) | (1 << HEADROOM_REGISTER),
            [0, 0, 7, 55],
        );
        let other = vote(
            (1 << MODE_REGISTER) | (1 << HEADROOM_REGISTER),
            [0, 0, 2, 30],
        );
        Broker::from_complete_set(CompleteVoteSet::test_fixture(
            address,
            IDENTITY,
            GENERATION,
            &[(CLIENT, initial), (OTHER, other)],
        ))
        .unwrap_or_else(|_| panic!("broker fixture {index}"))
    }

    fn brokers() -> [Broker; power::RAIL_COUNT] {
        [
            make_broker(0, ADDRESSES[0]),
            make_broker(1, ADDRESSES[1]),
            make_broker(2, ADDRESSES[2]),
            make_broker(3, ADDRESSES[3]),
            make_broker(4, ADDRESSES[4]),
        ]
    }

    #[test]
    fn settle_delay_waits_without_submitting_a_vrm_vote() {
        let proof = profile(false);
        let db_bytes = command_db_bytes(ADDRESSES);
        let command_db = Database::parse(&db_bytes).unwrap();
        let mut resource_brokers = brokers();
        let mut transport = FakeTransport::ready();
        let mut io = BrokeredWlanIo::with_delay(
            &proof,
            &command_db,
            CLIENT,
            refs(&mut resource_brokers),
            FakeGate {
                generation: GENERATION,
                valid: true,
            },
            &mut transport,
            FakeDelay::default(),
        )
        .unwrap();
        let lease = io.acquire(GENERATION).unwrap();

        io.apply(
            &lease,
            Action::DelayMicroseconds {
                rail: Rail::Rfa13,
                microseconds: 75,
            },
        )
        .unwrap();

        assert_eq!(io.delay.calls, [75]);
        assert!(io.test_transport().calls.is_empty());
    }

    #[test]
    fn settle_delay_revalidates_the_live_lease_after_waiting() {
        let proof = profile(false);
        let db_bytes = command_db_bytes(ADDRESSES);
        let command_db = Database::parse(&db_bytes).unwrap();
        let mut resource_brokers = brokers();
        let mut transport = FakeTransport::ready();
        let live = Rc::new(Cell::new(true));
        let mut io = BrokeredWlanIo::with_delay(
            &proof,
            &command_db,
            CLIENT,
            refs(&mut resource_brokers),
            SharedGate(live.clone()),
            &mut transport,
            RevokeDuringDelay {
                live,
                calls: Vec::new(),
            },
        )
        .unwrap();
        let lease = io.acquire(GENERATION).unwrap();

        assert_eq!(
            io.apply(
                &lease,
                Action::DelayMicroseconds {
                    rail: Rail::Rfa13,
                    microseconds: 25,
                },
            ),
            Err(IoError::Uncertain)
        );
        assert_eq!(io.delay.calls, [25]);
        assert!(io.test_transport().calls.is_empty());
    }

    #[test]
    fn settle_delay_over_limit_is_rejected_before_wait_or_rpmh_io() {
        let proof = profile(false);
        let db_bytes = command_db_bytes(ADDRESSES);
        let command_db = Database::parse(&db_bytes).unwrap();
        let mut resource_brokers = brokers();
        let mut transport = FakeTransport::ready();
        let mut io = BrokeredWlanIo::with_delay(
            &proof,
            &command_db,
            CLIENT,
            refs(&mut resource_brokers),
            FakeGate {
                generation: GENERATION,
                valid: true,
            },
            &mut transport,
            FakeDelay::default(),
        )
        .unwrap();
        let lease = io.acquire(GENERATION).unwrap();

        assert_eq!(
            io.apply(
                &lease,
                Action::DelayMicroseconds {
                    rail: Rail::Rfa13,
                    microseconds: MAX_SETTLE_DELAY_MICROSECONDS + 1,
                },
            ),
            Err(IoError::Rejected)
        );
        assert!(io.delay.calls.is_empty());
        assert!(io.test_transport().calls.is_empty());
    }

    fn refs<'a>(
        brokers: &'a mut [Broker; power::RAIL_COUNT],
    ) -> [Option<&'a mut Broker>; power::RAIL_COUNT] {
        let [a, b, c, d, e] = brokers;
        [Some(a), Some(b), Some(c), Some(d), Some(e)]
    }

    fn profile(clock: bool) -> VerifiedIcNssProfile {
        VerifiedIcNssProfile::test_fixture_with_bindings(
            GENERATION,
            IDENTITY,
            Config::new(ALL_RAILS_MASK, clock),
            ADDRESSES,
        )
    }

    fn command_db_bytes(addresses: [u32; power::RAIL_COUNT]) -> Vec<u8> {
        let entry_bytes = 24;
        let entry_count = power::RAIL_COUNT;
        let mut bytes = vec![0; crate::qcom_cmd_db::HEADER_BYTES + entry_bytes * entry_count];
        bytes[0..4].copy_from_slice(&1u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&MAGIC.to_le_bytes());
        bytes[8..10].copy_from_slice(&3u16.to_le_bytes());
        bytes[14..16].copy_from_slice(&(entry_count as u16).to_le_bytes());
        bytes[16..18].copy_from_slice(&0x0100u16.to_le_bytes());
        for index in 0..entry_count {
            let start = crate::qcom_cmd_db::HEADER_BYTES + entry_bytes * index;
            let key = RESOURCE_KEYS[index].as_bytes();
            bytes[start..start + key.len()].copy_from_slice(key);
            bytes[start + 16..start + 20].copy_from_slice(&addresses[index].to_le_bytes());
        }
        bytes
    }

    #[test]
    fn source_plan_runs_through_each_shared_vote_and_preserves_other_clients() {
        let proof = profile(false);
        let plan = verified_sunfish_plan(&proof).unwrap();
        let db_bytes = command_db_bytes(ADDRESSES);
        let command_db = Database::parse(&db_bytes).unwrap();
        let mut resource_brokers = brokers();
        let mut transport = FakeTransport::ready();
        let mut io = BrokeredWlanIo::new(
            &proof,
            &command_db,
            CLIENT,
            refs(&mut resource_brokers),
            FakeGate {
                generation: GENERATION,
                valid: true,
            },
            &mut transport,
        )
        .unwrap();
        let mut sequencer = Sequencer::<ClientVote>::default();

        assert_eq!(
            sequencer.start(&mut io, GENERATION, plan),
            Ok(StartDisposition::Started)
        );
        assert_eq!(sequencer.phase(), power::Phase::On);
        assert_eq!(io.test_transport().calls.len(), 10);
        for (index, call) in io.test_transport().calls.iter().enumerate() {
            let rail = index / 2;
            let register = if index % 2 == 0 {
                VOLTAGE_REGISTER
            } else {
                crate::qcom_rpmh_vote_aggregate::ENABLE_REGISTER
            };
            assert_eq!(call.address, ADDRESSES[rail]);
            assert_eq!(call.phase, AggregatePhase::Active);
            assert_eq!(call.command_count, 1);
            assert_eq!(
                call.first_command,
                Some(Command {
                    register: register as u8,
                    address: ADDRESSES[rail] + register as u32 * 4,
                    data: if register == VOLTAGE_REGISTER {
                        EXPECTED_MV[rail]
                    } else {
                        1
                    },
                })
            );
        }
        drop(io);

        for index in 0..power::RAIL_COUNT {
            let wlan_vote = resource_brokers[index].client_vote(CLIENT).unwrap();
            assert_eq!(wlan_vote.registers[VOLTAGE_REGISTER], EXPECTED_MV[index]);
            assert_eq!(wlan_vote.registers[ENABLE_REGISTER], 1);
            assert_eq!(wlan_vote.registers[MODE_REGISTER], 7);
            assert_eq!(wlan_vote.registers[HEADROOM_REGISTER], 55);
            assert_eq!(wlan_vote.valid_mask, 0b1111);
            assert_eq!(
                resource_brokers[index].client_vote(OTHER),
                Some(vote(0b1100, [0, 0, 2, 30]))
            );
            assert_eq!(
                resource_brokers[index].aggregate().active_registers[MODE_REGISTER],
                7
            );
            assert_eq!(
                resource_brokers[index].aggregate().sleep_registers[MODE_REGISTER],
                7
            );
            assert_eq!(resource_brokers[index].phase(), BrokerState::Ready);
        }
    }

    #[test]
    fn failed_second_active_update_quarantines_power_and_stops_sequence() {
        let proof = profile(false);
        let plan: PlanView = verified_sunfish_plan(&proof).unwrap();
        let db_bytes = command_db_bytes(ADDRESSES);
        let command_db = Database::parse(&db_bytes).unwrap();
        let mut resource_brokers = brokers();
        let mut transport = FakeTransport::ready();
        transport.fail_at = Some(1);
        let mut io = BrokeredWlanIo::new(
            &proof,
            &command_db,
            CLIENT,
            refs(&mut resource_brokers),
            FakeGate {
                generation: GENERATION,
                valid: true,
            },
            &mut transport,
        )
        .unwrap();
        let mut sequencer = Sequencer::<ClientVote>::default();

        assert!(matches!(
            sequencer.start(&mut io, GENERATION, plan),
            Err(power::Error::Rollback {
                resource: Resource::Rail(Rail::CxMx),
                ..
            })
        ));
        assert_eq!(sequencer.phase(), power::Phase::Unknown);
        assert_eq!(io.test_transport().calls.len(), 2);
        assert_eq!(resource_brokers[0].phase(), BrokerState::Unknown);
        assert!(
            resource_brokers[1..]
                .iter()
                .all(|broker| broker.phase() == BrokerState::Ready)
        );
    }

    #[test]
    fn constructor_rejects_unmatched_resource_and_unimplemented_clock() {
        let proof = profile(false);
        let db_bytes = command_db_bytes(ADDRESSES);
        let command_db = Database::parse(&db_bytes).unwrap();
        let mut resource_brokers = brokers();
        let mut mismatched = make_broker(0, 0xdead);
        let mut transport = FakeTransport::ready();
        let gate = FakeGate {
            generation: GENERATION,
            valid: true,
        };
        let mut bad_refs = refs(&mut resource_brokers);
        bad_refs[0] = Some(&mut mismatched);
        assert_eq!(
            BrokeredWlanIo::new(&proof, &command_db, CLIENT, bad_refs, gate, &mut transport).err(),
            Some(Error::ResourceMismatch { rail: 0 })
        );

        let clock_profile = profile(true);
        let mut resource_brokers = brokers();
        assert_eq!(
            BrokeredWlanIo::new(
                &clock_profile,
                &command_db,
                CLIENT,
                refs(&mut resource_brokers),
                FakeGate {
                    generation: GENERATION,
                    valid: true,
                },
                &mut transport,
            )
            .err(),
            Some(Error::ClockBackendRequired)
        );
        assert!(transport.calls.is_empty());
    }

    #[test]
    fn command_database_mapping_requires_exact_unique_vrm_resources() {
        let valid_bytes = command_db_bytes(ADDRESSES);
        let valid_db = Database::parse(&valid_bytes).unwrap();
        assert_eq!(
            qcom_wlan_rpmh_map::sunfish_rail_addresses(&valid_db),
            Ok(ADDRESSES)
        );

        let mut missing_bytes = command_db_bytes(ADDRESSES);
        let missing_key = crate::qcom_cmd_db::HEADER_BYTES + 2 * 24;
        missing_bytes[missing_key..missing_key + 5].copy_from_slice(b"other");
        let missing_db = Database::parse(&missing_bytes).unwrap();
        assert_eq!(
            qcom_wlan_rpmh_map::sunfish_rail_addresses(&missing_db),
            Err(ResourceMapError::Database(
                crate::qcom_cmd_db::Error::NotFound
            ))
        );

        let mut wrong_slave_addresses = ADDRESSES;
        wrong_slave_addresses[1] = 0x40004;
        let wrong_slave_bytes = command_db_bytes(wrong_slave_addresses);
        let wrong_slave_db = Database::parse(&wrong_slave_bytes).unwrap();
        assert_eq!(
            qcom_wlan_rpmh_map::sunfish_rail_addresses(&wrong_slave_db),
            Err(ResourceMapError::Database(
                crate::qcom_cmd_db::Error::SlaveId
            ))
        );

        let mut duplicate_addresses = ADDRESSES;
        duplicate_addresses[4] = duplicate_addresses[0];
        let duplicate_bytes = command_db_bytes(duplicate_addresses);
        let duplicate_db = Database::parse(&duplicate_bytes).unwrap();
        assert_eq!(
            qcom_wlan_rpmh_map::sunfish_rail_addresses(&duplicate_db),
            Err(ResourceMapError::DuplicateAddress {
                first_rail: 0,
                second_rail: 4,
            })
        );
    }

    #[test]
    fn constructor_rejects_profile_command_db_address_disagreement() {
        let proof = profile(false);
        let mut changed_addresses = ADDRESSES;
        changed_addresses[3] = 0x30020;
        let db_bytes = command_db_bytes(changed_addresses);
        let command_db = Database::parse(&db_bytes).unwrap();
        let mut resource_brokers = brokers();
        let mut transport = FakeTransport::ready();
        let gate = FakeGate {
            generation: GENERATION,
            valid: true,
        };
        assert_eq!(
            BrokeredWlanIo::new(
                &proof,
                &command_db,
                CLIENT,
                refs(&mut resource_brokers),
                gate,
                &mut transport,
            )
            .err(),
            Some(Error::ResourceMismatch { rail: 3 })
        );
    }
}
