//! Native Microkit owner integration. Compiled only for the explicit owner mode.
//! The default runtime has no host-service handlers. `attach_with_handlers`
//! can retain and dispatch a caller-supplied backend, but this module does not
//! construct a production storage, SCM-ownership, UFS, or DMA provider.
#[cfg(feature = "sunfish-modem-servreg-state-probe")]
use crate::pixel_probe_display::Display;
use crate::{
    pixel_glink_channel::ChannelCheckpoint,
    pixel_glink_inventory as inventory,
    pixel_glink_prepare::{DESCRIPTOR, PART, TX},
    pixel_modem_owner_gate::LiveGate,
    pixel_modem_status::{Window, sync},
    pixel_smp2p_handshake::Handshake,
};
#[cfg(not(feature = "sunfish-modem-servreg-state-probe"))]
use personal_os_rust_server::qcom_modem_handlers::{Backend as HandlerBackend, Handlers};
#[cfg(feature = "sunfish-modem-servreg-state-probe")]
use personal_os_rust_server::qcom_wlfw_client::RegistrationOutcome as WlfwRegistrationOutcome;
#[cfg(feature = "sunfish-modem-servreg-state-probe")]
use personal_os_rust_server::{
    qcom_glink_fifo as fifo, qcom_glink_qmi as glink, qcom_qrtr_control as qrtr,
};
use personal_os_rust_server::{
    qcom_glink_fifo::Adapter,
    qcom_glink_native_io::{Gate, Mappings, NativeIo},
    qcom_glink_qmi::{Config, Session},
    qcom_ipa_service::{
        ApReadyPermit, PreparedIndication, PreparedResponse, Server as IpaServiceServer,
    },
    qcom_modem_handlers::Fault,
    qcom_modem_owner::{self as core, Error, Interrupts, Layout, Owner, Prelude},
    qcom_modem_pools::{ARG_BYTES, Call},
    qcom_modem_services::Service,
    qcom_modem_status::{self as smem, Read},
    qcom_msa::{HandoffPlan, MsaReadyPermit, Reservation as MsaReservation},
    qcom_msa_handoff::Handoff,
    qcom_msa_native::{self, Lifecycle as MsaLifecycle, Scm as MsaScm, Scratch as MsaScratch},
    qcom_qmi_router::Address,
    qcom_wlfw_client::State as WlfwState,
};
use sel4_microkit::{Channel, ChannelSet, debug_println};

pub type NativeOwner = Owner<NativeIo<LiveGate>>;

/// Service dispatch mode retained with the native owner. The normal diagnostic
/// runtime keeps host services disabled; a production caller can attach
/// validated handlers and activate them only after their backend is ready.
pub trait RuntimeServices {
    /// Revalidate that storage and heap handlers already hold the platform
    /// resources required to answer boot-time requests before MPSS starts.
    /// This is separate from `memory_services_ready`, which records that the
    /// handlers were activated and advertised by the attached runtime.
    fn preboot_memory_services_ready(&mut self) -> bool {
        false
    }
    /// Revalidate that an authorized WLAN image source is retained and
    /// available for the pre-MPSS QRTR TFTP server. This is separate from
    /// memory-service readiness and defaults off for observer-only runtimes.
    fn preboot_tftp_service_ready(&mut self) -> bool {
        false
    }
    /// True only after the remote-storage and heap-memory handlers have
    /// validated and been advertised. This does not prove IPA, SERVREG mapper,
    /// or TFTP readiness, which remain separate pre-boot requirements.
    fn memory_services_ready(&self) -> bool {
        false
    }
    /// True only after the source-matched IPA `INIT_MODEM_DRIVER` exchange
    /// succeeded and, when requested by the modem, the separate completion
    /// request was committed by the AP IPA server.
    fn ipa_modem_ready(&self) -> bool {
        false
    }
    /// True only after the modem's separate IPA
    /// `INIT_MODEM_DRIVER_CMPLT` request has been committed by the AP IPA
    /// server. This remains false by default so a pending handshake cannot
    /// release WLAN MSA progression.
    fn ipa_modem_driver_complete(&self) -> bool {
        false
    }
    /// Reserve transport clients and service endpoints before QRTR control
    /// starts. Services that need no client endpoint may keep the default.
    fn prepare_transport<I: Interrupts>(
        &mut self,
        _owner: &mut NativeOwner,
        _interrupts: &mut I,
    ) -> Result<(), Error> {
        Ok(())
    }
    fn activate<I: Interrupts>(
        &mut self,
        owner: &mut NativeOwner,
        interrupts: &mut I,
    ) -> Result<(), Error>;
    fn notified<I: Interrupts>(
        &mut self,
        owner: &mut NativeOwner,
        channels: u64,
        interrupts: &mut I,
    ) -> Result<core::Progress, Error>;
    /// Advance source-ordered WLAN power lifecycle work after transport has
    /// consumed QRTR discovery events and before its queued WLFW work resumes.
    /// The default intentionally does nothing, leaving a power-gated client
    /// unable to register until a trusted platform backend is attached.
    fn wlan_startup_step<I: Interrupts>(
        &mut self,
        _owner: &mut NativeOwner,
        _interrupts: &mut I,
    ) -> Result<(), Error> {
        Ok(())
    }
}

/// A bounded local QRTR session with the AP IPA server and SERVREG mapper
/// endpoints registered and ready before control starts. It performs no I/O;
/// the GLINK adapter takes ownership only after its channel is negotiated.
pub struct PreparedPrebootSession {
    session: Session,
    servreg_staged: bool,
    ipa_staged: bool,
    storage_staged: bool,
    heap_staged: bool,
    memory_staged: bool,
    tftp_staged: bool,
}

/// Couples the normal IPA runtime service state with the session prepared for
/// it before MPSS starts. Only `attach_with_prepared_services` consumes this
/// value, preventing accidental attachment through the post-boot path.
pub struct PreparedIpaServerServices<S: RuntimeServices> {
    services: IpaServerServices<S>,
    session: PreparedPrebootSession,
}

impl<S: RuntimeServices> PreparedIpaServerServices<S> {
    pub fn generation(&self) -> u64 {
        self.session.session.generation()
    }

    fn validate(&mut self, generation: u64) -> Result<(), Error> {
        self.session.validate(generation)?;
        if self.session.memory_staged && !self.services.preboot_memory_services_ready() {
            return Err(Error::MemoryServicesNotReady);
        }
        if self.session.tftp_staged && !self.services.preboot_tftp_service_ready() {
            return Err(Error::TftpImageNotReady);
        }
        Ok(())
    }
}

/// Typed handoff between trusted IPA post-init, pre-MPSS service staging, and
/// the later native-owner attach after the actual GLINK channel checkpoint.
/// The caller carries this value across modem startup; it is not itself proof
/// that MPSS started or that WLAN DMA ownership is safe.
pub struct PreparedModemBoot<S: RuntimeServices> {
    prepared: PreparedIpaServerServices<S>,
    generation: u64,
}

impl<S: RuntimeServices> PreparedModemBoot<S> {
    fn new(mut prepared: PreparedIpaServerServices<S>) -> Result<Self, Error> {
        let generation = prepared.generation();
        if generation == 0 {
            return Err(Error::GenerationMismatch);
        }
        prepared.validate(generation)?;
        Ok(Self {
            prepared,
            generation,
        })
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Create the SMP2P handshake bound to this prepared boot's generation.
    ///
    /// # Safety
    /// `page` and `doorbell` must satisfy the exclusive mapping contract of
    /// [`Handshake::new_for_generation`] and remain valid through handshake
    /// completion.
    pub unsafe fn new_smp2p_handshake(
        &self,
        page: *mut u8,
        doorbell: *mut u32,
    ) -> Result<Handshake, Error> {
        // SAFETY: the caller retains and exclusively owns these mappings.
        unsafe { Handshake::new_for_generation(page, doorbell, self.generation) }
            .map_err(|_| Error::GenerationMismatch)
    }

    /// Consume the pre-MPSS preparation only after the matching SMP2P/GLINK
    /// checkpoint has been completed for this generation.
    ///
    /// # Safety
    /// The mapping, channel, MSA, and retained-runtime requirements documented
    /// by [`attach_with_prepared_services`] still apply.
    pub unsafe fn attach_after_glink(
        self,
        window: Window,
        checkpoint: ChannelCheckpoint,
        smem_base: *const u8,
        ap: *mut u8,
        doorbell: *mut u32,
        display_pixels: *mut u32,
        msa_scm: Option<ScmResources>,
        progress: impl FnMut(&'static str),
    ) -> Result<Runtime<IpaServerServices<S>>, Error> {
        if checkpoint.generation() != self.generation {
            return Err(Error::GenerationMismatch);
        }
        // SAFETY: the caller supplies the same mappings and lifetime contract
        // required by the lower-level attach function.
        unsafe {
            attach_with_prepared_services(
                window,
                checkpoint,
                smem_base,
                ap,
                doorbell,
                display_pixels,
                msa_scm,
                self.prepared,
                progress,
            )
        }
    }
}

impl<S: RuntimeServices> PreparedModemBoot<IpaModemInitServices<S>> {
    /// Stage IPA and SERVREG before MPSS from one completed, trusted IPA
    /// post-init result.
    pub fn after_ipa_post_init(
        inner: S,
        readiness: personal_os_rust_server::sunfish_ipa_post_init::Ready,
    ) -> Result<Self, Error> {
        Self::new(IpaServerServices::prepare_from_post_init(inner, readiness)?)
    }

    /// Variant for a retained, validated storage/heap backend.
    pub fn after_ipa_post_init_with_memory(
        inner: S,
        readiness: personal_os_rust_server::sunfish_ipa_post_init::Ready,
    ) -> Result<Self, Error> {
        Self::new(IpaServerServices::prepare_from_post_init_with_memory(
            inner, readiness,
        )?)
    }

    /// Variant for retained storage/heap services and an authorized WLAN image
    /// source staged before MPSS.
    pub fn after_ipa_post_init_with_wlan_services(
        inner: S,
        readiness: personal_os_rust_server::sunfish_ipa_post_init::Ready,
    ) -> Result<Self, Error> {
        Self::new(IpaServerServices::prepare_from_post_init_with_wlan_services(inner, readiness)?)
    }
}

impl PreparedPrebootSession {
    fn new(generation: u64, memory_staged: bool, tftp_staged: bool) -> Result<Self, Error> {
        let mut session = Session::new(
            Config {
                local_node: 1,
                peer_node: 0,
                local_channel: 1,
                remote_channel: 2,
            },
            generation,
        )
        .map_err(|error| Error::Transport(error.into()))?;
        // Reserve the host storage and heap endpoints in the same pristine
        // router before MPSS starts, but do not advertise them yet. Their
        // handlers become ready only when the retained backend activates.
        session
            .register(generation, Service::RemoteStorage, 0x4000)
            .map_err(|_| Error::Mapping)?;
        session
            .register(generation, Service::HeapMemory, 0x4001)
            .map_err(|_| Error::Mapping)?;
        session
            .register(
                generation,
                Service::ServiceRegistry,
                personal_os_rust_server::qcom_servreg::LOCAL_PORT,
            )
            .map_err(|_| Error::Mapping)?;
        session
            .prepare_service_available(generation, Service::ServiceRegistry)
            .map_err(|_| Error::Mapping)?;
        session
            .register(generation, Service::IpaServer, IPA_SERVER_PORT)
            .map_err(|_| Error::Mapping)?;
        session
            .prepare_service_available(generation, Service::IpaServer)
            .map_err(|_| Error::Mapping)?;
        if memory_staged {
            session
                .prepare_service_available(generation, Service::RemoteStorage)
                .map_err(|_| Error::Mapping)?;
            session
                .prepare_service_available(generation, Service::HeapMemory)
                .map_err(|_| Error::Mapping)?;
        }
        if tftp_staged {
            session
                .prepare_tftp_server(generation, TFTP_SERVER_PORT)
                .map_err(|_| Error::Mapping)?;
        }
        Ok(Self {
            session,
            servreg_staged: true,
            ipa_staged: true,
            storage_staged: true,
            heap_staged: true,
            memory_staged,
            tftp_staged,
        })
    }

    fn validate(&self, generation: u64) -> Result<(), Error> {
        let expected_ready = (1 << 2)
            | (1 << 4)
            | if self.memory_staged { 0b11 } else { 0 }
            | if self.tftp_staged { 1 << 3 } else { 0 };
        let control = self.session.control_status();
        let storage = self.session.endpoint(Service::RemoteStorage);
        let heap = self.session.endpoint(Service::HeapMemory);
        let servreg = self.session.endpoint(Service::ServiceRegistry);
        let ipa = self.session.endpoint(Service::IpaServer);
        if self.session.generation() != generation
            || !self.session.transport_is_pristine()
            || control.started
            || control.ready_services != expected_ready
            || !self.servreg_staged
            || !self.ipa_staged
            || !self.storage_staged
            || !self.heap_staged
            || !storage.is_some_and(|endpoint| {
                endpoint.address.node == 1 && endpoint.address.port == 0x4000
            })
            || !heap.is_some_and(|endpoint| {
                endpoint.address.node == 1 && endpoint.address.port == 0x4001
            })
            || !servreg.is_some_and(|endpoint| {
                endpoint.address.node == 1
                    && endpoint.address.port == personal_os_rust_server::qcom_servreg::LOCAL_PORT
            })
            || !ipa.is_some_and(|endpoint| {
                endpoint.address.node == 1 && endpoint.address.port == IPA_SERVER_PORT
            })
            || (self.tftp_staged
                && !self
                    .session
                    .tftp_server_prepared(generation, TFTP_SERVER_PORT))
        {
            return Err(Error::Mapping);
        }
        Ok(())
    }
}

pub struct NoServices;
impl RuntimeServices for NoServices {
    fn memory_services_ready(&self) -> bool {
        false
    }

    fn activate<I: Interrupts>(
        &mut self,
        _owner: &mut NativeOwner,
        _interrupts: &mut I,
    ) -> Result<(), Error> {
        Ok(())
    }
    fn notified<I: Interrupts>(
        &mut self,
        owner: &mut NativeOwner,
        channels: u64,
        interrupts: &mut I,
    ) -> Result<core::Progress, Error> {
        owner.notified(channels, interrupts)
    }
}

/// Trusted platform backend for the source-ordered ICNSS vote transaction.
/// Implementations must use the RPMh broker to snapshot this client's exact
/// prior votes, retain a live exclusive lifecycle lease, and roll back or
/// quarantine uncertain state. `ensure_powered` must be idempotent for the
/// generation and return a permit only while all required votes are active.
pub trait IcnssPowerBackend {
    fn ensure_powered(
        &mut self,
        generation: u64,
    ) -> Result<personal_os_rust_server::qcom_wlan_power::PowerReadyPermit, Error>;
    fn stop_power(&mut self, generation: u64) -> Result<(), Error>;
}

/// ICNSS runtime backend composed from the shared per-resource RPMh brokers.
/// This connects the source-ordered rail sequencer to the already bounded
/// VRM/TCS implementation. Construction still requires an opaque verified
/// profile, complete rail ledgers, a live exclusive lease gate, and a trusted
/// exact-phase transport; this type creates none of those authorities.
pub struct BrokeredIcnssPowerBackend<'a, G, T>
where
    G: personal_os_rust_server::qcom_wlan_rpmh_io::LeaseGate,
    T: personal_os_rust_server::qcom_rpmh_vote_broker::ExactPhaseTransport<G::Lease>,
{
    generation: u64,
    plan: personal_os_rust_server::qcom_wlan_power::PlanView,
    sequencer: personal_os_rust_server::qcom_wlan_power::Sequencer<
        personal_os_rust_server::qcom_rpmh_vote_aggregate::ClientVote,
    >,
    io: personal_os_rust_server::qcom_wlan_rpmh_io::BrokeredWlanIo<'a, G, T>,
}

impl<'a, G, T> BrokeredIcnssPowerBackend<'a, G, T>
where
    G: personal_os_rust_server::qcom_wlan_rpmh_io::LeaseGate,
    T: personal_os_rust_server::qcom_rpmh_vote_broker::ExactPhaseTransport<G::Lease>,
{
    /// Resolve the source-derived rail map and bind it to exact broker
    /// identities/generations. The profile constructor is intentionally kept
    /// behind the trusted Sunfish platform-verification boundary.
    pub fn new(
        profile: &personal_os_rust_server::qcom_wlan_power::VerifiedIcNssProfile,
        command_db: &personal_os_rust_server::qcom_cmd_db::Database<'_>,
        client_id: u8,
        brokers: [Option<&'a mut personal_os_rust_server::qcom_rpmh_vote_broker::Broker>;
            personal_os_rust_server::qcom_wlan_power::RAIL_COUNT],
        gate: G,
        transport: &'a mut T,
    ) -> Result<Self, personal_os_rust_server::qcom_wlan_rpmh_io::Error> {
        let plan = personal_os_rust_server::qcom_wlan_power::verified_sunfish_plan(profile)
            .map_err(|_| personal_os_rust_server::qcom_wlan_rpmh_io::Error::InvalidProfile)?;
        let io = personal_os_rust_server::qcom_wlan_rpmh_io::BrokeredWlanIo::new(
            profile, command_db, client_id, brokers, gate, transport,
        )?;
        Ok(Self {
            generation: plan.generation(),
            plan,
            sequencer: personal_os_rust_server::qcom_wlan_power::Sequencer::default(),
            io,
        })
    }
}

impl<G, T> IcnssPowerBackend for BrokeredIcnssPowerBackend<'_, G, T>
where
    G: personal_os_rust_server::qcom_wlan_rpmh_io::LeaseGate,
    T: personal_os_rust_server::qcom_rpmh_vote_broker::ExactPhaseTransport<G::Lease>,
{
    fn ensure_powered(
        &mut self,
        generation: u64,
    ) -> Result<personal_os_rust_server::qcom_wlan_power::PowerReadyPermit, Error> {
        if generation != self.generation {
            return Err(Error::Stopped);
        }
        if let Ok(permit) = self.sequencer.power_ready_permit(generation) {
            return Ok(permit);
        }
        self.sequencer
            .start(&mut self.io, generation, self.plan)
            .map_err(|_| Error::Stopped)?;
        self.sequencer
            .power_ready_permit(generation)
            .map_err(|_| Error::Stopped)
    }

    fn stop_power(&mut self, generation: u64) -> Result<(), Error> {
        if generation != self.generation {
            return Err(Error::Stopped);
        }
        self.sequencer
            .stop_with_permit(&mut self.io, generation)
            .map(|_| ())
            .map_err(|_| Error::Stopped)
    }
}

/// Runtime composition that keeps WLFW indication registration behind a live
/// power permit and restores the captured RPMh votes after the WLFW server
/// disappears. The default `NoServices` path never creates this wrapper and
/// therefore remains unable to start WLAN.
pub struct IcnssPowerServices<S: RuntimeServices, B: IcnssPowerBackend> {
    inner: S,
    backend: B,
    powered_generation: Option<u64>,
}

impl<S: RuntimeServices, B: IcnssPowerBackend> IcnssPowerServices<S, B> {
    pub const fn new(inner: S, backend: B) -> Self {
        Self {
            inner,
            backend,
            powered_generation: None,
        }
    }

    pub fn into_parts(self) -> (S, B) {
        (self.inner, self.backend)
    }

    fn stop_power_if_active(&mut self, owner: &mut NativeOwner) -> Result<(), Error> {
        let Some(generation) = self.powered_generation else {
            return Ok(());
        };
        if self.backend.stop_power(generation).is_err() {
            owner.fault();
            return Err(Error::Stopped);
        }
        self.powered_generation = None;
        Ok(())
    }
}

impl<S: RuntimeServices, B: IcnssPowerBackend> RuntimeServices for IcnssPowerServices<S, B> {
    fn preboot_memory_services_ready(&mut self) -> bool {
        self.inner.preboot_memory_services_ready()
    }

    fn preboot_tftp_service_ready(&mut self) -> bool {
        self.inner.preboot_tftp_service_ready()
    }

    fn memory_services_ready(&self) -> bool {
        self.inner.memory_services_ready()
    }

    fn ipa_modem_ready(&self) -> bool {
        self.inner.ipa_modem_ready()
    }

    fn ipa_modem_driver_complete(&self) -> bool {
        self.inner.ipa_modem_driver_complete()
    }

    fn prepare_transport<I: Interrupts>(
        &mut self,
        owner: &mut NativeOwner,
        interrupts: &mut I,
    ) -> Result<(), Error> {
        self.inner.prepare_transport(owner, interrupts)
    }

    fn activate<I: Interrupts>(
        &mut self,
        owner: &mut NativeOwner,
        interrupts: &mut I,
    ) -> Result<(), Error> {
        self.inner.activate(owner, interrupts)
    }

    fn notified<I: Interrupts>(
        &mut self,
        owner: &mut NativeOwner,
        channels: u64,
        interrupts: &mut I,
    ) -> Result<core::Progress, Error> {
        match self.inner.notified(owner, channels, interrupts) {
            Ok(progress) => Ok(progress),
            Err(error) => {
                self.stop_power_if_active(owner)?;
                Err(error)
            }
        }
    }

    fn wlan_startup_step<I: Interrupts>(
        &mut self,
        owner: &mut NativeOwner,
        interrupts: &mut I,
    ) -> Result<(), Error> {
        self.inner.wlan_startup_step(owner, interrupts)?;
        let generation = owner.generation();
        let endpoint = match owner.wlfw_endpoint() {
            Ok(endpoint) => endpoint,
            Err(error) => {
                self.stop_power_if_active(owner)?;
                return Err(error);
            }
        };

        if self
            .powered_generation
            .is_some_and(|active| active != generation)
        {
            self.stop_power_if_active(owner)?;
        }

        if endpoint.is_none() {
            return self.stop_power_if_active(owner);
        }
        if owner.wlfw_state() != Some(WlfwState::ReadyToRegister) {
            return Ok(());
        }

        let permit = match self.backend.ensure_powered(generation) {
            Ok(permit) => permit,
            Err(error) => {
                owner.fault();
                return Err(error);
            }
        };
        if let Err(error) = owner.authorize_wlfw_power(permit, interrupts) {
            if self.backend.stop_power(generation).is_err() {
                owner.fault();
                return Err(Error::Stopped);
            }
            self.powered_generation = None;
            return Err(error);
        }
        self.powered_generation = Some(generation);
        Ok(())
    }
}

const TFTP_SERVER_PORT: u32 = 0x4300;
const TFTP_FIRST_TRANSFER_PORT: u32 = 0x4400;
const TFTP_RESERVED_LOCAL_PORTS: [u32; 5] = [0x4000, 0x4001, 0x4003, 0x4100, 0x4200];
const IPA_SERVER_PORT: u32 = 0x4002;
const IPA_MODEM_CLIENT_PORT: u32 = 0x4004;
const IPA_MODEM_PEER: Address = Address { node: 0, port: 27 };

/// Runtime adapter for the source-observed AP-to-modem IPA initialization
/// transaction. The permit binds the request fields to a successful AP IPA
/// post-init provider; it is not minted by this transport adapter.
pub struct IpaModemInitServices<S: RuntimeServices> {
    inner: S,
    readiness: personal_os_rust_server::qcom_ipa_client::InitPermit,
    request_started: bool,
    modem_ready: bool,
    modem_completion_pending: bool,
}

impl<S: RuntimeServices> IpaModemInitServices<S> {
    pub fn new(inner: S, readiness: personal_os_rust_server::qcom_ipa_client::InitPermit) -> Self {
        Self {
            inner,
            readiness,
            request_started: false,
            modem_ready: false,
            modem_completion_pending: false,
        }
    }

    fn check_readiness(&self, owner: &mut NativeOwner) -> Result<(), Error> {
        if self.readiness.fields(owner.generation()).is_err() {
            owner.fault();
            return Err(Error::IpaReadinessRequired);
        }
        Ok(())
    }
}

impl<S: RuntimeServices> RuntimeServices for IpaModemInitServices<S> {
    fn preboot_memory_services_ready(&mut self) -> bool {
        self.inner.preboot_memory_services_ready()
    }

    fn preboot_tftp_service_ready(&mut self) -> bool {
        self.inner.preboot_tftp_service_ready()
    }

    fn memory_services_ready(&self) -> bool {
        self.inner.memory_services_ready()
    }

    fn ipa_modem_ready(&self) -> bool {
        self.modem_ready
            && (!self.modem_completion_pending || self.inner.ipa_modem_driver_complete())
    }

    fn ipa_modem_driver_complete(&self) -> bool {
        self.inner.ipa_modem_driver_complete()
    }

    fn prepare_transport<I: Interrupts>(
        &mut self,
        owner: &mut NativeOwner,
        interrupts: &mut I,
    ) -> Result<(), Error> {
        self.check_readiness(owner)?;
        owner.enable_ipa_client(IPA_MODEM_CLIENT_PORT)?;
        self.inner.prepare_transport(owner, interrupts)
    }

    fn activate<I: Interrupts>(
        &mut self,
        owner: &mut NativeOwner,
        interrupts: &mut I,
    ) -> Result<(), Error> {
        self.check_readiness(owner)?;
        self.inner.activate(owner, interrupts)
    }

    fn notified<I: Interrupts>(
        &mut self,
        owner: &mut NativeOwner,
        channels: u64,
        interrupts: &mut I,
    ) -> Result<core::Progress, Error> {
        let progress = self.inner.notified(owner, channels, interrupts)?;
        self.check_readiness(owner)?;
        if !self.request_started
            && owner.ipa_state() == Some(personal_os_rust_server::qcom_ipa_client::State::Ready)
        {
            let fields = *self
                .readiness
                .fields(owner.generation())
                .map_err(|_| Error::IpaReadinessRequired)?;
            owner.begin_ipa_init_modem_driver(&fields, interrupts)?;
            self.request_started = true;
        }

        match owner.ipa_state() {
            Some(personal_os_rust_server::qcom_ipa_client::State::Indeterminate) => {
                owner.fault();
                return Err(Error::Stopped);
            }
            Some(
                personal_os_rust_server::qcom_ipa_client::State::ResponseReceived
                | personal_os_rust_server::qcom_ipa_client::State::ModemDriverCompletionPending,
            ) => {
                let outcome = owner.ipa_outcome().ok_or(Error::Stopped)?;
                if outcome.response.result != 0 || outcome.response.error != 0 {
                    owner.fault();
                    return Err(Error::Stopped);
                }
                self.modem_ready = true;
                self.modem_completion_pending =
                    outcome.response.modem_driver_init_pending == Some(true);
            }
            _ => {}
        }
        Ok(progress)
    }

    fn wlan_startup_step<I: Interrupts>(
        &mut self,
        owner: &mut NativeOwner,
        interrupts: &mut I,
    ) -> Result<(), Error> {
        self.inner.wlan_startup_step(owner, interrupts)
    }
}

/// Runtime-service adapter that dispatches held QRTR TFTP packets through the
/// bounded read server. The caller must provide the opaque image capability,
/// which can be minted only after the source, integrity, target match, and use
/// authorization are established by a trusted provider.
pub struct TftpServerServices<
    'image,
    S: RuntimeServices,
    R: personal_os_rust_server::qcom_tftp::ReadSource + ?Sized = [u8],
> {
    inner: S,
    server: personal_os_rust_server::qcom_qrtr_tftp::Server<'image, R>,
}

impl<'image, S: RuntimeServices, R: personal_os_rust_server::qcom_tftp::ReadSource + ?Sized>
    TftpServerServices<'image, S, R>
{
    pub fn new(
        inner: S,
        image: personal_os_rust_server::qcom_qrtr_tftp::AuthorizedWlanImage<'image, R>,
    ) -> Result<Self, Error> {
        let server = personal_os_rust_server::qcom_qrtr_tftp::Server::new(
            1,
            0,
            TFTP_SERVER_PORT,
            TFTP_FIRST_TRANSFER_PORT,
            &TFTP_RESERVED_LOCAL_PORTS,
            image,
        )
        .map_err(|_| Error::Mapping)?;
        Ok(Self { inner, server })
    }
}

impl<S: RuntimeServices, R: personal_os_rust_server::qcom_tftp::ReadSource + ?Sized> RuntimeServices
    for TftpServerServices<'_, S, R>
{
    fn preboot_memory_services_ready(&mut self) -> bool {
        self.inner.preboot_memory_services_ready()
    }

    fn preboot_tftp_service_ready(&mut self) -> bool {
        self.server.image_available()
    }

    fn memory_services_ready(&self) -> bool {
        self.inner.memory_services_ready()
    }

    fn ipa_modem_ready(&self) -> bool {
        self.inner.ipa_modem_ready()
    }

    fn ipa_modem_driver_complete(&self) -> bool {
        self.inner.ipa_modem_driver_complete()
    }

    fn prepare_transport<I: Interrupts>(
        &mut self,
        owner: &mut NativeOwner,
        interrupts: &mut I,
    ) -> Result<(), Error> {
        self.inner.prepare_transport(owner, interrupts)
    }

    fn activate<I: Interrupts>(
        &mut self,
        owner: &mut NativeOwner,
        interrupts: &mut I,
    ) -> Result<(), Error> {
        if !self.server.image_available() {
            return Err(Error::TftpImageNotReady);
        }
        if !owner.tftp_server_prepared(TFTP_SERVER_PORT) {
            owner.prepare_tftp_server(TFTP_SERVER_PORT)?;
        }
        self.inner.activate(owner, interrupts)
    }

    fn notified<I: Interrupts>(
        &mut self,
        owner: &mut NativeOwner,
        channels: u64,
        interrupts: &mut I,
    ) -> Result<core::Progress, Error> {
        let progress = self.inner.notified(owner, channels, interrupts)?;
        for _ in 0..personal_os_rust_server::qcom_glink_qmi::MAX_REQUESTS {
            match owner.service_tftp_step(&mut self.server, interrupts)? {
                personal_os_rust_server::qcom_modem_owner::TftpServiceProgress::Idle
                | personal_os_rust_server::qcom_modem_owner::TftpServiceProgress::WaitingReply => {
                    break;
                }
                personal_os_rust_server::qcom_modem_owner::TftpServiceProgress::ResponseQueued
                | personal_os_rust_server::qcom_modem_owner::TftpServiceProgress::Closed(_)
                | personal_os_rust_server::qcom_modem_owner::TftpServiceProgress::Ignored(_) => {}
            }
        }
        Ok(progress)
    }

    fn wlan_startup_step<I: Interrupts>(
        &mut self,
        owner: &mut NativeOwner,
        interrupts: &mut I,
    ) -> Result<(), Error> {
        self.inner.wlan_startup_step(owner, interrupts)
    }
}

#[derive(Clone, Copy)]
enum PendingIpaResponse {
    Queue {
        handle: personal_os_rust_server::qcom_glink_qmi::RequestHandle,
        response: PreparedResponse,
    },
    Completion(PreparedResponse),
}

#[derive(Clone, Copy)]
enum PendingIpaIndication {
    Queue(PreparedIndication),
    Completion(PreparedIndication),
}

/// AP IPA QMI server adapter for the native owner. The caller must supply the
/// opaque permit returned by a trusted live IPA provider after `ipa3_post_init`;
/// this adapter publishes the QMI endpoint and dispatches bounded messages.
pub struct IpaServerServices<S: RuntimeServices> {
    inner: S,
    readiness: ApReadyPermit,
    server: Option<IpaServiceServer>,
    preboot_staged: bool,
    pending_response: Option<PendingIpaResponse>,
    pending_indication: Option<PendingIpaIndication>,
    advertised: bool,
}

impl<S: RuntimeServices> IpaServerServices<S> {
    pub fn new(inner: S, readiness: ApReadyPermit) -> Self {
        Self {
            inner,
            readiness,
            server: None,
            preboot_staged: false,
            pending_response: None,
            pending_indication: None,
            advertised: false,
        }
    }

    /// Prepare IPA and SERVREG local service state before the caller starts
    /// MPSS. This only builds bounded in-memory QRTR state; it does not start
    /// MPSS, negotiate GLINK, publish packets, or initialize IPA hardware.
    /// Pass the returned value to `attach_with_prepared_services` after the
    /// GLINK channel is ready so the same session is attached intact.
    pub fn prepare_before_mpss(
        inner: S,
        readiness: ApReadyPermit,
    ) -> Result<PreparedIpaServerServices<S>, Error> {
        Self::prepare_before_mpss_inner(inner, readiness, false, false)
    }

    /// Prepare the full boot-time local service set before MPSS. In addition
    /// to the AP IPA server and SERVREG mapper, this requires a live validation
    /// from the retained remote-storage/heap backend before those endpoints
    /// can be marked ready. The backend is revalidated again before attach.
    pub fn prepare_before_mpss_with_memory(
        mut inner: S,
        readiness: ApReadyPermit,
    ) -> Result<PreparedIpaServerServices<S>, Error> {
        if !inner.preboot_memory_services_ready() {
            return Err(Error::MemoryServicesNotReady);
        }
        Self::prepare_before_mpss_inner(inner, readiness, true, false)
    }

    /// Prepare IPA, SERVREG, storage, heap, and the authorized WLAN TFTP
    /// endpoint before MPSS. The retained image source is revalidated here
    /// and again immediately before the prepared session is attached.
    pub fn prepare_before_mpss_with_wlan_services(
        mut inner: S,
        readiness: ApReadyPermit,
    ) -> Result<PreparedIpaServerServices<S>, Error> {
        if !inner.preboot_memory_services_ready() {
            return Err(Error::MemoryServicesNotReady);
        }
        if !inner.preboot_tftp_service_ready() {
            return Err(Error::TftpImageNotReady);
        }
        Self::prepare_before_mpss_inner(inner, readiness, true, true)
    }

    fn prepare_before_mpss_inner(
        inner: S,
        readiness: ApReadyPermit,
        memory_staged: bool,
        tftp_staged: bool,
    ) -> Result<PreparedIpaServerServices<S>, Error> {
        let generation = readiness.generation();
        let mut server =
            IpaServiceServer::new(IPA_MODEM_PEER, generation).map_err(|_| Error::Mapping)?;
        server
            .note_ap_init_complete(generation, &readiness)
            .map_err(|_| Error::IpaReadinessRequired)?;
        Ok(PreparedIpaServerServices {
            services: Self {
                inner,
                readiness,
                server: Some(server),
                preboot_staged: true,
                pending_response: None,
                pending_indication: None,
                advertised: false,
            },
            session: PreparedPrebootSession::new(generation, memory_staged, tftp_staged)?,
        })
    }

    /// Prepare both ends of the IPA startup exchange from one completed
    /// post-init proof. This prevents combining server/client permits from
    /// different boot generations and returns the exact service wrapper that
    /// `attach_with_prepared_services` consumes after GLINK negotiation.
    pub fn prepare_from_post_init(
        inner: S,
        readiness: personal_os_rust_server::sunfish_ipa_post_init::Ready,
    ) -> Result<PreparedIpaServerServices<IpaModemInitServices<S>>, Error> {
        let ap_permit = readiness
            .ap_service_permit()
            .map_err(|_| Error::IpaReadinessRequired)?;
        let modem_permit = readiness
            .modem_client_permit()
            .map_err(|_| Error::IpaReadinessRequired)?;
        IpaServerServices::<IpaModemInitServices<S>>::prepare_before_mpss(
            IpaModemInitServices::new(inner, modem_permit),
            ap_permit,
        )
    }

    /// Variant of `prepare_from_post_init` for platforms that have already
    /// validated and retained the remote-storage/heap backend before MPSS.
    pub fn prepare_from_post_init_with_memory(
        inner: S,
        readiness: personal_os_rust_server::sunfish_ipa_post_init::Ready,
    ) -> Result<PreparedIpaServerServices<IpaModemInitServices<S>>, Error> {
        let ap_permit = readiness
            .ap_service_permit()
            .map_err(|_| Error::IpaReadinessRequired)?;
        let modem_permit = readiness
            .modem_client_permit()
            .map_err(|_| Error::IpaReadinessRequired)?;
        IpaServerServices::<IpaModemInitServices<S>>::prepare_before_mpss_with_memory(
            IpaModemInitServices::new(inner, modem_permit),
            ap_permit,
        )
    }

    /// Post-init-validated variant that also requires the memory backend and
    /// authorized WLAN TFTP source before the caller may start MPSS.
    pub fn prepare_from_post_init_with_wlan_services(
        inner: S,
        readiness: personal_os_rust_server::sunfish_ipa_post_init::Ready,
    ) -> Result<PreparedIpaServerServices<IpaModemInitServices<S>>, Error> {
        let ap_permit = readiness
            .ap_service_permit()
            .map_err(|_| Error::IpaReadinessRequired)?;
        let modem_permit = readiness
            .modem_client_permit()
            .map_err(|_| Error::IpaReadinessRequired)?;
        IpaServerServices::<IpaModemInitServices<S>>::prepare_before_mpss_with_wlan_services(
            IpaModemInitServices::new(inner, modem_permit),
            ap_permit,
        )
    }

    fn backpressure(error: &Error) -> bool {
        matches!(
            error,
            Error::Transport(personal_os_rust_server::qcom_glink_fifo::Error::Session(
                personal_os_rust_server::qcom_glink_qmi::Error::FlowBusy
                    | personal_os_rust_server::qcom_glink_qmi::Error::NoCapacity
            ))
        )
    }

    fn fail_response(&mut self, owner: &mut NativeOwner, response: PreparedResponse) -> Error {
        if let Some(server) = self.server.as_mut() {
            let _ = server.mark_response_uncertain(owner.generation(), &response);
        }
        owner.fault();
        Error::Stopped
    }

    fn fail_indication(
        &mut self,
        owner: &mut NativeOwner,
        indication: PreparedIndication,
    ) -> Error {
        if let Some(server) = self.server.as_mut() {
            let _ = server.mark_indication_uncertain(owner.generation(), &indication);
        }
        owner.fault();
        Error::Stopped
    }

    /// Advance one serialized IPA transaction. A response or indication stays
    /// retained across flow-control backpressure and commits only after both
    /// GLINK RX_DONE and QRTR RESUME_TX have been observed by the transport.
    fn service_ipa<I: Interrupts>(
        &mut self,
        owner: &mut NativeOwner,
        interrupts: &mut I,
    ) -> Result<(), Error> {
        if let Some(pending) = self.pending_response.take() {
            match pending {
                PendingIpaResponse::Queue { handle, response } => {
                    match owner.queue_ipa_service_reply(&handle, &response, interrupts) {
                        Ok(()) => {
                            self.pending_response = Some(PendingIpaResponse::Completion(response));
                        }
                        Err(error) if Self::backpressure(&error) => {
                            self.pending_response =
                                Some(PendingIpaResponse::Queue { handle, response });
                        }
                        Err(_) => return Err(self.fail_response(owner, response)),
                    }
                    return Ok(());
                }
                PendingIpaResponse::Completion(response) => {
                    let completion = match owner.take_ipa_service_response_completion() {
                        Ok(completion) => completion,
                        Err(_) => return Err(self.fail_response(owner, response)),
                    };
                    match completion {
                        None => {
                            self.pending_response = Some(PendingIpaResponse::Completion(response));
                            return Ok(());
                        }
                        Some(token) if token == response.token() => {
                            let Some(server) = self.server.as_mut() else {
                                return Err(self.fail_response(owner, response));
                            };
                            if server
                                .commit_response(owner.generation(), &response)
                                .is_err()
                            {
                                return Err(self.fail_response(owner, response));
                            }
                        }
                        Some(_) => return Err(self.fail_response(owner, response)),
                    }
                }
            }
        }

        if let Some(pending) = self.pending_indication.take() {
            match pending {
                PendingIpaIndication::Queue(indication) => {
                    match owner.queue_ipa_master_init_indication(
                        &indication,
                        &self.readiness,
                        interrupts,
                    ) {
                        Ok(()) => {
                            self.pending_indication =
                                Some(PendingIpaIndication::Completion(indication));
                        }
                        Err(error) if Self::backpressure(&error) => {
                            self.pending_indication = Some(PendingIpaIndication::Queue(indication));
                        }
                        Err(_) => return Err(self.fail_indication(owner, indication)),
                    }
                    return Ok(());
                }
                PendingIpaIndication::Completion(indication) => {
                    let completion = match owner.take_ipa_indication_completion() {
                        Ok(completion) => completion,
                        Err(_) => return Err(self.fail_indication(owner, indication)),
                    };
                    match completion {
                        None => {
                            self.pending_indication =
                                Some(PendingIpaIndication::Completion(indication));
                            return Ok(());
                        }
                        Some(token) if token == indication.token() => {
                            let Some(server) = self.server.as_mut() else {
                                return Err(self.fail_indication(owner, indication));
                            };
                            if server
                                .commit_master_init_indication(owner.generation(), &indication)
                                .is_err()
                            {
                                return Err(self.fail_indication(owner, indication));
                            }
                        }
                        Some(_) => return Err(self.fail_indication(owner, indication)),
                    }
                }
            }
        }

        let generation = owner.generation();
        let indication = self
            .server
            .as_mut()
            .ok_or(Error::Mapping)?
            .prepare_master_init_indication(
                generation,
                personal_os_rust_server::qcom_ipa_service::QMI_MESSAGE_BYTES,
            )
            .map_err(|_| {
                owner.fault();
                Error::Stopped
            })?;
        if let Some(indication) = indication {
            self.pending_indication = Some(PendingIpaIndication::Queue(indication));
            return self.service_ipa(owner, interrupts);
        }

        let Some(handle) = owner.pending_request_for_service(Service::IpaServer)? else {
            return Ok(());
        };
        let (context, bytes) = owner.request_qmi(&handle)?;
        if context.service != Service::IpaServer {
            return Ok(());
        }
        if context.generation != generation
            || context.local.node != 1
            || context.local.port != IPA_SERVER_PORT
            || context.peer != IPA_MODEM_PEER
        {
            owner.fault();
            return Err(Error::Stopped);
        }
        let response = self
            .server
            .as_mut()
            .ok_or(Error::Mapping)?
            .prepare_response(
                generation,
                context.peer,
                bytes,
                personal_os_rust_server::qcom_ipa_service::QMI_MESSAGE_BYTES,
            )
            .map_err(|_| {
                owner.fault();
                Error::Stopped
            })?;
        self.pending_response = Some(PendingIpaResponse::Queue { handle, response });
        self.service_ipa(owner, interrupts)
    }
}

impl<S: RuntimeServices> RuntimeServices for IpaServerServices<S> {
    fn preboot_memory_services_ready(&mut self) -> bool {
        self.inner.preboot_memory_services_ready()
    }

    fn preboot_tftp_service_ready(&mut self) -> bool {
        self.inner.preboot_tftp_service_ready()
    }

    fn memory_services_ready(&self) -> bool {
        self.inner.memory_services_ready()
    }

    fn ipa_modem_ready(&self) -> bool {
        self.inner.ipa_modem_ready()
    }

    fn ipa_modem_driver_complete(&self) -> bool {
        self.server
            .as_ref()
            .is_some_and(|server| server.modem_init_status().is_some())
            || self.inner.ipa_modem_driver_complete()
    }

    fn prepare_transport<I: Interrupts>(
        &mut self,
        owner: &mut NativeOwner,
        interrupts: &mut I,
    ) -> Result<(), Error> {
        self.inner.prepare_transport(owner, interrupts)
    }

    fn activate<I: Interrupts>(
        &mut self,
        owner: &mut NativeOwner,
        interrupts: &mut I,
    ) -> Result<(), Error> {
        let generation = owner.generation();
        if let Some(server) = self.server.as_mut() {
            if server
                .note_ap_init_complete(generation, &self.readiness)
                .is_err()
            {
                owner.fault();
                return Err(Error::IpaReadinessRequired);
            }
        } else {
            let mut server =
                IpaServiceServer::new(IPA_MODEM_PEER, generation).map_err(|_| Error::Mapping)?;
            if server
                .note_ap_init_complete(generation, &self.readiness)
                .is_err()
            {
                owner.fault();
                return Err(Error::IpaReadinessRequired);
            }
            owner.prepare_ipa_server_endpoint(IPA_SERVER_PORT)?;
            self.server = Some(server);
        }
        self.inner.activate(owner, interrupts)?;
        Ok(())
    }

    fn notified<I: Interrupts>(
        &mut self,
        owner: &mut NativeOwner,
        channels: u64,
        interrupts: &mut I,
    ) -> Result<core::Progress, Error> {
        let progress = self.inner.notified(owner, channels, interrupts)?;
        if self.preboot_staged && !self.advertised {
            if owner.control_status().announced_services & (1 << 4) != 0 {
                self.advertised = true;
            }
        } else if !self.advertised {
            let control = owner.control_status();
            if control.hello_sent && control.peer_hello {
                owner.enable_ipa_server(&self.readiness, interrupts)?;
                self.advertised = true;
            }
        }
        if self.advertised {
            self.service_ipa(owner, interrupts)?;
        }
        Ok(progress)
    }

    fn wlan_startup_step<I: Interrupts>(
        &mut self,
        owner: &mut NativeOwner,
        interrupts: &mut I,
    ) -> Result<(), Error> {
        self.inner.wlan_startup_step(owner, interrupts)
    }
}

#[cfg(not(feature = "sunfish-modem-servreg-state-probe"))]
pub struct ModemHostServices<B: HandlerBackend> {
    handlers: Handlers<B>,
    memory_services_ready: bool,
}

#[cfg(not(feature = "sunfish-modem-servreg-state-probe"))]
impl<B: HandlerBackend> ModemHostServices<B> {
    pub const fn new(handlers: Handlers<B>) -> Self {
        Self {
            handlers,
            memory_services_ready: false,
        }
    }
}

#[cfg(not(feature = "sunfish-modem-servreg-state-probe"))]
impl<B: HandlerBackend> RuntimeServices for ModemHostServices<B> {
    fn preboot_memory_services_ready(&mut self) -> bool {
        self.handlers.validate().is_ok()
    }

    fn memory_services_ready(&self) -> bool {
        self.memory_services_ready
    }

    fn activate<I: Interrupts>(
        &mut self,
        owner: &mut NativeOwner,
        interrupts: &mut I,
    ) -> Result<(), Error> {
        self.memory_services_ready = false;
        owner.activate_handlers(&mut self.handlers, interrupts)?;
        self.memory_services_ready = true;
        Ok(())
    }
    fn notified<I: Interrupts>(
        &mut self,
        owner: &mut NativeOwner,
        channels: u64,
        interrupts: &mut I,
    ) -> Result<core::Progress, Error> {
        match owner.notified_with_handlers(channels, &mut self.handlers, interrupts) {
            Ok(progress) => Ok(progress.transport),
            Err(error) => {
                self.memory_services_ready = false;
                Err(error)
            }
        }
    }
}

pub struct Runtime<S: RuntimeServices = NoServices> {
    owner: Owner<NativeIo<LiveGate>>,
    services: S,
    display_pixels: *mut u32,
    _retained_prelude: Prelude,
    msa_reservation: Option<MsaReservation>,
    msa_scm: Option<ScmResources>,
    msa_info_started: bool,
    msa_handoff_started: bool,
    capability_started: bool,
    started: bool,
    servreg_query_started: bool,
    servreg_query_failed: bool,
    servreg_restart_started: bool,
    last_servreg_screen: Option<(
        Option<personal_os_rust_server::qcom_servreg_notif_client::State>,
        Option<personal_os_rust_server::qcom_servreg_notif_client::Outcome>,
        bool,
        personal_os_rust_server::qcom_qrtr_control::Status,
        Option<personal_os_rust_server::qcom_glink_qmi::CompletionDiagnostic>,
        Option<personal_os_rust_server::qcom_glink_qmi::CreditDiagnostic>,
        Option<Error>,
        u32,
        personal_os_rust_server::qcom_modem_owner::TftpObserverTrace,
        personal_os_rust_server::qcom_modem_owner::ServiceRegistryTrace,
    )>,
}
const WLFW_CLIENT_PORT: u32 = 0x4100;
#[cfg(feature = "sunfish-modem-servreg-state-probe")]
const SERVREG_NOTIF_CLIENT_PORT: u32 = 0x4200;
#[cfg(feature = "sunfish-modem-tftp-observer")]
const TFTP_OBSERVER_PORT: u32 = 0x4300;

#[cfg(feature = "sunfish-modem-servreg-state-probe")]
pub fn paint_servreg_owner_stage(display: &Display, status: &str) {
    display.clear();
    display.line(0, "SERVREG OWNER PATH");
    display.line(1, status);
    display.line(
        3,
        if cfg!(feature = "sunfish-modem-servreg-restart-probe") {
            "ONE PDR LISTENER"
        } else {
            "ONE READ ONLY QUERY"
        },
    );
    display.line(
        4,
        if cfg!(feature = "sunfish-modem-servreg-restart-probe") {
            "WLFW REG AND MSA INFO"
        } else {
            "NO WLAN START"
        },
    );
    display.line(5, "NO SCM GRANT OR DMA");
    display.line(28, "NO WLAN FIRMWARE START");
    display.line(29, "RESTART WHEN DONE");
    display.flush();
}

#[cfg(feature = "sunfish-modem-servreg-state-probe")]
fn servreg_session_stop_reason(error: glink::Error) -> &'static str {
    match error {
        glink::Error::Control(error) => match error {
            qrtr::Error::Length => "QRTR BAD LENGTH",
            qrtr::Error::Header => "QRTR BAD HEADER",
            qrtr::Error::Address => "QRTR BAD ADDRESS",
            qrtr::Error::Payload => "QRTR BAD PAYLOAD",
            qrtr::Error::Unsupported => "QRTR UNSUPPORTED",
            qrtr::Error::NotStarted => "QRTR NOT STARTED",
            qrtr::Error::PeerNotReady => "QRTR PEER NOT READY",
            qrtr::Error::PeerGone => "QRTR PEER GONE",
            qrtr::Error::ClientGone => "QRTR CLIENT GONE",
            qrtr::Error::NoCapacity => "QRTR NO CAPACITY",
            qrtr::Error::ServerConflict => "QRTR SERVER CONFLICT",
            qrtr::Error::Busy => "QRTR BUSY",
            qrtr::Error::Unavailable => "QRTR UNAVAILABLE",
        },
        glink::Error::Routing(_) => "GLINK QRTR ROUTE ERROR",
        glink::Error::Wire(_) => "GLINK QMI WIRE ERROR",
        glink::Error::InvalidChannel => "GLINK INVALID CHANNEL",
        glink::Error::StaleGeneration => "GLINK STALE GENERATION",
        glink::Error::Quarantined => "GLINK QUARANTINED",
        glink::Error::TruncatedRecord => "GLINK TRUNCATED RECORD",
        glink::Error::RecordLength => "GLINK BAD RECORD LENGTH",
        glink::Error::UnsupportedCommand => "GLINK BAD COMMAND",
        glink::Error::WrongChannel => "GLINK WRONG CHANNEL",
        glink::Error::InvalidIntent => "GLINK BAD INTENT",
        glink::Error::IntentCapacity => "GLINK INTENT CAPACITY",
        glink::Error::IntentAlreadyActive => "GLINK ACTIVE INTENT ID",
        glink::Error::NoCapacity => "GLINK NO CAPACITY",
        glink::Error::FragmentOrder => "GLINK FRAGMENT ORDER",
        glink::Error::FragmentLength => "GLINK FRAGMENT LENGTH",
        glink::Error::FragmentLimit => "GLINK FRAGMENT LIMIT",
        glink::Error::InvalidHandle => "GLINK BAD HANDLE",
        glink::Error::NoRemoteIntent => "GLINK NO REMOTE INTENT",
        glink::Error::InvalidCompletion => "GLINK BAD COMPLETION",
        glink::Error::InvalidCredit => "GLINK BAD CREDIT",
        glink::Error::FlowBusy => "GLINK FLOW BUSY",
        glink::Error::InvalidReply => "GLINK BAD REPLY",
        glink::Error::WouldBlock => "GLINK WOULD BLOCK",
        glink::Error::PublicationUncertain => "GLINK PUB UNCERTAIN",
        glink::Error::IdExhausted => "GLINK ID EXHAUSTED",
        glink::Error::IpaClient(_) => "GLINK IPA CLIENT ERROR",
        glink::Error::Wlfw(_) => "GLINK WLFW CLIENT ERROR",
        glink::Error::ServregNotif(_) => "GLINK SERVREG CLIENT ERROR",
    }
}

#[cfg(feature = "sunfish-modem-servreg-state-probe")]
fn servreg_stop_reason(error: Option<Error>, transport_diagnostic: u32) -> &'static str {
    match error {
        None => "NO OWNER ERROR",
        Some(Error::Transport(fifo::Error::Session(error))) => servreg_session_stop_reason(error),
        Some(Error::Transport(fifo::Error::StaleGeneration)) => "FIFO STALE GENERATION",
        Some(Error::Transport(fifo::Error::Quarantined)) => "FIFO QUARANTINED",
        Some(Error::Transport(fifo::Error::Access))
            if transport_diagnostic == smem::Error::Fatal.code() =>
        {
            "MODEM ERR-FATAL"
        }
        Some(Error::Transport(fifo::Error::Access)) => "FIFO ACCESS ERROR",
        Some(Error::Transport(fifo::Error::NotReady)) => "FIFO NOT READY",
        Some(Error::Transport(fifo::Error::Fatal)) => "FIFO FATAL STATUS",
        Some(Error::Transport(fifo::Error::IdentityChanged)) => "FIFO IDENTITY CHANGED",
        Some(Error::Transport(fifo::Error::CursorBounds)) => "FIFO CURSOR BOUNDS",
        Some(Error::Transport(fifo::Error::OwnedCursorChanged)) => "FIFO OWNED CURSOR",
        Some(Error::Transport(fifo::Error::PeerOverrun)) => "FIFO PEER OVERRUN",
        Some(Error::Transport(fifo::Error::ReservedChanged)) => "FIFO RESERVED WORD",
        Some(Error::Transport(fifo::Error::UnsupportedCommand)) => "FIFO BAD COMMAND",
        Some(Error::Transport(fifo::Error::InvalidRecord)) => "FIFO INVALID RECORD",
        Some(Error::Transport(fifo::Error::ChangedRecord)) => "FIFO CHANGED RECORD",
        Some(Error::Transport(fifo::Error::Readback)) => "FIFO READBACK ERROR",
        Some(Error::Transport(fifo::Error::WouldBlock)) => "FIFO WOULD BLOCK",
        Some(Error::Transport(fifo::Error::SessionNotFresh)) => "FIFO SESSION NOT FRESH",
        Some(Error::Transport(fifo::Error::ResetNotEmpty)) => "FIFO RESET NOT EMPTY",
        Some(Error::UnexpectedNotification) => "UNEXPECTED CHANNEL",
        Some(Error::Ack) => "IRQ ACK ERROR",
        Some(Error::Schedule) => "WORK SCHEDULE ERROR",
        Some(Error::QueueFull) => "OWNER QUEUE FULL",
        Some(Error::Mapping) => "MAPPING ERROR",
        Some(Error::Prelude) => "PRELUDE ERROR",
        Some(Error::Service(_)) => "SERVICE OWNER ERROR",
        Some(Error::Stopped) => "OWNER STOPPED",
        Some(Error::IpaReadinessRequired) => "IPA READINESS REQUIRED",
        Some(Error::GenerationMismatch) => "BOOT GENERATION MISMATCH",
    }
}

/// Validated private SCM argument page and the modem owner's serialized SMC
/// capability. The caller must keep the exact scratch mapping and capability
/// valid for the entire retained Runtime lifetime.
pub struct ScmResources {
    args: *mut u8,
    invoke: fn(&Call, &mut dyn FnMut() -> Result<(), Fault>) -> Result<[u64; 8], Fault>,
}

impl ScmResources {
    /// # Safety
    /// `args` must be the exact validated uncached 4-KiB SCM scratch mapping,
    /// retained exclusively for the returned Runtime. `invoke` must be the
    /// serialized SMC capability for the fixed MEM_PROT_ASSIGN operation.
    pub unsafe fn new(
        args: *mut u8,
        invoke: fn(&Call, &mut dyn FnMut() -> Result<(), Fault>) -> Result<[u64; 8], Fault>,
    ) -> Result<Self, Error> {
        if args.is_null() || args as usize % 4096 != 0 {
            return Err(Error::Mapping);
        }
        Ok(Self { args, invoke })
    }
}

struct OwnerLifecycle<'a>(&'a mut Owner<NativeIo<LiveGate>>);
impl MsaLifecycle for OwnerLifecycle<'_> {
    fn revalidate(&mut self, generation: u64, identity: u64) -> Result<(), Fault> {
        if self.0.generation() != generation || self.0.identity() != identity {
            return Err(Fault::Failed);
        }
        self.0.revalidate().map_err(|_| Fault::Uncertain)
    }
}

struct OwnerScm<'a>(&'a ScmResources);
impl MsaScm for OwnerScm<'_> {
    fn invoke(
        &mut self,
        call: &Call,
        lifecycle: &mut dyn FnMut() -> Result<(), Fault>,
    ) -> Result<[u64; 8], Fault> {
        (self.0.invoke)(call, lifecycle)
    }
}

struct OwnerScratch<'a>(&'a ScmResources);
impl MsaScratch for OwnerScratch<'_> {
    fn stage(&mut self, bytes: &[u8; ARG_BYTES]) -> Result<(), Fault> {
        if self.0.args.is_null() {
            return Err(Fault::Failed);
        }
        for (index, byte) in bytes.iter().enumerate() {
            // SAFETY: ScmResources' constructor contract retains the exact
            // exclusive uncached scratch mapping for Runtime's lifetime.
            unsafe { self.0.args.add(index).write_volatile(*byte) };
        }
        msa_sync();
        for (index, byte) in bytes.iter().enumerate() {
            // SAFETY: the same retained mapping and bounded ARG_BYTES span.
            if unsafe { self.0.args.add(index).read_volatile() } != *byte {
                return Err(Fault::Uncertain);
            }
        }
        msa_sync();
        Ok(())
    }

    fn complete(&mut self) -> Result<(), Fault> {
        msa_sync();
        Ok(())
    }
}

fn msa_sync() {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        ::core::arch::asm!("dsb sy", options(nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "aarch64"))]
    ::core::sync::atomic::fence(::core::sync::atomic::Ordering::SeqCst);
}
struct KernelInterrupts;
impl Interrupts for KernelInterrupts {
    fn acknowledge(&mut self, channel: u8) -> Result<(), Error> {
        if !matches!(channel, core::GLINK_CHANNEL | core::STATUS_CHANNEL) {
            return Err(Error::Ack);
        }
        Channel::new(channel as usize)
            .irq_ack()
            .map_err(|_| Error::Ack)
    }
    fn schedule(&mut self) -> Result<(), Error> {
        Channel::new(core::CONTINUE_SEND as usize).notify();
        Ok(())
    }
}

/// Kernel IRQ acknowledger for the separately mapped candidate UFS channel.
/// Runtime attachment stays disabled until a production UFS platform gate
/// provides an exclusive DMA/SMMU lease and operational host.
pub struct UfsKernelAcknowledger;

impl personal_os_rust_server::qcom_ufs_irq::KernelAcknowledger for UfsKernelAcknowledger {
    fn acknowledge(&mut self, channel_id: usize) -> Result<(), ()> {
        if channel_id != personal_os_rust_server::qcom_ufs_irq::UFS_IRQ_CHANNEL_ID {
            return Err(());
        }
        Channel::new(channel_id).irq_ack().map_err(|_| ())
    }
}

/// # Safety
/// The caller transfers the actual authenticated fresh-boot channel checkpoint
/// and exclusive mapped SMEM/APCS/lock ownership. All symbols were checked before
/// first use. The five AP pages are contiguous uncached RW; the rest of SMEM is
/// RO, APCS/lock are Device. RX may share the final RW allocation page, but
/// its byte range is disjoint and NativeIo never writes RX. No old writer,
/// observer or IRQ callback remains usable.
/// `msa_scm` must own the exact validated uncached scratch page and serialized
/// MEM_PROT_ASSIGN capability; both remain retained by Runtime after failure.
/// Runtime and all mappings must be retained through every outcome until reboot.
pub unsafe fn attach(
    window: Window,
    checkpoint: ChannelCheckpoint,
    smem_base: *const u8,
    ap: *mut u8,
    doorbell: *mut u32,
    display_pixels: *mut u32,
    msa_scm: Option<ScmResources>,
    progress: impl FnMut(&'static str),
) -> Result<Runtime, Error> {
    // SAFETY: forwarded unchanged from this function's caller contract.
    unsafe {
        attach_with_services(
            window,
            checkpoint,
            smem_base,
            ap,
            doorbell,
            display_pixels,
            msa_scm,
            NoServices,
            progress,
        )
    }
}

/// # Safety
/// The channel and MSA requirements are identical to `attach`. In addition,
/// `handlers` must own a validated backend whose physical media, DMA and SCM
/// authority remains exclusively retained by it for the entire Runtime life.
#[cfg(not(feature = "sunfish-modem-servreg-state-probe"))]
pub unsafe fn attach_with_handlers<B: HandlerBackend>(
    window: Window,
    checkpoint: ChannelCheckpoint,
    smem_base: *const u8,
    ap: *mut u8,
    doorbell: *mut u32,
    display_pixels: *mut u32,
    msa_scm: Option<ScmResources>,
    handlers: Handlers<B>,
    progress: impl FnMut(&'static str),
) -> Result<Runtime<ModemHostServices<B>>, Error> {
    // SAFETY: forwarded unchanged; service backend capability requirements are
    // documented above and it remains retained in ModemHostServices.
    unsafe {
        attach_with_services(
            window,
            checkpoint,
            smem_base,
            ap,
            doorbell,
            display_pixels,
            msa_scm,
            ModemHostServices::new(handlers),
            progress,
        )
    }
}

/// Attach the native owner with an explicitly chosen runtime service set.
///
/// This is the extension point for bounded services such as the QRTR TFTP
/// dispatcher. The default `attach` path continues to use `NoServices`.
///
/// # Safety
/// The channel, MSA, exclusive mappings, and retained-runtime requirements are
/// identical to [`attach`]. `services` and every resource it references must
/// remain valid for the returned `Runtime` lifetime and must not bypass the
/// owner's lifecycle or transport gates.
pub unsafe fn attach_with_services<S: RuntimeServices>(
    window: Window,
    checkpoint: ChannelCheckpoint,
    smem_base: *const u8,
    ap: *mut u8,
    doorbell: *mut u32,
    display_pixels: *mut u32,
    msa_scm: Option<ScmResources>,
    services: S,
    progress: impl FnMut(&'static str),
) -> Result<Runtime<S>, Error> {
    // SAFETY: forwarded unchanged from this function's caller contract.
    unsafe {
        attach_with_optional_preboot_session(
            window,
            checkpoint,
            smem_base,
            ap,
            doorbell,
            display_pixels,
            msa_scm,
            None,
            1,
            services,
            progress,
        )
    }
}

/// Attach the native owner with an explicit ICNSS power backend. In the
/// production owner profile this is the composition that can release the
/// power-gated WLFW registration path; callers using `NoServices` or ordinary
/// handlers remain fail-closed.
///
/// # Safety
/// The requirements of [`attach_with_services`] apply. In addition, the
/// backend must retain the verified Sunfish RPMh broker, exact client-vote
/// snapshots, and exclusive lifecycle lease for the entire returned runtime.
pub unsafe fn attach_with_icnss_power<S: RuntimeServices, B: IcnssPowerBackend>(
    window: Window,
    checkpoint: ChannelCheckpoint,
    smem_base: *const u8,
    ap: *mut u8,
    doorbell: *mut u32,
    display_pixels: *mut u32,
    msa_scm: Option<ScmResources>,
    services: S,
    backend: B,
    progress: impl FnMut(&'static str),
) -> Result<Runtime<IcnssPowerServices<S, B>>, Error> {
    // SAFETY: the caller upholds the forwarded mapping and retained-runtime
    // contract, including the new backend retained by the wrapper.
    unsafe {
        attach_with_services(
            window,
            checkpoint,
            smem_base,
            ap,
            doorbell,
            display_pixels,
            msa_scm,
            IcnssPowerServices::new(services, backend),
            progress,
        )
    }
}

/// Attach the native owner while preserving a QRTR session prepared before
/// MPSS startup. The returned runtime then owns both the session and service
/// state; callers must retain it through every outcome until reboot.
///
/// # Safety
/// The same channel, MSA, mapping, and retained-runtime requirements as
/// [`attach_with_services`] apply. The prepared capability must belong to this
/// boot generation and must not have been attached or used for transport.
pub unsafe fn attach_with_prepared_services<S: RuntimeServices>(
    window: Window,
    checkpoint: ChannelCheckpoint,
    smem_base: *const u8,
    ap: *mut u8,
    doorbell: *mut u32,
    display_pixels: *mut u32,
    msa_scm: Option<ScmResources>,
    mut prepared: PreparedIpaServerServices<S>,
    progress: impl FnMut(&'static str),
) -> Result<Runtime<IpaServerServices<S>>, Error> {
    let generation = prepared.generation();
    if checkpoint.generation() != generation {
        return Err(Error::GenerationMismatch);
    }
    prepared.validate(generation)?;
    // SAFETY: both service state and its pristine prepared session move into
    // the same runtime owner and remain retained for the full lifetime.
    unsafe {
        attach_with_optional_preboot_session(
            window,
            checkpoint,
            smem_base,
            ap,
            doorbell,
            display_pixels,
            msa_scm,
            Some(prepared.session),
            generation,
            prepared.services,
            progress,
        )
    }
}

unsafe fn attach_with_optional_preboot_session<S: RuntimeServices>(
    mut window: Window,
    checkpoint: ChannelCheckpoint,
    smem_base: *const u8,
    ap: *mut u8,
    doorbell: *mut u32,
    display_pixels: *mut u32,
    msa_scm: Option<ScmResources>,
    prepared_session: Option<PreparedPrebootSession>,
    generation: u64,
    mut services: S,
    mut progress: impl FnMut(&'static str),
) -> Result<Runtime<S>, Error> {
    progress("VALIDATING CHANNEL CHECKPOINT");
    let (ready, _, old, _, report) = checkpoint.into_parts();
    if ready.generation() != generation {
        return Err(Error::GenerationMismatch);
    }
    if !report.passed() || report.words[0..3] != [40, 40, 8] {
        return Err(Error::Prelude);
    }
    if display_pixels.is_null() {
        return Err(Error::Mapping);
    }
    let mut bytes = [0; 88];
    bytes[..80].copy_from_slice(&old.bytes[..80]);
    bytes[80..].copy_from_slice(&[4, 0, 1, 0, 0, 0, 0, 0]);
    progress("VALIDATING OPEN PRELUDE");
    let prelude = Prelude::validate(bytes)?;
    progress("CHECKING SMEM INVENTORY");
    let baseline = window
        .locked(|memory| inventory::inspect_ref(memory, &ready))
        .map_err(|_| Error::Mapping)?;
    if baseline.items[0] != Some(DESCRIPTOR) || baseline.items[1] != Some(TX) {
        return Err(Error::Mapping);
    }
    let rx = baseline.items[2].ok_or(Error::Mapping)?;
    let layout = Layout::validate(
        (DESCRIPTOR.offset, DESCRIPTOR.size),
        (TX.offset, TX.size),
        (rx.offset, rx.size),
    )?;
    let (descriptor_offset, tx_offset, rx_offset) = layout.offsets();
    // Revalidate copied negotiation evidence before its sole tail publication.
    // Secondary OPEN records are retained above, not acknowledged or accepted.
    progress("REVALIDATING GLINK STATE");
    window
        .locked(|memory| {
            let current = inventory::inspect_ref(memory, &ready)?;
            if current.items != baseline.items
                || current.directory.route != baseline.directory.route
            {
                return Err(smem::Error::RouteChanged);
            }
            let at = DESCRIPTOR.offset;
            let read_word = |offset| {
                u32::from_le_bytes(::core::array::from_fn(|i| memory.read8(at + offset + i)))
            };
            if Prelude::validate_cursors(::core::array::from_fn(|index| read_word(index * 4)))
                .is_err()
            {
                return Err(smem::Error::Outbound);
            }
            for _ in 0..2 {
                for (i, byte) in prelude.retained_bytes().iter().enumerate() {
                    if memory.read8(rx.offset + 8 + i) != *byte {
                        return Err(smem::Error::Unstable);
                    }
                }
                sync();
            }
            Ok(())
        })
        .map_err(|_| Error::Prelude)?;
    let mut gate = LiveGate::new(window, ready, baseline);
    progress("CHECKING LIVE OWNER GATE");
    gate.observe().map_err(|_| Error::Mapping)?;
    // SAFETY: Layout bounded the descriptor to the exclusively mapped AP pages;
    // this consumes exactly the 88-byte, privately retained negotiation prefix.
    sync();
    unsafe {
        ap.add(descriptor_offset - PART + 8)
            .cast::<u32>()
            .write_volatile(96u32.to_le())
    };
    sync();
    if unsafe {
        ap.add(descriptor_offset - PART + 8)
            .cast::<u32>()
            .read_volatile()
    } != 96u32.to_le()
    {
        return Err(Error::Prelude);
    }
    progress("ATTACHING GLINK FIFO");
    gate.observe().map_err(|_| Error::Mapping)?;
    // SAFETY: layout and fixed symbol/manifest checks establish these exact,
    // disjoint retained mappings. Gate owns the sole directory-lock observer.
    let io = unsafe {
        NativeIo::new(
            Mappings {
                descriptor: ap.add(descriptor_offset - PART).cast(),
                tx: ap.add(tx_offset - PART),
                rx: smem_base.add(rx_offset),
                doorbell: doorbell.add(3),
            },
            gate,
        )
    }
    .map_err(|_| Error::Mapping)?;
    let preboot_staged = prepared_session.is_some();
    let mut session = match prepared_session {
        Some(prepared) => {
            prepared.validate(generation)?;
            prepared.session
        }
        None => Session::new(
            Config {
                local_node: 1,
                peer_node: 0,
                local_channel: 1,
                remote_channel: 2,
            },
            generation,
        )
        .map_err(|e| Error::Transport(e.into()))?,
    };
    #[cfg(not(feature = "sunfish-modem-servreg-state-probe"))]
    if !preboot_staged {
        session
            .register(generation, Service::RemoteStorage, 0x4000)
            .map_err(|_| Error::Mapping)?;
        session
            .register(generation, Service::HeapMemory, 0x4001)
            .map_err(|_| Error::Mapping)?;
    }
    let adapter = Adapter::attach(io, session, generation)?;
    progress("ATTACHING MODEM OWNER");
    let mut owner = Owner::attach(adapter, generation)?;
    #[cfg(all(
        feature = "sunfish-modem-servreg-state-probe",
        feature = "sunfish-modem-servreg-restart-probe"
    ))]
    let msa_reservation = {
        if msa_scm.is_some() {
            return Err(Error::Mapping);
        }
        Some(
            MsaReservation::validate_sunfish(generation, owner.identity())
                .map_err(|_| Error::Mapping)?,
        )
    };
    #[cfg(all(
        feature = "sunfish-modem-servreg-state-probe",
        not(feature = "sunfish-modem-servreg-restart-probe")
    ))]
    let msa_reservation = {
        if msa_scm.is_some() {
            return Err(Error::Mapping);
        }
        None
    };
    #[cfg(not(feature = "sunfish-modem-servreg-state-probe"))]
    let msa_reservation = Some(
        MsaReservation::validate_sunfish(generation, owner.identity())
            .map_err(|_| Error::Mapping)?,
    );
    #[cfg(not(feature = "sunfish-modem-servreg-state-probe"))]
    let msa_scm = Some(msa_scm.ok_or(Error::Mapping)?);
    // Keep the standalone SERVREG_NOTIF state probe out of WLFW startup. It
    // reserves one read-only query port and never enters WLAN MSA ownership.
    #[cfg(feature = "sunfish-modem-servreg-state-probe")]
    progress("REGISTERING SERVREG CLIENT");
    #[cfg(feature = "sunfish-modem-servreg-state-probe")]
    owner.enable_servreg_notif_client(SERVREG_NOTIF_CLIENT_PORT)?;
    // The restart profile also follows the normal WLFW discovery step. A
    // matching NEW_SERVER announcement may trigger only indication
    // registration; the state-probe event loop never advances to MSA_INFO.
    #[cfg(feature = "sunfish-modem-servreg-restart-probe")]
    owner.enable_wlfw_client(WLFW_CLIENT_PORT)?;
    #[cfg(not(feature = "sunfish-modem-servreg-state-probe"))]
    progress("REGISTERING WLFW CLIENT");
    #[cfg(not(feature = "sunfish-modem-servreg-state-probe"))]
    // Production cannot issue even the first WLFW request until a trusted
    // RuntimeServices implementation has applied the verified ICNSS power
    // plan and presents its generation-bound permit. The probe profile above
    // remains explicitly opt-in to the unpowered client model.
    owner.enable_wlfw_client_power_gated(WLFW_CLIENT_PORT)?;
    // Stage the map endpoint before the first QRTR HELLO exchange so it can
    // be announced before storage and heap services once the peer is ready.
    progress("PREPARING SERVREG MAP SERVICE");
    if !preboot_staged {
        owner.prepare_service_registry()?;
    }
    #[cfg(feature = "sunfish-modem-tftp-observer")]
    {
        progress("PREPARING TFTP OBSERVER");
        owner.prepare_tftp_observer(TFTP_OBSERVER_PORT)?;
    }
    progress("STARTING QRTR CONTROL");
    services.prepare_transport(&mut owner, &mut KernelInterrupts)?;
    owner.start_control()?;
    progress("POSTING RX INTENTS");
    owner.offer_receive()?;
    owner.offer_receive()?;
    progress("NATIVE OWNER READY");
    Ok(Runtime {
        owner,
        services,
        display_pixels,
        _retained_prelude: prelude,
        msa_reservation,
        msa_scm,
        msa_info_started: false,
        msa_handoff_started: false,
        capability_started: false,
        started: false,
        servreg_query_started: false,
        servreg_query_failed: false,
        servreg_restart_started: false,
        last_servreg_screen: None,
    })
}

impl<S: RuntimeServices> Runtime<S> {
    pub fn generation(&self) -> u64 {
        self.owner.generation()
    }

    pub fn control_status(&self) -> personal_os_rust_server::qcom_qrtr_control::Status {
        self.owner.control_status()
    }
    pub fn ipa_client_state(&self) -> Option<personal_os_rust_server::qcom_ipa_client::State> {
        self.owner.ipa_state()
    }
    pub fn ipa_client_outcome(
        &self,
    ) -> Option<personal_os_rust_server::qcom_ipa_client::ResponseOutcome> {
        self.owner.ipa_outcome()
    }
    /// Read the unique announced WLFW endpoint only after a fresh live SMEM
    /// lifecycle check; a failed observation permanently stops this owner.
    pub fn wlfw_endpoint(
        &mut self,
    ) -> Result<Option<personal_os_rust_server::qcom_qmi_router::Address>, Error> {
        self.owner.wlfw_endpoint()
    }
    #[cfg(feature = "sunfish-modem-servreg-state-probe")]
    fn begin_servreg_query_if_ready(&mut self) -> Result<bool, Error> {
        use personal_os_rust_server::qcom_servreg_notif_client::State;
        if self.servreg_query_started || self.owner.servreg_notif_state() != Some(State::Ready) {
            return Ok(false);
        }
        self.owner
            .begin_servreg_notif_query(&mut KernelInterrupts)?;
        self.servreg_query_started = true;
        Ok(true)
    }
    #[cfg(feature = "sunfish-modem-servreg-restart-probe")]
    fn begin_servreg_listener_if_ready(&mut self) -> Result<bool, Error> {
        use personal_os_rust_server::qcom_servreg_notif::ServiceState;
        use personal_os_rust_server::qcom_servreg_notif_client::State;
        let state = self.owner.servreg_notif_state();
        if !matches!(state, Some(State::Ready | State::Complete)) {
            return Ok(false);
        }
        if state == Some(State::Complete) {
            let Some(query) = self
                .owner
                .servreg_notif_outcome()
                .and_then(|outcome| outcome.query)
            else {
                return Ok(false);
            };
            if query.result != 0 || query.error != 0 || query.state != Some(ServiceState::Uninit) {
                return Ok(false);
            }
        }
        self.owner
            .begin_servreg_notif_listener(&mut KernelInterrupts)?;
        Ok(true)
    }

    #[cfg(feature = "sunfish-modem-servreg-restart-probe")]
    fn begin_servreg_restart_if_uninitialized(&mut self) -> Result<bool, Error> {
        use personal_os_rust_server::qcom_servreg_notif::ServiceState;
        use personal_os_rust_server::qcom_servreg_notif_client::State;
        if self.servreg_restart_started
            || self.owner.servreg_notif_state() != Some(State::ListenerComplete)
        {
            return Ok(false);
        }
        let Some(outcome) = self.owner.servreg_notif_outcome() else {
            return Ok(false);
        };
        let listener = outcome.listener;
        if outcome.current_result() != Some((0, 0))
            || outcome.current_state() != Some(ServiceState::Uninit)
            || !listener.is_some_and(|response| {
                response.result == 0
                    && response.error == 0
                    && response.state == Some(ServiceState::Uninit)
            })
        {
            return Ok(false);
        }
        // Latch before queuing: even a transport failure must never replay the
        // modem protection-domain restart request in this boot.
        self.servreg_restart_started = true;
        self.owner
            .begin_servreg_notif_restart_pd(&mut KernelInterrupts)?;
        Ok(true)
    }
    #[cfg(feature = "sunfish-modem-servreg-state-probe")]
    pub fn mark_servreg_query_failed(&mut self) {
        self.servreg_query_failed = true;
    }
    #[cfg(feature = "sunfish-modem-servreg-state-probe")]
    pub fn paint_servreg_query(&mut self, display: &Display) {
        use personal_os_rust_server::qcom_servreg_notif::ServiceState;
        use personal_os_rust_server::qcom_servreg_notif_client::{MAX_TRACE_BYTES, State};

        let state = self.owner.servreg_notif_state();
        let outcome = self.owner.servreg_notif_outcome();
        let control = self.owner.control_status();
        let completion = self.owner.last_completion_error();
        let credit_diagnostic = self.owner.last_credit_diagnostic();
        let owner_error = self.owner.error();
        let transport_diagnostic = self.owner.transport_diagnostic();
        let tftp_trace = self.owner.tftp_observer_trace();
        let map_trace = self.owner.service_registry_trace();
        let screen = (
            state,
            outcome,
            self.servreg_query_failed,
            control,
            completion,
            credit_diagnostic,
            owner_error,
            transport_diagnostic,
            tftp_trace,
            map_trace,
        );
        if self.last_servreg_screen == Some(screen) {
            return;
        }
        self.last_servreg_screen = Some(screen);
        display.clear();
        if let Some(diagnostic) = credit_diagnostic {
            let words = diagnostic.words;
            display.line(0, "QRTR RESUME DETAIL");
            display.line(1, "STOPPED BAD CREDIT");
            display.hex(2, "BAD FIELD MASK", diagnostic.bad_fields);
            display.hex(3, "PACKET BYTES", diagnostic.packet_len);
            display.hex(4, "QRTR VERSION", words[0]);
            display.hex(5, "QRTR TYPE", words[1]);
            display.hex(6, "SRC NODE", words[2]);
            display.hex(7, "SRC PORT", words[3]);
            display.hex(8, "CONFIRM", words[4]);
            display.hex(9, "BODY BYTES", words[5]);
            display.hex(10, "DST NODE", words[6]);
            display.hex(11, "DST PORT", words[7]);
            display.hex(12, "COMMAND", words[8]);
            display.hex(13, "FLOW NODE", words[9]);
            display.hex(14, "FLOW PORT", words[10]);
            display.hex(15, "PADDING 0", words[11]);
            display.hex(16, "PADDING 1", words[12]);
            display.hex(17, "KNOWN FLOW", u32::from(diagnostic.known_flow));
            display.hex(18, "REPLY STATE", diagnostic.reply_state);
            display.hex(20, "QRTR SERVERS", control.servers as u32);
            display.hex(21, "PEER HELLO", u32::from(control.peer_hello));
            display.hex(22, "HELLO SENT", u32::from(control.hello_sent));
            display.line(23, "NO RX CONFIRM");
            display.line(24, "NO PDR OR MSA");
            display.line(25, "NO WLAN DMA OR FW");
            display.line(29, "RESTART WHEN DONE");
            display.flush();
            return;
        }
        if let Some(rejected) = outcome.filter(|value| {
            value
                .listener
                .is_some_and(|response| response.result != 0 || response.error != 0)
        }) {
            let trace = rejected.trace;
            let listener = rejected.listener.unwrap();
            let trace_word = |bytes: &[u8; MAX_TRACE_BYTES], offset: usize| {
                let mut word = [0; 4];
                for (index, byte) in word.iter_mut().enumerate() {
                    *byte = bytes.get(offset + index).copied().unwrap_or(0);
                }
                u32::from_le_bytes(word)
            };
            display.clear();
            display.line(0, "SERVREG LISTENER REJECT");
            display.hex(1, "QMI RESULT", u32::from(listener.result));
            display.hex(2, "QMI ERROR", u32::from(listener.error));
            display.hex(3, "SERVER NODE", trace.destination.node);
            display.hex(4, "SERVER PORT", trace.destination.port);
            display.hex(5, "LOCAL NODE", trace.source.node);
            display.hex(6, "LOCAL PORT", trace.source.port);
            display.hex(7, "QMI TXN", u32::from(trace.transaction));
            display.hex(8, "QMI MSG ID", u32::from(trace.message_id));
            display.hex(9, "QMI TX BYTES", u32::from(trace.request_len));
            display.line(10, "QMI TX WORDS LE");
            for index in 0..8 {
                display.hex(
                    10 + index,
                    match index {
                        0 => "TX WORD 0",
                        1 => "TX WORD 1",
                        2 => "TX WORD 2",
                        3 => "TX WORD 3",
                        4 => "TX WORD 4",
                        5 => "TX WORD 5",
                        6 => "TX WORD 6",
                        _ => "TX WORD 7",
                    },
                    trace_word(&trace.request, index * 4),
                );
            }
            display.hex(18, "QMI RX BYTES", u32::from(trace.response_len));
            for index in 0..8 {
                display.hex(
                    19 + index,
                    match index {
                        0 => "RX WORD 0",
                        1 => "RX WORD 1",
                        2 => "RX WORD 2",
                        3 => "RX WORD 3",
                        4 => "RX WORD 4",
                        5 => "RX WORD 5",
                        6 => "RX WORD 6",
                        _ => "RX WORD 7",
                    },
                    trace_word(&trace.response, index * 4),
                );
            }
            display.line(27, servreg_stop_reason(owner_error, transport_diagnostic));
            display.line(28, "NO PDR OR WLAN START");
            display.line(29, "RESTART WHEN DONE");
            display.flush();
            return;
        }
        #[cfg(feature = "sunfish-modem-tftp-observer")]
        {
            let control = self.owner.control_status();
            display.line(0, "QRTR TFTP OBSERVER");
            display.line(
                1,
                if self.servreg_query_failed {
                    if outcome
                        .and_then(|value| value.current_result())
                        .is_some_and(|(result, error)| result == 0 && error == 0)
                    {
                        "OWNER STOPPED POST QMI"
                    } else {
                        "SERVREG QUERY STOPPED"
                    }
                } else if let Some((result, error)) =
                    outcome.and_then(|value| value.current_result())
                {
                    if result != 0 || error != 0 {
                        "SERVREG QMI ERROR"
                    } else {
                        match outcome.and_then(|value| value.current_state()) {
                            Some(ServiceState::Down) => "SERVREG FW DOWN",
                            Some(ServiceState::Up) => "SERVREG FW UP",
                            Some(ServiceState::EarlyDown) => "SERVREG FW EARLY DOWN",
                            Some(ServiceState::Uninit) => "SERVREG FW UNINIT",
                            Some(ServiceState::Unknown(_)) => "SERVREG FW UNKNOWN",
                            None => "SERVREG STATE MISSING",
                        }
                    }
                } else {
                    match state {
                        Some(State::Ready) => "SERVREG SERVER READY",
                        Some(State::Pending) => "SERVREG QUERY PENDING",
                        Some(State::Complete) => "SERVREG QUERY NO RESULT",
                        _ => "SERVREG WAITING",
                    }
                },
            );
            let mut ipa = None;
            display.hex(3, "QRTR SERVERS", control.servers as u32);
            display.hex(4, "PEER HELLO", u32::from(control.peer_hello));
            let mut wlfw = None;
            for index in 0..personal_os_rust_server::qcom_qrtr_control::MAX_SERVERS {
                if let Some(server) = self.owner.remote_server(index) {
                    if server.service == personal_os_rust_server::qcom_ipa::QRTR_SERVICE_ID
                        && server.instance
                            == personal_os_rust_server::qcom_ipa::QRTR_VERSION_INSTANCE
                    {
                        ipa = Some(server);
                    }
                    if server.service == personal_os_rust_server::qcom_qrtr_control::WLFW_SERVICE_ID
                        && server.instance
                            == personal_os_rust_server::qcom_qrtr_control::WLFW_VERSION_INSTANCE
                    {
                        wlfw = Some(server);
                    }
                }
            }
            let service_flags = u32::from(control.announced_services & (1 << 3) != 0)
                | (u32::from(ipa.is_some()) << 1)
                | (u32::from(wlfw.is_some()) << 2);
            // Bit 0: local TFTP advertised; bit 1: modem IPA server found;
            // bit 2: modem WLFW server found.
            display.hex(2, "QRTR SVC FLAGS", service_flags);
            display.hex(
                5,
                "IPA PORT",
                ipa.map_or(u32::MAX, |server| server.address.port),
            );
            display.hex(6, "WLFW SERVER", u32::from(wlfw.is_some()));
            display.hex(
                7,
                "WLFW PORT",
                wlfw.map_or(u32::MAX, |server| server.address.port),
            );
            display.hex(8, "TFTP PACKETS", tftp_trace.packets);
            display.hex(9, "TFTP RRQ", tftp_trace.read_requests);
            display.hex(
                10,
                "PEER NODE",
                tftp_trace.peer.map_or(u32::MAX, |peer| peer.node),
            );
            display.hex(
                11,
                "PEER PORT",
                tftp_trace.peer.map_or(u32::MAX, |peer| peer.port),
            );
            display.hex(
                12,
                "TFTP OPCODE",
                tftp_trace
                    .last
                    .map_or(u32::MAX, |observation| u32::from(observation.opcode)),
            );
            if tftp_trace.last.is_none() && tftp_trace.parse_error.is_none() {
                display.line(
                    13,
                    match state {
                        Some(State::Searching) => "CLIENT SEARCHING",
                        Some(State::Ready) => "CLIENT SERVER READY",
                        Some(State::Pending) => "CLIENT QUERY PENDING",
                        Some(State::Complete) => "CLIENT COMPLETE",
                        Some(State::ListenerPending) => "CLIENT LISTENER PENDING",
                        Some(State::ListenerComplete) => "CLIENT LISTENER READY",
                        Some(State::RestartPending) => "CLIENT PDR PENDING",
                        Some(State::RestartComplete) => "CLIENT PDR COMPLETE",
                        None => "CLIENT NOT CREATED",
                    },
                );
                let query_result = outcome.and_then(|value| value.current_result());
                display.hex(
                    14,
                    "QMI RESULT",
                    query_result.map_or(u32::MAX, |value| u32::from(value.0)),
                );
                display.hex(
                    15,
                    "QMI ERROR",
                    query_result.map_or(u32::MAX, |value| u32::from(value.1)),
                );
                display.hex(
                    16,
                    "FW STATE",
                    match outcome.and_then(|value| value.current_state()) {
                        Some(ServiceState::Down) => 0,
                        Some(ServiceState::Up) => 1,
                        Some(ServiceState::EarlyDown) => 2,
                        Some(ServiceState::Uninit) => 0x7fff_ffff,
                        Some(ServiceState::Unknown(value)) => value as u32,
                        None => u32::MAX,
                    },
                );
                display.line(17, "OWNER STOP REASON");
                display.line(18, servreg_stop_reason(owner_error, transport_diagnostic));
                display.hex(19, "TRANS DETAIL", transport_diagnostic);
                display.hex(20, "QUERY STARTED", u32::from(self.servreg_query_started));
                for index in 0..2 {
                    let server = self.owner.remote_server(index);
                    display.hex(
                        21 + index * 2,
                        if index == 0 {
                            "SERVER 0 SVC"
                        } else {
                            "SERVER 1 SVC"
                        },
                        server.map_or(u32::MAX, |value| value.service),
                    );
                    display.hex(
                        22 + index * 2,
                        if index == 0 {
                            "SERVER 0 INST"
                        } else {
                            "SERVER 1 INST"
                        },
                        server.map_or(u32::MAX, |value| value.instance),
                    );
                }
                display.line(25, "WAITING FOR TFTP RRQ");
            } else if let Some(observation) = tftp_trace.last {
                for index in 0..4 {
                    display.line(13 + index, observation.path_part(index));
                }
                display.line(17, observation.mode_text());
                display.hex(
                    18,
                    "TSIZE QUERY",
                    observation
                        .options
                        .map_or(u32::MAX, |options| u32::from(options.request_file_size)),
                );
                display.hex(
                    19,
                    "BLKSIZE",
                    observation
                        .options
                        .map_or(u32::MAX, |options| u32::from(options.block_size)),
                );
                display.hex(
                    20,
                    "TIMEOUT MS",
                    observation
                        .options
                        .map_or(u32::MAX, |options| options.timeout_ms),
                );
                display.hex(
                    21,
                    "RSIZE",
                    observation
                        .options
                        .and_then(|options| options.read_size)
                        .unwrap_or(u32::MAX),
                );
                display.hex(
                    22,
                    "WSIZE",
                    observation
                        .options
                        .map_or(u32::MAX, |options| u32::from(options.window_size)),
                );
                display.hex(
                    23,
                    "SEEK LOW",
                    observation
                        .options
                        .map_or(u32::MAX, |options| options.seek as u32),
                );
                display.hex(
                    24,
                    "SEEK HIGH",
                    observation
                        .options
                        .map_or(u32::MAX, |options| (options.seek >> 32) as u32),
                );
                display.line(
                    25,
                    match observation.error {
                        None => "REQUEST PARSED",
                        Some(personal_os_rust_server::qcom_tftp::Error::UnavailablePath) => {
                            "PATH NOT ALLOWED"
                        }
                        Some(personal_os_rust_server::qcom_tftp::Error::WriteDenied) => {
                            "WRITE DENIED"
                        }
                        Some(_) => "REQUEST REJECTED",
                    },
                );
            } else if let Some(error) = tftp_trace.parse_error {
                display.line(
                    25,
                    match error {
                        personal_os_rust_server::qcom_tftp::Error::PacketTooLarge => {
                            "REQUEST TOO LARGE"
                        }
                        _ => "REQUEST MALFORMED",
                    },
                );
            } else {
                display.line(25, "WAITING FOR TFTP RRQ");
            }
            display.line(26, "NO TFTP RESPONSE SENT");
            display.line(27, "NO FIRMWARE BYTES SERVED");
            display.line(28, "NO PDR OR WLAN DMA");
            display.line(29, "RESTART WHEN DONE");
            display.flush();
            return;
        }
        display.line(0, "SERVREG WLAN FW STATE");
        display.line(
            1,
            if self.servreg_query_failed {
                "QUERY STOPPED"
            } else {
                match state {
                    Some(State::Searching) => "WAITING FOR SERVER",
                    Some(State::Ready) => "SERVER READY",
                    Some(State::Pending) => "QUERY PUBLISHED",
                    Some(State::Complete) => "QUERY COMPLETE",
                    Some(State::ListenerPending) => "LISTENER PUBLISHED",
                    Some(State::ListenerComplete) => "LISTENER COMPLETE",
                    Some(State::AckPending) => "SET ACK PENDING",
                    Some(State::AckFailed) => "SET ACK FAILED",
                    Some(State::RestartPending) => "PDR REQUEST PUBLISHED",
                    Some(State::RestartComplete) => "PDR RESPONSE COMPLETE",
                    None => "CLIENT NOT READY",
                }
            },
        );
        display.hex(
            2,
            "QMI RESULT",
            outcome
                .and_then(|value| value.current_result())
                .map_or(u32::MAX, |value| u32::from(value.0)),
        );
        display.hex(
            3,
            "QMI ERROR",
            outcome
                .and_then(|value| value.current_result())
                .map_or(u32::MAX, |value| u32::from(value.1)),
        );
        let service_state = outcome.and_then(|value| value.current_state());
        display.hex(
            4,
            if cfg!(feature = "sunfish-modem-servreg-restart-probe") {
                "LISTENER STATE"
            } else {
                "FW STATE"
            },
            match service_state {
                Some(ServiceState::Down) => 0,
                Some(ServiceState::Up) => 1,
                Some(ServiceState::EarlyDown) => 2,
                Some(ServiceState::Uninit) => 0x7fff_ffff,
                Some(ServiceState::Unknown(value)) => value as u32,
                None => u32::MAX,
            },
        );
        display.line(
            5,
            match service_state {
                Some(ServiceState::Down) => "SERVICE DOWN",
                Some(ServiceState::Up) => "SERVICE UP",
                Some(ServiceState::EarlyDown) => "SERVICE EARLY DOWN",
                Some(ServiceState::Uninit) => "SERVICE UNINIT",
                Some(ServiceState::Unknown(_)) => "SERVICE UNKNOWN",
                None if outcome.is_some() => "STATE NOT RETURNED",
                None => "NO RESPONSE YET",
            },
        );
        #[cfg(all(
            feature = "sunfish-modem-servreg-state-probe",
            not(feature = "sunfish-modem-servreg-restart-probe")
        ))]
        {
            use personal_os_rust_server::qcom_servreg;
            display.hex(
                6,
                "WAIT PDR ENABLE",
                map_trace.reply_wait_name_counts[qcom_servreg::NAME_COUNTER_TMS_PDR_ENABLED],
            );
            display.hex(
                7,
                "WAIT PDDUMP OFF",
                map_trace.reply_wait_name_counts[qcom_servreg::NAME_COUNTER_TMS_PDDUMP_DISABLED],
            );
            display.hex(
                8,
                "WAIT OTHER NAME",
                map_trace.reply_wait_name_counts[qcom_servreg::NAME_COUNTER_OTHER],
            );
        }
        #[cfg(any(
            not(feature = "sunfish-modem-servreg-state-probe"),
            feature = "sunfish-modem-servreg-restart-probe"
        ))]
        {
            display.hex(
                6,
                "PDR RESULT",
                outcome
                    .and_then(|value| value.restart)
                    .map_or(u32::MAX, |value| u32::from(value.result)),
            );
            display.hex(
                7,
                "PDR ERROR",
                outcome
                    .and_then(|value| value.restart)
                    .map_or(u32::MAX, |value| u32::from(value.error)),
            );
            display.line(
                8,
                match state {
                    Some(State::RestartPending) => "PDR REQUEST IN FLIGHT",
                    Some(State::RestartComplete) => "PDR RESPONSE RECEIVED",
                    _ if self.servreg_restart_started => "PDR REQUEST QUEUED",
                    _ => "PDR REQUEST NOT SENT",
                },
            );
        }
        #[cfg(all(
            feature = "sunfish-modem-servreg-state-probe",
            not(feature = "sunfish-modem-servreg-restart-probe")
        ))]
        display.hex(9, "FIRST RSP NAME", map_trace.first_reply_name_code);
        #[cfg(any(
            not(feature = "sunfish-modem-servreg-state-probe"),
            feature = "sunfish-modem-servreg-restart-probe"
        ))]
        if matches!(owner_error, Some(Error::Transport(fifo::Error::Access))) {
            display.hex(9, "LOCAL ERROR CODE", transport_diagnostic);
        }
        display.hex(10, "QRTR SERVERS", control.servers as u32);
        display.hex(11, "PEER HELLO", u32::from(control.peer_hello));
        display.hex(12, "HELLO SENT", u32::from(control.hello_sent));
        display.line(13, "STOP REASON");
        display.line(14, servreg_stop_reason(owner_error, transport_diagnostic));
        display.line(
            15,
            match state {
                Some(State::Searching) => "CLIENT SEARCHING",
                Some(State::Ready) => "CLIENT SERVER READY",
                Some(State::Pending) => "CLIENT QUERY PENDING",
                Some(State::Complete) => "CLIENT COMPLETE",
                Some(State::ListenerPending) => "CLIENT LISTENER PENDING",
                Some(State::ListenerComplete) => "CLIENT LISTENER READY",
                Some(State::AckPending) => "CLIENT SET ACK PENDING",
                Some(State::AckFailed) => "CLIENT ACK FAILED",
                Some(State::RestartPending) => "CLIENT PDR PENDING",
                Some(State::RestartComplete) => "CLIENT PDR COMPLETE",
                None => "CLIENT NOT CREATED",
            },
        );
        if let Some(completion) = completion {
            debug_println!(
                "MODEM_OWNER|completion intent={} reuse={} busy={} partial={} sent={} failure={:?}",
                completion.intent_id,
                completion.reuse,
                completion.busy_remote_intents,
                completion.incomplete_transmits,
                completion.completed_transmits,
                completion.failure
            );
        }
        display.line(
            16,
            if self.owner.wlfw_firmware_ready_seen() {
                "WLFW FW READY"
            } else {
                match self.owner.wlfw_state() {
                    Some(WlfwState::Searching) => "WLFW SEARCHING",
                    Some(WlfwState::ReadyToRegister) => "WLFW SERVER FOUND",
                    Some(WlfwState::RegistrationPending) => "WLFW REG PENDING",
                    Some(WlfwState::IcnssAlreadyRegistered) => "WLFW ICNSS READY",
                    Some(WlfwState::MsaInfoReady) => "WLFW REGISTERED",
                    Some(WlfwState::MsaInfoPending) => "WLFW MSA INFO PENDING",
                    Some(WlfwState::MsaOwnershipRequired) => "WLFW MSA REQUIRED",
                    Some(WlfwState::MsaReadyPending) => "WLFW MSA READY PENDING",
                    Some(WlfwState::CapabilityReady) => "WLFW CAPABILITY READY",
                    Some(WlfwState::CapabilityPending) => "WLFW CAPABILITY PENDING",
                    Some(WlfwState::CapabilitiesAccepted) => "WLFW CAPABILITIES OK",
                    Some(WlfwState::BdfPending) => "WLFW BDF PENDING",
                    Some(WlfwState::BdfReady) => "WLFW BDF READY",
                    Some(WlfwState::BdfComplete) => "WLFW BDF COMPLETE",
                    Some(WlfwState::CalReportPending) => "WLFW CAL PENDING",
                    Some(WlfwState::CalReportAccepted) => "WLFW CAL COMPLETE",
                    Some(WlfwState::DynamicFeatureMaskPending) => "WLFW FEATURE PENDING",
                    Some(WlfwState::WlanConfigPending) => "WLFW CONFIG PENDING",
                    Some(WlfwState::WlanModeReady) => "WLFW MODE READY",
                    Some(WlfwState::WlanModePending) => "WLFW MODE PENDING",
                    Some(WlfwState::WlanModeAccepted) => "WLFW MODE ACCEPTED",
                    Some(WlfwState::Failed) => "WLFW CLIENT FAILED",
                    None => "WLFW CLIENT DISABLED",
                }
            },
        );
        #[cfg(any(
            not(feature = "sunfish-modem-servreg-state-probe"),
            feature = "sunfish-modem-servreg-restart-probe"
        ))]
        let registration = self.owner.wlfw_outcome();
        #[cfg(all(
            feature = "sunfish-modem-servreg-state-probe",
            not(feature = "sunfish-modem-servreg-restart-probe")
        ))]
        display.hex(
            17,
            "QRTR RESUME",
            u32::from(map_trace.first_reply_qrtr_resume),
        );
        #[cfg(any(
            not(feature = "sunfish-modem-servreg-state-probe"),
            feature = "sunfish-modem-servreg-restart-probe"
        ))]
        display.hex(
            17,
            "WLFW ERR:RESULT",
            registration.map_or(u32::MAX, |value| match value {
                WlfwRegistrationOutcome::Accepted { .. } => 0,
                WlfwRegistrationOutcome::Rejected { result, error, .. } => {
                    (u32::from(error) << 16) | u32::from(result)
                }
            }),
        );
        #[cfg(any(
            not(feature = "sunfish-modem-servreg-state-probe"),
            feature = "sunfish-modem-servreg-restart-probe"
        ))]
        let msa_info = self.owner.wlfw_msa_info_outcome();
        #[cfg(all(
            feature = "sunfish-modem-servreg-state-probe",
            not(feature = "sunfish-modem-servreg-restart-probe")
        ))]
        {
            display.hex(18, "LOC DOMAINS", map_trace.last_total_domains);
            display.line(19, "READ ONLY; NO PDR OR WLAN");
        }
        #[cfg(any(
            not(feature = "sunfish-modem-servreg-state-probe"),
            feature = "sunfish-modem-servreg-restart-probe"
        ))]
        display.hex(
            18,
            "MSA ERR:RESULT",
            msa_info.map_or(u32::MAX, |value| {
                (u32::from(value.error) << 16) | u32::from(value.result)
            }),
        );
        #[cfg(any(
            not(feature = "sunfish-modem-servreg-state-probe"),
            feature = "sunfish-modem-servreg-restart-probe"
        ))]
        display.hex(
            19,
            "MSA REGION COUNT",
            msa_info.map_or(u32::MAX, |value| value.region_count as u32),
        );
        #[cfg(all(
            feature = "sunfish-modem-servreg-state-probe",
            not(feature = "sunfish-modem-servreg-restart-probe")
        ))]
        {
            use personal_os_rust_server::qcom_servreg;
            display.hex(
                25,
                "LOC WLAN FW",
                map_trace.name_counts[qcom_servreg::NAME_COUNTER_WLAN_FW],
            );
            display.hex(
                26,
                "LOC PDR ENABLE",
                map_trace.name_counts[qcom_servreg::NAME_COUNTER_TMS_PDR_ENABLED],
            );
            display.hex(
                27,
                "LOC PDDUMP OFF",
                map_trace.name_counts[qcom_servreg::NAME_COUNTER_TMS_PDDUMP_DISABLED],
            );
            let wait_count = (map_trace.reply_flow_busy.min(u16::MAX as u32) << 16)
                | map_trace.reply_no_capacity.min(u16::MAX as u32);
            display.hex(28, "LOC WAIT B/C", wait_count);
        }
        #[cfg(any(
            not(feature = "sunfish-modem-servreg-state-probe"),
            feature = "sunfish-modem-servreg-restart-probe"
        ))]
        for (index, service_row, instance_row) in [(0, 25, 26), (1, 27, 28)] {
            let server = self.owner.remote_server(index);
            display.hex(
                service_row,
                if index == 0 {
                    "S0 SERVICE"
                } else {
                    "S1 SERVICE"
                },
                server.map_or(u32::MAX, |value| value.service),
            );
            display.hex(
                instance_row,
                if index == 0 {
                    "S0 INSTANCE"
                } else {
                    "S1 INSTANCE"
                },
                server.map_or(u32::MAX, |value| value.instance),
            );
        }
        #[cfg(not(feature = "sunfish-modem-servreg-state-probe"))]
        for (line, label, region_index, address) in [
            (25, "MSA REGION0 BASE", 0, true),
            (26, "MSA REGION0 SIZE", 0, false),
            (27, "MSA REGION1 BASE", 1, true),
            (28, "MSA REGION1 SIZE", 1, false),
        ] {
            let value = msa_info
                .filter(|outcome| outcome.region_count > region_index)
                .map_or(u32::MAX, |outcome| {
                    let region = outcome.regions[region_index];
                    if address {
                        u32::try_from(region.address).unwrap_or(u32::MAX)
                    } else {
                        region.size
                    }
                });
            display.hex(line, label, value);
        }
        display.hex(20, "LOC REQUESTS", map_trace.requests);
        display.hex(21, "LOC RESP QUEUED", map_trace.replies_queued);
        #[cfg(all(
            feature = "sunfish-modem-servreg-state-probe",
            not(feature = "sunfish-modem-servreg-restart-probe")
        ))]
        {
            display.line(22, "FIRST LOC NAME");
            let (name_first, _) = map_trace.first_service_name_parts();
            display.line(23, name_first);
            display.hex(
                24,
                "FIRST RXDONE",
                u32::from(map_trace.first_reply_glink_rx_done),
            );
        }
        #[cfg(any(
            not(feature = "sunfish-modem-servreg-state-probe"),
            feature = "sunfish-modem-servreg-restart-probe"
        ))]
        {
            display.hex(22, "LOC NAME CODE", map_trace.last_name_code);
            display.hex(23, "LOC DOMAINS", map_trace.last_total_domains);
            display.line(
                24,
                if cfg!(feature = "sunfish-modem-servreg-restart-probe") {
                    "ONE PDR MAX; NO WLAN DMA"
                } else {
                    "READ ONLY; NO PDR OR WLAN"
                },
            );
        }
        display.line(29, "RESTART WHEN DONE");
        display.flush();
    }
    fn begin_msa_info_if_ready(&mut self) -> Result<bool, Error> {
        if self.msa_info_started
            || !self.services.memory_services_ready()
            || !self.services.ipa_modem_ready()
            || self.owner.wlfw_state() != Some(WlfwState::MsaInfoReady)
        {
            return Ok(false);
        }
        #[cfg(feature = "sunfish-modem-servreg-restart-probe")]
        {
            use personal_os_rust_server::qcom_servreg_notif_client::State as ServregState;
            let pdr_succeeded = self.servreg_restart_started
                && !self.servreg_query_failed
                && self.owner.servreg_notif_state() == Some(ServregState::RestartComplete)
                && self
                    .owner
                    .servreg_notif_outcome()
                    .is_some_and(|outcome| outcome.restart_succeeded());
            if !pdr_succeeded {
                return Ok(false);
            }
        }
        self.owner.begin_wlfw_msa_info(
            self.msa_reservation.ok_or(Error::Mapping)?,
            &mut KernelInterrupts,
        )?;
        self.msa_info_started = true;
        Ok(true)
    }
    fn execute_msa_handoff(&mut self, plan: HandoffPlan) -> Result<MsaReadyPermit, Error> {
        let reservation = self.msa_reservation.ok_or(Error::Mapping)?;
        let scm = self.msa_scm.as_ref().ok_or(Error::Mapping)?;
        let io = qcom_msa_native::Native::new(
            plan,
            reservation,
            OwnerLifecycle(&mut self.owner),
            OwnerScm(scm),
            OwnerScratch(scm),
        )
        .map_err(|_| Error::Stopped)?;
        let mut handoff = Handoff::new(plan, io).map_err(|_| Error::Stopped)?;
        for _ in 0..2 {
            handoff.step().map_err(|_| Error::Stopped)?;
        }
        handoff.into_msa_ready_permit().map_err(|_| Error::Stopped)
    }
    fn begin_msa_handoff_if_ready(&mut self) -> Result<bool, Error> {
        if self.msa_handoff_started || !self.services.memory_services_ready() {
            return Ok(false);
        }
        let Some(outcome) = self.owner.wlfw_msa_info_outcome() else {
            return Ok(false);
        };
        if outcome.result != 0 || outcome.error != 0 {
            self.owner.fault();
            return Err(Error::Stopped);
        }
        if self.owner.wlfw_state() != Some(WlfwState::MsaOwnershipRequired) {
            return Ok(false);
        }
        let Some(plan) = outcome.handoff_plan else {
            self.owner.fault();
            return Err(Error::Stopped);
        };
        // Latch before the first SCM boundary. Every failure is terminal and
        // the exact fixed handoff must never be replayed in this boot.
        self.msa_handoff_started = true;
        let mut permit = match self.execute_msa_handoff(plan) {
            Ok(permit) => permit,
            Err(error) => {
                self.owner.fault();
                return Err(error);
            }
        };
        if let Err(error) = self
            .owner
            .begin_wlfw_msa_ready(&mut permit, &mut KernelInterrupts)
        {
            self.owner.fault();
            return Err(error);
        }
        Ok(true)
    }
    fn begin_capability_if_ready(&mut self) -> Result<bool, Error> {
        if self.capability_started
            || !self.services.memory_services_ready()
            || self.owner.wlfw_state() != Some(WlfwState::CapabilityReady)
        {
            return Ok(false);
        }
        self.owner
            .begin_wlfw_capability_request(&mut KernelInterrupts)?;
        self.capability_started = true;
        Ok(true)
    }
    /// The active PD uses a custom event loop without Microkit's unconditional
    /// startup ACK. Treat the initial unmask exactly like pending IRQs: drain
    /// bounded work and validate lifecycle before arming either capability.
    pub fn start(&mut self) -> Result<(), Error> {
        if self.started {
            self.owner.fault();
            return Err(Error::Stopped);
        }
        self.started = true;
        self.services
            .activate(&mut self.owner, &mut KernelInterrupts)?;
        self.services
            .notified(
                &mut self.owner,
                (1 << core::CONTINUE_RECEIVE)
                    | (1 << core::GLINK_CHANNEL)
                    | (1 << core::STATUS_CHANNEL),
                &mut KernelInterrupts,
            )
            .map_err(|_| Error::Stopped)?;
        self.services
            .wlan_startup_step(&mut self.owner, &mut KernelInterrupts)?;
        self.owner
            .dispatch_service_registry_step(&mut KernelInterrupts)?;
        #[cfg(feature = "sunfish-modem-servreg-state-probe")]
        {
            #[cfg(feature = "sunfish-modem-servreg-restart-probe")]
            {
                self.begin_servreg_listener_if_ready()?;
                self.begin_servreg_restart_if_uninitialized()?;
                self.begin_msa_info_if_ready()?;
            }
            #[cfg(not(feature = "sunfish-modem-servreg-restart-probe"))]
            self.begin_servreg_query_if_ready()?;
            return Ok(());
        }
        #[cfg(not(feature = "sunfish-modem-servreg-state-probe"))]
        {
            self.begin_msa_info_if_ready()?;
            self.begin_msa_handoff_if_ready()?;
            self.begin_capability_if_ready().map(|_| ())
        }
    }
    pub fn notified(&mut self, channels: ChannelSet) {
        let bits = channels
            .iter()
            .fold(0u64, |bits, channel| bits | (1u64 << channel.index()));
        let result = self
            .services
            .notified(&mut self.owner, bits, &mut KernelInterrupts)
            .and_then(|progress| {
                self.services
                    .wlan_startup_step(&mut self.owner, &mut KernelInterrupts)?;
                self.owner
                    .dispatch_service_registry_step(&mut KernelInterrupts)?;
                Ok(progress)
            });
        #[cfg(all(
            feature = "sunfish-modem-servreg-state-probe",
            not(feature = "sunfish-modem-servreg-restart-probe")
        ))]
        let query = if result.is_ok() {
            self.begin_servreg_query_if_ready()
        } else {
            Ok(false)
        };
        #[cfg(all(
            feature = "sunfish-modem-servreg-state-probe",
            feature = "sunfish-modem-servreg-restart-probe"
        ))]
        let query: Result<bool, Error> = Ok(false);
        #[cfg(not(feature = "sunfish-modem-servreg-state-probe"))]
        let query: Result<bool, Error> = Ok(false);
        #[cfg(feature = "sunfish-modem-servreg-restart-probe")]
        let pdr_listener = if result.is_ok() && query.is_ok() {
            self.begin_servreg_listener_if_ready()
        } else {
            Ok(false)
        };
        #[cfg(not(feature = "sunfish-modem-servreg-restart-probe"))]
        let pdr_listener: Result<bool, Error> = Ok(false);
        #[cfg(feature = "sunfish-modem-servreg-restart-probe")]
        let pdr_restart = if result.is_ok() && query.is_ok() && pdr_listener.is_ok() {
            self.begin_servreg_restart_if_uninitialized()
        } else {
            Ok(false)
        };
        #[cfg(not(feature = "sunfish-modem-servreg-restart-probe"))]
        let pdr_restart: Result<bool, Error> = Ok(false);
        #[cfg(all(
            feature = "sunfish-modem-servreg-state-probe",
            feature = "sunfish-modem-servreg-restart-probe"
        ))]
        let msa_info = if result.is_ok() && pdr_restart.is_ok() {
            self.begin_msa_info_if_ready()
        } else {
            Ok(false)
        };
        #[cfg(all(
            feature = "sunfish-modem-servreg-state-probe",
            not(feature = "sunfish-modem-servreg-restart-probe")
        ))]
        let msa_info: Result<bool, Error> = Ok(false);
        #[cfg(not(feature = "sunfish-modem-servreg-state-probe"))]
        let msa_info = if result.is_ok() {
            self.begin_msa_info_if_ready()
        } else {
            Ok(false)
        };
        #[cfg(feature = "sunfish-modem-servreg-state-probe")]
        let msa_handoff: Result<bool, Error> = Ok(false);
        #[cfg(not(feature = "sunfish-modem-servreg-state-probe"))]
        let msa_handoff = if result.is_ok() && msa_info.is_ok() {
            self.begin_msa_handoff_if_ready()
        } else {
            Ok(false)
        };
        #[cfg(feature = "sunfish-modem-servreg-state-probe")]
        let capability: Result<bool, Error> = Ok(false);
        #[cfg(not(feature = "sunfish-modem-servreg-state-probe"))]
        let capability = if result.is_ok() && msa_handoff.is_ok() {
            self.begin_capability_if_ready()
        } else {
            Ok(false)
        };
        #[cfg(feature = "sunfish-modem-servreg-state-probe")]
        let wlfw: Result<Option<personal_os_rust_server::qcom_qmi_router::Address>, Error> =
            Ok(None);
        #[cfg(not(feature = "sunfish-modem-servreg-state-probe"))]
        let wlfw = match result.as_ref() {
            Ok(progress) if progress.controls != 0 => self.owner.wlfw_endpoint(),
            _ => Ok(None),
        };
        #[cfg(feature = "sunfish-modem-servreg-state-probe")]
        if query.is_err() || pdr_listener.is_err() || pdr_restart.is_err() || result.is_err() {
            self.servreg_query_failed = true;
        }
        let control = self.control_status();
        match (
            result,
            query,
            pdr_listener,
            pdr_restart,
            msa_info,
            msa_handoff,
            capability,
            wlfw,
        ) {
            (
                Ok(progress),
                Ok(query_started),
                Ok(pdr_listener_started),
                Ok(pdr_restart_started),
                Ok(msa_started),
                Ok(msa_handoff),
                Ok(capability),
                Ok(wlfw),
            ) => {
                debug_println!(
                    "MODEM_OWNER|rx={} tx={} requests={} controls={} deferred={} peer_hello={} announced={} servers={} wlfw={:?} query_started={} listener_started={} pdr_restart_started={} query_state={:?} query_outcome={:?} msa_info_started={} msa_handoff_started={} capability_started={}",
                    progress.received,
                    progress.published,
                    progress.requests,
                    progress.controls,
                    progress.deferred,
                    control.peer_hello,
                    control.announced_services,
                    control.servers,
                    wlfw,
                    query_started || self.servreg_query_started,
                    pdr_listener_started,
                    pdr_restart_started || self.servreg_restart_started,
                    self.owner.servreg_notif_state(),
                    self.owner.servreg_notif_outcome(),
                    self.msa_info_started || msa_started,
                    self.msa_handoff_started || msa_handoff,
                    self.capability_started || capability
                )
            }
            (Ok(_), Err(error), _, _, _, _, _, _)
            | (Ok(_), _, Err(error), _, _, _, _, _)
            | (Ok(_), _, _, Err(error), _, _, _, _)
            | (Ok(_), _, _, _, Err(error), _, _, _)
            | (Ok(_), _, _, _, _, Err(error), _, _)
            | (Ok(_), _, _, _, _, _, Err(error), _)
            | (Ok(_), _, _, _, _, _, _, Err(error))
            | (Err(error), _, _, _, _, _, _, _) => debug_println!(
                "MODEM_OWNER|STOPPED {:?}; mappings and requests retained",
                error
            ),
        }
        #[cfg(feature = "sunfish-modem-servreg-state-probe")]
        {
            let display = unsafe { Display::new(self.display_pixels) };
            self.paint_servreg_query(&display);
        }
    }
}

#[cfg(all(test, not(feature = "sunfish-modem-servreg-state-probe")))]
mod tests {
    use super::*;

    struct IpaCompletionService(bool);

    impl RuntimeServices for IpaCompletionService {
        fn ipa_modem_driver_complete(&self) -> bool {
            self.0
        }

        fn activate<I: Interrupts>(
            &mut self,
            _owner: &mut NativeOwner,
            _interrupts: &mut I,
        ) -> Result<(), Error> {
            Ok(())
        }

        fn notified<I: Interrupts>(
            &mut self,
            owner: &mut NativeOwner,
            channels: u64,
            interrupts: &mut I,
        ) -> Result<core::Progress, Error> {
            owner.notified(channels, interrupts)
        }
    }

    struct PrebootMemoryServices(bool);

    impl RuntimeServices for PrebootMemoryServices {
        fn preboot_memory_services_ready(&mut self) -> bool {
            self.0
        }

        fn activate<I: Interrupts>(
            &mut self,
            _owner: &mut NativeOwner,
            _interrupts: &mut I,
        ) -> Result<(), Error> {
            Ok(())
        }

        fn notified<I: Interrupts>(
            &mut self,
            owner: &mut NativeOwner,
            channels: u64,
            interrupts: &mut I,
        ) -> Result<core::Progress, Error> {
            owner.notified(channels, interrupts)
        }
    }

    struct PrebootWlanServices {
        memory_ready: bool,
        tftp_ready: bool,
    }

    impl RuntimeServices for PrebootWlanServices {
        fn preboot_memory_services_ready(&mut self) -> bool {
            self.memory_ready
        }

        fn preboot_tftp_service_ready(&mut self) -> bool {
            self.tftp_ready
        }

        fn activate<I: Interrupts>(
            &mut self,
            _owner: &mut NativeOwner,
            _interrupts: &mut I,
        ) -> Result<(), Error> {
            Ok(())
        }

        fn notified<I: Interrupts>(
            &mut self,
            owner: &mut NativeOwner,
            channels: u64,
            interrupts: &mut I,
        ) -> Result<core::Progress, Error> {
            owner.notified(channels, interrupts)
        }
    }

    struct PostInitReadyBackend;

    impl personal_os_rust_server::sunfish_ipa_post_init::Backend for PostInitReadyBackend {
        type Error = ();

        fn apply_initial_bus_vote(
            &mut self,
            _generation: u64,
            _use_case: &'static personal_os_rust_server::sunfish_ipa_bus_vote::UseCase,
        ) -> Result<(), Self::Error> {
            Ok(())
        }

        fn establish_register_access(&mut self, _generation: u64) -> Result<(), Self::Error> {
            Ok(())
        }

        fn initialize_fixed_registers(&mut self, _generation: u64) -> Result<(), Self::Error> {
            Ok(())
        }

        fn validate_modem_sram_partition(&mut self, _generation: u64) -> Result<(), Self::Error> {
            Ok(())
        }

        fn allocate_gsi_support_buffers(&mut self, _generation: u64) -> Result<(), Self::Error> {
            Ok(())
        }

        fn initialize_interrupts(&mut self, _generation: u64) -> Result<(), Self::Error> {
            Ok(())
        }

        fn register_gsi(&mut self, _generation: u64) -> Result<(), Self::Error> {
            Ok(())
        }

        fn connect_ap_pipes(&mut self, _generation: u64) -> Result<(), Self::Error> {
            Ok(())
        }

        fn capture_runtime_snapshot(
            &mut self,
            _generation: u64,
        ) -> Result<personal_os_rust_server::sunfish_ipa_profile::RuntimeSnapshot, Self::Error>
        {
            Ok(
                personal_os_rust_server::sunfish_ipa_profile::RuntimeSnapshot {
                    shared_mem_baddr_units: 0x10,
                    shared_mem_size_units: 0x400,
                    apps_wan_cons_endpoint: Some(
                        personal_os_rust_server::sunfish_ipa_profile::APPS_WAN_CONS_ENDPOINT,
                    ),
                    ipa_uc_loaded: false,
                },
            )
        }
    }

    #[test]
    fn owner_native_ipa_completion_pending_does_not_release_msa_gate_early() {
        let mut services = IpaModemInitServices::new(
            IpaCompletionService(false),
            personal_os_rust_server::qcom_ipa_client::InitPermit::test_after_post_init(1),
        );
        services.modem_ready = true;
        services.modem_completion_pending = true;
        assert!(!services.ipa_modem_ready());

        services.inner.0 = true;
        assert!(services.ipa_modem_ready());

        services.modem_completion_pending = false;
        services.inner.0 = false;
        assert!(services.ipa_modem_ready());
    }

    #[test]
    fn owner_native_diagnostic_service_adapters_never_claim_memory_readiness() {
        assert!(!NoServices.memory_services_ready());

        static IMAGE: [u8; 4] = *b"test";
        let mut services = TftpServerServices::new(
            NoServices,
            personal_os_rust_server::qcom_qrtr_tftp::AuthorizedWlanImage::test_fixture(&IMAGE),
        )
        .unwrap();
        assert!(!services.memory_services_ready());
        assert!(services.preboot_tftp_service_ready());
    }

    #[test]
    fn ipa_pre_mpss_session_reserves_endpoints_but_only_stages_ipa_and_servreg() {
        let prepared = IpaServerServices::prepare_before_mpss(
            NoServices,
            ApReadyPermit::test_after_post_init(1),
        )
        .unwrap();
        let status = prepared.session.session.control_status();
        assert!(!status.started);
        assert_eq!(
            status.ready_services & ((1 << 2) | (1 << 4)),
            (1 << 2) | (1 << 4)
        );
        assert_eq!(status.ready_services & 0b11, 0);
        for (service, port) in [
            (Service::RemoteStorage, 0x4000),
            (Service::HeapMemory, 0x4001),
        ] {
            assert_eq!(
                prepared
                    .session
                    .session
                    .endpoint(service)
                    .unwrap()
                    .address
                    .port,
                port
            );
        }
        assert_eq!(
            prepared
                .session
                .session
                .endpoint(Service::ServiceRegistry)
                .unwrap()
                .address
                .port,
            personal_os_rust_server::qcom_servreg::LOCAL_PORT
        );
        assert_eq!(
            prepared
                .session
                .session
                .endpoint(Service::IpaServer)
                .unwrap()
                .address
                .port,
            IPA_SERVER_PORT
        );
        assert!(prepared.session.session.transport_is_pristine());
        prepared.session.validate(1).unwrap();
        assert!(prepared.services.preboot_staged);
    }

    #[test]
    fn pre_mpss_memory_services_require_and_revalidate_the_retained_backend() {
        assert!(matches!(
            IpaServerServices::prepare_before_mpss_with_memory(
                NoServices,
                ApReadyPermit::test_after_post_init(1),
            ),
            Err(Error::MemoryServicesNotReady)
        ));

        let mut prepared = IpaServerServices::prepare_before_mpss_with_memory(
            PrebootMemoryServices(true),
            ApReadyPermit::test_after_post_init(1),
        )
        .unwrap();
        let expected = (1 << 2) | (1 << 4) | 0b11;
        assert_eq!(
            prepared.session.session.control_status().ready_services,
            expected
        );
        assert!(prepared.session.memory_staged);
        prepared.validate(1).unwrap();

        prepared.services.inner.0 = false;
        assert_eq!(prepared.validate(1), Err(Error::MemoryServicesNotReady));
    }

    #[test]
    fn pre_mpss_wlan_services_require_and_revalidate_memory_and_tftp() {
        assert_eq!(
            IpaServerServices::prepare_before_mpss_with_wlan_services(
                PrebootWlanServices {
                    memory_ready: true,
                    tftp_ready: false,
                },
                ApReadyPermit::test_after_post_init(1),
            )
            .err(),
            Some(Error::TftpImageNotReady)
        );

        let mut prepared = IpaServerServices::prepare_before_mpss_with_wlan_services(
            PrebootWlanServices {
                memory_ready: true,
                tftp_ready: true,
            },
            ApReadyPermit::test_after_post_init(1),
        )
        .unwrap();
        assert_eq!(
            prepared.session.session.control_status().ready_services,
            0b1_1111
        );
        assert!(prepared.session.tftp_staged);
        assert!(
            prepared
                .session
                .session
                .tftp_server_prepared(1, TFTP_SERVER_PORT)
        );
        prepared.validate(1).unwrap();

        prepared.services.inner.tftp_ready = false;
        assert_eq!(prepared.validate(1), Err(Error::TftpImageNotReady));
    }

    #[test]
    fn owner_native_tftp_adapter_stages_its_retained_source_before_mpss() {
        static IMAGE: [u8; 8] = *b"sunfish!";
        let services = TftpServerServices::new(
            PrebootMemoryServices(true),
            personal_os_rust_server::qcom_qrtr_tftp::AuthorizedWlanImage::test_fixture(&IMAGE),
        )
        .unwrap();
        let mut prepared = IpaServerServices::prepare_before_mpss_with_wlan_services(
            services,
            ApReadyPermit::test_after_post_init(1),
        )
        .unwrap();

        assert_eq!(
            prepared.session.session.control_status().ready_services,
            0b1_1111
        );
        assert!(prepared.session.tftp_staged);
        assert!(prepared.services.inner.server.image_available());
        assert!(
            prepared
                .session
                .session
                .tftp_server_prepared(1, TFTP_SERVER_PORT)
        );
        prepared.validate(1).unwrap();
        assert!(prepared.services.inner.server.image_available());
    }

    #[test]
    fn ipa_pre_mpss_session_rejects_a_different_generation_without_consuming_it() {
        let prepared = IpaServerServices::prepare_before_mpss(
            NoServices,
            ApReadyPermit::test_after_post_init(1),
        )
        .unwrap();
        assert!(matches!(prepared.session.validate(2), Err(Error::Mapping)));
    }

    #[test]
    fn post_init_ready_prepares_matching_ipa_server_and_modem_client_permits() {
        let mut runner = personal_os_rust_server::sunfish_ipa_post_init::Runner::new(7);
        let ready = runner.run(&mut PostInitReadyBackend).unwrap();
        let prepared = IpaServerServices::prepare_from_post_init(NoServices, ready).unwrap();

        assert_eq!(prepared.session.session.generation(), 7);
        assert!(prepared.session.validate(7).is_ok());
        assert!(prepared.services.inner.readiness.fields(7).is_ok());
        assert_eq!(prepared.services.readiness.generation(), 7);
    }

    #[test]
    fn owner_native_msa_and_wlfw_progress_waits_for_memory_services() {
        let (mut memory, checkpoint, _) = crate::native_owner_checkpoint();
        let mut lock = 0;
        let mut doorbell = [0; 4];
        let mut scratch = crate::ScmScratch([0; ARG_BYTES]);
        let mut runtime = crate::owner_attach(
            &mut memory,
            checkpoint,
            &mut lock,
            &mut doorbell,
            &mut scratch,
        )
        .unwrap();

        crate::SCM_CALLS.with(|calls| calls.borrow_mut().clear());
        assert!(!runtime.services.memory_services_ready());
        assert!(!runtime.begin_msa_info_if_ready().unwrap());
        assert!(!runtime.begin_msa_handoff_if_ready().unwrap());
        assert!(!runtime.begin_capability_if_ready().unwrap());
        assert!(!runtime.msa_info_started);
        assert!(!runtime.msa_handoff_started);
        assert!(!runtime.capability_started);
        crate::SCM_CALLS.with(|calls| assert!(calls.borrow().is_empty()));
    }

    #[test]
    fn tftp_runtime_service_uses_fixed_sunfish_service_and_transfer_ports() {
        static IMAGE: [u8; 4] = *b"test";
        let services = TftpServerServices::new(
            NoServices,
            personal_os_rust_server::qcom_qrtr_tftp::AuthorizedWlanImage::test_fixture(&IMAGE),
        )
        .unwrap();
        assert_eq!(
            services.server.next_transfer_port(),
            TFTP_FIRST_TRANSFER_PORT
        );
        assert_eq!(services.server.active_transfers(), 0);
    }

    #[test]
    fn owner_native_msa_handoff_stages_exact_assignments_and_mints_one_permit() {
        let (mut memory, checkpoint, _) = crate::native_owner_checkpoint();
        let mut lock = 0;
        let mut doorbell = [0; 4];
        let mut scratch = crate::ScmScratch([0; ARG_BYTES]);
        let mut runtime = crate::owner_attach(
            &mut memory,
            checkpoint,
            &mut lock,
            &mut doorbell,
            &mut scratch,
        )
        .unwrap();
        let regions = [
            personal_os_rust_server::qcom_msa::MsaRegion {
                base: personal_os_rust_server::qcom_msa::SUNFISH_MSA_BASE,
                bytes: 0x4000,
                secure: true,
            },
            personal_os_rust_server::qcom_msa::MsaRegion {
                base: personal_os_rust_server::qcom_msa::SUNFISH_MSA_BASE + 0x4000,
                bytes: 0x1fc000,
                secure: false,
            },
        ];
        let plan = personal_os_rust_server::qcom_msa::plan_handoff(
            runtime.msa_reservation.unwrap(),
            &regions,
        )
        .unwrap();
        crate::SCM_CALLS.with(|calls| calls.borrow_mut().clear());
        let permit = runtime.execute_msa_handoff(plan).unwrap();
        assert_eq!(permit.generation(), plan.generation());
        assert_eq!(permit.identity(), plan.identity());
        crate::SCM_CALLS.with(|calls| {
            let calls = calls.borrow();
            assert_eq!(calls.len(), 2);
            for (actual, transfer) in calls.iter().zip(plan.transfers()) {
                assert_eq!(
                    *actual,
                    Call::for_msa_transfer(transfer).unwrap().words().to_owned()
                );
            }
        });
        assert_eq!(
            scratch.0.as_slice(),
            Call::for_msa_transfer(plan.transfers()[1])
                .unwrap()
                .bytes()
                .as_slice()
        );
        drop(runtime);
    }
}

#[cfg(all(test, feature = "sunfish-modem-servreg-state-probe"))]
mod servreg_state_probe_tests {
    use super::*;

    #[test]
    fn stop_reason_distinguishes_modem_err_fatal_from_transport_fatal() {
        let access = Some(Error::Transport(fifo::Error::Access));
        assert_eq!(
            servreg_stop_reason(access, smem::Error::Fatal.code()),
            "MODEM ERR-FATAL"
        );
        assert_eq!(servreg_stop_reason(access, 0), "FIFO ACCESS ERROR");
        assert_eq!(
            servreg_stop_reason(Some(Error::Transport(fifo::Error::Fatal)), 0x0e),
            "FIFO FATAL STATUS"
        );
    }
}
