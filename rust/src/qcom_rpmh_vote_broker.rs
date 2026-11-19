//! Transaction model for one shared RPMh VRM resource.
//!
//! One instance represents one command-database resource, such as a single
//! Sunfish rail. Its records are already-resolved requests from regulator
//! devices associated with that exact resource, not raw consumer
//! voltage/load/enables. It starts only from a complete, authenticated
//! per-resource vote ledger, computes separate active/sleep max aggregates,
//! and publishes the new ledger
//! only after a trusted transport accepts every changed phase. The sleep phase
//! is staged in owner state; the active phase must complete through TCS. Neither
//! acceptance nor completion claims that RPMh echoed register values. This
//! module has no MMIO, TCS, retry, guessed rollback, or production receipt/ledger
//! constructor. An ambiguous result latches the instance unknown.

use crate::qcom_rpmh_vote_aggregate::{
    Aggregate, ClientVote, Error as AggregateError, MAX_CLIENTS, REGISTER_COUNT, aggregate,
};
use crate::qcom_rpmh_vrm_command::{Batch, Error as CommandError, build_batch};

const EMPTY_VOTE: ClientVote = ClientVote {
    set_active: false,
    set_sleep: false,
    valid_mask: 0,
    registers: [0; REGISTER_COUNT],
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ClientRecord {
    id: u8,
    vote: ClientVote,
}

/// Proof that a complete per-resource regulator-device ledger and regulator
/// cache state were captured under one authenticated platform identity and generation.
/// Production construction remains absent until all current requesters and
/// the sleep/awake regulator mode can be enumerated.
pub struct CompleteVoteSet {
    resource_address: u32,
    identity: u64,
    generation: u64,
    records: [ClientRecord; MAX_CLIENTS],
    count: usize,
    sleep_request_sent: bool,
    use_awake_state: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Ready,
    Pending,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// RPMh regulator state. `Sleep` and `Wake` update deferred owner caches;
/// `Active` submits the active set through TCS and requires completion.
pub enum AggregatePhase {
    Active,
    Sleep,
    Wake,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UpdatePlan {
    resource_address: u32,
    identity: u64,
    generation: u64,
    ticket: u64,
    desired: Aggregate,
    active_command_mask: u8,
    sleep_command_mask: u8,
    wake_command_mask: u8,
    active_removed_mask: u8,
    sleep_removed_mask: u8,
    wake_removed_mask: u8,
    active_required: bool,
    sleep_required: bool,
    wake_required: bool,
}

/// One-shot evidence that the trusted transport accepted one exact plan phase.
/// Sleep and Wake acceptance mean the future sets were staged in owner state;
/// active acceptance means the submitted TCS work completed. None is a
/// hardware value echo. It is minted only after the transport returns success.
struct PhaseReceipt {
    plan: UpdatePlan,
    phase: AggregatePhase,
}

/// Trusted platform RPMh phase handler. The pinned Linux API caches SLEEP and
/// WAKE_ONLY requests and does not send them to the controller until
/// `rpmh_flush`; therefore Sleep and Wake must stage exact aggregates in the
/// owner's bounded state without claiming TCS completion. Active must submit
/// the exact aggregate and return only after the corresponding TCS transaction
/// completes. The owner must retain sleep/wake/active state for the later
/// system-wide flush path, never infer completion from register readback, and
/// never retry an ambiguous command. It must validate the live lease around
/// every physical access. An error may mean hardware changed, so the broker
/// quarantines its ledger. Only the trusted platform RPMh owner may implement
/// this contract after command-database mapping and exclusive RSC ownership
/// are proven.
pub trait ExactPhaseTransport<L> {
    type Error;

    fn accept_exact_phase(&mut self, lease: &L, batch: Batch) -> Result<(), Self::Error>;
}

#[derive(Debug, Eq, PartialEq)]
pub enum ExecuteError<E> {
    Plan(Error),
    Command(CommandError),
    Transport(E),
    Completion(Error),
}

impl UpdatePlan {
    pub const fn identity(self) -> u64 {
        self.identity
    }

    pub const fn resource_address(self) -> u32 {
        self.resource_address
    }

    pub const fn generation(self) -> u64 {
        self.generation
    }

    pub const fn ticket(self) -> u64 {
        self.ticket
    }

    pub const fn desired(self) -> Aggregate {
        self.desired
    }

    /// Register fields the source regulator would include in this phase.
    pub const fn command_mask(self, phase: AggregatePhase) -> u8 {
        match phase {
            AggregatePhase::Active => self.active_command_mask,
            AggregatePhase::Sleep => self.sleep_command_mask,
            AggregatePhase::Wake => self.wake_command_mask,
        }
    }

    pub const fn removed_mask(self, phase: AggregatePhase) -> u8 {
        match phase {
            AggregatePhase::Active => self.active_removed_mask,
            AggregatePhase::Sleep => self.sleep_removed_mask,
            AggregatePhase::Wake => self.wake_removed_mask,
        }
    }

    pub const fn active_required(self) -> bool {
        self.active_required
    }

    pub const fn sleep_required(self) -> bool {
        self.sleep_required
    }

    pub const fn wake_required(self) -> bool {
        self.wake_required
    }
}

impl PhaseReceipt {
    fn from_transport(plan: UpdatePlan, phase: AggregatePhase) -> Self {
        Self { plan, phase }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Commit {
    Waiting,
    Committed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidResource,
    InvalidIdentity,
    InvalidGeneration,
    TooManyClients,
    DuplicateClient,
    InvalidAggregate(AggregateError),
    NotReady,
    ClientMissing,
    TicketExhausted,
    StaleTicket,
    UnexpectedPhase,
}

#[derive(Clone, Copy)]
struct Pending {
    plan: UpdatePlan,
    records: [ClientRecord; MAX_CLIENTS],
    desired: Aggregate,
    active_required: bool,
    sleep_required: bool,
    wake_required: bool,
    active_accepted: bool,
    sleep_accepted: bool,
    wake_accepted: bool,
}

enum State {
    Ready,
    Pending(Pending),
    Unknown,
}

/// Per-command-database-resource ledger. Register indices follow
/// `qcom_rpmh_vote_aggregate`; client IDs identify already-resolved regulator
/// child contributions and are minted by the trusted owner boundary. Raw
/// Bluetooth/ICNSS consumer requests must be combined before entering here.
pub struct Broker {
    resource_address: u32,
    identity: u64,
    generation: u64,
    records: [ClientRecord; MAX_CLIENTS],
    count: usize,
    current: Aggregate,
    sleep_request_sent: bool,
    use_awake_state: bool,
    next_ticket: u64,
    state: State,
}

impl CompleteVoteSet {
    #[cfg(test)]
    pub(crate) fn test_fixture(
        resource_address: u32,
        identity: u64,
        generation: u64,
        clients: &[(u8, ClientVote)],
    ) -> Self {
        let mut set = Self::test_fixture_with_state(
            resource_address,
            identity,
            generation,
            clients,
            false,
            false,
        );
        if set.count <= MAX_CLIENTS {
            let current = aggregate(&votes(&set.records, set.count)).ok();
            set.sleep_request_sent = current.is_some_and(sleep_differs_from_active);
        }
        set
    }

    #[cfg(test)]
    pub(crate) fn test_fixture_with_state(
        resource_address: u32,
        identity: u64,
        generation: u64,
        clients: &[(u8, ClientVote)],
        sleep_request_sent: bool,
        use_awake_state: bool,
    ) -> Self {
        let mut records = [ClientRecord {
            id: 0,
            vote: EMPTY_VOTE,
        }; MAX_CLIENTS];
        for (index, (id, vote)) in clients.iter().copied().enumerate().take(MAX_CLIENTS) {
            records[index] = ClientRecord { id, vote };
        }
        Self {
            resource_address,
            identity,
            generation,
            records,
            count: clients.len(),
            sleep_request_sent,
            use_awake_state,
        }
    }
}

impl Broker {
    pub fn from_complete_set(set: CompleteVoteSet) -> Result<Self, Error> {
        if set.resource_address == 0 {
            return Err(Error::InvalidResource);
        }
        if set.identity == 0 {
            return Err(Error::InvalidIdentity);
        }
        if set.generation == 0 {
            return Err(Error::InvalidGeneration);
        }
        if set.count > MAX_CLIENTS {
            return Err(Error::TooManyClients);
        }
        if set.records[..set.count]
            .iter()
            .enumerate()
            .any(|(index, record)| {
                set.records[..index]
                    .iter()
                    .any(|other| other.id == record.id)
            })
        {
            return Err(Error::DuplicateClient);
        }
        let current =
            aggregate(&votes(&set.records, set.count)).map_err(Error::InvalidAggregate)?;
        Ok(Self {
            resource_address: set.resource_address,
            identity: set.identity,
            generation: set.generation,
            records: set.records,
            count: set.count,
            current,
            sleep_request_sent: set.sleep_request_sent,
            use_awake_state: set.use_awake_state,
            next_ticket: 1,
            state: State::Ready,
        })
    }

    pub const fn identity(&self) -> u64 {
        self.identity
    }

    pub const fn resource_address(&self) -> u32 {
        self.resource_address
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub const fn aggregate(&self) -> Aggregate {
        self.current
    }

    pub fn phase(&self) -> Phase {
        match self.state {
            State::Ready => Phase::Ready,
            State::Pending(_) => Phase::Pending,
            State::Unknown => Phase::Unknown,
        }
    }

    pub fn client_vote(&self, client_id: u8) -> Option<ClientVote> {
        self.records[..self.count]
            .iter()
            .find(|record| record.id == client_id)
            .map(|record| record.vote)
    }

    /// Stage one client's complete new vote. The public aggregate remains
    /// unchanged until every changed phase is accepted by the trusted owner.
    pub fn begin_update(&mut self, client_id: u8, vote: ClientVote) -> Result<UpdatePlan, Error> {
        if !matches!(self.state, State::Ready) {
            return Err(Error::NotReady);
        }
        let Some(index) = self.records[..self.count]
            .iter()
            .position(|record| record.id == client_id)
        else {
            return Err(Error::ClientMissing);
        };
        let mut updated = self.records;
        updated[index].vote = vote;
        let desired = aggregate(&votes(&updated, self.count)).map_err(Error::InvalidAggregate)?;
        let active_changed = self.current.active_valid_mask != desired.active_valid_mask
            || self.current.active_registers != desired.active_registers;
        let sleep_changed = self.current.sleep_valid_mask != desired.sleep_valid_mask
            || self.current.sleep_registers != desired.sleep_registers;
        let first_differing_sleep = !self.sleep_request_sent && sleep_differs_from_active(desired);
        let sleep_set_differs = self.sleep_request_sent || first_differing_sleep;
        let active_changed_mask = changed_register_mask(
            self.current.active_valid_mask,
            self.current.active_registers,
            desired.active_valid_mask,
            desired.active_registers,
        );
        let sleep_changed_mask = changed_register_mask(
            self.current.sleep_valid_mask,
            self.current.sleep_registers,
            desired.sleep_valid_mask,
            desired.sleep_registers,
        );
        let sleep_required = (first_differing_sleep && desired.sleep_valid_mask != 0)
            || (self.sleep_request_sent && sleep_changed);
        let resend_active =
            first_differing_sleep && !self.use_awake_state && desired.active_valid_mask != 0;
        let active_required = active_changed || resend_active;
        let wake_required = sleep_set_differs && !self.use_awake_state && active_required;
        let active_command_mask = if !active_required {
            0
        } else if resend_active {
            desired.active_valid_mask
        } else {
            active_changed_mask
        };
        let sleep_command_mask = if !sleep_required {
            0
        } else if !self.sleep_request_sent {
            desired.sleep_valid_mask
        } else {
            sleep_changed_mask
        };
        let wake_command_mask = if !wake_required {
            0
        } else if resend_active {
            desired.active_valid_mask
        } else {
            active_changed_mask
        };

        if !active_required && !sleep_required && !wake_required {
            self.records = updated;
            return Ok(UpdatePlan {
                resource_address: self.resource_address,
                identity: self.identity,
                generation: self.generation,
                ticket: 0,
                desired,
                active_command_mask: 0,
                sleep_command_mask: 0,
                wake_command_mask: 0,
                active_removed_mask: 0,
                sleep_removed_mask: 0,
                wake_removed_mask: 0,
                active_required: false,
                sleep_required: false,
                wake_required: false,
            });
        }

        let ticket = self.next_ticket;
        self.next_ticket = match self.next_ticket.checked_add(1) {
            Some(next) => next,
            None => {
                self.state = State::Unknown;
                return Err(Error::TicketExhausted);
            }
        };
        let pending = Pending {
            plan: UpdatePlan {
                resource_address: self.resource_address,
                identity: self.identity,
                generation: self.generation,
                ticket,
                desired,
                active_command_mask,
                sleep_command_mask,
                wake_command_mask,
                active_removed_mask: self.current.active_valid_mask & !desired.active_valid_mask,
                sleep_removed_mask: if self.sleep_request_sent {
                    self.current.sleep_valid_mask & !desired.sleep_valid_mask
                } else {
                    0
                },
                wake_removed_mask: if self.sleep_request_sent {
                    self.current.active_valid_mask & !desired.active_valid_mask
                } else {
                    0
                },
                active_required,
                sleep_required,
                wake_required,
            },
            records: updated,
            desired,
            active_required,
            sleep_required,
            wake_required,
            active_accepted: false,
            sleep_accepted: false,
            wake_accepted: false,
        };
        self.state = State::Pending(pending);
        Ok(pending.plan)
    }

    /// Consume one phase receipt after the transport accepted its exact state.
    /// Sleep/Wake acceptance is cache staging; active acceptance is TCS
    /// completion. A timeout or ambiguous status must quarantine the resource.
    fn accept_phase(&mut self, receipt: PhaseReceipt) -> Result<Commit, Error> {
        let State::Pending(mut pending) = self.state else {
            return Err(Error::NotReady);
        };
        if receipt.plan.ticket == 0 || receipt.plan != pending.plan {
            return Err(Error::StaleTicket);
        }
        let (required, seen) = match receipt.phase {
            AggregatePhase::Active => (pending.active_required, pending.active_accepted),
            AggregatePhase::Sleep => (pending.sleep_required, pending.sleep_accepted),
            AggregatePhase::Wake => (pending.wake_required, pending.wake_accepted),
        };
        if !required || seen {
            return Err(Error::UnexpectedPhase);
        }
        match receipt.phase {
            AggregatePhase::Active => pending.active_accepted = true,
            AggregatePhase::Sleep => {
                pending.sleep_accepted = true;
                self.sleep_request_sent = true;
            }
            AggregatePhase::Wake => pending.wake_accepted = true,
        }
        if (!pending.active_required || pending.active_accepted)
            && (!pending.sleep_required || pending.sleep_accepted)
            && (!pending.wake_required || pending.wake_accepted)
        {
            self.records = pending.records;
            self.current = pending.desired;
            self.state = State::Ready;
            Ok(Commit::Committed)
        } else {
            self.state = State::Pending(pending);
            Ok(Commit::Waiting)
        }
    }

    /// Accept every changed aggregate phase in source order. Sleep and Wake are
    /// staged first; active TCS completion is required before publishing the
    /// ledger. The broker mints a phase receipt only after transport success.
    /// If any phase fails, state is ambiguous and this resource is quarantined.
    pub fn execute<L, T: ExactPhaseTransport<L>>(
        &mut self,
        plan: UpdatePlan,
        lease: &L,
        transport: &mut T,
    ) -> Result<Commit, ExecuteError<T::Error>> {
        if plan.resource_address != self.resource_address
            || plan.identity != self.identity
            || plan.generation != self.generation
        {
            return Err(ExecuteError::Plan(Error::StaleTicket));
        }
        if plan.ticket == 0 {
            if plan.active_required || plan.sleep_required || plan.wake_required {
                return Err(ExecuteError::Plan(Error::StaleTicket));
            }
            return Ok(Commit::Committed);
        }
        match self.state {
            State::Pending(pending) if pending.plan == plan => {}
            State::Pending(_) => return Err(ExecuteError::Plan(Error::StaleTicket)),
            State::Ready | State::Unknown => return Err(ExecuteError::Plan(Error::NotReady)),
        }

        let phases = [
            AggregatePhase::Sleep,
            AggregatePhase::Wake,
            AggregatePhase::Active,
        ];
        let mut batches = [None; 3];
        for (index, phase) in phases.into_iter().enumerate() {
            let required = match phase {
                AggregatePhase::Sleep => plan.sleep_required,
                AggregatePhase::Wake => plan.wake_required,
                AggregatePhase::Active => plan.active_required,
            };
            if required {
                batches[index] = match build_batch(plan, phase) {
                    Ok(batch) => Some(batch),
                    Err(error) => {
                        let _ = self.cancel_pending_before_io(plan);
                        return Err(ExecuteError::Command(error));
                    }
                };
            }
        }

        let mut result = Commit::Waiting;
        // Match rpmh-regulator.c: stage changed sleep and wake sets before
        // submitting the active/awake set. A later failure quarantines state.
        for batch in batches.into_iter().flatten() {
            if let Err(error) = transport.accept_exact_phase(lease, batch) {
                let _ = self.fail_pending(plan);
                self.state = State::Unknown;
                return Err(ExecuteError::Transport(error));
            }
            result = match self.accept_phase(PhaseReceipt::from_transport(plan, batch.phase)) {
                Ok(commit) => commit,
                Err(error) => {
                    self.state = State::Unknown;
                    return Err(ExecuteError::Completion(error));
                }
            };
        }
        Ok(result)
    }

    /// Abandon a plan only when command validation failed before transport
    /// access. Once any phase was accepted, no rollback is inferred.
    fn cancel_pending_before_io(&mut self, plan: UpdatePlan) -> Result<(), Error> {
        match self.state {
            State::Pending(pending)
                if pending.plan == plan
                    && !pending.active_accepted
                    && !pending.sleep_accepted
                    && !pending.wake_accepted =>
            {
                self.state = State::Ready;
                Ok(())
            }
            State::Pending(_) => Err(Error::StaleTicket),
            State::Ready | State::Unknown => Err(Error::NotReady),
        }
    }

    /// A timeout, lost completion, or transport ambiguity has no safe rollback
    /// value. The owner must reconcile from a new authenticated boot snapshot.
    pub fn fail_pending(&mut self, plan: UpdatePlan) -> Result<(), Error> {
        match self.state {
            State::Pending(pending) if pending.plan == plan => {
                self.state = State::Unknown;
                Ok(())
            }
            State::Pending(_) => Err(Error::StaleTicket),
            State::Ready | State::Unknown => Err(Error::NotReady),
        }
    }
}

fn sleep_differs_from_active(aggregate: Aggregate) -> bool {
    (0..REGISTER_COUNT).any(|index| {
        aggregate.sleep_valid_mask & (1 << index) != 0
            && aggregate.active_registers[index] != aggregate.sleep_registers[index]
    })
}

fn changed_register_mask(
    previous_mask: u8,
    previous: [u32; REGISTER_COUNT],
    desired_mask: u8,
    desired: [u32; REGISTER_COUNT],
) -> u8 {
    (0..REGISTER_COUNT).fold(0, |mask, index| {
        let bit = 1 << index;
        if desired_mask & bit != 0
            && (previous_mask & bit == 0 || previous[index] != desired[index])
        {
            mask | bit
        } else {
            mask
        }
    })
}

fn votes(records: &[ClientRecord; MAX_CLIENTS], count: usize) -> [ClientVote; MAX_CLIENTS] {
    let mut result = [EMPTY_VOTE; MAX_CLIENTS];
    for (destination, record) in result.iter_mut().zip(records.iter()).take(count) {
        *destination = record.vote;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vote(active: bool, sleep: bool, mask: u8, registers: [u32; REGISTER_COUNT]) -> ClientVote {
        ClientVote {
            set_active: active,
            set_sleep: sleep,
            valid_mask: mask,
            registers,
        }
    }

    fn broker() -> Broker {
        Broker::from_complete_set(CompleteVoteSet::test_fixture(
            0x1001,
            0x51,
            9,
            &[
                (1, vote(true, true, 0b0011, [5, 1, 0, 0])),
                (2, vote(true, false, 0b0011, [7, 1, 0, 0])),
                (3, vote(false, true, 0b0011, [6, 1, 0, 0])),
            ],
        ))
        .unwrap()
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum TransportFault {
        Ambiguous,
    }

    struct FakeTransport {
        fail_phase: Option<AggregatePhase>,
        phases: [Option<AggregatePhase>; 3],
        count: usize,
        last_batch: Option<Batch>,
    }

    impl FakeTransport {
        fn ready() -> Self {
            Self {
                fail_phase: None,
                phases: [None; 3],
                count: 0,
                last_batch: None,
            }
        }
    }

    impl ExactPhaseTransport<u64> for FakeTransport {
        type Error = TransportFault;

        fn accept_exact_phase(&mut self, lease: &u64, batch: Batch) -> Result<(), Self::Error> {
            self.last_batch = Some(batch);
            self.phases[self.count] = Some(batch.phase);
            self.count += 1;
            if *lease != batch.generation || self.fail_phase == Some(batch.phase) {
                return Err(TransportFault::Ambiguous);
            }
            Ok(())
        }
    }

    #[test]
    fn complete_snapshot_aggregates_active_and_sleep_independently() {
        let broker = broker();
        assert_eq!(broker.resource_address(), 0x1001);
        assert_eq!(broker.identity(), 0x51);
        assert_eq!(broker.generation(), 9);
        assert_eq!(broker.phase(), Phase::Ready);
        assert_eq!(broker.aggregate().active_valid_mask, 0b0011);
        assert_eq!(broker.aggregate().active_registers, [7, 1, 0, 0]);
        assert_eq!(broker.aggregate().sleep_valid_mask, 0b0011);
        assert_eq!(broker.aggregate().sleep_registers, [6, 1, 0, 0]);
    }

    #[test]
    fn hidden_lower_vote_commits_without_hardware_changes() {
        let mut broker = broker();
        let previous = broker.aggregate();
        let plan = broker
            .begin_update(1, vote(true, true, 0b0011, [4, 1, 0, 0]))
            .unwrap();
        assert_eq!(plan.ticket(), 0);
        assert_eq!(plan.resource_address(), 0x1001);
        assert!(!plan.active_required());
        assert!(!plan.sleep_required());
        assert_eq!(broker.phase(), Phase::Ready);
        let mut transport = FakeTransport::ready();
        assert_eq!(
            broker.execute(plan, &9, &mut transport),
            Ok(Commit::Committed)
        );
        assert_eq!(transport.count, 0);
        assert_eq!(
            broker.client_vote(1),
            Some(vote(true, true, 0b0011, [4, 1, 0, 0]))
        );
        assert_eq!(broker.aggregate(), previous);
    }

    #[test]
    fn changed_active_vote_stages_wake_before_active_completion() {
        let mut broker = broker();
        let old = broker.aggregate();
        let plan = broker
            .begin_update(2, vote(true, false, 0b0011, [9, 2, 0, 0]))
            .unwrap();
        assert_eq!(plan.generation(), 9);
        assert!(plan.active_required());
        assert!(!plan.sleep_required());
        assert!(plan.wake_required());
        assert_eq!(broker.phase(), Phase::Pending);
        assert_eq!(broker.aggregate(), old);
        assert_eq!(
            broker.accept_phase(PhaseReceipt::from_transport(plan, AggregatePhase::Wake)),
            Ok(Commit::Waiting)
        );
        assert_eq!(
            broker.accept_phase(PhaseReceipt::from_transport(plan, AggregatePhase::Active)),
            Ok(Commit::Committed)
        );
        assert_eq!(broker.phase(), Phase::Ready);
        assert_eq!(broker.aggregate(), plan.desired());
    }

    #[test]
    fn sleep_wake_caches_and_active_completion_are_all_required() {
        let mut broker = broker();
        let plan = broker
            .begin_update(1, vote(true, true, 0b0011, [8, 2, 0, 0]))
            .unwrap();
        assert!(plan.active_required());
        assert!(plan.sleep_required());
        assert!(plan.wake_required());
        assert_eq!(
            broker.accept_phase(PhaseReceipt::from_transport(plan, AggregatePhase::Sleep)),
            Ok(Commit::Waiting)
        );
        assert_eq!(broker.phase(), Phase::Pending);
        assert_eq!(
            broker.accept_phase(PhaseReceipt::from_transport(plan, AggregatePhase::Wake)),
            Ok(Commit::Waiting)
        );
        assert_eq!(
            broker.accept_phase(PhaseReceipt::from_transport(plan, AggregatePhase::Active)),
            Ok(Commit::Committed)
        );
        assert_eq!(broker.aggregate(), plan.desired());
    }

    #[test]
    fn first_differing_sleep_set_stages_all_sets_and_resends_active() {
        let mut broker = Broker::from_complete_set(CompleteVoteSet::test_fixture_with_state(
            0x1001,
            0x51,
            9,
            &[
                (1, vote(true, false, 0b0011, [7, 1, 0, 0])),
                (2, vote(false, true, 0b0011, [6, 1, 0, 0])),
            ],
            false,
            false,
        ))
        .unwrap();
        let plan = broker
            .begin_update(1, vote(true, false, 0b0011, [9, 1, 0, 0]))
            .unwrap();

        assert!(plan.sleep_required());
        assert!(plan.wake_required());
        assert!(plan.active_required());
        let mut transport = FakeTransport::ready();
        assert_eq!(
            broker.execute(plan, &9, &mut transport),
            Ok(Commit::Committed)
        );
        assert_eq!(
            transport.phases,
            [
                Some(AggregatePhase::Sleep),
                Some(AggregatePhase::Wake),
                Some(AggregatePhase::Active)
            ]
        );
    }

    #[test]
    fn awake_state_skips_wake_cache_and_first_sleep_active_resend() {
        let mut broker = Broker::from_complete_set(CompleteVoteSet::test_fixture_with_state(
            0x1001,
            0x51,
            9,
            &[
                (1, vote(true, false, 0b0011, [7, 1, 0, 0])),
                (2, vote(false, true, 0b0011, [6, 1, 0, 0])),
            ],
            false,
            true,
        ))
        .unwrap();
        let plan = broker
            .begin_update(1, vote(true, false, 0b0011, [9, 1, 0, 0]))
            .unwrap();

        assert!(plan.sleep_required());
        assert!(!plan.wake_required());
        assert!(plan.active_required());
        let mut transport = FakeTransport::ready();
        assert_eq!(
            broker.execute(plan, &9, &mut transport),
            Ok(Commit::Committed)
        );
        assert_eq!(
            transport.phases,
            [
                Some(AggregatePhase::Sleep),
                Some(AggregatePhase::Active),
                None
            ]
        );
    }

    #[test]
    fn first_sleep_stage_does_not_treat_uncached_fields_as_removals() {
        let mut broker = Broker::from_complete_set(CompleteVoteSet::test_fixture_with_state(
            0x1001,
            0x51,
            9,
            &[
                (1, vote(true, true, 0b0011, [7, 1, 0, 0])),
                (2, vote(true, false, 0b0011, [9, 2, 0, 0])),
            ],
            false,
            false,
        ))
        .unwrap();
        let plan = broker
            .begin_update(1, vote(true, true, 0b0001, [8, 0, 0, 0]))
            .unwrap();

        assert!(plan.sleep_required());
        assert!(plan.wake_required());
        assert_eq!(plan.removed_mask(AggregatePhase::Sleep), 0);
        assert_eq!(plan.removed_mask(AggregatePhase::Wake), 0);
    }

    #[test]
    fn invalid_command_plan_is_cancelled_before_transport_access() {
        let initial = vote(true, true, 0b0011, [640, 1, 7, 55]);
        let mut broker = Broker::from_complete_set(CompleteVoteSet::test_fixture_with_state(
            0x1001,
            0x51,
            9,
            &[(1, initial)],
            true,
            false,
        ))
        .unwrap();
        let old = broker.aggregate();
        let plan = broker
            .begin_update(1, vote(true, true, 0b0001, [640, 0, 0, 0]))
            .unwrap();
        let mut transport = FakeTransport::ready();

        assert_eq!(
            broker.execute(plan, &9, &mut transport),
            Err(ExecuteError::Command(CommandError::VoteRemovalUnsupported))
        );
        assert_eq!(transport.count, 0);
        assert_eq!(broker.phase(), Phase::Ready);
        assert_eq!(broker.aggregate(), old);
        assert_eq!(broker.client_vote(1), Some(initial));
    }

    #[test]
    fn receipt_correlation_and_timeout_quarantine() {
        let mut broker = broker();
        let plan = broker
            .begin_update(2, vote(true, false, 0b0011, [9, 1, 0, 0]))
            .unwrap();
        assert_eq!(
            broker.accept_phase(PhaseReceipt::from_transport(
                UpdatePlan {
                    ticket: plan.ticket() + 1,
                    ..plan
                },
                AggregatePhase::Active,
            )),
            Err(Error::StaleTicket)
        );
        assert_eq!(
            broker.accept_phase(PhaseReceipt::from_transport(plan, AggregatePhase::Sleep)),
            Err(Error::UnexpectedPhase)
        );
        assert_eq!(broker.phase(), Phase::Pending);
        assert_eq!(broker.fail_pending(plan), Ok(()));
        assert_eq!(broker.phase(), Phase::Unknown);
        assert_eq!(broker.begin_update(1, EMPTY_VOTE), Err(Error::NotReady));
    }

    #[test]
    fn execute_commits_after_sleep_staging_and_active_completion() {
        let mut broker = broker();
        let plan = broker
            .begin_update(1, vote(true, true, 0b0011, [8, 2, 0, 0]))
            .unwrap();
        let mut transport = FakeTransport::ready();

        assert_eq!(
            broker.execute(plan, &9, &mut transport),
            Ok(Commit::Committed)
        );
        assert_eq!(
            transport.phases,
            [
                Some(AggregatePhase::Sleep),
                Some(AggregatePhase::Wake),
                Some(AggregatePhase::Active)
            ]
        );
        assert_eq!(transport.count, 3);
        assert_eq!(transport.last_batch.unwrap().ticket, plan.ticket());
        assert_eq!(broker.phase(), Phase::Ready);
        assert_eq!(broker.aggregate(), plan.desired());
    }

    #[test]
    fn active_failure_after_sleep_staging_quarantines_resource() {
        let mut broker = broker();
        let old = broker.aggregate();
        let plan = broker
            .begin_update(1, vote(true, true, 0b0011, [8, 2, 0, 0]))
            .unwrap();
        let mut transport = FakeTransport::ready();
        transport.fail_phase = Some(AggregatePhase::Active);

        assert_eq!(
            broker.execute(plan, &9, &mut transport),
            Err(ExecuteError::Transport(TransportFault::Ambiguous))
        );
        assert_eq!(
            transport.phases,
            [
                Some(AggregatePhase::Sleep),
                Some(AggregatePhase::Wake),
                Some(AggregatePhase::Active)
            ]
        );
        assert_eq!(broker.phase(), Phase::Unknown);
        assert_eq!(broker.aggregate(), old);
        assert_eq!(broker.begin_update(1, EMPTY_VOTE), Err(Error::NotReady));
    }

    #[test]
    fn wake_stage_failure_after_sleep_staging_quarantines_resource() {
        let mut broker = broker();
        let plan = broker
            .begin_update(1, vote(true, true, 0b0011, [8, 2, 0, 0]))
            .unwrap();
        let mut transport = FakeTransport::ready();
        transport.fail_phase = Some(AggregatePhase::Wake);

        assert_eq!(
            broker.execute(plan, &9, &mut transport),
            Err(ExecuteError::Transport(TransportFault::Ambiguous))
        );
        assert_eq!(
            transport.phases,
            [
                Some(AggregatePhase::Sleep),
                Some(AggregatePhase::Wake),
                None
            ]
        );
        assert_eq!(broker.phase(), Phase::Unknown);
        assert_eq!(broker.begin_update(1, EMPTY_VOTE), Err(Error::NotReady));
    }

    #[test]
    fn timeout_and_bad_initial_ledgers_fail_closed() {
        let mut broker = broker();
        let plan = broker
            .begin_update(2, vote(true, false, 0b0011, [8, 1, 0, 0]))
            .unwrap();
        assert_eq!(broker.fail_pending(plan), Ok(()));
        assert_eq!(broker.phase(), Phase::Unknown);

        let duplicate = CompleteVoteSet::test_fixture(1, 1, 1, &[(7, EMPTY_VOTE), (7, EMPTY_VOTE)]);
        assert_eq!(
            Broker::from_complete_set(duplicate).err(),
            Some(Error::DuplicateClient)
        );
        let invalid =
            CompleteVoteSet::test_fixture(1, 1, 1, &[(7, vote(false, false, 0x80, [0; 4]))]);
        assert!(matches!(
            Broker::from_complete_set(invalid),
            Err(Error::InvalidAggregate(AggregateError::InvalidRegisterMask))
        ));
    }
}
