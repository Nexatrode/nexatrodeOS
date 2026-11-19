//! Source-ordered Sunfish ICNSS power sequencing primitives.
//!
//! The reference profile is derived from the pinned Lineage ICNSS driver and
//! captured Sunfish DTB. The live kernel release reports the expected pinned
//! source revision, and the audited ICNSS source files are clean at that tree
//! revision; this is not binary-level attestation and does not authorize the
//! native platform. Production code therefore still cannot mint
//! `VerifiedIcNssProfile`, and this sequencer is not wired to hardware. A
//! future `Io` implementation must use a trusted shared
//! RPMh broker, snapshot this client's exact prior votes, aggregate all clients,
//! and revalidate an exclusive lifecycle lease on every operation. Register
//! readback is not a substitute for the per-client snapshot.

pub const RAIL_COUNT: usize = 5;
pub const RESOURCE_COUNT: usize = 6;
pub const MAX_ACTIONS: usize = RAIL_COUNT * 4 + 1;
pub const ALL_RAILS_MASK: u8 = (1 << RAIL_COUNT) - 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Rail {
    CxMx,
    Xo18,
    Rfa13,
    Ch1_33,
    Ch0_33,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Resource {
    Rail(Rail),
    CxoRefClock,
}

impl Resource {
    const fn index(self) -> usize {
        match self {
            Self::Rail(Rail::CxMx) => 0,
            Self::Rail(Rail::Xo18) => 1,
            Self::Rail(Rail::Rfa13) => 2,
            Self::Rail(Rail::Ch1_33) => 3,
            Self::Rail(Rail::Ch0_33) => 4,
            Self::CxoRefClock => 5,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RailSpec {
    pub rail: Rail,
    pub min_microvolts: u32,
    pub max_microvolts: u32,
    pub load_microamps: u32,
    pub settle_delay_microseconds: u32,
}

/// Values from the pinned ICNSS configuration used by the captured DTB.
/// The 3.3 V values are the driver's 3.312 V defaults because both captured
/// DTBs omit the optional `qcom,vdd-3.3-ch*-config` properties. They remain
/// unusable for hardware until the running driver identity is verified.
pub const SUNFISH_RAILS: [RailSpec; RAIL_COUNT] = [
    RailSpec {
        rail: Rail::CxMx,
        min_microvolts: 640_000,
        max_microvolts: 640_000,
        load_microamps: 0,
        settle_delay_microseconds: 0,
    },
    RailSpec {
        rail: Rail::Xo18,
        min_microvolts: 1_800_000,
        max_microvolts: 1_800_000,
        load_microamps: 0,
        settle_delay_microseconds: 0,
    },
    RailSpec {
        rail: Rail::Rfa13,
        min_microvolts: 1_304_000,
        max_microvolts: 1_304_000,
        load_microamps: 0,
        settle_delay_microseconds: 0,
    },
    RailSpec {
        rail: Rail::Ch1_33,
        min_microvolts: 3_312_000,
        max_microvolts: 3_312_000,
        load_microamps: 0,
        settle_delay_microseconds: 0,
    },
    RailSpec {
        rail: Rail::Ch0_33,
        min_microvolts: 3_312_000,
        max_microvolts: 3_312_000,
        load_microamps: 0,
        settle_delay_microseconds: 0,
    },
];

/// Opaque source/device proof required to construct an executable plan.
/// Production minting is intentionally absent until the active driver and DTB
/// are matched to this reference. Tests alone can create a fixture.
pub struct VerifiedIcNssProfile {
    generation: u64,
    identity: u64,
    config: Config,
    rail_addresses: [u32; RAIL_COUNT],
    _private: (),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Config {
    present_rails: u8,
    cxo_ref_clock_present: bool,
}

impl Config {
    /// The bits follow `SUNFISH_RAILS` order. Presence must come from a fresh,
    /// validated platform inventory; absent optional regulators are skipped.
    pub const fn new(present_rails: u8, cxo_ref_clock_present: bool) -> Self {
        Self {
            present_rails,
            cxo_ref_clock_present,
        }
    }

    pub(crate) const fn present_rails(self) -> u8 {
        self.present_rails
    }

    pub(crate) const fn cxo_ref_clock_present(self) -> bool {
        self.cxo_ref_clock_present
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    SetVoltage {
        rail: Rail,
        min_microvolts: u32,
        max_microvolts: u32,
    },
    SetLoad {
        rail: Rail,
        microamps: u32,
    },
    EnableRail(Rail),
    DelayMicroseconds {
        rail: Rail,
        microseconds: u32,
    },
    EnableCxoRefClock,
}

impl Action {
    const fn resource(self) -> Resource {
        match self {
            Self::SetVoltage { rail, .. }
            | Self::SetLoad { rail, .. }
            | Self::EnableRail(rail)
            | Self::DelayMicroseconds { rail, .. } => Resource::Rail(rail),
            Self::EnableCxoRefClock => Resource::CxoRefClock,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IoError {
    Rejected,
    Uncertain,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidRailMask,
    EmptyPlan,
    GenerationZero,
    ProfileGenerationMismatch,
    LockedOut,
    StateConflict,
    Acquire {
        cause: IoError,
    },
    Snapshot {
        resource: Resource,
        cause: IoError,
    },
    Apply {
        action_index: usize,
        action: Action,
        cause: IoError,
    },
    Rollback {
        action_index: usize,
        resource: Resource,
        cause: IoError,
    },
    RestoreLease {
        cause: IoError,
    },
    Restore {
        resource: Resource,
        cause: IoError,
    },
    StaleGeneration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartDisposition {
    Started,
    AlreadyOn,
}

/// A trusted backend owns RPMh arbitration and exposes only this named WLAN
/// transaction. `snapshot`/`restore` must capture and restore the exact local
/// contribution for one resource while preserving other consumers' votes.
/// `acquire` must pin fresh lifecycle identity and exclusive WLAN power
/// ownership until the operation sequence completes.
pub trait Io {
    type Snapshot: Copy;
    type Lease;

    fn acquire(&mut self, generation: u64) -> Result<Self::Lease, IoError>;
    fn snapshot(
        &mut self,
        lease: &Self::Lease,
        resource: Resource,
    ) -> Result<Self::Snapshot, IoError>;
    fn apply(&mut self, lease: &Self::Lease, action: Action) -> Result<(), IoError>;
    fn restore(
        &mut self,
        lease: &Self::Lease,
        resource: Resource,
        snapshot: Self::Snapshot,
    ) -> Result<(), IoError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Plan {
    actions: [Option<Action>; MAX_ACTIONS],
    action_count: usize,
    resources: [Option<Resource>; RESOURCE_COUNT],
    resource_count: usize,
}

impl Plan {
    fn build(config: Config) -> Result<Self, Error> {
        if config.present_rails & !ALL_RAILS_MASK != 0 {
            return Err(Error::InvalidRailMask);
        }

        let mut plan = Self {
            actions: [None; MAX_ACTIONS],
            action_count: 0,
            resources: [None; RESOURCE_COUNT],
            resource_count: 0,
        };
        for (index, spec) in SUNFISH_RAILS.iter().enumerate() {
            if config.present_rails & (1 << index) == 0 {
                continue;
            }
            let resource = Resource::Rail(spec.rail);
            plan.push_resource(resource);
            plan.push_action(Action::SetVoltage {
                rail: spec.rail,
                min_microvolts: spec.min_microvolts,
                max_microvolts: spec.max_microvolts,
            });
            if spec.load_microamps != 0 {
                plan.push_action(Action::SetLoad {
                    rail: spec.rail,
                    microamps: spec.load_microamps,
                });
            }
            plan.push_action(Action::EnableRail(spec.rail));
            if spec.settle_delay_microseconds != 0 {
                plan.push_action(Action::DelayMicroseconds {
                    rail: spec.rail,
                    microseconds: spec.settle_delay_microseconds,
                });
            }
        }
        if config.cxo_ref_clock_present {
            plan.push_resource(Resource::CxoRefClock);
            plan.push_action(Action::EnableCxoRefClock);
        }
        if plan.action_count == 0 {
            return Err(Error::EmptyPlan);
        }
        Ok(plan)
    }

    fn push_resource(&mut self, resource: Resource) {
        self.resources[self.resource_count] = Some(resource);
        self.resource_count += 1;
    }

    fn push_action(&mut self, action: Action) {
        self.actions[self.action_count] = Some(action);
        self.action_count += 1;
    }

    fn resource_position(&self, resource: Resource) -> Option<usize> {
        self.resources[..self.resource_count]
            .iter()
            .position(|candidate| *candidate == Some(resource))
    }
}

enum State<S: Copy> {
    Off,
    On {
        generation: u64,
        plan: PlanView,
        snapshots: [Option<S>; RESOURCE_COUNT],
    },
    Unknown,
}

/// Failure-closed executor. A failed action restores every resource touched by
/// the attempted prefix, including the action that returned an error. Failed
/// restoration latches `Unknown`; there is no retry or guessed compensating
/// vote from that state.
pub struct Sequencer<S: Copy> {
    state: State<S>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Off,
    On,
    Unknown,
}

/// Proof that the source-checked ICNSS power plan is active for one boot
/// generation. The private field prevents callers from authorizing WLFW
/// startup with a generation number alone.
#[derive(Debug, Eq, PartialEq)]
pub struct PowerReadyPermit {
    generation: u64,
}

impl PowerReadyPermit {
    pub const fn generation(&self) -> u64 {
        self.generation
    }
}

/// Proof that this sequencer successfully restored its captured votes for
/// one boot generation. Used before discarding the matching WLFW endpoint.
#[derive(Debug, Eq, PartialEq)]
pub struct PowerStoppedPermit {
    generation: u64,
}

impl PowerStoppedPermit {
    pub const fn generation(&self) -> u64 {
        self.generation
    }
}

impl<S: Copy> Default for Sequencer<S> {
    fn default() -> Self {
        Self { state: State::Off }
    }
}

impl<S: Copy> Sequencer<S> {
    pub fn phase(&self) -> Phase {
        match self.state {
            State::Off => Phase::Off,
            State::On { .. } => Phase::On,
            State::Unknown => Phase::Unknown,
        }
    }

    /// Mint a one-use authorization only while the matching plan is active.
    pub fn power_ready_permit(&self, generation: u64) -> Result<PowerReadyPermit, Error> {
        match self.state {
            State::On {
                generation: active, ..
            } if active == generation => Ok(PowerReadyPermit { generation }),
            State::Unknown => Err(Error::LockedOut),
            State::On { .. } => Err(Error::StateConflict),
            State::Off => Err(Error::StateConflict),
        }
    }

    /// Restore the exact captured votes and return proof that shutdown
    /// completed. A permit is never produced from an already-off or unknown
    /// sequencer state.
    pub fn stop_with_permit<I>(
        &mut self,
        io: &mut I,
        generation: u64,
    ) -> Result<PowerStoppedPermit, Error>
    where
        I: Io<Snapshot = S>,
    {
        if !matches!(self.state, State::On { generation: active, .. } if active == generation) {
            return match self.state {
                State::Unknown => Err(Error::LockedOut),
                State::On { .. } => Err(Error::StateConflict),
                State::Off => Err(Error::StateConflict),
            };
        }
        self.stop(io, generation)?;
        Ok(PowerStoppedPermit { generation })
    }

    pub fn start<I>(
        &mut self,
        io: &mut I,
        generation: u64,
        plan: PlanView,
    ) -> Result<StartDisposition, Error>
    where
        I: Io<Snapshot = S>,
    {
        if generation == 0 {
            return Err(Error::GenerationZero);
        }
        if plan.generation != generation {
            return Err(Error::ProfileGenerationMismatch);
        }
        let plan_data = plan.plan;
        match self.state {
            State::Unknown => return Err(Error::LockedOut),
            State::On {
                generation: current,
                plan: current_plan,
                ..
            } => {
                return if current == generation && current_plan == plan {
                    Ok(StartDisposition::AlreadyOn)
                } else {
                    Err(Error::StateConflict)
                };
            }
            State::Off => {}
        }

        let lease = io
            .acquire(generation)
            .map_err(|cause| Error::Acquire { cause })?;
        let mut snapshots = [None; RESOURCE_COUNT];
        for resource in plan_data.resources[..plan_data.resource_count]
            .iter()
            .flatten()
            .copied()
        {
            snapshots[resource.index()] = Some(
                io.snapshot(&lease, resource)
                    .map_err(|cause| Error::Snapshot { resource, cause })?,
            );
        }

        let mut touched = 0u8;
        for action_index in 0..plan_data.action_count {
            let action = plan_data.actions[action_index].expect("bounded action slot");
            let resource = action.resource();
            let resource_position = plan_data
                .resource_position(resource)
                .expect("every action has a planned resource");
            touched |= 1 << resource_position;
            if let Err(cause) = io.apply(&lease, action) {
                let rollback_error = restore_mask(io, &lease, &plan_data, &snapshots, touched);
                if let Some((resource, rollback_cause)) = rollback_error {
                    self.state = State::Unknown;
                    return Err(Error::Rollback {
                        action_index,
                        resource,
                        cause: rollback_cause,
                    });
                }
                self.state = State::Off;
                return Err(Error::Apply {
                    action_index,
                    action,
                    cause,
                });
            }
        }

        self.state = State::On {
            generation,
            plan,
            snapshots,
        };
        Ok(StartDisposition::Started)
    }

    /// Restore the exact pre-start local votes in reverse source order. If the
    /// generation changed or any restore fails, latch `Unknown` and stop.
    pub fn stop<I>(&mut self, io: &mut I, generation: u64) -> Result<(), Error>
    where
        I: Io<Snapshot = S>,
    {
        let (current_generation, plan, snapshots) = match self.state {
            State::Off => return Ok(()),
            State::Unknown => return Err(Error::LockedOut),
            State::On {
                generation,
                plan,
                snapshots,
            } => (generation, plan, snapshots),
        };
        if generation != current_generation {
            self.state = State::Unknown;
            return Err(Error::StaleGeneration);
        }
        let plan = plan.plan;
        let lease = match io.acquire(generation) {
            Ok(lease) => lease,
            Err(cause) => {
                self.state = State::Unknown;
                return Err(Error::RestoreLease { cause });
            }
        };
        let touched = (1u8 << plan.resource_count) - 1;
        if let Some((resource, cause)) = restore_mask(io, &lease, &plan, &snapshots, touched) {
            self.state = State::Unknown;
            return Err(Error::Restore { resource, cause });
        }
        self.state = State::Off;
        Ok(())
    }
}

fn restore_mask<I: Io>(
    io: &mut I,
    lease: &I::Lease,
    plan: &Plan,
    snapshots: &[Option<I::Snapshot>; RESOURCE_COUNT],
    mask: u8,
) -> Option<(Resource, IoError)> {
    let mut first_error = None;
    for position in (0..plan.resource_count).rev() {
        if mask & (1 << position) == 0 {
            continue;
        }
        let resource = plan.resources[position].expect("bounded resource slot");
        let snapshot = snapshots[resource.index()].expect("resource snapshot acquired");
        if let Err(cause) = io.restore(lease, resource, snapshot)
            && first_error.is_none()
        {
            first_error = Some((resource, cause));
        }
    }
    first_error
}

impl VerifiedIcNssProfile {
    pub(crate) const fn identity(&self) -> u64 {
        self.identity
    }

    pub(crate) const fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) const fn config(&self) -> Config {
        self.config
    }

    pub(crate) const fn rail_address(&self, index: usize) -> Option<u32> {
        if index < RAIL_COUNT {
            Some(self.rail_addresses[index])
        } else {
            None
        }
    }

    #[cfg(test)]
    pub(crate) fn test_fixture(generation: u64, config: Config) -> Self {
        Self::test_fixture_with_bindings(
            generation,
            0x51,
            config,
            [0x1001, 0x1002, 0x1003, 0x1004, 0x1005],
        )
    }

    #[cfg(test)]
    pub(crate) fn test_fixture_with_bindings(
        generation: u64,
        identity: u64,
        config: Config,
        rail_addresses: [u32; RAIL_COUNT],
    ) -> Self {
        Self {
            generation,
            identity,
            config,
            rail_addresses,
            _private: (),
        }
    }
}

/// Build the pinned Sunfish reference plan after an external verifier has
/// matched the running ICNSS source and DTB. The config's optional-resource
/// presence must also come from a fresh validated platform inventory.
pub fn verified_sunfish_plan(_profile: &VerifiedIcNssProfile) -> Result<PlanView, Error> {
    if _profile.generation == 0 {
        return Err(Error::GenerationZero);
    }
    let plan = Plan::build(_profile.config)?;
    Ok(PlanView {
        generation: _profile.generation,
        plan,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlanView {
    generation: u64,
    plan: Plan,
}

impl PlanView {
    /// Boot generation bound to this verified plan.
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub const fn action_count(&self) -> usize {
        self.plan.action_count
    }

    pub fn action(&self, index: usize) -> Option<Action> {
        if index < self.plan.action_count {
            self.plan.actions[index]
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct Snapshot(i32);

    struct FakeIo {
        values: [i32; RESOURCE_COUNT],
        next: i32,
        fail_acquire: bool,
        fail_snapshot: Option<Resource>,
        fail_apply: Option<usize>,
        fail_restore: Option<Resource>,
        apply_count: usize,
        restores: [Option<Resource>; RESOURCE_COUNT],
        restore_count: usize,
    }

    impl FakeIo {
        fn new() -> Self {
            Self {
                values: [0; RESOURCE_COUNT],
                next: 1,
                fail_acquire: false,
                fail_snapshot: None,
                fail_apply: None,
                fail_restore: None,
                apply_count: 0,
                restores: [None; RESOURCE_COUNT],
                restore_count: 0,
            }
        }
    }

    impl Io for FakeIo {
        type Snapshot = Snapshot;
        type Lease = u64;

        fn acquire(&mut self, generation: u64) -> Result<Self::Lease, IoError> {
            if generation == 0 || self.fail_acquire {
                Err(IoError::Rejected)
            } else {
                Ok(generation)
            }
        }

        fn snapshot(
            &mut self,
            _lease: &Self::Lease,
            resource: Resource,
        ) -> Result<Self::Snapshot, IoError> {
            if self.fail_snapshot == Some(resource) {
                return Err(IoError::Rejected);
            }
            Ok(Snapshot(self.values[resource.index()]))
        }

        fn apply(&mut self, _lease: &Self::Lease, action: Action) -> Result<(), IoError> {
            let index = self.apply_count;
            self.apply_count += 1;
            let resource = action.resource();
            self.values[resource.index()] = self.next;
            self.next += 1;
            if self.fail_apply == Some(index) {
                return Err(IoError::Uncertain);
            }
            Ok(())
        }

        fn restore(
            &mut self,
            _lease: &Self::Lease,
            resource: Resource,
            snapshot: Self::Snapshot,
        ) -> Result<(), IoError> {
            self.restores[self.restore_count] = Some(resource);
            self.restore_count += 1;
            if self.fail_restore == Some(resource) {
                return Err(IoError::Uncertain);
            }
            self.values[resource.index()] = snapshot.0;
            Ok(())
        }
    }

    fn full_config(clock: bool) -> Config {
        Config::new(ALL_RAILS_MASK, clock)
    }

    fn full_plan(generation: u64, clock: bool) -> PlanView {
        let proof = VerifiedIcNssProfile::test_fixture(generation, full_config(clock));
        verified_sunfish_plan(&proof).unwrap()
    }

    #[test]
    fn plan_matches_icnss_rail_order_values_and_optional_clock() {
        let proof = VerifiedIcNssProfile::test_fixture(1, full_config(true));
        let plan = verified_sunfish_plan(&proof).unwrap();
        assert_eq!(plan.action_count(), 11);
        assert_eq!(
            plan.action(0),
            Some(Action::SetVoltage {
                rail: Rail::CxMx,
                min_microvolts: 640_000,
                max_microvolts: 640_000,
            })
        );
        assert_eq!(plan.action(1), Some(Action::EnableRail(Rail::CxMx)));
        assert_eq!(
            plan.action(2),
            Some(Action::SetVoltage {
                rail: Rail::Xo18,
                min_microvolts: 1_800_000,
                max_microvolts: 1_800_000,
            })
        );
        assert_eq!(
            plan.action(6),
            Some(Action::SetVoltage {
                rail: Rail::Ch1_33,
                min_microvolts: 3_312_000,
                max_microvolts: 3_312_000,
            })
        );
        assert_eq!(
            plan.action(8),
            Some(Action::SetVoltage {
                rail: Rail::Ch0_33,
                min_microvolts: 3_312_000,
                max_microvolts: 3_312_000,
            })
        );
        assert_eq!(plan.action(10), Some(Action::EnableCxoRefClock));
        assert_eq!(plan.action(11), None);
    }

    #[test]
    fn optional_missing_resources_are_skipped_and_empty_plan_is_rejected() {
        let proof = VerifiedIcNssProfile::test_fixture(1, Config::new(0b0_0101, false));
        let plan = verified_sunfish_plan(&proof).unwrap();
        assert_eq!(plan.action_count(), 4);
        assert_eq!(
            plan.action(0),
            Some(Action::SetVoltage {
                rail: Rail::CxMx,
                min_microvolts: 640_000,
                max_microvolts: 640_000,
            })
        );
        assert_eq!(plan.action(1), Some(Action::EnableRail(Rail::CxMx)));
        assert_eq!(
            plan.action(2),
            Some(Action::SetVoltage {
                rail: Rail::Rfa13,
                min_microvolts: 1_304_000,
                max_microvolts: 1_304_000,
            })
        );
        assert_eq!(plan.action(3), Some(Action::EnableRail(Rail::Rfa13)));
        assert_eq!(
            verified_sunfish_plan(&VerifiedIcNssProfile::test_fixture(
                1,
                Config::new(0, false)
            ))
            .err(),
            Some(Error::EmptyPlan)
        );
        assert_eq!(
            verified_sunfish_plan(&VerifiedIcNssProfile::test_fixture(
                1,
                Config::new(0x80, false)
            ))
            .err(),
            Some(Error::InvalidRailMask)
        );
    }

    #[test]
    fn start_is_idempotent_and_stop_restores_prior_votes_in_reverse_order() {
        let mut io = FakeIo::new();
        io.values = [5, 11, 17, 23, 29, 31];
        let original = io.values;
        let mut sequencer = Sequencer::default();
        let plan = full_plan(7, true);
        assert_eq!(
            sequencer.start(&mut io, 7, plan),
            Ok(StartDisposition::Started)
        );
        assert_eq!(sequencer.phase(), Phase::On);
        let after_start = io.values;
        assert_eq!(
            sequencer.start(&mut io, 7, plan),
            Ok(StartDisposition::AlreadyOn)
        );
        assert_eq!(io.values, after_start);
        assert_eq!(sequencer.stop(&mut io, 7), Ok(()));
        assert_eq!(sequencer.phase(), Phase::Off);
        assert_eq!(io.values, original);
        assert_eq!(
            &io.restores[..io.restore_count],
            &[
                Some(Resource::CxoRefClock),
                Some(Resource::Rail(Rail::Ch0_33)),
                Some(Resource::Rail(Rail::Ch1_33)),
                Some(Resource::Rail(Rail::Rfa13)),
                Some(Resource::Rail(Rail::Xo18)),
                Some(Resource::Rail(Rail::CxMx)),
            ],
        );
    }

    #[test]
    fn lifecycle_permits_exist_only_while_power_is_in_the_matching_generation() {
        let mut io = FakeIo::new();
        let mut sequencer = Sequencer::default();
        assert_eq!(sequencer.power_ready_permit(7), Err(Error::StateConflict));
        assert_eq!(
            sequencer.start(&mut io, 7, full_plan(7, true)),
            Ok(StartDisposition::Started)
        );
        assert_eq!(sequencer.power_ready_permit(8), Err(Error::StateConflict));
        let ready = sequencer.power_ready_permit(7).unwrap();
        assert_eq!(ready.generation(), 7);

        let stopped = sequencer.stop_with_permit(&mut io, 7).unwrap();
        assert_eq!(stopped.generation(), 7);
        assert_eq!(sequencer.phase(), Phase::Off);
        assert_eq!(
            sequencer.stop_with_permit(&mut io, 7),
            Err(Error::StateConflict)
        );
        assert_eq!(sequencer.power_ready_permit(7), Err(Error::StateConflict));
    }

    #[test]
    fn every_action_failure_restores_touched_prefix_in_reverse_order() {
        for fail_at in 0..11 {
            let mut io = FakeIo::new();
            io.values = [2, 4, 6, 8, 10, 12];
            let original = io.values;
            io.fail_apply = Some(fail_at);
            let mut sequencer = Sequencer::default();
            let error = sequencer.start(&mut io, 9, full_plan(9, true)).unwrap_err();
            assert!(matches!(error, Error::Apply { action_index, .. } if action_index == fail_at));
            assert_eq!(sequencer.phase(), Phase::Off);
            assert_eq!(io.values, original);
            let expected_count = if fail_at < 2 {
                1
            } else if fail_at < 4 {
                2
            } else if fail_at < 6 {
                3
            } else if fail_at < 8 {
                4
            } else if fail_at < 10 {
                5
            } else {
                6
            };
            assert_eq!(io.restore_count, expected_count);
        }
    }

    #[test]
    fn rollback_failure_latches_unknown_and_blocks_reentry() {
        let mut io = FakeIo::new();
        io.fail_apply = Some(3);
        io.fail_restore = Some(Resource::Rail(Rail::Xo18));
        let mut sequencer = Sequencer::default();
        assert!(matches!(
            sequencer.start(&mut io, 5, full_plan(5, false)),
            Err(Error::Rollback {
                resource: Resource::Rail(Rail::Xo18),
                ..
            })
        ));
        assert_eq!(sequencer.phase(), Phase::Unknown);
        let apply_count = io.apply_count;
        assert_eq!(
            sequencer.start(&mut io, 5, full_plan(5, false)),
            Err(Error::LockedOut)
        );
        assert_eq!(io.apply_count, apply_count);
        assert_eq!(sequencer.stop(&mut io, 5), Err(Error::LockedOut));
    }

    #[test]
    fn snapshot_failure_performs_no_writes() {
        let mut io = FakeIo::new();
        io.fail_snapshot = Some(Resource::Rail(Rail::Rfa13));
        let mut sequencer = Sequencer::default();
        assert_eq!(
            sequencer.start(&mut io, 4, full_plan(4, false)),
            Err(Error::Snapshot {
                resource: Resource::Rail(Rail::Rfa13),
                cause: IoError::Rejected
            })
        );
        assert_eq!(io.apply_count, 0);
        assert_eq!(io.restore_count, 0);
        assert_eq!(sequencer.phase(), Phase::Off);
    }

    #[test]
    fn stale_generation_quarantines_powered_state() {
        let mut io = FakeIo::new();
        let mut sequencer = Sequencer::default();
        assert_eq!(
            sequencer.start(&mut io, 3, full_plan(2, false)),
            Err(Error::ProfileGenerationMismatch)
        );
        assert_eq!(io.apply_count, 0);
        assert_eq!(sequencer.phase(), Phase::Off);
        sequencer.start(&mut io, 3, full_plan(3, false)).unwrap();
        assert_eq!(sequencer.stop(&mut io, 4), Err(Error::StaleGeneration));
        assert_eq!(sequencer.phase(), Phase::Unknown);
    }

    #[test]
    fn stop_lease_loss_quarantines_without_guessing_a_restore() {
        let mut io = FakeIo::new();
        let mut sequencer = Sequencer::default();
        sequencer.start(&mut io, 6, full_plan(6, false)).unwrap();
        io.fail_acquire = true;
        assert_eq!(
            sequencer.stop(&mut io, 6),
            Err(Error::RestoreLease {
                cause: IoError::Rejected
            })
        );
        assert_eq!(sequencer.phase(), Phase::Unknown);
        assert_eq!(io.restore_count, 0);
    }
}
