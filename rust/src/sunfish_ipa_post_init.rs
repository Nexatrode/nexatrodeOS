//! Fail-closed coordinator for the source-observed Sunfish IPA post-init path.
//!
//! LineageOS `ipa3_post_init()` succeeds only after register access and fixed
//! initialization, modem SRAM validation, GSI support-buffer allocation,
//! interrupt setup, GSI registration, and AP-pipe setup. This module enforces
//! that ordering for a trusted platform backend and mints generation-bound
//! readiness evidence only after every step and the runtime request snapshot
//! validate. On Sunfish IPA v4.2, the initial bus vote precedes register
//! mapping; the Linux clock handle is absent and uC clock notification is a
//! no-op. This module does not provide MMIO, bus votes, SMMU mappings, DMA,
//! IRQ ownership, PAS, or firmware.
#![forbid(unsafe_code)]

use crate::qcom_ipa::{self as ipa, InitRequestFields};
use crate::qcom_ipa_client::{self, InitPermit};
use crate::qcom_ipa_service::{self, ApReadyPermit, PostInitProof};
use crate::sunfish_ipa_bus_vote::{self, UseCase};
use crate::sunfish_ipa_profile::{self, RuntimeSnapshot};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stage {
    InitialBusVote,
    RegisterAccess,
    FixedRegisters,
    ModemSram,
    GsiSupportBuffers,
    Interrupts,
    GsiRegistration,
    ApPipes,
    RuntimeSnapshot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    Fresh,
    Running,
    Complete,
    Quarantined,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error<E> {
    InvalidGeneration,
    AlreadyStarted,
    BusProfile(sunfish_ipa_bus_vote::Error),
    Stage { stage: Stage, source: E },
    RuntimeSnapshot(ipa::Error),
}

/// Trusted platform implementation of each source-ordered post-init stage.
/// Each method must return `Ok(())` only after its real operation and required
/// readback/IRQ completion have succeeded for the runner's generation. An
/// implementation must retain acquired leases for the resulting IPA service,
/// including partial leases after an error, and must not retry uncertain work.
pub trait Backend {
    type Error;

    /// Submit the IPA contribution through the complete shared MSM-bus/RPMh
    /// owner. The supplied profile is the initial IPA v4.2 SVS2 usecase. The
    /// backend must preserve every other client's contribution.
    fn apply_initial_bus_vote(
        &mut self,
        generation: u64,
        use_case: &'static UseCase,
    ) -> Result<(), Self::Error>;
    fn establish_register_access(&mut self, generation: u64) -> Result<(), Self::Error>;
    fn initialize_fixed_registers(&mut self, generation: u64) -> Result<(), Self::Error>;
    fn validate_modem_sram_partition(&mut self, generation: u64) -> Result<(), Self::Error>;
    fn allocate_gsi_support_buffers(&mut self, generation: u64) -> Result<(), Self::Error>;
    fn initialize_interrupts(&mut self, generation: u64) -> Result<(), Self::Error>;
    fn register_gsi(&mut self, generation: u64) -> Result<(), Self::Error>;
    fn connect_ap_pipes(&mut self, generation: u64) -> Result<(), Self::Error>;
    fn capture_runtime_snapshot(&mut self, generation: u64)
    -> Result<RuntimeSnapshot, Self::Error>;
}

/// Successful post-init evidence and the validated request fields captured
/// from that same runtime generation.
pub struct Ready {
    proof: PostInitProof,
    request_fields: InitRequestFields,
}

impl Ready {
    pub const fn generation(&self) -> u64 {
        self.proof.generation()
    }

    pub const fn request_fields(&self) -> &InitRequestFields {
        &self.request_fields
    }

    /// Readiness required before publishing the AP-side IPA QMI service.
    pub fn ap_service_permit(&self) -> Result<ApReadyPermit, qcom_ipa_service::Error> {
        ApReadyPermit::after_post_init(&self.proof)
    }

    /// Runtime fields for the modem-side `INIT_MODEM_DRIVER` request.
    pub fn modem_client_permit(&self) -> Result<InitPermit, qcom_ipa_client::Error> {
        InitPermit::after_post_init(&self.proof, self.request_fields)
    }
}

/// One-shot runner. Any backend error or invalid snapshot quarantines the
/// generation; partially completed hardware setup must be recovered by the
/// platform's explicit shutdown/reboot path, never by replaying post-init.
pub struct Runner {
    generation: u64,
    state: State,
}

impl Runner {
    pub const fn new(generation: u64) -> Self {
        Self {
            generation,
            state: State::Fresh,
        }
    }

    pub const fn is_quarantined(&self) -> bool {
        matches!(self.state, State::Quarantined)
    }

    pub fn run<B: Backend>(&mut self, backend: &mut B) -> Result<Ready, Error<B::Error>> {
        if self.generation == 0 {
            self.state = State::Quarantined;
            return Err(Error::InvalidGeneration);
        }
        if self.state != State::Fresh {
            return Err(Error::AlreadyStarted);
        }
        self.state = State::Running;

        macro_rules! stage {
            ($name:ident, $stage:ident) => {
                if let Err(source) = backend.$name(self.generation) {
                    self.state = State::Quarantined;
                    return Err(Error::Stage {
                        stage: Stage::$stage,
                        source,
                    });
                }
            };
        }

        let initial_use_case = match sunfish_ipa_bus_vote::use_case_for_clock_rate(
            sunfish_ipa_bus_vote::SUNFISH_IPA_HW_V4_2,
            sunfish_ipa_bus_vote::boot_clock_rate_hz(),
            sunfish_ipa_bus_vote::USE_CASE_COUNT,
        ) {
            Ok(use_case) => use_case,
            Err(error) => {
                self.state = State::Quarantined;
                return Err(Error::BusProfile(error));
            }
        };
        if let Err(source) = backend.apply_initial_bus_vote(self.generation, initial_use_case) {
            self.state = State::Quarantined;
            return Err(Error::Stage {
                stage: Stage::InitialBusVote,
                source,
            });
        }
        stage!(establish_register_access, RegisterAccess);
        stage!(initialize_fixed_registers, FixedRegisters);
        stage!(validate_modem_sram_partition, ModemSram);
        stage!(allocate_gsi_support_buffers, GsiSupportBuffers);
        stage!(initialize_interrupts, Interrupts);
        stage!(register_gsi, GsiRegistration);
        stage!(connect_ap_pipes, ApPipes);

        let snapshot = match backend.capture_runtime_snapshot(self.generation) {
            Ok(snapshot) => snapshot,
            Err(source) => {
                self.state = State::Quarantined;
                return Err(Error::Stage {
                    stage: Stage::RuntimeSnapshot,
                    source,
                });
            }
        };
        let request_fields = match sunfish_ipa_profile::init_request_fields(snapshot) {
            Ok(fields) => fields,
            Err(error) => {
                self.state = State::Quarantined;
                return Err(Error::RuntimeSnapshot(error));
            }
        };

        self.state = State::Complete;
        Ok(Ready {
            proof: PostInitProof::from_coordinator(self.generation),
            request_fields,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sunfish_ipa_profile::{APPS_WAN_CONS_ENDPOINT, RuntimeSnapshot};

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct BackendError(Stage);

    struct FakeBackend {
        generation: u64,
        fail_at: Option<Stage>,
        seen: [Option<Stage>; 9],
        count: usize,
        snapshot: RuntimeSnapshot,
    }

    impl FakeBackend {
        fn new(generation: u64, fail_at: Option<Stage>, snapshot: RuntimeSnapshot) -> Self {
            Self {
                generation,
                fail_at,
                seen: [None; 9],
                count: 0,
                snapshot,
            }
        }

        fn hit(&mut self, generation: u64, stage: Stage) -> Result<(), BackendError> {
            if generation != self.generation {
                return Err(BackendError(stage));
            }
            self.seen[self.count] = Some(stage);
            self.count += 1;
            if self.fail_at == Some(stage) {
                Err(BackendError(stage))
            } else {
                Ok(())
            }
        }
    }

    impl Backend for FakeBackend {
        type Error = BackendError;

        fn apply_initial_bus_vote(
            &mut self,
            generation: u64,
            use_case: &'static UseCase,
        ) -> Result<(), Self::Error> {
            assert_eq!(use_case.index, 1);
            assert_eq!(use_case.name, "SVS2");
            self.hit(generation, Stage::InitialBusVote)
        }

        fn establish_register_access(&mut self, generation: u64) -> Result<(), Self::Error> {
            self.hit(generation, Stage::RegisterAccess)
        }

        fn initialize_fixed_registers(&mut self, generation: u64) -> Result<(), Self::Error> {
            self.hit(generation, Stage::FixedRegisters)
        }

        fn validate_modem_sram_partition(&mut self, generation: u64) -> Result<(), Self::Error> {
            self.hit(generation, Stage::ModemSram)
        }

        fn allocate_gsi_support_buffers(&mut self, generation: u64) -> Result<(), Self::Error> {
            self.hit(generation, Stage::GsiSupportBuffers)
        }

        fn initialize_interrupts(&mut self, generation: u64) -> Result<(), Self::Error> {
            self.hit(generation, Stage::Interrupts)
        }

        fn register_gsi(&mut self, generation: u64) -> Result<(), Self::Error> {
            self.hit(generation, Stage::GsiRegistration)
        }

        fn connect_ap_pipes(&mut self, generation: u64) -> Result<(), Self::Error> {
            self.hit(generation, Stage::ApPipes)
        }

        fn capture_runtime_snapshot(
            &mut self,
            generation: u64,
        ) -> Result<RuntimeSnapshot, Self::Error> {
            self.hit(generation, Stage::RuntimeSnapshot)?;
            Ok(self.snapshot)
        }
    }

    fn snapshot() -> RuntimeSnapshot {
        RuntimeSnapshot {
            shared_mem_baddr_units: 0x10,
            shared_mem_size_units: 0x400,
            apps_wan_cons_endpoint: Some(APPS_WAN_CONS_ENDPOINT),
            ipa_uc_loaded: false,
        }
    }

    const SOURCE_ORDER: [Option<Stage>; 9] = [
        Some(Stage::InitialBusVote),
        Some(Stage::RegisterAccess),
        Some(Stage::FixedRegisters),
        Some(Stage::ModemSram),
        Some(Stage::GsiSupportBuffers),
        Some(Stage::Interrupts),
        Some(Stage::GsiRegistration),
        Some(Stage::ApPipes),
        Some(Stage::RuntimeSnapshot),
    ];

    #[test]
    fn successful_run_follows_source_order_and_mints_both_permits() {
        let mut runner = Runner::new(7);
        let mut backend = FakeBackend::new(7, None, snapshot());
        let ready = runner.run(&mut backend).unwrap();
        assert_eq!(ready.generation(), 7);
        assert_eq!(backend.seen, SOURCE_ORDER);
        assert_eq!(backend.count, SOURCE_ORDER.len());
        assert!(ready.ap_service_permit().is_ok());
        assert!(ready.modem_client_permit().is_ok());
        assert!(matches!(
            runner.run(&mut backend),
            Err(Error::AlreadyStarted)
        ));
    }

    #[test]
    fn backend_failure_stops_at_that_stage_and_quarantines_generation() {
        let failing = Stage::GsiSupportBuffers;
        let mut runner = Runner::new(9);
        let mut backend = FakeBackend::new(9, Some(failing), snapshot());
        assert!(matches!(
            runner.run(&mut backend),
            Err(Error::Stage {
                stage,
                source: BackendError(source_stage),
            }) if stage == failing && source_stage == failing
        ));
        assert_eq!(backend.count, 5);
        assert_eq!(backend.seen[4], Some(failing));
        assert!(runner.is_quarantined());
        assert!(matches!(
            runner.run(&mut backend),
            Err(Error::AlreadyStarted)
        ));
        assert_eq!(backend.count, 5);
    }

    #[test]
    fn invalid_runtime_geometry_prevents_readiness_permits() {
        let invalid = RuntimeSnapshot {
            shared_mem_baddr_units: u32::MAX,
            ..snapshot()
        };
        let mut runner = Runner::new(11);
        let mut backend = FakeBackend::new(11, None, invalid);
        assert!(matches!(
            runner.run(&mut backend),
            Err(Error::RuntimeSnapshot(_))
        ));
        assert!(runner.is_quarantined());
        assert!(matches!(
            runner.run(&mut backend),
            Err(Error::AlreadyStarted)
        ));
    }

    #[test]
    fn zero_generation_is_rejected_before_backend_access() {
        let mut runner = Runner::new(0);
        let mut backend = FakeBackend::new(0, None, snapshot());
        assert!(matches!(
            runner.run(&mut backend),
            Err(Error::InvalidGeneration)
        ));
        assert_eq!(backend.count, 0);
        assert!(runner.is_quarantined());
    }

    #[test]
    fn generation_mismatch_is_rejected_at_the_first_backend_stage() {
        let mut runner = Runner::new(13);
        let mut backend = FakeBackend::new(12, None, snapshot());
        assert!(matches!(
            runner.run(&mut backend),
            Err(Error::Stage {
                stage: Stage::InitialBusVote,
                source: BackendError(Stage::InitialBusVote),
            })
        ));
        assert_eq!(backend.count, 0);
        assert!(runner.is_quarantined());
    }

    #[test]
    fn initial_bus_vote_failure_never_reaches_register_access() {
        let mut runner = Runner::new(19);
        let mut backend = FakeBackend::new(19, Some(Stage::InitialBusVote), snapshot());
        assert!(matches!(
            runner.run(&mut backend),
            Err(Error::Stage {
                stage: Stage::InitialBusVote,
                source: BackendError(Stage::InitialBusVote),
            })
        ));
        assert_eq!(backend.count, 1);
        assert!(!backend.seen.contains(&Some(Stage::RegisterAccess)));
        assert!(runner.is_quarantined());
    }
}
