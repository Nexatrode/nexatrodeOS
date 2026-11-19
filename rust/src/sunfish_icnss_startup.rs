//! Source-ordered first ICNSS startup after WLFW QRTR discovery.
//!
//! Linux's pinned Sunfish ICNSS path powers the WLAN resources before it
//! registers WLFW indications. This coordinator keeps that order in the
//! bounded client model and rolls back power and endpoint state if the first
//! QMI request cannot be prepared. It has no platform I/O of its own. The
//! profile still cannot be minted by production code, and no production
//! Sunfish power backend or hardware authority is provided here.
#![forbid(unsafe_code)]

use crate::qcom_qmi_router::Address;
use crate::qcom_qrtr_control::Server;
use crate::qcom_wlan_power::{
    self as power, Io as PowerIo, Phase as PowerPhase, PlanView, Sequencer, VerifiedIcNssProfile,
};
use crate::qcom_wlfw_client::{self as wlfw, Client, OutboundRequest};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Searching,
    RegistrationQueued,
    Quarantined,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Failure {
    Client(wlfw::Error),
    Power(power::Error),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidInitialState,
    Plan(power::Error),
    Failure(Failure),
    Cleanup { primary: Failure, cleanup: Failure },
    Quarantined,
}

pub struct Startup<S: Copy> {
    generation: u64,
    plan: PlanView,
    client: Client,
    sequencer: Sequencer<S>,
    phase: Phase,
}

impl<S: Copy> Startup<S> {
    /// Create a coordinator from the source-checked ICNSS profile.
    ///
    /// The WLFW client must be fresh. The profile generation is bound to the
    /// startup transaction; `Client` independently rejects a mismatched
    /// generation when a server event arrives.
    pub fn new(client: Client, profile: &VerifiedIcNssProfile) -> Result<Self, Error> {
        if client.state() != wlfw::State::Searching || client.endpoint().is_some() {
            return Err(Error::InvalidInitialState);
        }
        let plan = power::verified_sunfish_plan(profile).map_err(Error::Plan)?;
        Ok(Self {
            generation: profile.generation(),
            plan,
            client,
            sequencer: Sequencer::default(),
            phase: Phase::Searching,
        })
    }

    pub const fn phase(&self) -> Phase {
        self.phase
    }

    pub fn power_phase(&self) -> PowerPhase {
        self.sequencer.phase()
    }

    pub fn endpoint(&self) -> Option<Address> {
        self.client.endpoint()
    }

    /// Bind the first exact WLFW server, apply the ICNSS power plan, then
    /// prepare the native indication-registration request. Duplicate server
    /// announcements are idempotent and do not reapply power or enqueue QMI.
    pub fn observe_server<I: PowerIo<Snapshot = S>>(
        &mut self,
        io: &mut I,
        server: Server,
        output_capacity: usize,
    ) -> Result<Option<OutboundRequest>, Error> {
        if self.phase == Phase::Quarantined {
            return Err(Error::Quarantined);
        }

        let fresh = self
            .client
            .observe_server(self.generation, server)
            .map_err(|error| Error::Failure(Failure::Client(error)))?;
        if !fresh {
            return Ok(None);
        }

        if let Err(error) = self.sequencer.start(io, self.generation, self.plan) {
            let primary = Failure::Power(error);
            if self.sequencer.phase() == PowerPhase::Unknown {
                self.phase = Phase::Quarantined;
                return Err(Error::Failure(primary));
            }
            return Err(self.remove_endpoint_after_failure(server, primary));
        }

        match self
            .client
            .begin_icnss_indication_registration(self.generation, output_capacity)
        {
            Ok(request) => {
                self.phase = Phase::RegistrationQueued;
                Ok(Some(request))
            }
            Err(error) => {
                let primary = Failure::Client(error);
                if let Err(cleanup) = self.sequencer.stop(io, self.generation) {
                    self.phase = Phase::Quarantined;
                    return Err(Error::Cleanup {
                        primary,
                        cleanup: Failure::Power(cleanup),
                    });
                }
                Err(self.remove_endpoint_after_failure(server, primary))
            }
        }
    }

    /// Remove the active WLFW server only after restoring this client's
    /// captured power votes. A failed restore quarantines the coordinator and
    /// retains the endpoint for diagnosis; stale removals have no side effects.
    pub fn remove_server<I: PowerIo<Snapshot = S>>(
        &mut self,
        io: &mut I,
        server: Server,
    ) -> Result<bool, Error> {
        if self.phase == Phase::Quarantined {
            return Err(Error::Quarantined);
        }
        if self.client.endpoint() != Some(server.address)
            || server.service != crate::qcom_qrtr_control::WLFW_SERVICE_ID
            || server.instance != crate::qcom_qrtr_control::WLFW_VERSION_INSTANCE
        {
            return self
                .client
                .remove_server(self.generation, server)
                .map_err(|error| Error::Failure(Failure::Client(error)));
        }

        if self.phase == Phase::RegistrationQueued {
            if let Err(error) = self.sequencer.stop(io, self.generation) {
                self.phase = Phase::Quarantined;
                return Err(Error::Failure(Failure::Power(error)));
            }
        }

        match self.client.remove_server(self.generation, server) {
            Ok(removed) => {
                if removed {
                    self.phase = Phase::Searching;
                }
                Ok(removed)
            }
            Err(error) => {
                self.phase = Phase::Quarantined;
                Err(Error::Failure(Failure::Client(error)))
            }
        }
    }

    /// Access the WLFW exchange state after registration has been queued.
    /// Server deletion must still go through `remove_server` so power cleanup
    /// is ordered before the client endpoint is discarded.
    pub fn client_mut(&mut self) -> Result<&mut Client, Error> {
        if self.phase != Phase::RegistrationQueued {
            return Err(if self.phase == Phase::Quarantined {
                Error::Quarantined
            } else {
                Error::InvalidInitialState
            });
        }
        Ok(&mut self.client)
    }

    fn remove_endpoint_after_failure(&mut self, server: Server, primary: Failure) -> Error {
        match self.client.remove_server(self.generation, server) {
            Ok(true) => {
                self.phase = Phase::Searching;
                Error::Failure(primary)
            }
            Ok(false) => {
                self.phase = Phase::Quarantined;
                Error::Cleanup {
                    primary,
                    cleanup: Failure::Client(wlfw::Error::NoEndpoint),
                }
            }
            Err(cleanup) => {
                self.phase = Phase::Quarantined;
                Error::Cleanup {
                    primary,
                    cleanup: Failure::Client(cleanup),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qcom_qrtr_control::{WLFW_SERVICE_ID, WLFW_VERSION_INSTANCE};
    use crate::qcom_wlan_power::{
        ALL_RAILS_MASK, Action, Config, IoError, RESOURCE_COUNT, Resource, VerifiedIcNssProfile,
    };
    use crate::qcom_wlfw_client::MAX_DATAGRAM_BYTES;
    use crate::wlfw_protocol::WLFW_IND_REGISTER_MESSAGE_ID;

    const GENERATION: u64 = 7;
    const LOCAL: Address = Address {
        node: 1,
        port: 0x4100,
    };
    const SERVER: Server = Server {
        service: WLFW_SERVICE_ID,
        instance: WLFW_VERSION_INSTANCE,
        address: Address { node: 5, port: 700 },
    };

    #[derive(Default)]
    struct FakeIo {
        values: [u32; RESOURCE_COUNT],
        apply_count: usize,
        restore_count: usize,
        fail_apply: Option<usize>,
        fail_restore: Option<usize>,
    }

    impl FakeIo {
        fn index(resource: Resource) -> usize {
            match resource {
                Resource::Rail(crate::qcom_wlan_power::Rail::CxMx) => 0,
                Resource::Rail(crate::qcom_wlan_power::Rail::Xo18) => 1,
                Resource::Rail(crate::qcom_wlan_power::Rail::Rfa13) => 2,
                Resource::Rail(crate::qcom_wlan_power::Rail::Ch1_33) => 3,
                Resource::Rail(crate::qcom_wlan_power::Rail::Ch0_33) => 4,
                Resource::CxoRefClock => 5,
            }
        }
    }

    impl PowerIo for FakeIo {
        type Snapshot = u32;
        type Lease = ();

        fn acquire(&mut self, _generation: u64) -> Result<Self::Lease, IoError> {
            Ok(())
        }

        fn snapshot(
            &mut self,
            _lease: &Self::Lease,
            resource: Resource,
        ) -> Result<Self::Snapshot, IoError> {
            Ok(self.values[Self::index(resource)])
        }

        fn apply(&mut self, _lease: &Self::Lease, action: Action) -> Result<(), IoError> {
            let index = self.apply_count;
            self.apply_count += 1;
            self.values[Self::index(match action {
                Action::SetVoltage { rail, .. }
                | Action::SetLoad { rail, .. }
                | Action::EnableRail(rail)
                | Action::DelayMicroseconds { rail, .. } => Resource::Rail(rail),
                Action::EnableCxoRefClock => Resource::CxoRefClock,
            })] = (index + 1) as u32;
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
            let index = self.restore_count;
            self.restore_count += 1;
            if self.fail_restore == Some(index) {
                return Err(IoError::Uncertain);
            }
            self.values[Self::index(resource)] = snapshot;
            Ok(())
        }
    }

    fn startup() -> Startup<u32> {
        let client = Client::new(LOCAL, GENERATION).unwrap();
        let profile =
            VerifiedIcNssProfile::test_fixture(GENERATION, Config::new(ALL_RAILS_MASK, true));
        Startup::new(client, &profile).unwrap()
    }

    #[test]
    fn first_server_powers_before_queueing_native_registration() {
        let mut startup = startup();
        let mut io = FakeIo::default();
        let request = startup
            .observe_server(&mut io, SERVER, MAX_DATAGRAM_BYTES)
            .unwrap()
            .unwrap();
        assert_eq!(request.message_id, WLFW_IND_REGISTER_MESSAGE_ID);
        assert_eq!(request.destination, SERVER.address);
        assert_eq!(startup.phase(), Phase::RegistrationQueued);
        assert_eq!(startup.power_phase(), PowerPhase::On);
        assert_eq!(io.apply_count, 11);
    }

    #[test]
    fn duplicate_server_is_idempotent_after_registration_is_queued() {
        let mut startup = startup();
        let mut io = FakeIo::default();
        startup
            .observe_server(&mut io, SERVER, MAX_DATAGRAM_BYTES)
            .unwrap();
        let applied = io.apply_count;
        assert!(
            startup
                .observe_server(&mut io, SERVER, MAX_DATAGRAM_BYTES)
                .unwrap()
                .is_none()
        );
        assert_eq!(io.apply_count, applied);
    }

    #[test]
    fn invalid_server_is_rejected_before_power_changes() {
        let mut startup = startup();
        let mut io = FakeIo::default();
        let invalid = Server {
            service: 0x31,
            ..SERVER
        };
        assert_eq!(
            startup.observe_server(&mut io, invalid, MAX_DATAGRAM_BYTES),
            Err(Error::Failure(Failure::Client(wlfw::Error::InvalidServer)))
        );
        assert_eq!(io.apply_count, 0);
        assert_eq!(startup.power_phase(), PowerPhase::Off);
    }

    #[test]
    fn client_generation_mismatch_is_rejected_before_power_changes() {
        let client = Client::new(LOCAL, GENERATION + 1).unwrap();
        let profile =
            VerifiedIcNssProfile::test_fixture(GENERATION, Config::new(ALL_RAILS_MASK, true));
        let mut startup = Startup::<u32>::new(client, &profile).unwrap();
        let mut io = FakeIo::default();
        assert_eq!(
            startup.observe_server(&mut io, SERVER, MAX_DATAGRAM_BYTES),
            Err(Error::Failure(Failure::Client(
                wlfw::Error::StaleGeneration
            )))
        );
        assert_eq!(io.apply_count, 0);
        assert_eq!(startup.power_phase(), PowerPhase::Off);
    }

    #[test]
    fn registration_encoding_failure_restores_power_and_clears_endpoint() {
        let mut startup = startup();
        let mut io = FakeIo {
            values: [9, 8, 7, 6, 5, 4],
            ..FakeIo::default()
        };
        let original = io.values;
        assert_eq!(
            startup.observe_server(&mut io, SERVER, 0),
            Err(Error::Failure(Failure::Client(wlfw::Error::OutputTooSmall)))
        );
        assert_eq!(startup.phase(), Phase::Searching);
        assert_eq!(startup.power_phase(), PowerPhase::Off);
        assert_eq!(startup.endpoint(), None);
        assert_eq!(io.values, original);
    }

    #[test]
    fn power_failure_with_successful_rollback_clears_endpoint() {
        let mut startup = startup();
        let mut io = FakeIo {
            fail_apply: Some(0),
            ..FakeIo::default()
        };
        assert!(matches!(
            startup.observe_server(&mut io, SERVER, MAX_DATAGRAM_BYTES),
            Err(Error::Failure(Failure::Power(power::Error::Apply { .. })))
        ));
        assert_eq!(startup.phase(), Phase::Searching);
        assert_eq!(startup.power_phase(), PowerPhase::Off);
        assert_eq!(startup.endpoint(), None);
    }

    #[test]
    fn failed_power_rollback_quarantines_and_retains_endpoint() {
        let mut startup = startup();
        let mut io = FakeIo {
            fail_apply: Some(0),
            fail_restore: Some(0),
            ..FakeIo::default()
        };
        assert!(matches!(
            startup.observe_server(&mut io, SERVER, MAX_DATAGRAM_BYTES),
            Err(Error::Failure(Failure::Power(
                power::Error::Rollback { .. }
            )))
        ));
        assert_eq!(startup.phase(), Phase::Quarantined);
        assert_eq!(startup.power_phase(), PowerPhase::Unknown);
        assert_eq!(startup.endpoint(), Some(SERVER.address));
    }

    #[test]
    fn server_removal_restores_power_before_clearing_endpoint() {
        let mut startup = startup();
        let mut io = FakeIo {
            values: [9, 8, 7, 6, 5, 4],
            ..FakeIo::default()
        };
        let original = io.values;
        startup
            .observe_server(&mut io, SERVER, MAX_DATAGRAM_BYTES)
            .unwrap();
        assert!(startup.remove_server(&mut io, SERVER).unwrap());
        assert_eq!(startup.phase(), Phase::Searching);
        assert_eq!(startup.power_phase(), PowerPhase::Off);
        assert_eq!(startup.endpoint(), None);
        assert_eq!(io.values, original);
    }

    #[test]
    fn failed_server_removal_restore_quarantines_and_retains_endpoint() {
        let mut startup = startup();
        let mut io = FakeIo::default();
        startup
            .observe_server(&mut io, SERVER, MAX_DATAGRAM_BYTES)
            .unwrap();
        io.fail_restore = Some(0);
        assert!(matches!(
            startup.remove_server(&mut io, SERVER),
            Err(Error::Failure(Failure::Power(power::Error::Restore { .. })))
        ));
        assert_eq!(startup.phase(), Phase::Quarantined);
        assert_eq!(startup.power_phase(), PowerPhase::Unknown);
        assert_eq!(startup.endpoint(), Some(SERVER.address));
    }
}
