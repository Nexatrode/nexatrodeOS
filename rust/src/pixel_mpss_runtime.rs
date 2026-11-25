//! Same-boot post-auth GLINK/QRTR runtime. This stage stops at bounded service
//! discovery; it never publishes or serves WLAN firmware.
use super::*;
use core::mem::ManuallyDrop;
use personal_os_rust_server::qcom_glink_fifo::{BootDrive, BootIo, BootPhase, BootTransport};
use personal_os_rust_server::{
    qcom_glink_fifo,
    qcom_modem_owner::{self, Owner},
    sunfish_mpss_linux_transport_native::{self as transport, Retained},
    sunfish_mpss_runtime_irq::{self as irq, Router},
    sunfish_smmu_native,
};
use sel4_microkit::Channel;
#[path = "pixel_mpss_runtime_diagnostic.rs"]
mod diagnostic;
#[cfg(feature="sunfish-wrq-post-return-callers")]
#[path="pixel_mpss_wrq_callers.rs"]
mod wrq_callers;
#[cfg(feature="sunfish-first-data-ack-elf-trace")]
#[path="pixel_mpss_tftp_completion.rs"]
mod tftp_completion;
#[cfg(all(feature="sunfish-ack-elf-phase-trace", not(feature="sunfish-registry-window-trace")))]
#[path="pixel_mpss_ack_elf_stages.rs"]
mod ack_elf_stages;
#[cfg(all(feature="sunfish-registry-window-trace", not(feature="sunfish-registry-handoff-turn")))]
#[path="pixel_mpss_registry_window.rs"]
mod registry_window;
#[cfg(all(feature="sunfish-registry-handoff-turn", not(feature="sunfish-registry-predecessor-pressure")))]
#[path="pixel_mpss_registry_handoff.rs"]
mod registry_handoff;
#[cfg(all(feature="sunfish-registry-predecessor-pressure", not(feature="sunfish-registry-grant-eligibility")))]
#[path="pixel_mpss_registry_pressure.rs"]
mod registry_pressure;
#[cfg(all(feature="sunfish-registry-grant-eligibility",not(feature="sunfish-registry-post-reply-trace")))]
#[path="pixel_mpss_registry_grant_eligibility.rs"]
mod registry_grant_eligibility;
#[cfg(all(feature="sunfish-registry-post-reply-trace",not(feature="sunfish-registry-conversion-lifetime-trace")))]
#[path="pixel_mpss_registry_post_reply.rs"]
mod registry_post_reply;
#[cfg(all(feature="sunfish-registry-conversion-lifetime-trace",not(feature="sunfish-registry-converted-credit-turn")))]
#[path="pixel_mpss_registry_conversion_lifetime.rs"]
mod registry_conversion_lifetime;
#[cfg(all(feature="sunfish-registry-converted-credit-turn",not(feature="sunfish-registry-prearm-io-trace")))]
#[path="pixel_mpss_registry_credit_turn.rs"]
mod registry_credit_turn;
#[cfg(all(feature="sunfish-registry-prearm-io-trace",not(feature="sunfish-registry-elf-admission-panel")))]
#[path="pixel_mpss_registry_prearm_io.rs"]
mod registry_prearm_io;
#[cfg(all(feature="sunfish-registry-elf-admission-panel",not(feature="sunfish-registry-tx3-receipt-panel")))]
#[path="pixel_mpss_registry_elf_admission.rs"]
mod registry_elf_admission;
#[cfg(all(feature="sunfish-registry-tx3-receipt-panel",not(feature="sunfish-registry-tx3-wire-headers")))]
#[path="pixel_mpss_registry_tx3_receipt.rs"]
mod registry_tx3_receipt;
#[cfg(all(feature="sunfish-registry-tx3-wire-headers",not(feature="sunfish-registry-tftp-wire")))]
#[path="pixel_mpss_registry_tx3_headers.rs"]
mod registry_tx3_headers;
#[cfg(all(feature="sunfish-registry-tftp-wire",not(feature="sunfish-post-elf-startup-panel")))]
#[path="pixel_mpss_registry_tftp_identity.rs"]
mod registry_tftp_identity;
#[cfg(all(feature="sunfish-post-elf-startup-panel",not(feature="sunfish-post-elf-mcfg-trace")))]
#[path="pixel_mpss_post_elf_startup.rs"]
mod post_elf_startup;
#[cfg(all(feature="sunfish-post-elf-mcfg-trace",not(feature="sunfish-elf-queued-handoff")))]
#[path="pixel_mpss_post_elf_mcfg.rs"]
mod post_elf_mcfg;
#[cfg(all(feature="sunfish-elf-queued-handoff",not(feature="sunfish-elf-window-trace")))]
#[path="pixel_mpss_elf_queued_turn.rs"]
mod elf_queued_turn;
#[cfg(all(feature="sunfish-elf-window-trace",not(feature="sunfish-elf-writer-trace")))]
#[path="pixel_mpss_elf_window.rs"]
mod elf_window;
#[cfg(feature="sunfish-elf-writer-trace")]
#[cfg_attr(feature="sunfish-elf-tx-slots",path="pixel_mpss_elf_tx_slots.rs")]
#[cfg_attr(all(feature="sunfish-elf-third-phases",not(feature="sunfish-elf-tx-slots")),path="pixel_mpss_elf_third_phases.rs")]
#[cfg_attr(not(feature="sunfish-elf-third-phases"),path="pixel_mpss_elf_publication.rs")]
mod elf_publication;
#[cfg(feature="sunfish-registry-defer-paint")]
#[path="pixel_mpss_registry_paint.rs"]
mod registry_paint;

#[cfg(all(feature = "sunfish-ipa-independent-server-experiment", not(feature = "sunfish-ipa-independent-server-runtime")))]
compile_error!("independent IPA server experiment is offline only; native composition not validated");
#[cfg(all(feature = "sunfish-ipa-independent-server-runtime", feature = "sunfish-ipa-schedule-trace"))]
compile_error!("independent IPA runtime uses overlapping event timestamps, not serialized schedule windows");
const GENERATION: u64 = 1;
const IPA_SHARED_BYTES: u32 = 0x415;
const IPA_SERVER_PORT: u32 = 0x4002;
#[cfg(all(feature = "sunfish-native-tftp-observer", feature = "sunfish-native-tftp-server-check"))]
compile_error!("observer and server-check modes are mutually exclusive");
#[cfg(all(feature = "sunfish-tftp-publish-io-observer", feature = "sunfish-tftp-early-dispatch-experiment"))]
compile_error!("publication IO observer requires normal router dispatch");
#[cfg(all(feature = "sunfish-wlan-elf-oack-router-turn", feature = "sunfish-tftp-early-dispatch-experiment"))]
compile_error!("select only one early TFTP router turn");
#[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
compile_error!("close-handoff prototype is not yet validated for native boot; host tests only");
const GLINK_CHANNEL: usize = irq::GLINK_CHANNEL as usize;
type NativeIo = Retained<'static, fn(usize) -> Result<(), sunfish_smmu_native::Error>>;
type Opening = BootTransport<NativeIo>;
type Adapter = qcom_glink_fifo::Adapter<NativeIo>;
type ModemOwner = Owner<NativeIo>;

struct Kernel;
impl irq::Kernel for Kernel {
    fn acknowledge(&mut self, channel: u8) -> Result<(), ()> {
        if channel != irq::GLINK_CHANNEL && channel != irq::SMP_CHANNEL {
            return Err(());
        }
        Channel::new(channel as usize).irq_ack().map_err(|_| ())
    }
}

enum Stage {
    Opening(Opening),
    Adapter(Adapter),
    Services(ModemOwner),
}

/// A non-self-referential owner. The authenticated Boot, actual Q6-ready
/// receipt, and IPA receive runtime stay retained beside one transport phase;
/// the only Native adapter is inside that phase's checked RefCell gate.
pub(super) struct RuntimeDomain {
    #[cfg(feature="sunfish-registry-defer-paint")]
    registry_paint: registry_paint::Gate,
    stage: Option<Stage>,
    boot: ManuallyDrop<pixel_mpss_auth::NativeBoot>,
    ready: ManuallyDrop<personal_os_rust_server::sunfish_mpss_linux_smp2p::ReadyObservation>,
    receive: ManuallyDrop<pixel_ipa_receive::Runtime>,
    router: Router<Kernel>,
    hz: u64,
    pending: u64,
    error: u32,
    detail: u32,
    receive_failure: Option<diagnostic::ReceiveFailure>,
    guard_failure: Option<(u32, u32, u32, u32, u32, u32)>,
    config_failure: Option<([u32; 9], u32, u32, u32, u32)>,
    reference_pair_count: u32,
    service_registry_prepared: bool,
    rmtfs_bound: bool,
    rmtfs_report: Option<personal_os_rust_server::sunfish_linux_rmtfs_server::Report>,
    ipa_report: Option<qcom_modem_owner::LinuxIpaReport>,
    ipa_complete_seen: bool,
    #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
    ipa_ready_service_at: Option<u64>,
    #[cfg(feature = "sunfish-wlan-state-query")]
    wlan_query: qcom_modem_owner::WlanStateQuery,
    #[cfg(feature = "sunfish-wlan-state-listener")]
    wlan_listener: qcom_modem_owner::WlanStateListener,
    progress_trace: diagnostic::ProgressTrace,
    #[cfg(feature = "sunfish-tftp-quiet-oack-experiment")]
    oack_paint_gate: diagnostic::OackPaintGate,
    timing: diagnostic::TimingTrace,
    #[cfg(feature = "sunfish-ipa-registration-prep-trace")]
    registration_prep_work: [u64; 6],
    #[cfg(feature = "sunfish-ipa-schedule-trace")]
    ipa_schedule: diagnostic::IpaScheduleTiming,
    #[cfg(feature = "sunfish-wlan-elf-registry-gap")]
    registry_gap: diagnostic::RegistryGap,
    #[cfg(feature = "sunfish-servreg-bounded-second-turn")]
    registry_extra_attempts: u32,
    #[cfg(feature = "sunfish-servreg-bounded-second-turn")]
    registry_extra_outcome: u32,
    #[cfg(feature = "sunfish-tftp-oack-phase-observer")]
    oack_phases: diagnostic::OackPhases,
    #[cfg(feature = "sunfish-tftp-publish-io-observer")]
    oack_publish_io: diagnostic::OackPublishIo,
    #[cfg(feature = "sunfish-tftp-publish-op-observer")]
    oack_publish_ops: diagnostic::OackPublishIo<6>,
    #[cfg(feature = "sunfish-tftp-barrier-domain-observer")]
    oack_barrier_domains: diagnostic::OackPublishIo<2>,
    runtime_started: u64,
    io_timing: transport::IoTiming,
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    intent_deadline: diagnostic::IntentDeadline,
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    intent_status: (u32, u32),
    #[cfg(feature = "sunfish-native-tftp-server-check")]
    server_check: personal_os_rust_server::qcom_tftp_server_check::ServerCheck,
    #[cfg(feature = "sunfish-tftp-wlan-mux")]
    firmware: ManuallyDrop<personal_os_rust_server::qcom_qrtr_tftp::Server<'static>>,
    #[cfg(feature = "sunfish-native-tftp-server-check")]
    server_check_queued: u32,
    #[cfg(feature = "sunfish-native-tftp-server-check")]
    server_check_ignored: u32,
}

#[cfg(feature = "sunfish-ipa-schedule-trace")]
fn ipa_schedule_milestones(owner: &ModemOwner) -> [Option<u64>; 5] {
    let (init, transport) = owner.ipa_init_trace();
    [transport.and_then(|t| t.queued_at), transport.and_then(|t| t.published_at),
     transport.and_then(|t| t.retired_at), init.and_then(|t| t.committed_at),
     owner.ipa_registration_trace().and_then(|t| t.prepared_at)]
}

#[cfg(feature = "sunfish-ipa-registration-prep-trace")]
fn record_registration_prep_work(
    total: &mut u64, trace: Option<personal_os_rust_server::qcom_glink_qmi::IpaRegistrationTrace>,
    started: u64, ended: u64,
) {
    if let Some(trace) = trace
        && let Some(routed) = trace.routed_at
    {
        let end = ended.min(trace.prepared_at.unwrap_or(ended));
        *total = total.saturating_add(end.saturating_sub(started.max(routed)));
    }
}

impl RuntimeDomain {
    #[cfg(feature = "sunfish-ipa-schedule-trace")]
    fn schedule_sample(&mut self, bucket: usize) {
        if let Some(Stage::Services(owner)) = self.stage.as_ref() {
            self.ipa_schedule.observe(counter(), bucket, ipa_schedule_milestones(owner));
        }
    }

    fn new(parts: pixel_mpss_auth::RuntimeParts) -> Self {
        let session = parts.session;
        #[cfg(feature = "sunfish-ipa-before-first-oack")]
        let session = {
            let mut session = session;
            session.enable_ipa_before_first_oack();
            #[cfg(feature = "sunfish-first-oack-after-ipa-publish")]
            session.enable_first_oack_after_ipa_publish();
            #[cfg(feature = "sunfish-first-wrq-spare-oack")]
            session.enable_first_wrq_spare_oack();
            #[cfg(all(feature = "sunfish-tftp-first-oack-unheld", not(feature = "sunfish-ipa-reply-before-oack")))]
            session.observe_first_tftp_without_oack_hold();
            session
        };
        let mut opening = transport::start_live(parts.gate, parts.auth, session, parts.hz);
        opening.with_io(|io| io.owner_timing_clock(counter));
        Self {
            #[cfg(feature="sunfish-registry-defer-paint")]
            registry_paint: registry_paint::Gate::default(),
            stage: Some(Stage::Opening(opening)),
            boot: ManuallyDrop::new(parts.boot),
            ready: ManuallyDrop::new(parts.ready),
            receive: ManuallyDrop::new(parts.receive),
            router: Router::new(Kernel, GENERATION),
            hz: parts.hz,
            pending: parts.pending,
            error: 0,
            detail: 0,
            receive_failure: None,
            guard_failure: None,
            config_failure: None,
            reference_pair_count: 0,
            service_registry_prepared: false,
            rmtfs_bound: false,
            rmtfs_report: None,
            ipa_report: None,
            ipa_complete_seen: false,
            #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
            ipa_ready_service_at: None,
            #[cfg(feature = "sunfish-wlan-state-query")]
            wlan_query: qcom_modem_owner::WlanStateQuery::default(),
            #[cfg(feature = "sunfish-wlan-state-listener")]
            wlan_listener: qcom_modem_owner::WlanStateListener::default(),
            progress_trace: diagnostic::ProgressTrace::default(),
            #[cfg(feature = "sunfish-tftp-quiet-oack-experiment")]
            oack_paint_gate: diagnostic::OackPaintGate::default(),
            timing: diagnostic::TimingTrace::default(),
            #[cfg(feature = "sunfish-ipa-registration-prep-trace")]
            registration_prep_work: [0; 6],
            #[cfg(feature = "sunfish-ipa-schedule-trace")]
            ipa_schedule: diagnostic::IpaScheduleTiming::default(),
            #[cfg(feature = "sunfish-wlan-elf-registry-gap")]
            registry_gap: diagnostic::RegistryGap::default(),
            #[cfg(feature = "sunfish-servreg-bounded-second-turn")]
            registry_extra_attempts: 0,
            #[cfg(feature = "sunfish-servreg-bounded-second-turn")]
            registry_extra_outcome: 0,
            #[cfg(feature = "sunfish-tftp-oack-phase-observer")]
            oack_phases: diagnostic::OackPhases::default(),
            #[cfg(feature = "sunfish-tftp-publish-io-observer")]
            oack_publish_io: diagnostic::OackPublishIo::default(),
            #[cfg(feature = "sunfish-tftp-publish-op-observer")]
            oack_publish_ops: diagnostic::OackPublishIo::default(),
            #[cfg(feature = "sunfish-tftp-barrier-domain-observer")]
            oack_barrier_domains: diagnostic::OackPublishIo::default(),
            runtime_started: counter(),
            io_timing: transport::IoTiming::default(),
            #[cfg(feature = "sunfish-glink-intent-request-experiment")]
            intent_deadline: diagnostic::IntentDeadline::default(),
            #[cfg(feature = "sunfish-glink-intent-request-experiment")]
            intent_status: (0, 0),
            #[cfg(feature = "sunfish-native-tftp-server-check")]
            server_check: personal_os_rust_server::qcom_tftp_server_check::ServerCheck::new(),
            #[cfg(feature = "sunfish-tftp-wlan-mux")]
            firmware: ManuallyDrop::new(parts.firmware),
            #[cfg(feature = "sunfish-native-tftp-server-check")]
            server_check_queued: 0,
            #[cfg(feature = "sunfish-native-tftp-server-check")]
            server_check_ignored: 0,
        }
    }

    fn begin(&mut self, display: &Display, state: &mut Startup) {
        if self.error != 0 {
            self.timed_show(display);
            return;
        }
        let pending = core::mem::take(&mut self.pending);
        if pending != 0 {
            let captured = self.router.capture(pending);
            let other = self.router.take_other();
            if other != 0 {
                if let Err(detail) = self.dispatch_receive(state, other, captured.is_ok()) {
                    self.detail = detail;
                    self.fail(state, 2);
                }
            }
            if self.error == 0 && captured.is_err() {
                self.fail(state, 3);
            }
        }
        if self.error == 0 {
            // A GLINK edge may have been co-delivered with the final Q6-ready
            // notification and retained by the auth handoff. Rearm only that
            // observed edge, after a successful bounded transport step.
            self.step_opening(state, pending & (1 << irq::GLINK_CHANNEL) != 0);
        }
        self.timed_show(display);
    }

    fn dispatch_receive(&mut self, state: &mut Startup, badge: u64, allow_reference_pair: bool) -> Result<(), u32> {
        if badge == 0 {
            return Ok(());
        }
        let allow_reference_pair = if cfg!(feature = "sunfish-ipa-postmodem-reference-experiment") && allow_reference_pair {
            if let Some(Stage::Services(owner)) = self.stage.as_mut() {
                owner.revalidate().map_err(|_| 0x13u32)?;
                true
            } else { false }
        } else { false };
        let receive = &mut *self.receive;
        let ready = &*self.ready;
        let result = match self.stage.as_mut().ok_or(0x10u32)? {
            Stage::Opening(opening) => opening.with_io(|io| {
                io.with_native(|native| {
                    pixel_ipa_receive::notify_runtime_badge(state, receive, native, ready, allow_reference_pair, badge)
                })
            }),
            Stage::Adapter(adapter) => adapter.with_io(|io| {
                io.with_native(|native| {
                    pixel_ipa_receive::notify_runtime_badge(state, receive, native, ready, allow_reference_pair, badge)
                })
            }),
            Stage::Services(owner) => owner.with_io(|io| {
                io.with_native(|native| {
                    pixel_ipa_receive::notify_runtime_badge(state, receive, native, ready, allow_reference_pair, badge)
                })
            }),
        };
        self.reference_pair_count = pixel_ipa_receive::postmodem_pair_count(state);
        match result {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(error)) => {
                let (native_error, _) = pixel_ipa_receive::diagnostic_snapshot(state);
                self.guard_failure = pixel_ipa_receive::guard_diagnostic(state);
                self.config_failure = pixel_ipa_receive::config_diagnostic(state);
                self.receive_failure = Some(diagnostic::ReceiveFailure::capture(badge, 0x11, native_error, &error));
                Err(0x11)
            }
            Err(error) => {
                self.receive_failure = Some(diagnostic::ReceiveFailure::capture(badge, 0x12, u32::MAX, &error));
                Err(0x12)
            }
        }
    }

    fn step_opening(&mut self, state: &mut Startup, glink_delivered: bool) {
        let result = match self.stage.as_mut() {
            Some(Stage::Opening(opening)) => opening.drive(counter(), 128),
            _ => return,
        };
        match result {
            Ok(BootDrive::BudgetExhausted) => self.fail(state, 0x23),
            Ok(progress) => {
                if glink_delivered && self.router.acknowledge_boot_glink().is_err() {
                    self.fail(state, 0x20);
                    return;
                }
                if progress == BootDrive::Ready {
                    if let Err(detail) = self.promote(state) {
                        self.detail = detail;
                        self.fail(state, 0x21);
                    }
                }
            }
            Err(_) => self.fail(state, 0x22),
        }
    }

    fn promote(&mut self, state: &mut Startup) -> Result<(), u32> {
        let Some(Stage::Opening(opening)) = self.stage.take() else {
            return Err(0x30);
        };
        let adapter = match opening.into_adapter() {
            Ok(adapter) => adapter,
            Err(opening) => {
                self.stage = Some(Stage::Opening(opening));
                return Err(0x31);
            }
        };
        self.stage = Some(Stage::Adapter(adapter));
        let Some(Stage::Adapter(adapter)) = self.stage.take() else {
            return Err(0x32);
        };
        let mut owner = match ModemOwner::attach_linux_ipa_retained(
            adapter,
            GENERATION,
            IPA_SHARED_BYTES,
            self.hz,
        ) {
            Ok(owner) => owner,
            Err((_error, adapter)) => {
                self.stage = Some(Stage::Adapter(adapter));
                return Err(0x33);
            }
        };
        owner.set_rmtfs_event_clock(counter);
        self.stage = Some(Stage::Services(owner));
        self.activate_services(state)?;
        // Run the first bounded owner pass only after SERVREG staging and
        // successful RMTFS binding have completed.
        self.service_owner(state)?;
        Ok(())
    }

    fn activate_services(&mut self, state: &mut Startup) -> Result<(), u32> {
        let Some(Stage::Services(owner)) = self.stage.as_mut() else {
            return Err(0x40);
        };
        if cfg!(feature = "sunfish-ipa-postmodem-reference-experiment") {
            owner.revalidate().map_err(|_| 0x49u32)?;
        }
        // Use the same checked SMMU gate and retained receive owner that will
        // service interrupts. The actual Q6 receipt binds the permit to this
        // boot generation; no cached startup flag alone grants readiness.
        let permit = owner.with_io(|io| {
            io.with_native(|native| {
                pixel_ipa_receive::validate_runtime_setup(
                    state, &self.receive, native, &self.ready,
                )
            })
        }).map_err(|_| 0x45u32)?.map_err(|_| 0x46u32)?;
        self.reference_pair_count = pixel_ipa_receive::postmodem_pair_count(state);
        owner.prepare_service_registry().map_err(|_| 0x41u32)?;
        self.service_registry_prepared = true;
        owner.prepare_ipa_server_endpoint(IPA_SERVER_PORT).map_err(|_| 0x47u32)?;
        // Stage the observer before QRTR control starts. No TFTP response or
        // firmware bytes are produced by this endpoint.
        #[cfg(feature = "sunfish-native-tftp-observer")]
        owner.prepare_tftp_observer(0x5000).map_err(|_| 0x49u32)?;
        #[cfg(feature = "sunfish-native-tftp-server-check")]
        owner.prepare_tftp_server(0x5000).map_err(|_| 0x4au32)?;
        #[cfg(feature = "sunfish-wlan-state-query")]
        owner.enable_servreg_notif_client(0x4005).map_err(|_| 0x4bu32)?;
        owner.start_control().map_err(|_| 0x42u32)?;
        owner.enable_ipa_server(&permit, &mut self.router).map_err(|_| 0x48u32)?;
        let server = (&mut *self.boot).server().map_err(|_| 0x43u32)?;
        owner
            .activate_linux_rmtfs(server, &mut self.router)
            .map_err(|_| 0x44u32)?;
        self.rmtfs_bound = true;
        Ok(())
    }

    fn service_pass(&mut self, _state: &mut Startup) -> Result<(), u32> {
        let Some(Stage::Services(owner)) = self.stage.as_mut() else {
            return Err(0x50);
        };
        let server = (&mut *self.boot).server().map_err(|_| 0x51u32)?;
        // Both configurations call the same bounded handler once per pass.
        // The opt-in ordering gives it the first turn without skipping the
        // registry, storage, or IPA services on a successful pass.
        #[cfg(feature = "sunfish-native-tftp-server-check")]
        macro_rules! tftp_service_step {
            () => {
                #[cfg(feature = "sunfish-ipa-schedule-trace")]
                self.ipa_schedule.observe(counter(), 7, ipa_schedule_milestones(owner));
                #[cfg(feature = "sunfish-ipa-registration-prep-trace")]
                let tftp_started = counter();
                #[cfg(feature = "sunfish-tftp-wlan-mux")]
                let tftp_progress = owner.service_combined_tftp_step(&mut self.server_check, &mut self.firmware, &mut self.router);
                #[cfg(not(feature = "sunfish-tftp-wlan-mux"))]
                let tftp_progress = owner.service_tftp_server_check_step(&mut self.server_check, &mut self.router);
                match tftp_progress.map_err(|_| 0x55u32)? {
                    qcom_modem_owner::TftpServiceProgress::ResponseQueued => {
                        self.server_check_queued = self.server_check_queued.saturating_add(1);
                        // The router consumes its existing continuation and
                        // retains all interrupt and transport checks. One
                        // extra bounded turn does not loop on backpressure.
                        #[cfg(feature = "sunfish-tftp-early-dispatch-experiment")]
                        {
                            self.router.dispatch(owner).map_err(|_| 0x61u32)?;
                            #[cfg(feature = "sunfish-tftp-wlan-close-handoff")]
                            if owner.pending_firmware_close().is_some() {
                                owner.resolve_firmware_close(&mut self.firmware).map_err(|_| 0x63u32)?;
                            }
                            #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
                            if owner.pending_tftp_close().is_some() {
                                owner.resolve_pending_tftp_close(&mut self.server_check).map_err(|_| 0x62u32)?;
                            }
                        }
                        #[cfg(feature = "sunfish-wlan-elf-oack-router-turn")]
                        {
                            #[cfg(feature = "sunfish-wlan-first-server-check-retry")]
                            let dispatched = self.router.dispatch_queued_tftp_retry(owner)
                                .map_err(|_| 0x61u32)?;
                            #[cfg(feature = "sunfish-tftp-wlan-close-handoff")]
                            if owner.pending_firmware_close().is_some() {
                                owner.resolve_firmware_close(&mut self.firmware).map_err(|_| 0x63u32)?;
                            }
                            #[cfg(not(feature = "sunfish-wlan-first-server-check-retry"))]
                            let dispatched = false;
                            let dispatched = dispatched || self.router.dispatch_queued_server_check_oack(owner, counter)
                                .map_err(|_| 0x61u32)?;
                            #[cfg(feature = "sunfish-tftp-wlan-close-handoff")]
                            if owner.pending_firmware_close().is_some() {
                                owner.resolve_firmware_close(&mut self.firmware).map_err(|_| 0x63u32)?;
                            }
                            #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
                            if dispatched && owner.pending_tftp_close().is_some() {
                                owner.resolve_pending_tftp_close(&mut self.server_check).map_err(|_| 0x62u32)?;
                            }
                            #[cfg(not(feature = "sunfish-tftp-close-handoff-prototype"))]
                            let _ = dispatched;
                        }
                        #[cfg(feature = "sunfish-original-final-ack-turn")]
                        {
                            self.router.dispatch_queued_original_final_ack(owner)
                                .map_err(|_| 0x61u32)?;
                            #[cfg(feature = "sunfish-tftp-wlan-close-handoff")]
                            if owner.pending_firmware_close().is_some() {
                                owner.resolve_firmware_close(&mut self.firmware).map_err(|_| 0x63u32)?;
                            }
                        }
                    },
                    qcom_modem_owner::TftpServiceProgress::Ignored(_) => self.server_check_ignored = self.server_check_ignored.saturating_add(1),
                    _ => {},
                }
                #[cfg(feature = "sunfish-ipa-schedule-trace")]
                self.ipa_schedule.observe(counter(), 5, ipa_schedule_milestones(owner));
                #[cfg(feature = "sunfish-ipa-registration-prep-trace")]
            record_registration_prep_work(&mut self.registration_prep_work[5], owner.ipa_registration_trace(), tftp_started, counter());
            };
        }
        #[cfg(feature = "sunfish-ipa-registration-cold-handoff")]
        macro_rules! cold_ipa_step {
            () => {{
                let handled = self.router.service_cold_ipa_registration(owner, counter)
                    .map_err(|_| 0x54u32)?;
                #[cfg(feature = "sunfish-tftp-wlan-close-handoff")]
                if owner.pending_firmware_close().is_some() {
                    owner.resolve_firmware_close(&mut self.firmware).map_err(|_| 0x63u32)?;
                }
                #[cfg(all(feature = "sunfish-ipa-reply-before-oack", feature = "sunfish-tftp-close-handoff-prototype"))]
                if handled && owner.pending_tftp_close().is_some() {
                    owner.resolve_pending_tftp_close(&mut self.server_check).map_err(|_| 0x62u32)?;
                }
                handled
            }};
        }
        #[cfg(feature = "sunfish-ipa-registration-early-turn")]
        let early_ipa = {
            let started = counter();
            #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
            if self.ipa_ready_service_at.is_none() && owner.ipa_wrq_timing().0.is_some() {
                self.ipa_ready_service_at = Some(started);
            }
            #[cfg(feature = "sunfish-ipa-schedule-trace")]
            self.ipa_schedule.observe(started, 7, ipa_schedule_milestones(owner));
            #[cfg(feature = "sunfish-ipa-registration-cold-handoff")]
            let result = if cold_ipa_step!() {
                Ok(true)
            } else {
                owner.service_ipa_registration_early(started, &mut self.router)
            };
            #[cfg(not(feature = "sunfish-ipa-registration-cold-handoff"))]
            let result = owner.service_ipa_registration_early(started, &mut self.router);
            #[cfg(feature = "sunfish-ipa-schedule-trace")]
            self.ipa_schedule.observe(counter(), 2, ipa_schedule_milestones(owner));
            if !matches!(result, Ok(false)) {
                self.ipa_report = owner.linux_ipa_report();
                self.ipa_complete_seen |= self.ipa_report.is_some_and(|r| r.protocol_complete);
                let ended = counter();
                self.timing.record(2, ended.wrapping_sub(started));
            #[cfg(feature = "sunfish-ipa-registration-prep-trace")]
            record_registration_prep_work(&mut self.registration_prep_work[2], owner.ipa_registration_trace(), started, ended);
                #[cfg(feature = "sunfish-tftp-oack-phase-observer")]
                self.oack_phases.mark(3, ended);
            }
            result.map_err(|_| 0x54u32)?
        };
        #[cfg(not(feature = "sunfish-ipa-registration-early-turn"))]
        let early_ipa = false;
        #[cfg(all(feature = "sunfish-native-tftp-server-check", feature = "sunfish-tftp-early-service-experiment"))]
        tftp_service_step!();
        // TFTP can dispatch the router after the first IPA check and arm
        // the cold handoff. Consume it before ordinary IPA can discover INIT.
        #[cfg(feature = "sunfish-ipa-registration-cold-handoff")]
        let early_ipa = if early_ipa {
            true
        } else {
            let started = counter();
            let handled = cold_ipa_step!();
            if handled {
                let ended = counter();
                self.timing.record(2, ended.wrapping_sub(started));
                record_registration_prep_work(&mut self.registration_prep_work[2],
                    owner.ipa_registration_trace(), started, ended);
                self.ipa_report = owner.linux_ipa_report();
                self.ipa_complete_seen |= self.ipa_report.is_some_and(|r| r.protocol_complete);
            }
            handled
        };
        let started = counter();
        #[cfg(feature = "sunfish-ipa-schedule-trace")]
        self.ipa_schedule.observe(started, 7, ipa_schedule_milestones(owner));
        #[cfg(feature = "sunfish-wlan-elf-registry-gap")]
        if owner.service_registry_trace().elf.is_some_and(|elf| elf.dispatch.selected.is_none()) {
            self.registry_gap.enter_registry(started, self.timing.total);
        }
        #[cfg(not(feature = "sunfish-servreg-bounded-second-turn"))]
        let registry_result = owner.dispatch_service_registry_step(&mut self.router);
        #[cfg(feature = "sunfish-servreg-bounded-second-turn")]
        let registry_result = match owner.dispatch_service_registry_bounded(&mut self.router) {
            Ok((first, second)) => {
                #[cfg(feature = "sunfish-registry-handoff-turn")]
                {
                    self.router.dispatch_registry_handoff(owner).map_err(|_| 0x61u32)?;
                    #[cfg(feature = "sunfish-tftp-wlan-close-handoff")]
                    if owner.pending_firmware_close().is_some() {
                        owner.resolve_firmware_close(&mut self.firmware).map_err(|_| 0x63u32)?;
                    }
                    #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
                    if owner.pending_tftp_close().is_some() {
                        owner.resolve_pending_tftp_close(&mut self.server_check).map_err(|_| 0x62u32)?;
                    }
                }
                if let Some(progress) = second {
                    self.registry_extra_attempts = self.registry_extra_attempts.saturating_add(1);
                    self.registry_extra_outcome = match progress {
                        qcom_modem_owner::ServiceProgress::Idle => 1,
                        qcom_modem_owner::ServiceProgress::Progress => 2,
                        qcom_modem_owner::ServiceProgress::ReplyQueued => 3,
                        qcom_modem_owner::ServiceProgress::WaitingReply => 4,
                    };
                }
                Ok(first)
            },
            Err(error) => Err(error),
        };
        let ended = counter();
        self.timing.record(4, ended.wrapping_sub(started));
            #[cfg(feature = "sunfish-ipa-registration-prep-trace")]
            record_registration_prep_work(&mut self.registration_prep_work[4], owner.ipa_registration_trace(), started, ended);
        #[cfg(feature = "sunfish-ipa-schedule-trace")]
        self.ipa_schedule.observe(ended, 4, ipa_schedule_milestones(owner));
        #[cfg(feature = "sunfish-tftp-oack-phase-observer")]
        self.oack_phases.mark(1, ended);
        registry_result.map_err(|_| 0x52u32)?;
        #[cfg(feature="sunfish-elf-queued-handoff")]
        {self.router.dispatch_queued_elf_reply(owner).map_err(|_|0x61u32)?;}
        let started = counter();
        #[cfg(feature = "sunfish-ipa-schedule-trace")]
        self.ipa_schedule.observe(started, 7, ipa_schedule_milestones(owner));
        let rmtfs_result = owner.dispatch_linux_rmtfs_burst(server, &mut self.router);
        let ended = counter();
        self.timing.record(1, ended.wrapping_sub(started));
            #[cfg(feature = "sunfish-ipa-registration-prep-trace")]
            record_registration_prep_work(&mut self.registration_prep_work[1], owner.ipa_registration_trace(), started, ended);
        #[cfg(feature = "sunfish-ipa-schedule-trace")]
        self.ipa_schedule.observe(ended, 1, ipa_schedule_milestones(owner));
        #[cfg(feature = "sunfish-tftp-oack-phase-observer")]
        self.oack_phases.mark(2, ended);
        self.rmtfs_report = Some(server.report());
        rmtfs_result.map_err(|_| 0x53u32)?;
        if !early_ipa {
            let started = counter();
            #[cfg(feature = "sunfish-ipa-schedule-trace")]
            self.ipa_schedule.observe(started, 7, ipa_schedule_milestones(owner));
            let ipa_result = owner.service_linux_ipa_step(started, &mut self.router);
            #[cfg(feature = "sunfish-ipa-init-continuation")]
            let ipa_result = ipa_result.and_then(|progress| {
                owner.continue_ipa_init_after_discovery(progress, counter, &mut self.router)
            });
            self.ipa_report = owner.linux_ipa_report();
            self.ipa_complete_seen |= self.ipa_report.is_some_and(|r| r.protocol_complete);
            let ended = counter();
            self.timing.record(2, ended.wrapping_sub(started));
            #[cfg(feature = "sunfish-ipa-registration-prep-trace")]
            record_registration_prep_work(&mut self.registration_prep_work[2], owner.ipa_registration_trace(), started, ended);
            #[cfg(feature = "sunfish-ipa-schedule-trace")]
            self.ipa_schedule.observe(ended, 2, ipa_schedule_milestones(owner));
            #[cfg(feature = "sunfish-tftp-oack-phase-observer")]
            self.oack_phases.mark(3, ended);
            ipa_result.map_err(|_| 0x54u32)?;
        }
        #[cfg(all(feature = "sunfish-native-tftp-server-check", not(feature = "sunfish-tftp-early-service-experiment")))]
        tftp_service_step!();
        #[cfg(feature = "sunfish-wlan-state-query")]
        self.wlan_query.step(owner, self.ipa_report.is_some_and(|r| r.protocol_complete), &mut self.router).map_err(|_| 0x56u32)?;
        #[cfg(feature = "sunfish-wlan-state-listener")]
        self.wlan_listener.step(owner, self.ipa_report.is_some_and(|r| r.protocol_complete), &mut self.router).map_err(|_| 0x57u32)?;
        #[cfg(feature = "sunfish-ipa-schedule-trace")]
        self.ipa_schedule.observe(counter(), 7, ipa_schedule_milestones(owner));
        Ok(())
    }

    #[cfg(feature="sunfish-registry-defer-paint")]
    fn registry_display_busy(&self) -> bool {
        matches!(self.stage, Some(Stage::Services(_))) && self.router.report().continuation_pending
    }

    #[cfg(feature="sunfish-registry-defer-paint")]
    fn record_registry_paint(&mut self, start: u64, end: u64) {
        if let Some(Stage::Services(owner)) = self.stage.as_ref()
            && let Some(io) = owner.registry_prearm_io()
        { self.registry_paint.record(io.queued_at, io.first_enter_at, start, end); }
    }

    fn paint_progress(&mut self, display: &Display, phase: u32) {
        #[cfg(feature = "sunfish-ipa-schedule-trace")]
        self.schedule_sample(7);
        let started = counter();
        self.progress_trace.checkpoint(phase);
        #[cfg(feature = "sunfish-tftp-quiet-oack-experiment")]
        if self.awaiting_first_oack_completion() { return; }
        #[cfg(feature="sunfish-registry-defer-paint")]
        { let busy=self.registry_display_busy(); if !self.registry_paint.progress(self.error,busy) { return; } }
        display.runtime_progress(cfg!(feature = "sunfish-ipa-independent-server-runtime"),
            self.progress_trace.phase, self.progress_trace.wake_sent, self.progress_trace.wake_received,
            self.rmtfs_report.map_or(0, |r| r.chunks.min(u64::from(u32::MAX)) as u32));
        display.flush();
        let ended = counter();
        self.timing.record(3, ended.wrapping_sub(started));
        #[cfg(feature="sunfish-registry-defer-paint")]
        self.record_registry_paint(started,ended);
        #[cfg(feature = "sunfish-ipa-registration-prep-trace")]
        if let Some(Stage::Services(owner)) = self.stage.as_ref() {
            record_registration_prep_work(&mut self.registration_prep_work[3], owner.ipa_registration_trace(), started, ended);
        }
        #[cfg(feature = "sunfish-ipa-schedule-trace")]
        self.schedule_sample(3);
    }

    fn timed_show(&mut self, display: &Display) {
        #[cfg(feature = "sunfish-ipa-schedule-trace")]
        self.schedule_sample(7);
        let started = counter();
        self.show(display);
        let ended = counter();
        self.timing.record(3, ended.wrapping_sub(started));
        #[cfg(feature="sunfish-registry-defer-paint")]
        self.record_registry_paint(started,ended);
        #[cfg(feature = "sunfish-ipa-registration-prep-trace")]
        if let Some(Stage::Services(owner)) = self.stage.as_ref() {
            record_registration_prep_work(&mut self.registration_prep_work[3], owner.ipa_registration_trace(), started, ended);
        }
        #[cfg(feature = "sunfish-ipa-schedule-trace")]
        self.schedule_sample(3);
    }

    #[cfg(feature = "sunfish-tftp-quiet-oack-experiment")]
    fn awaiting_first_oack_completion(&self) -> bool {
        match self.stage.as_ref() {
            Some(Stage::Services(owner)) => {
                let timing = owner.tftp_timing();
                timing.request.is_some() && timing.done.is_none()
            }
            _ => false,
        }
    }

    fn service_owner(&mut self, state: &mut Startup) -> Result<(), u32> {
        let display = state._irq_registers.map(|registers| unsafe { Display::new(registers.pixels) });
        for pass in 0..qcom_modem_owner::WORK_BUDGET {
            if pass == 0 && self.rmtfs_report.is_some_and(|r| r.requests >= 7) {
                if let Some(display) = &display { self.paint_progress(display, 1); }
            }
            let Some(Stage::Services(owner)) = self.stage.as_mut() else {
                return Err(0x60);
            };
            #[cfg(feature = "sunfish-tftp-publish-io-observer")]
            let before_flags = owner.tftp_endpoint_trace().map(|trace| trace.flags);
            #[cfg(feature = "sunfish-tftp-publish-io-observer")]
            let before_io = owner.with_io(|io| { io.owner_timing_enable(true); io.owner_timing() });
            #[cfg(not(feature = "sunfish-tftp-publish-io-observer"))]
            owner.with_io(|io| io.owner_timing_enable(true));
            let started = counter();
            #[cfg(feature = "sunfish-ipa-schedule-trace")]
            self.ipa_schedule.observe(started, 7, ipa_schedule_milestones(owner));
            #[cfg(feature = "sunfish-grant-rx-open")]
            let dispatched = self.router.dispatch_with_grant_receive_turn(owner);
            #[cfg(not(feature = "sunfish-grant-rx-open"))]
            let dispatched = self.router.dispatch(owner);
            self.ipa_report = owner.linux_ipa_report();
            self.ipa_complete_seen |= self.ipa_report.is_some_and(|r| r.protocol_complete);
            let ended = counter();
            self.timing.record(0, ended.wrapping_sub(started));
            #[cfg(feature = "sunfish-ipa-registration-prep-trace")]
            record_registration_prep_work(&mut self.registration_prep_work[0], owner.ipa_registration_trace(), started, ended);
            #[cfg(feature = "sunfish-ipa-schedule-trace")]
            self.ipa_schedule.observe(ended, 0, ipa_schedule_milestones(owner));
            #[cfg(feature = "sunfish-tftp-oack-phase-observer")]
            self.oack_phases.begin(owner.tftp_timing().request, ended);
            self.io_timing = owner.with_io(|io| { io.owner_timing_enable(false); io.owner_timing() });
            #[cfg(feature = "sunfish-wlan-elf-gap-io")]
            self.registry_gap.record_router_io(
                [
                    before_io.total[0], before_io.total[1], before_io.total[2], before_io.total[3],
                    before_io.detail_total[0], before_io.detail_total[1], before_io.detail_total[2],
                    before_io.detail_total[3], before_io.detail_total[4], before_io.detail_total[5],
                    before_io.barrier_domain_total[0], before_io.barrier_domain_total[1],
                ],
                [
                    self.io_timing.total[0], self.io_timing.total[1], self.io_timing.total[2], self.io_timing.total[3],
                    self.io_timing.detail_total[0], self.io_timing.detail_total[1], self.io_timing.detail_total[2],
                    self.io_timing.detail_total[3], self.io_timing.detail_total[4], self.io_timing.detail_total[5],
                    self.io_timing.barrier_domain_total[0], self.io_timing.barrier_domain_total[1],
                ],
            );
            #[cfg(feature = "sunfish-tftp-publish-io-observer")]
            self.oack_publish_io.observe(before_flags, owner.tftp_endpoint_trace().map(|trace| trace.flags), before_io.total, self.io_timing.total);
            #[cfg(feature = "sunfish-tftp-publish-op-observer")]
            self.oack_publish_ops.observe(before_flags, owner.tftp_endpoint_trace().map(|trace| trace.flags), before_io.detail_total, self.io_timing.detail_total);
            #[cfg(feature = "sunfish-tftp-barrier-domain-observer")]
            {
                let after = self.io_timing.barrier_domain_observation(&before_io);
                self.oack_barrier_domains.observe(before_flags, owner.tftp_endpoint_trace().map(|trace| trace.flags), before_io.barrier_domain_total, after);
            }
            dispatched.map_err(|_| 0x61u32)?;
            #[cfg(feature = "sunfish-tftp-wlan-close-handoff")]
            if owner.pending_firmware_close().is_some() {
                owner.resolve_firmware_close(&mut self.firmware).map_err(|_| 0x63u32)?;
            }
            #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
            if owner.pending_tftp_close().is_some() {
                owner.resolve_pending_tftp_close(&mut self.server_check).map_err(|_| 0x62u32)?;
            }
            self.service_pass(state)?;
            if !self.router.report().continuation_pending {
                return Ok(());
            }
        }
        // The router retains the work flag. A software self-notification
        // resumes it after this bounded slice without inventing an IRQ.
        if self.router.report().continuation_pending {
            self.progress_trace.sent();
            if let Some(display) = &display { self.paint_progress(display, 3); }
            Channel::new(irq::CONTINUATION_SEND_CHANNEL as usize).notify();
        }
        Ok(())
    }

    fn notified(&mut self, display: &Display, state: &mut Startup, badge: u64) {
        #[cfg(feature = "sunfish-ipa-schedule-trace")]
        self.schedule_sample(6);
        #[cfg(feature = "sunfish-wlan-elf-registry-gap")]
        self.registry_gap.begin_callback(counter(), badge);
        if self.error != 0 {
            self.show(display);
            return;
        }
        let previous_ipa_report = self.ipa_report;
        #[cfg(feature = "sunfish-wlan-state-listener")]
        let previous_wlan_listener = match self.stage.as_ref() {
            Some(Stage::Services(owner)) => Some(self.wlan_listener.snapshot(owner)), _ => None,
        };
        #[cfg(feature = "sunfish-wlan-state-query")]
        let previous_wlan_query = match self.stage.as_ref() {
            Some(Stage::Services(owner)) => Some(self.wlan_query.snapshot(owner)), _ => None,
        };
        #[cfg(feature = "sunfish-tftp-endpoint-observer")]
        let previous_tftp_endpoint = match self.stage.as_ref() {
            Some(Stage::Services(owner)) => owner.tftp_endpoint_trace(), _ => None,
        };
        #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
        let previous_close_commits = match self.stage.as_ref() {
            Some(Stage::Services(owner)) => owner.tftp_close_commits(), _ => 0,
        };
        #[cfg(feature = "sunfish-glink-intent-request-experiment")]
        let previous_intent_status = self.intent_status;
        #[cfg(feature = "sunfish-tftp-missing-temp-replies")]
        let previous_missing = (self.server_check.missing_requests(), match self.stage.as_ref() {
            Some(Stage::Services(owner)) => owner.tftp_missing_reply_trace(), _ => None,
        });
        #[cfg(feature = "sunfish-tftp-final-ack-observer")]
        let previous_final_ack = match self.stage.as_ref() {
            Some(Stage::Services(owner)) => owner.tftp_final_ack_trace(), _ => None,
        };
        #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
        let previous_oacks = match self.stage.as_ref() {
            Some(Stage::Services(owner)) => owner.server_check_oacks(),
            _ => [None; 2],
        };
        #[cfg(feature = "sunfish-tftp-oack-resources")]
        let previous_peer_49 = match self.stage.as_ref() {
            Some(Stage::Services(owner)) => owner.server_check_49(),
            _ => None,
        };
        #[cfg(feature = "sunfish-tftp-oack-slot-follow")]
        let previous_slot = match self.stage.as_ref() {
            Some(Stage::Services(owner)) => owner.oack_slot_follow(),
            _ => None,
        };
        #[cfg(feature = "sunfish-tftp-oack-peer-tail")]
        let previous_oack_tail = match self.stage.as_ref() {
            Some(Stage::Services(owner)) => owner.oack_peer_tail().map(|tail| (tail.first_crossed, tail.at_close)),
            _ => None,
        };
        #[cfg(all(feature = "sunfish-wlan-first-wrq-window", not(feature = "sunfish-ipa-registration-trace")))]
        let previous_wrq_window = match self.stage.as_ref() {
            Some(Stage::Services(owner)) => Some(owner.first_wrq_window()),
            _ => None,
        };
        #[cfg(feature = "sunfish-ipa-registration-trace")]
        let previous_registration = match self.stage.as_ref() {
            Some(Stage::Services(owner)) => owner.ipa_registration_trace(),
            _ => None,
        };
        #[cfg(feature = "sunfish-ipa-registration-owner-trace")]
        let previous_registration_owner = match self.stage.as_ref() {
            Some(Stage::Services(owner)) => owner.ipa_registration_owner_trace(),
            _ => None,
        };
        #[cfg(feature = "sunfish-ipa-init-continuation")]
        let previous_ipa_init = match self.stage.as_ref() {
            Some(Stage::Services(owner)) => owner.ipa_init_trace(),
            _ => (None, None),
        };
        #[cfg(feature = "sunfish-wlan-startup-trace")]
        let previous_registry = match self.stage.as_ref() {
            Some(Stage::Services(owner)) => Some(owner.service_registry_trace()),
            _ => None,
        };
        #[cfg(feature = "sunfish-wlan-intent-ack-turn")]
        let previous_ack_summary = match self.stage.as_ref() {
            Some(Stage::Services(owner)) => Some(owner.intent_ack_summary()),
            _ => None,
        };
        #[cfg(feature = "sunfish-tftp-mcfg-trace")]
        let previous_mcfg_trace = match self.stage.as_ref() {
            Some(Stage::Services(owner)) => owner.mcfg_trace(),
            _ => (None, None),
        };
        #[cfg(feature = "sunfish-tftp-mcfg-ram")]
        let previous_mcfg = self.server_check.mcfg_progress();
        #[cfg(feature="sunfish-post-elf-mcfg-trace")]
        let previous_post_mcfg=match self.stage.as_ref(){Some(Stage::Services(owner))=>owner.post_elf_mcfg().map_or(0,|t|t.revision),_=>0};
        #[cfg(feature = "sunfish-native-tftp-server-check")]
        let previous_server_check = (self.server_check_queued, self.server_check_ignored, self.server_check.total_bytes(), self.server_check.completed_count(), self.server_check.active_count());
        #[cfg(feature = "sunfish-native-tftp-server-check")]
        let previous_coalesced = match self.stage.as_ref() {
            Some(Stage::Services(owner)) => owner.tftp_wrq_coalesced(), _ => 0,
        };
        #[cfg(feature = "sunfish-native-tftp-server-check")]
        let previous_tftp_tx = match self.stage.as_ref() {
            Some(Stage::Services(owner)) => owner.tftp_tx_progress(), _ => [0; 3],
        };
        #[cfg(feature = "sunfish-native-tftp-server-check")]
        let previous_tftp_wire = match self.stage.as_ref() {
            Some(Stage::Services(owner)) => owner.tftp_wire_counts(), _ => [0; 4],
        };
        #[cfg(feature = "sunfish-native-tftp-observer")]
        let previous_observer = match self.stage.as_ref() {
            Some(Stage::Services(owner)) => Some((owner.tftp_observer_trace(), owner.control_status())),
            _ => None,
        };
        if badge & (1 << irq::CONTINUATION_RECEIVE_CHANNEL) != 0 {
            self.progress_trace.received();
            self.progress_trace.checkpoint(4);
        }
        #[cfg(feature = "sunfish-glink-intent-request-experiment")]
        let deadline_only = diagnostic::intent_deadline_only(self.intent_status.0, badge,
            1 << irq::CONTINUATION_RECEIVE_CHANNEL, self.router.report().continuation_pending,
            self.router.report().pending_modem);
        #[cfg(not(feature = "sunfish-glink-intent-request-experiment"))]
        let deadline_only = false;
        let captured = self.router.capture(badge);
        let other = self.router.take_other();
        if other != 0 {
            if let Err(detail) = self.dispatch_receive(state, other, captured.is_ok()) {
                self.detail = detail;
                self.fail(state, 0x70);
                self.show(display);
                return;
            }
        }
        if captured.is_err() {
            self.fail(state, 0x71);
            self.show(display);
            return;
        }
        match self.stage.as_ref() {
            Some(Stage::Opening(_)) => {
                self.step_opening(state, badge & (1 << irq::GLINK_CHANNEL) != 0);
            }
            Some(Stage::Adapter(_)) => self.fail(state, 0x72),
            Some(Stage::Services(_)) => {
                if !deadline_only && let Err(detail) = self.service_owner(state) {
                    self.detail = detail;
                    self.fail(state, 0x73);
                }
            }
            None => self.fail(state, 0x74),
        }
        #[cfg(feature = "sunfish-glink-intent-request-experiment")]
        {
            self.intent_status = match self.stage.as_ref() {
                Some(Stage::Services(owner)) => owner.outbound_intent_diagnostic(),
                _ => (0, 0),
            };
        }
        #[cfg(feature = "sunfish-glink-intent-request-experiment")]
        if self.error == 0 {
            let (wake, timeout) = self.intent_deadline.update(self.intent_status.0, counter(), self.hz);
            if timeout {
                self.fail(state, 0x75);
            } else if wake {
                // This one-PD image has no userspace timer service. Reuse the
                // audited self-notification for a bounded 10-second poll; no
                // additional GLINK request or hardware IRQ is generated.
                Channel::new(irq::CONTINUATION_SEND_CHANNEL as usize).notify();
            }
        }
        #[cfg(feature = "sunfish-native-tftp-observer")]
        let report_changed = {
            let current_observer = match self.stage.as_ref() {
                Some(Stage::Services(owner)) => Some((owner.tftp_observer_trace(), owner.control_status())),
                _ => None,
            };
            self.ipa_report != previous_ipa_report || current_observer != previous_observer
        };
        #[cfg(feature = "sunfish-native-tftp-server-check")]
        let report_changed = self.ipa_report != previous_ipa_report
            || previous_server_check != (self.server_check_queued, self.server_check_ignored, self.server_check.total_bytes(), self.server_check.completed_count(), self.server_check.active_count())
            || match self.stage.as_ref() { Some(Stage::Services(owner)) => owner.tftp_wrq_coalesced() != previous_coalesced || owner.tftp_tx_progress() != previous_tftp_tx || owner.tftp_wire_counts() != previous_tftp_wire, _ => false };
        #[cfg(not(any(feature = "sunfish-native-tftp-observer", feature = "sunfish-native-tftp-server-check")))]
        let report_changed = self.ipa_report != previous_ipa_report;
        #[cfg(feature = "sunfish-wlan-startup-trace")]
        let report_changed = report_changed || match self.stage.as_ref() {
            Some(Stage::Services(owner)) => Some(owner.service_registry_trace()) != previous_registry,
            _ => false,
        };
        #[cfg(feature = "sunfish-wlan-intent-ack-turn")]
        let report_changed = report_changed || match self.stage.as_ref() {
            Some(Stage::Services(owner)) => Some(owner.intent_ack_summary()) != previous_ack_summary,
            _ => false,
        };
        #[cfg(feature = "sunfish-tftp-mcfg-trace")]
        let report_changed = report_changed || match self.stage.as_ref() {
            Some(Stage::Services(owner)) => owner.mcfg_trace() != previous_mcfg_trace,
            _ => false,
        };
        #[cfg(feature = "sunfish-tftp-mcfg-ram")]
        let report_changed = report_changed || previous_mcfg != self.server_check.mcfg_progress();
        #[cfg(feature="sunfish-post-elf-mcfg-trace")]
        let report_changed=report_changed || match self.stage.as_ref(){Some(Stage::Services(owner))=>owner.post_elf_mcfg().map_or(0,|t|t.revision)!=previous_post_mcfg,_=>false};
        #[cfg(feature = "sunfish-tftp-missing-temp-replies")]
        let report_changed = report_changed || previous_missing != (self.server_check.missing_requests(), match self.stage.as_ref() {
            Some(Stage::Services(owner)) => owner.tftp_missing_reply_trace(), _ => None,
        });
        #[cfg(feature = "sunfish-tftp-final-ack-observer")]
        let report_changed = report_changed || match self.stage.as_ref() {
            Some(Stage::Services(owner)) => owner.tftp_final_ack_trace() != previous_final_ack, _ => false,
        };
        #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
        let report_changed = report_changed || match self.stage.as_ref() {
            Some(Stage::Services(owner)) => owner.server_check_oacks() != previous_oacks,
            _ => false,
        };
        #[cfg(feature = "sunfish-tftp-oack-resources")]
        let report_changed = report_changed || match self.stage.as_ref() {
            Some(Stage::Services(owner)) => owner.server_check_49() != previous_peer_49,
            _ => false,
        };
        #[cfg(feature = "sunfish-tftp-oack-slot-follow")]
        let report_changed = report_changed || match self.stage.as_ref() {
            Some(Stage::Services(owner)) => owner.oack_slot_follow() != previous_slot,
            _ => false,
        };
        #[cfg(feature = "sunfish-tftp-oack-peer-tail")]
        let report_changed = report_changed || match self.stage.as_ref() {
            Some(Stage::Services(owner)) => owner.oack_peer_tail()
                .map(|tail| (tail.first_crossed, tail.at_close)) != previous_oack_tail,
            _ => false,
        };
        #[cfg(all(feature = "sunfish-wlan-first-wrq-window", not(feature = "sunfish-ipa-registration-trace")))]
        let report_changed = report_changed || match self.stage.as_ref() {
            Some(Stage::Services(owner)) => owner.server_check_oacks()[0]
                .is_some_and(|trace| trace.oack_at.is_some() || trace.first_close_at.is_some())
                && Some(owner.first_wrq_window()) != previous_wrq_window,
            _ => false,
        };
        #[cfg(feature = "sunfish-ipa-registration-trace")]
        let report_changed = report_changed || match self.stage.as_ref() {
            Some(Stage::Services(owner)) => owner.ipa_registration_trace() != previous_registration,
            _ => false,
        };
        #[cfg(feature = "sunfish-ipa-registration-owner-trace")]
        let report_changed = report_changed || match self.stage.as_ref() {
            Some(Stage::Services(owner)) => owner.ipa_registration_owner_trace() != previous_registration_owner,
            _ => false,
        };
        #[cfg(feature = "sunfish-ipa-init-continuation")]
        let report_changed = report_changed || match self.stage.as_ref() {
            Some(Stage::Services(owner)) => owner.ipa_init_trace() != previous_ipa_init,
            _ => false,
        };
        #[cfg(feature = "sunfish-glink-intent-request-experiment")]
        let report_changed = report_changed || self.intent_status != previous_intent_status;
        #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
        let report_changed = report_changed || match self.stage.as_ref() {
            Some(Stage::Services(owner)) => owner.tftp_close_commits() != previous_close_commits, _ => false,
        };
        #[cfg(feature = "sunfish-tftp-endpoint-observer")]
        let report_changed = report_changed || match self.stage.as_ref() {
            Some(Stage::Services(owner)) => owner.tftp_endpoint_trace() != previous_tftp_endpoint, _ => false,
        };
        #[cfg(feature = "sunfish-wlan-state-query")]
        let report_changed = report_changed || previous_wlan_query != match self.stage.as_ref() {
            Some(Stage::Services(owner)) => Some(self.wlan_query.snapshot(owner)), _ => None,
        };
        #[cfg(feature = "sunfish-wlan-state-listener")]
        let report_changed = report_changed || previous_wlan_listener != match self.stage.as_ref() {
            Some(Stage::Services(owner)) => Some(self.wlan_listener.snapshot(owner)), _ => None,
        };
        let repaint = diagnostic::should_repaint(self.error, badge,
            1 << irq::CONTINUATION_RECEIVE_CHANNEL, report_changed);
        #[cfg(feature = "sunfish-tftp-quiet-oack-experiment")]
        let repaint = if repaint {
            let waiting = self.awaiting_first_oack_completion();
            self.oack_paint_gate.allow(waiting, self.error != 0, counter(), self.hz)
        } else { false };
        #[cfg(feature="sunfish-registry-defer-paint")]
        let repaint = { let busy=self.registry_display_busy(); self.registry_paint.full(self.error,busy,repaint) };
        if repaint { self.timed_show(display); }
        #[cfg(feature = "sunfish-wlan-elf-registry-gap")]
        self.registry_gap.end_callback(counter());
        #[cfg(feature = "sunfish-ipa-schedule-trace")]
        self.schedule_sample(7);
    }

    fn fail(&mut self, state: &mut Startup, code: u32) {
        if self.error == 0 {
            self.error = code;
        }
        self.router.quarantine();
        (&mut *self.boot).quarantine();
        if let Ok(server) = (&mut *self.boot).server() {
            server.fault();
        }
        match self.stage.as_mut() {
            Some(Stage::Opening(opening)) => opening.with_io(|io| io.fault_boot()),
            Some(Stage::Adapter(adapter)) => adapter.suspend(),
            Some(Stage::Services(owner)) => owner.fault(),
            None => {}
        }
        pixel_ipa_receive::quarantine_runtime(state, &mut self.receive, code);
    }

    fn show(&self, display: &Display) {
        display.clear();
        display.replace_line(0, "NATIVE MPSS GLINK QRTR");
        match self.stage.as_ref() {
            Some(Stage::Opening(opening)) => {
                let report = opening.report();
                display.replace_line(
                    1,
                    match report.phase {
                        BootPhase::New => "GLINK WAITING FOR PEER",
                        BootPhase::Negotiating => "GLINK NEGOTIATING",
                        BootPhase::Ready => "GLINK OPEN PASS",
                        BootPhase::Failed => "GLINK TRANSPORT STOP",
                    },
                );
                display.hex(2, "GLINK TX", report.tx_records);
                display.hex(3, "GLINK RX", report.rx_records);
                display.hex(
                    4,
                    "REMOTE CHANNEL",
                    u32::from(report.remote_channel.unwrap_or(0)),
                );
                display.hex(5, "OTHER CHANNELS", report.other_channels as u32);
            }
            Some(Stage::Adapter(_)) => display.replace_line(1, "OWNER ATTACH STOP"),
            Some(Stage::Services(owner)) => {
                display.replace_line(1, "QRTR SERVICE RUNTIME");
                let status = owner.control_status();
                display.hex(2, "QRTR READY", u32::from(status.ready_services));
                display.hex(3, "QRTR ANNOUNCED", u32::from(status.announced_services));
                display.hex(
                    4,
                    "SERVREG STAGED",
                    u32::from(self.service_registry_prepared),
                );
                display.hex(5, "RMTFS BOUND", u32::from(self.rmtfs_bound));
                display.hex(
                    6,
                    "IPA CLIENT",
                    u32::from(owner.linux_ipa_report().is_some()),
                );
                display.hex(7, "OWNER ERROR", u32::from(owner.error().is_some()));
                if let Some(report) = self.ipa_report {
                        let state = report.protocol;
                        display.hex(4, "IPA CORE", u32::from(state.is_some()));
                        let flags = diagnostic::ipa_flags(
                            report.pending_record, report.record_queued, report.held_response,
                            self.ipa_complete_seen,
                            state.map(|s| [s.modem_ready, s.qmi_uc_ready,
                                s.indication_requested, s.indication_sent, s.initial_boot, s.failed]),
                        );
                        display.hex(5, "IPA FLAGS", flags);
                        display.hex(6, "IPA COMMITS", report.publications_committed);
                        display.hex(7, "IPA RESPONSES", report.responses_accepted);
                }
            }
            None => display.replace_line(1, "RUNTIME OWNER MISSING"),
        }
        display.hex(
            9,
            "Q6 READY",
            u32::from(self.ready.generation() == GENERATION),
        );
        display.hex(
            10,
            "OTHER BADGE",
            self.router.report().pending_other as u32,
        );
        display.hex(11, "GLINK ACKS", self.router.report().acknowledgements);
        display.hex(12, "RUNTIME ERROR", self.error);
        if self.error == 0 {
            if let Some(Stage::Services(owner)) = self.stage.as_ref() {
                display.hex(13, "CLOSED PORTS", owner.retired_rmtfs_peer_count() as u32);
                display.hex(14, "PEER SERVERS", owner.control_status().servers as u32);
                display.hex(15, "CHECKPOINT", self.progress_trace.phase);
                display.hex(16, "WAKE TRY", self.progress_trace.wake_sent);
                display.hex(17, "WAKE RECV", self.progress_trace.wake_received);
                display.hex(18, "RMTFS CHUNKS", self.rmtfs_report.map_or(0, |r| r.chunks.min(u64::from(u32::MAX)) as u32));
            }
            if let Some(report) = self.rmtfs_report {
                display.hex(19, "RMTFS REQUESTS", report.requests.min(u64::from(u32::MAX)) as u32);
                display.hex(20, "RMTFS REPLIES", report.replies_queued.min(u64::from(u32::MAX)) as u32);
                display.hex(21, "RMTFS HANDLES", report.open_handles as u32);
                display.hex(22, "RMTFS ACTIVE", u32::from(report.active_request));
                display.hex(23, "RMTFS MSG", report.active_message.map_or(u32::MAX, u32::from));
            }
        }
        if self.error != 0 {
            display.hex(13, "ERROR DETAIL", self.detail);
            if let Some(Stage::Services(owner)) = self.stage.as_ref() {
                if let Some(info) = owner.open_diagnostic() {
                    display.hex(14, "OPEN STAGE", info.stage);
                    display.hex(15, "OPEN CHANNEL", info.channel);
                    display.hex(16, "OPEN LENGTH", info.advertised_length);
                    display.hex(17, "OPEN REASON", info.reason);
                    display.hex(23, "OPEN OTHERS", info.other_channels);
                    let mut name_bytes = info.name;
                    name_bytes.make_ascii_uppercase();
                    let visible_len = name_bytes[..info.name_len].iter().position(|byte| *byte == 0).unwrap_or(info.name_len);
                    let name = core::str::from_utf8(&name_bytes[..visible_len]).unwrap_or("NON ASCII");
                    display.replace_line(28, name);
                } else if let Some(info) = owner.client_gone_diagnostic() {
                    display.hex(14, "QRTR COMMAND", info.command);
                    display.hex(15, "PEER PORT", info.port);
                    display.hex(16, "QMI SERVICE", info.service.descriptor().0);
                    display.hex(17, "RX STATE", info.receive_state);
                    display.hex(18, "TX COUNT", info.reply_count);
                    display.hex(19, "TX SENT", info.reply_sent);
                    display.hex(20, "TX LENGTH", info.reply_len);
                    let flags = u32::from(info.reply_glink_done)
                        | (u32::from(info.reply_qrtr_done) << 1)
                        | (u32::from(info.done_pending) << 2);
                    display.hex(21, "TX FLAGS", flags);
                } else if let Some(evidence) = owner.crash_evidence().filter(|e| e.after.is_some()) {
                    use personal_os_rust_server::qcom_modem_crash as crash;
                    match evidence.after {
                        Some(Ok(snapshot)) => {
                            for row in 0..4 {
                                let text = crash::text_row(&snapshot, row);
                                display.replace_line(14 + row, core::str::from_utf8(&text).unwrap_or("REASON ENCODING ERROR"));
                            }
                            let changed = crash::evidence_changed(evidence.before, evidence.after);
                            let flags = u32::from(snapshot.directory.item.is_some())
                                | (u32::from(changed.is_some()) << 1)
                                | (u32::from(changed == Some(true)) << 2)
                                | (u32::from(snapshot.terminated) << 3)
                                | (u32::from(snapshot.text_len > 96 || snapshot.truncated) << 4);
                            display.hex(23, "REASON FLAGS", flags);
                        }
                        Some(Err(error)) => display.hex(14, "REASON ERROR", error.code()),
                        None => {},
                    }
                } else if let Some(error) = owner.error() {
                    let text = diagnostic::ErrorText::from_debug(&error);
                    for row in 0..3 { display.replace_line(14 + row, text.line(row)); }
                    display.hex(17, "OWNER TRUNC", u32::from(text.truncated()));
                }
                display.hex(22, "IO DIAG", owner.transport_diagnostic());
            }
            let client_gone = matches!(self.stage.as_ref(), Some(Stage::Services(owner)) if owner.client_gone_diagnostic().is_some());
            if !client_gone {
                display.hex(18, "WAKE RECV", self.progress_trace.wake_received);
                display.hex(19, "ELAPSED MS", diagnostic::ticks_to_ms(counter().wrapping_sub(self.runtime_started), self.hz));
                display.hex(20, "OWNER TOTAL MS", diagnostic::ticks_to_ms(self.timing.total[0], self.hz));
                display.hex(21, "PAINT TOTAL MS", diagnostic::ticks_to_ms(self.timing.total[3], self.hz));
                if !matches!(self.stage.as_ref(), Some(Stage::Services(owner)) if owner.open_diagnostic().is_some() || owner.crash_evidence().is_some_and(|e| e.after.is_some())) {
                    display.hex(23, "RMTFS MSG", self.rmtfs_report.and_then(|r| r.active_message).map_or(u32::MAX, u32::from));
                }
            }
        }
        if self.error != 0 {
            // Last completed server report, captured before failure/quarantine.
            // A missing report is distinct from a measured zero.
            let report = self.rmtfs_report;
            if let Some(Stage::Services(owner)) = self.stage.as_ref() {
                display.hex(18, "READ RSP FLAGS", owner.rmtfs_large_reply_flags());
                let event_ms = |index| owner.rmtfs_event_time(index)
                    .map_or(u32::MAX, |ticks| diagnostic::ticks_to_ms(ticks.wrapping_sub(self.runtime_started), self.hz));
                display.hex(19, "QUEUE EVENT MS", event_ms(0));
                display.hex(20, "ERROR EVENT MS", event_ms(4));
                display.hex(21, "PUB EVENT MS", event_ms(1));
                display.hex(26, "RXDONE EVENT MS", event_ms(2));
                display.hex(27, "RESUME EVENT MS", event_ms(3));
            }
            display.hex(10, "RMTFS HANDLES", report.map_or(u32::MAX, |r| r.open_handles.min(u32::MAX as usize) as u32));
            display.hex(24, "RMTFS REQUESTS", report.map_or(u32::MAX, |r| r.requests.min(u64::from(u32::MAX)) as u32));
            display.hex(25, "RMTFS REPLIES", report.map_or(u32::MAX, |r| r.replies_queued.min(u64::from(u32::MAX)) as u32));
        } else {
            display.replace_line(24, "BOUNDED QRTR DISCOVERY ONLY");
            display.replace_line(25, "NO TFTP OR WLFW START");
            display.replace_line(26, "NO WLAN OR NETWORK FRAMES");
            display.replace_line(27, "NO FLASH OR RESET RETRY");
            // These are already-retained software counters, not new MMIO
            // reads or evidence that a missing interrupt never occurred.
            if self.ipa_report.is_some_and(|report| {
                !report.pending_record && report.protocol.is_some_and(|s|
                    s.modem_ready && s.indication_requested && !s.qmi_uc_ready)
            }) {
                let (uc, _) = self.receive.ipa_reports();
                display.hex(14, "UC IRQ NOTICES", uc.notifications);
                display.hex(15, "UC STATUS READS", uc.status_reads);
                display.hex(16, "UC INIT OBS", uc.init_observed);
                display.hex(17, "UC LAST RESP", uc.last_response.map_or(u32::MAX, u32::from));
                display.hex(18, "UC LAST STATUS", uc.last_status.unwrap_or(u32::MAX));
                if let Some(report) = self.ipa_report {
                    display.hex(19, "QMI REG RX", report.registration_requests);
                    display.hex(20, "QMI DRIVER RX", report.driver_requests);
                    display.hex(21, "INTENT ASK", report.peer_intent_requests);
                    display.hex(22, "INTENT DENY", report.peer_intent_denied);
                }
                if let Some(Stage::Services(owner)) = self.stage.as_ref() {
                    display.hex(23, "INTENT READY", owner.linux_ipa_receive_available());
                    let snapshot = owner.receive_snapshot();
                    display.hex(14, "SLOT STATES", snapshot.states);
                    display.hex(15, "SLOT SERVICE", snapshot.services);
                    display.hex(16, "SLOT REPLIES", snapshot.replies);
                    if let Some(denial) = owner.intent_denial_diagnostic() {
                        display.hex(17, "DENY SIZE", denial.requested);
                        display.hex(18, "DENY STATES", denial.states);
                    }
                }
            }
        }
        if let Some(Stage::Services(owner)) = self.stage.as_ref() {
            if let Some(info) = owner.capacity_diagnostic() {
                display.hex(14, "CAP PHASE", info.phase);
                display.hex(15, "CAP COMMAND", info.command);
                display.hex(16, "CAP PARAM", info.parameter);
                display.hex(17, "RX FREE", info.rx_free);
                display.hex(18, "TX FREE", info.tx_free);
                display.hex(19, "REMOTE FREE", info.remote_free);
                display.hex(20, "REMOTE BUSY", info.remote_busy);
                display.hex(21, "QMI REQUESTS", info.requests);
                display.hex(22, "FLOWS USED", info.flow_used);
                display.hex(23, "FLOWS ACTIVE", info.flow_active);
                display.hex(26, "ACK PENDING", u32::from(info.intent_ack_pending));
                display.hex(27, "RETIRED PEERS", info.retired_peers);
            }
        }
        if let Some(failure) = self.receive_failure.as_ref() {
            display.hex(14, "RX BADGE LOW", failure.badge as u32);
            display.hex(15, "RX BADGE HIGH", (failure.badge >> 32) as u32);
            display.hex(16, "RX SCOPE", failure.scope);
            display.hex(17, "RX NATIVE ERR", failure.native_error);
            for row in 0..3 { display.replace_line(18 + row, failure.text.line(row)); }
            display.hex(21, "RX TRUNC", u32::from(failure.text.truncated()));
        }
        if let Some((stage, space, offset, observed, expected, mask)) = self.guard_failure {
            display.hex(14, "GUARD STAGE", stage);
            display.hex(15, "GUARD SPACE", space);
            display.hex(16, "GUARD OFFSET", offset);
            display.hex(17, "GUARD ACTUAL", observed);
            display.hex(18, "GUARD EXPECT", expected);
            display.hex(19, "GUARD MASK", mask);
        }
        if let Some((actual, mismatch, baseline, last_good, count)) = self.config_failure {
          if mismatch != 0 {
            display.hex(14, "GUARD STAGE", 3);
            display.hex(15, "CONFIG DIFF", mismatch);
            for (index, label) in ["BCR", "TX CFG", "CLKON", "COMP CFG", "QSB WRITES", "QSB READS", "COUNTER", "HASH ENABLE", "IDLE CFG"].into_iter().enumerate() {
                display.hex(16 + index, label, actual[index]);
            }
            display.hex(25, "CLK BASE", baseline);
            display.hex(26, "CLK LAST GOOD", last_good);
            display.hex(27, "GOOD SNAPSHOTS", count);
            display.replace_line(28, "CONFIG DIFF BIT PER ROW");
          }
        }
        // The runtime currently has no timer wake for a silent IPA peer. Show
        // the retained request once its early response is held as well as on a
        // timeout, without consuming either record or scheduling extra work.
        let live_ipa_pending = self.error == 0
            && self.ipa_report.is_some_and(|report| report.pending_record && report.held_response);
        if live_ipa_pending || (self.error == 0x73 && self.detail == 0x54) {
            if let Some(Stage::Services(owner)) = self.stage.as_ref() {
                if live_ipa_pending || matches!(owner.error(), Some(qcom_modem_owner::Error::LinuxIpa(qcom_modem_owner::LinuxIpaError::Timeout))) {
                    if let Some(info) = owner.linux_ipa_tx_diagnostic() {
                        display.hex(14, "IPA TX LENGTH", info.len);
                        display.hex(15, "IPA TX SENT", info.sent);
                        display.hex(16, "IPA TX FLAGS", info.flags);
                        display.hex(17, "IPA TX INTENT", info.remote_intent);
                        display.hex(18, "IPA TXN", info.tx_transaction);
                        display.hex(19, "RSP TXN RAW", info.response_transaction);
                        display.hex(20, "RSP RESULT RAW", info.response_result);
                        display.hex(21, "RSP ERROR RAW", info.response_error);
                        display.replace_line(23, "FLAGS G Q C I R");
                    }
                }
            }
        }
        if self.error != 0 {
            if let Some(Stage::Services(owner)) = self.stage.as_ref() {
                if let Some(info) = owner.routing_diagnostic() {
                    display.hex(10, "WIRE SOURCE", info.source);
                    display.hex(18, "WIRE SERVICE", info.service);
                    display.hex(19, "WIRE BODY SIZE", info.body_bytes);
                    display.hex(20, "WIRE MSG ID", info.message_id);
                    display.hex(21, "WIRE TXN", info.transaction);
                    display.hex(22, "WIRE TLV 1", info.tlvs[0]);
                    display.hex(23, "WIRE TLV 2", info.tlvs[1]);
                    display.hex(24, "WIRE DEST", info.destination);
                    display.hex(25, "WIRE KIND", info.kind);
                }
            }
        }
        if self.reference_pair_count != 0
            && !matches!(self.stage.as_ref(), Some(Stage::Services(owner)) if owner.open_diagnostic().is_some()) {
            display.hex(28, "PAIR ACCEPTED", self.reference_pair_count);
        }
        if let Some(report) = self.ipa_report {
            if self.error == 0 && report.ignored_init_responses != 0 {
                display.hex(16, "INIT RSP IGN", report.ignored_init_responses);
            }
            if report.ignored_config_requests != 0 {
                display.hex(28, "CONFIG IGNORED", report.ignored_config_requests);
            }
            if let Some(id) = report.last_ignored_unhandled_id {
                display.hex(27, "IGNORED COUNT", report.ignored_unhandled_requests);
                display.hex(28, "IGNORED MSG ID", u32::from(id));
            }
        }
        if self.error != 0 {
            if let Some(Stage::Services(owner)) = self.stage.as_ref() {
                if let Some(info) = owner.session_failure_diagnostic() {
                    display.hex(26, "FAIL STAGE", info.stage);
                    display.hex(27, "FAIL COMMAND", info.command);
                    display.hex(28, "FAIL PARAM", info.parameter);
                }
            }
        }
        #[cfg(feature = "sunfish-native-tftp-server-check")]
        if self.error != 0 {
            if let Some(Stage::Services(owner)) = self.stage.as_ref() {
                display.hex(15, "QUEUE KIND", owner.queue_full_kind());
                display.hex(16, "TFTP QUEUED", self.server_check_queued);
                display.hex(17, "TFTP IGNORED", self.server_check_ignored);
                display.hex(23, "TFTP COALESCED", owner.tftp_wrq_coalesced());
                if owner.queue_full_kind() == 2 {
                    let (snapshot, pending) = owner.tftp_queue_snapshot();
                    display.hex(10, "REPLY RETAINED", u32::from(pending));
                    display.hex(11, "TFTP TX FLAGS", snapshot.tx_flags);
                    display.hex(13, "QUEUE KIND", 2);
                    for (index, label) in ["S0 META", "S1 META", "S2 META", "S3 META", "S4 META"].into_iter().enumerate() {
                        display.hex(14 + index * 2, label, snapshot.metadata[index]);
                    }
                    for (index, label) in ["S0 PEER", "S1 PEER", "S2 PEER", "S3 PEER", "S4 PEER"].into_iter().enumerate() {
                        display.hex(15 + index * 2, label, snapshot.peers[index]);
                    }
                    display.hex(24, "TFTP HELD", snapshot.total);
                    display.hex(25, "TFTP TX COUNT", snapshot.tx_count);
                    display.hex(26, "TFTP QUEUED", self.server_check_queued);
                    display.hex(27, "TFTP COALESCE", owner.tftp_wrq_coalesced());
                    let (reason, wait_tx, wait_flows, peer) = owner.tftp_wait_diagnostic();
                    display.hex(14, "WAIT REASON", reason);
                    display.hex(15, "WAIT ALL TX", wait_tx);
                    display.hex(16, "WAIT FLOWS", wait_flows);
                    display.hex(17, "WAIT PEER", peer);
                    display.hex(18, "NOW ALL TX", snapshot.all_tx_used);
                    display.hex(19, "NOW FLOWS", snapshot.flow_used);
                    display.replace_line(28, "WAIT 1 BUSY 2 CAPACITY");
                }
            }
        }
        #[cfg(feature = "sunfish-native-tftp-observer")]
        if self.error == 0 {
            if let Some(Stage::Services(owner)) = self.stage.as_ref() {
                let trace = owner.tftp_observer_trace();
                if let Some(request) = trace.last {
                    let path = request.path_text().as_bytes();
                    display.hex(10, "OPTION BYTES", u32::from(request.option_bytes));
                    display.hex(11, "SERVER CHECK", u32::from(path == b"/readwrite/server_check.txt"));
                    display.hex(13, "OPTIONS SAVED", u32::from(request.option_prefix_len));
                    let options = &request.option_prefix[..usize::from(request.option_prefix_len)];
                    for part in 0..4 {
                        let (hex, len) = diagnostic::path_hex_chunk(options, part);
                        if let Ok(text) = core::str::from_utf8(&hex[..len]) {
                            display.replace_line(14 + part, text);
                        }
                    }
                }
                let wlfw_count = (0..personal_os_rust_server::qcom_qrtr_control::MAX_SERVERS)
                    .filter_map(|i| owner.remote_server(i))
                    .filter(|s| s.service == personal_os_rust_server::qcom_qrtr_control::WLFW_SERVICE_ID)
                    .count();
                display.hex(18, "TFTP PACKETS", trace.packets);
                display.hex(19, "TFTP RRQ", trace.read_requests);
                display.hex(20, "TFTP PARSE ERR", u32::from(trace.parse_error.is_some()));
                display.hex(21, "TFTP OPCODE", trace.last.map_or(u32::MAX, |v| u32::from(v.opcode)));
                display.hex(22, "TFTP PEER", trace.peer.map_or(u32::MAX, |v| v.port));
                display.hex(23, "WLFW SERVERS", wlfw_count as u32);
                display.replace_line(24, "TFTP OBSERVER STAGED");
                display.replace_line(25, "NO TFTP RESPONSE OR BYTES");
                display.replace_line(26, "NO WLFW OR WLAN START");
                display.replace_line(27, "NO FLASH OR RESET RETRY");
            }
        }
        #[cfg(feature = "sunfish-native-tftp-server-check")]
        if self.error == 0 {
            let rejection = self.server_check.first_rejection();
            display.hex(15, "REJECT CODE", rejection.map_or(0, |(error, _)| error.code()));
            display.hex(16, "REJECT BYTES", rejection.map_or(0, |(_, bytes)| bytes));
            display.hex(18, "TFTP QUEUED", self.server_check_queued);
            display.hex(19, "TFTP IGNORED", self.server_check_ignored);
            display.hex(20, "RAM BYTES", self.server_check.total_bytes() as u32);
            display.hex(21, "ACTIVE SESSIONS", self.server_check.active_count() as u32);
            display.hex(22, "WRITES COMPLETE", self.server_check.completed_count() as u32);
            if let Some(Stage::Services(owner)) = self.stage.as_ref() {
                display.hex(23, "TFTP COALESCED", owner.tftp_wrq_coalesced());
            }
            display.replace_line(24, "SERVER CHECK RAM ONLY");
            display.replace_line(25, "NO FIRMWARE SERVED");
            display.replace_line(26, "NO WLFW OR WLAN START");
            display.replace_line(27, "NO FLASH OR RESET RETRY");
        }
        #[cfg(feature = "sunfish-glink-intent-request-experiment")]
        {
            // Read retained software state even if a failing dispatch skipped
            // the normal runtime-status refresh. No hardware access occurs.
            let intent = match self.stage.as_ref() {
                Some(Stage::Services(owner)) => owner.outbound_intent_diagnostic(),
                _ => self.intent_status,
            };
            let attempts = match self.stage.as_ref() {
                Some(Stage::Services(owner)) => owner.outbound_intent_attempt_diagnostic(),
                _ => (0, 0),
            };
            if self.error != 0 {
                display.hex(20, "INTENT ATTEMPTS", attempts.0);
                display.hex(21, "INTENT BLOCKED", attempts.1);
                if let Some(Stage::Services(owner)) = self.stage.as_ref() {
                    let (combinations, last) = owner.intent_gate_combinations();
                    let (tftp, ready_kind, remote_free) = owner.intent_gate_queue_snapshot();
                    display.hex(17, "TX TFTP UNSENT", tftp);
                    display.hex(18, "TX READY KIND", ready_kind);
                    display.hex(19, "REMOTE FREE", remote_free);
                    display.hex(22, "GATE COMBOS", combinations);
                    display.hex(23, "GATE LAST", last);
                    display.hex(28, "GATE MASK", owner.intent_gate_diagnostic().2);
                }
            }
            display.hex(24, "INTENT STATE", intent.0);
            display.hex(25, "INTENT SIZE", intent.1);
            display.replace_line(26, if self.error == 0x75 { "INTENT WAIT TIMEOUT" } else { "INTENT ONE REQUEST" });
            display.replace_line(27, "NO FLASH OR WLAN START");
        }
        #[cfg(feature = "sunfish-native-tftp-server-check")]
        if let Some(Stage::Services(owner)) = self.stage.as_ref() {
            if self.error == 0 || (self.error == 0x73 && owner.queue_full_kind() == 2) {
                let [started, published, completed] = owner.tftp_tx_progress();
                let [wrq, rrq, wlan, data] = owner.tftp_wire_counts();
                display.hex(15, "TFTP WRQ RX", wrq);
                display.hex(16, "TFTP RRQ RX", rrq);
                display.hex(17, "WLAN RRQ RX", wlan);
                display.hex(24, "TFTP TX START", started);
                display.hex(25, "TFTP TX SENT", published);
                display.hex(26, "TFTP TX DONE", completed);
                display.hex(28, "TFTP DATA RX", data);
                if self.error == 0 {
                    let rejection = self.server_check.first_rejection();
                    display.hex(10, "REJECT BYTES", rejection.map_or(0, |(_, bytes)| bytes));
                    display.hex(14, "REJECT CODE", rejection.map_or(0, |(error, _)| error.code()));
                }
            }
        }
        #[cfg(feature = "sunfish-tftp-first-rrq-observer")]
        if self.error == 0 {
            if let Some(Stage::Services(owner)) = self.stage.as_ref() {
                if let Some(request) = owner.first_rrq() {
                    display.hex(19, "RRQ PATH LEN", u32::from(request.path_len));
                    // The probe font supports only capital letters and digits;
                    // hex preserves path case and punctuation. Six rows show
                    // the first 60 bytes; the length reveals truncation.
                    let path = request.path_text().as_bytes();
                    for part in 0..6 {
                        let (hex, len) = diagnostic::path_hex_chunk(path, part);
                        if let Ok(text) = core::str::from_utf8(&hex[..len]) {
                            display.replace_line(20 + part, text);
                        }
                    }
                }
            }
        }
        // Keep the first capacity snapshot visible on errors. Earlier generic
        // intent and TFTP painters reuse these rows and otherwise hide the
        // command and resource counts needed to identify the full table.
        #[cfg(feature = "sunfish-native-tftp-server-check")]
        if self.error != 0 {
            if let Some(Stage::Services(owner)) = self.stage.as_ref() {
                if let Some(info) = owner.capacity_diagnostic() {
                    display.hex(14, "CAP PHASE", info.phase);
                    display.hex(15, "CAP COMMAND", info.command);
                    display.hex(16, "CAP PARAM", info.parameter);
                    display.hex(17, "RX FREE", info.rx_free);
                    display.hex(18, "TX FREE", info.tx_free);
                    display.hex(19, "REMOTE FREE", info.remote_free);
                    display.hex(20, "REMOTE BUSY", info.remote_busy);
                    display.hex(21, "QMI REQUESTS", info.requests);
                    display.hex(22, "FLOWS USED", info.flow_used);
                    display.hex(23, "FLOWS ACTIVE", info.flow_active);
                    display.hex(24, "TFTP QUEUED", self.server_check_queued);
                    display.hex(25, "TFTP IGNORED", self.server_check_ignored);
                    display.hex(26, "ACTIVE SESSIONS", self.server_check.active_count() as u32);
                    display.hex(27, "RAM BYTES", self.server_check.total_bytes() as u32);
                    display.hex(28, "WRITES COMPLETE", self.server_check.completed_count() as u32);
                }
            }
        }
        // A non-capacity transport failure needs its complete nested error.
        // Paint it last so the generic TFTP and intent fields cannot replace
        // the second and third text rows, as happened in the previous boot.
        #[cfg(feature = "sunfish-native-tftp-server-check")]
        if self.error != 0 {
            if let Some(Stage::Services(owner)) = self.stage.as_ref() {
                if owner.capacity_diagnostic().is_none()
                    && !owner.crash_evidence().is_some_and(|e| e.after.is_some()) {
                    if let Some(error) = owner.error() {
                        let text = diagnostic::ErrorText::from_debug(&error);
                        for row in 0..3 { display.replace_line(14 + row, text.line(row)); }
                        display.hex(17, "ERR TRUNCATED", u32::from(text.truncated()));
                        if let Some(info) = owner.session_failure_diagnostic() {
                            display.hex(18, "FAIL STAGE", info.stage);
                            display.hex(19, "FAIL COMMAND", info.command);
                            display.hex(20, "FAIL PARAM", info.parameter);
                        }
                        display.hex(21, "QUEUE KIND", owner.queue_full_kind());
                        display.hex(22, "TFTP QUEUED", self.server_check_queued);
                        display.hex(23, "TFTP IGNORED", self.server_check_ignored);
                        display.hex(24, "ACTIVE SESSIONS", self.server_check.active_count() as u32);
                        display.hex(25, "WRITES COMPLETE", self.server_check.completed_count() as u32);
                        let [wrq, rrq, wlan, data] = owner.tftp_wire_counts();
                        display.hex(26, "TFTP WRQ RX", wrq);
                        display.hex(27, "TFTP RRQ RX", rrq);
                        display.hex(28, "TFTP DATA RX", data);
                        let _ = wlan;
                        #[cfg(feature = "sunfish-qrtr-deferred-requests")]
                        if let Some(info) = owner.deferred_close_diagnostic() {
                            display.hex(21, "CLOSE CASE", 1);
                            display.hex(22, "CLOSE COMMAND", info.command);
                            display.hex(23, "CLOSE PEER", info.peer_port);
                            display.hex(24, "DEFERRED DEST", info.destination_port);
                            display.hex(25, "DEFERRED OPCODE", info.opcode);
                            display.hex(26, "DEFERRED COUNT", info.matching);
                            display.hex(27, "CLOSE BLOCKS", info.block_mask);
                            #[cfg(feature = "sunfish-tftp-peer-close-experiment")]
                            display.hex(28, "OWNER KINDS", owner.deferred_close_owner_kinds().unwrap_or(u32::MAX));
                        } else if let Some(info) = owner.client_gone_diagnostic() {
                            display.hex(21, "CLOSE CASE", 2);
                            display.hex(22, "CLOSE COMMAND", info.command);
                            display.hex(23, "CLOSE PEER", info.port);
                            display.hex(24, "CLOSE SERVICE", info.service.descriptor().0);
                            display.hex(25, "CLOSE RX STATE", info.receive_state);
                            display.hex(26, "CLOSE TX COUNT", info.reply_count);
                            display.hex(27, "CLOSE TX SENT", info.reply_sent);
                            display.hex(28, "CLOSE TX FLAGS", u32::from(info.reply_glink_done) | (u32::from(info.reply_qrtr_done) << 1) | (u32::from(info.done_pending) << 2));
                        }
                    }
                }
            }
        }
        // An IPA timeout can happen after a parsed RRQ. Preserve both the
        // transport snapshot and the RRQ prefix after all generic painters.
        #[cfg(feature = "sunfish-tftp-first-rrq-observer")]
        if self.error == 0x73 && self.detail == 0x54 {
            if let Some(Stage::Services(owner)) = self.stage.as_ref() {
                if matches!(owner.error(), Some(qcom_modem_owner::Error::LinuxIpa(qcom_modem_owner::LinuxIpaError::Timeout)))
                    && owner.capacity_diagnostic().is_none()
                    && !owner.crash_evidence().is_some_and(|e| e.after.is_some()) {
                    let info = owner.linux_ipa_tx_diagnostic();
                    if let Some(report) = owner.linux_ipa_report() {
                        display.hex(14, "IPA TIME CAUSE", report.timeout_causes);
                        display.hex(15, "IPA PENDING", u32::from(report.pending_record) | (u32::from(report.record_queued) << 1));
                        display.hex(16, "IPA HELD RSP", u32::from(report.held_response));
                    }
                    display.hex(17, "IPA TX LENGTH", info.map_or(u32::MAX, |v| v.len));
                    display.hex(18, "IPA TX SENT", info.map_or(u32::MAX, |v| v.sent));
                    display.hex(19, "IPA TX FLAGS", info.map_or(u32::MAX, |v| v.flags));
                    display.hex(20, "IPA TX INTENT", info.map_or(u32::MAX, |v| v.remote_intent));
                    // When no IPA TX record is retained, preserve the first
                    // OACK observations rather than four unavailable fields.
                    #[cfg(feature = "sunfish-tftp-timing-observer")]
                    if info.is_none() {
                        let timing = owner.tftp_timing();
                        let age = |end: Option<u64>| match (timing.request, end) {
                            (Some(start), Some(end)) => end.checked_sub(start)
                                .map_or(u32::MAX, |ticks| diagnostic::ticks_to_ms(ticks, self.hz)),
                            _ => u32::MAX,
                        };
                        display.hex(17, "QUEUE AGE MS", age(timing.queued));
                        display.hex(18, "PUBLISH AGE MS", age(timing.published));
                        display.hex(19, "RXDONE AGE MS", age(timing.done));
                        display.hex(20, "PEER MATCH", if timing.different_peer { 0 } else if timing.queued.is_some() { 1 } else { u32::MAX });
                    }
                    if let Some(report) = owner.linux_ipa_report() {
                        display.hex(21, "RSP AGE MS", report.response_observed_age
                            .map_or(u32::MAX, |ticks| diagnostic::ticks_to_ms(ticks, self.hz)));
                        // 0 unavailable, 1 observed before deadline, 2 at/after.
                        display.hex(22, "RSP DEADLINE", report.response_observed_age
                            .map_or(0, |age| if age < report.timeout_ticks { 1 } else { 2 }));
                        display.hex(23, "DEADLINE MS", diagnostic::ticks_to_ms(report.timeout_ticks, self.hz));
                    }
                    #[cfg(feature = "sunfish-tftp-host-gap-observer")]
                    {
                        let t = owner.tftp_timing();
                        display.hex(23, "MAX GAP MS", if t.host_observations > 1 {
                            diagnostic::ticks_to_ms(t.max_host_gap, self.hz)
                        } else { u32::MAX });
                    }
                    display.hex(24, "IPA RECEIPT", owner.linux_ipa_receipt_pending().map_or(u32::MAX, u32::from));
                    display.hex(25, "IPA PENDING ID", owner.linux_ipa_report().and_then(|r| r.pending_message_id).map_or(u32::MAX, u32::from));
                    #[cfg(feature = "sunfish-tftp-data-peer-observer")]
                    {
                        let t = owner.tftp_timing();
                        let age = match (t.request, t.first_matching_data) {
                            (Some(start), Some(end)) => end.checked_sub(start)
                                .map_or(u32::MAX, |ticks| diagnostic::ticks_to_ms(ticks, self.hz)),
                            _ => u32::MAX,
                        };
                        #[cfg(not(feature = "sunfish-tftp-rx-backlog-observer"))]
                        display.hex(26, "MATCH DATA", t.matching_data);
                        #[cfg(feature = "sunfish-tftp-rx-backlog-observer")]
                        {
                            let empty_age = match (t.request, t.last_rx_empty) {
                                (Some(start), Some(end)) => end.checked_sub(start)
                                    .map_or(u32::MAX, |ticks| diagnostic::ticks_to_ms(ticks, self.hz)),
                                _ => u32::MAX,
                            };
                            display.hex(26, "RX EMPTY MS", empty_age);
                        }
                        #[cfg(not(feature = "sunfish-tftp-tail-observer"))]
                        display.hex(27, "MATCH DATA MS", age);
                        #[cfg(feature = "sunfish-tftp-tail-observer")]
                        {
                            let _ = age;
                            let tail_age = match (t.request, t.peer_tail) {
                                (Some(start), Some(end)) => end.checked_sub(start)
                                    .map_or(u32::MAX, |ticks| diagnostic::ticks_to_ms(ticks, self.hz)),
                                _ => u32::MAX,
                            };
                            display.hex(27, "PEER TAIL MS", tail_age);
                        }
                    }
                    #[cfg(not(feature = "sunfish-tftp-host-gap-observer"))]
                    display.hex(28, "TFTP RRQ RX", owner.tftp_wire_counts()[1]);
                    #[cfg(all(feature = "sunfish-tftp-host-gap-observer", not(feature = "sunfish-tftp-rx-backlog-observer")))]
                    display.hex(28, "OBS COUNT", owner.tftp_timing().host_observations);
                    #[cfg(feature = "sunfish-tftp-rx-backlog-observer")]
                    display.hex(28, "RX MAX BYTES", owner.tftp_timing().max_rx_bytes);
                }
            }
        }
        #[cfg(feature = "sunfish-tftp-endpoint-observer")]
        if self.error == 0 {
            if let Some(Stage::Services(owner)) = self.stage.as_ref() {
                if let Some(trace) = owner.tftp_endpoint_trace() {
                    display.hex(18, "OACK SRC NODE", trace.source.node);
                    display.hex(19, "OACK SRC PORT", trace.source.port);
                    display.hex(20, "OACK DST NODE", trace.destination.node);
                    display.hex(21, "OACK DST PORT", trace.destination.port);
                    display.hex(22, "OACK BYTES", trace.payload_bytes);
                    display.hex(23, "OACK FLAGS", trace.flags);
                    display.hex(24, "OACK INTENT", trace.remote_intent);
                }
            }
        }
        // Host-observed latencies start at owner receipt, not modem send time.
        // Unmatched peers and missing timestamps remain visibly unavailable.
        #[cfg(feature = "sunfish-tftp-timing-observer")]
        if self.error == 0 {
            if let Some(Stage::Services(owner)) = self.stage.as_ref() {
                let t = owner.tftp_timing();
                let latency = |end: Option<u64>| match (t.request, end) {
                    (Some(start), Some(end)) => end.checked_sub(start)
                        .map_or(u32::MAX, |ticks| diagnostic::ticks_to_ms(ticks, self.hz)),
                    _ => u32::MAX,
                };
                #[cfg(feature = "sunfish-tftp-rx-backlog-observer")]
                display.hex(18, "RX EMPTY MS", latency(t.last_rx_empty));
                #[cfg(all(feature = "sunfish-tftp-data-peer-observer", not(feature = "sunfish-tftp-rx-backlog-observer")))]
                display.hex(18, "MATCH DATA", t.matching_data);
                #[cfg(all(not(feature = "sunfish-tftp-rx-backlog-observer"), not(feature = "sunfish-tftp-data-peer-observer"), feature = "sunfish-tftp-first-rrq-observer"))]
                display.hex(18, "FIRST RRQ KIND", diagnostic::first_rrq_kind(owner.first_rrq().as_ref().map(|r| r.path_text())));
                #[cfg(all(not(feature = "sunfish-tftp-rx-backlog-observer"), not(feature = "sunfish-tftp-data-peer-observer"), not(feature = "sunfish-tftp-first-rrq-observer")))]
                display.hex(18, "WRQ PEER NODE", t.peer.map_or(u32::MAX, |p| p.node));
                #[cfg(feature = "sunfish-tftp-host-gap-observer")]
                display.hex(19, "MAX GAP MS", if t.host_observations > 1 {
                    diagnostic::ticks_to_ms(t.max_host_gap, self.hz)
                } else { u32::MAX });
                #[cfg(all(feature = "sunfish-tftp-data-peer-observer", not(feature = "sunfish-tftp-host-gap-observer")))]
                display.hex(19, "MATCH DATA MS", latency(t.first_matching_data));
                #[cfg(all(not(feature = "sunfish-tftp-data-peer-observer"), not(feature = "sunfish-tftp-host-gap-observer")))]
                display.hex(19, "WRQ PEER PORT", t.peer.map_or(u32::MAX, |p| p.port));
                display.hex(20, "PEER MATCH", if t.different_peer { 0 } else if t.queued.is_some() { 1 } else { u32::MAX });
                display.hex(21, "QUEUE AGE MS", latency(t.queued));
                display.hex(22, "PUBLISH AGE MS", latency(t.published));
                display.hex(23, "RXDONE AGE MS", latency(t.done));
                display.hex(24, "OACK FLAGS", owner.tftp_endpoint_trace().map_or(u32::MAX, |v| v.flags));
                #[cfg(feature = "sunfish-glink-tftp-fairness-experiment")]
                if owner.tftp_endpoint_trace().is_some_and(|v| v.flags & 8 != 0) {
                    let scheduling = owner.tftp_schedule_trace();
                    display.hex(25, "PUBLISH CALLS", scheduling.calls);
                    display.hex(26, "WAIT MASK", scheduling.mask);
                }
                #[cfg(feature = "sunfish-tftp-tail-observer")]
                display.hex(25, "PEER TAIL MS", latency(t.peer_tail));
                #[cfg(all(feature = "sunfish-tftp-host-gap-observer", not(feature = "sunfish-tftp-rx-backlog-observer")))]
                display.hex(26, "OBS COUNT", t.host_observations);
                #[cfg(feature = "sunfish-tftp-rx-backlog-observer")]
                display.hex(26, "RX MAX BYTES", t.max_rx_bytes);
                #[cfg(feature = "sunfish-tftp-oack-phase-observer")]
                {
                    display.hex(18, "DISPATCH MS", self.oack_phases.age_ms(0, self.hz));
                    display.hex(19, "REGISTRY MS", self.oack_phases.age_ms(1, self.hz));
                    display.hex(25, "STORAGE MS", self.oack_phases.age_ms(2, self.hz));
                    display.hex(26, "IPA STEP MS", self.oack_phases.age_ms(3, self.hz));
                }
                #[cfg(feature = "sunfish-tftp-peer-close-observer")]
                {
                    display.hex(18, "PEER CLOSE MS", latency(t.peer_close));
                    display.hex(19, "CLOSE PHASE", if t.peer_close.is_some() { t.peer_close_phase } else { u32::MAX });
                }
                #[cfg(feature = "sunfish-tftp-publish-io-observer")]
                {
                    display.hex(18, "IO OBS MS", self.oack_publish_io.ms(0, self.hz));
                    display.hex(19, "IO DESC MS", self.oack_publish_io.ms(1, self.hz));
                    display.hex(25, "IO READ MS", self.oack_publish_io.ms(2, self.hz));
                    display.hex(26, "IO OTHER MS", self.oack_publish_io.ms(3, self.hz));
                }
                #[cfg(feature = "sunfish-tftp-publish-op-observer")]
                {
                    display.hex(16, "IO LEN MS", self.oack_publish_ops.ms(0, self.hz));
                    display.hex(17, "IO WRITE MS", self.oack_publish_ops.ms(1, self.hz));
                    display.hex(18, "IO HEAD MS", self.oack_publish_ops.ms(2, self.hz));
                    display.hex(19, "IO TAIL MS", self.oack_publish_ops.ms(3, self.hz));
                    display.hex(25, "IO BARR MS", self.oack_publish_ops.ms(4, self.hz));
                    display.hex(26, "IO NOTIFY MS", self.oack_publish_ops.ms(5, self.hz));
                }
                #[cfg(feature = "sunfish-tftp-barrier-domain-observer")]
                {
                    display.hex(16, "DOMAIN PRE MS", self.oack_barrier_domains.ms(0, self.hz));
                    display.hex(17, "DOMAIN POST MS", self.oack_barrier_domains.ms(1, self.hz));
                }
            }
        }
        #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
        if let Some(Stage::Services(owner)) = self.stage.as_ref() {
            if owner.tftp_close_commits() != 0 { display.hex(11, "CLOSE COMMITS", owner.tftp_close_commits()); }
        }
        // Final success overlay: do not hide an error diagnostic. A completed
        // local write is distinct from delivery of its final acknowledgement.
        #[cfg(feature = "sunfish-tftp-outcome-summary")]
        if self.error == 0 {
            display.hex(16, "WRITES COMPLETE", self.server_check.completed_count() as u32);
            display.hex(17, "RAM BYTES", self.server_check.total_bytes() as u32);
            display.hex(18, "FIRST COMPLETE", u32::from(self.server_check.complete()));
            display.hex(19, "ACTIVE SESSIONS", self.server_check.active_count() as u32);
        }
        // Keep this after the timing overlays. These fields follow the first
        // completed write's ACK; they are not first-OACK timings.
        #[cfg(feature = "sunfish-tftp-final-ack-observer")]
        if self.error == 0 {
            let trace = match self.stage.as_ref() {
                Some(Stage::Services(owner)) => owner.tftp_final_ack_trace(), _ => None,
            };
            display.hex(18, "FINAL ACK FLAGS", trace.map_or(0, |t| t.flags));
            display.hex(20, "ACK PEER NODE", trace.map_or(u32::MAX, |t| t.destination.node));
            display.hex(21, "ACK PEER PORT", trace.map_or(u32::MAX, |t| t.destination.port));
            display.hex(22, "ACK LOCAL NODE", trace.map_or(u32::MAX, |t| t.source.node));
            display.hex(23, "ACK LOCAL PORT", trace.map_or(u32::MAX, |t| t.source.port));
            display.hex(24, "ACK BLOCK", trace.map_or(u32::MAX, |t| u32::from(t.block)));
            display.hex(25, "ACK INTENT", trace.map_or(u32::MAX, |t| t.remote_intent));
            display.replace_line(26, "ACK TRACE NOT OACK");
        }
        #[cfg(feature = "sunfish-tftp-missing-temp-replies")]
        if self.error == 0 {
            let (trace, wire) = match self.stage.as_ref() {
                Some(Stage::Services(owner)) => (owner.tftp_missing_reply_trace(), owner.tftp_wire_counts()),
                _ => (None, [0; 4]),
            };
            display.hex(19, "MISSING RRQ", self.server_check.missing_requests());
            display.hex(20, "MISSING KIND", u32::from(self.server_check.first_missing_kind()));
            display.hex(21, "MISS RSP FLAGS", trace.map_or(0, |t| t.flags));
            display.hex(22, "MISS PEER PORT", trace.map_or(u32::MAX, |t| t.destination.port));
            display.hex(23, "MISS LOCAL PORT", trace.map_or(u32::MAX, |t| t.source.port));
            display.hex(24, "MISS INTENT", trace.map_or(u32::MAX, |t| t.remote_intent));
            display.hex(25, "TFTP RRQ RX", wire[1]);
            display.hex(26, "WLAN RRQ RX", wire[2]);
        }
        #[cfg(feature = "sunfish-wlan-state-query")]
        if self.error == 0 {
            if let Some(Stage::Services(owner)) = self.stage.as_ref() {
                let q = self.wlan_query.snapshot(owner);
                display.replace_line(1, "WLAN DOMAIN STATE QUERY");
                display.hex(15, "FINAL ACK FLAGS", owner.tftp_final_ack_trace().map_or(0, |t| t.flags));
                for (i, label) in ["QUERY PHASE", "PD STATE RAW", "QUERY RESULT", "QUERY ERROR", "NOTIF SERVERS", "NOTIF NODE", "NOTIF PORT", "WLFW SERVERS", "WLFW NODE", "WLFW PORT"].iter().enumerate() {
                    display.hex(16 + i, label, q[i]);
                }
                display.hex(26, "WRITES COMPLETE", self.server_check.completed_count() as u32);
                display.hex(27, "MISS RSP FLAGS", owner.tftp_missing_reply_trace().map_or(0, |t| t.flags));
                display.replace_line(28, "NO PD RESTART OR WLAN");
                #[cfg(feature = "sunfish-wlan-state-listener")]
                {
                    display.replace_line(1, "WLAN STATE LISTENER");
                    display.hex(15, "QUERY PHASE", q[0]);
                    let l = self.wlan_listener.snapshot(owner);
                    for (i, label) in ["LISTEN PHASE", "LISTEN STATE", "LISTEN RESULT", "LISTEN ERROR", "UPDATE TXN", "UPDATE STATE", "ACK RESULT", "ACK ERROR"].iter().enumerate() {
                        display.hex(16 + i, label, l[i]);
                    }
                    display.hex(24, "PD QUERY STATE", q[1]);
                    display.hex(25, "WLFW SERVERS", q[7]);
                    display.hex(26, "WLFW NODE", q[8]);
                    display.hex(27, "WLFW PORT", q[9]);
                }
            }
        }
        #[cfg(all(feature = "sunfish-tftp-rejection-capture", not(feature = "sunfish-tftp-mcfg-trace")))]
        if self.error == 0 {
            if let Some(c) = self.server_check.rejection_capture() {
                display.replace_line(1, "FIRST TFTP REJECTION");
                display.hex(7, "REJECT OPCODE", c.opcode);
                display.hex(8, "REJECT LENGTH", c.length);
                display.hex(9, "CAPTURE LENGTH", c.captured);
                display.hex(10, "SOURCE NODE", c.route.source.node);
                display.hex(11, "SOURCE PORT", c.route.source.port);
                display.hex(12, "DEST NODE", c.route.destination.node);
                display.hex(13, "DEST PORT", c.route.destination.port);
                for i in 0..15 {
                    let n = i * 4;
                    display.hex(14 + i, "REQUEST HEX", u32::from_be_bytes([c.bytes[n], c.bytes[n+1], c.bytes[n+2], c.bytes[n+3]]));
                }
            }
        }
        #[cfg(feature = "sunfish-tftp-mcfg-ram")]
        if self.error == 0 {
            let (writes, bytes, reads) = self.server_check.mcfg_progress();
            display.hex(4, "MCFG WRITES", writes);
            display.hex(5, "MCFG FILE BYTES", bytes.map_or(u32::MAX, |size| size as u32));
            display.hex(6, "MCFG READ DONE", reads);
        }
        #[cfg(all(feature = "sunfish-tftp-mcfg-trace", not(feature = "sunfish-wlan-startup-trace")))]
        if self.error == 0 {
            if let Some(Stage::Services(owner)) = self.stage.as_ref() {
                let (oack, data) = owner.mcfg_trace();
                let rejected = self.server_check.rejection_capture();
                display.replace_line(1, "MCFG TRANSFER TRACE");
                display.hex(7, "OACK FLAGS", oack.map_or(0, |t| t.flags));
                display.hex(8, "WRITE PEER PORT", oack.map_or(u32::MAX, |t| t.destination.port));
                display.hex(9, "WRITE LOCAL", oack.map_or(u32::MAX, |t| t.source.port));
                display.hex(10, "OACK INTENT", oack.map_or(u32::MAX, |t| t.remote_intent));
                display.hex(11, "PEER OPCODE", data.map_or(u32::MAX, |t| u32::from(t.opcode)));
                display.hex(12, "BLOCK OR ERROR", data.and_then(|t| t.block_or_error).map_or(u32::MAX, u32::from));
                display.hex(13, "PEER PKT BYTES", data.map_or(u32::MAX, |t| t.packet_bytes));
                display.hex(14, "PEER DEST PORT", data.map_or(u32::MAX, |t| t.local_port));
                display.hex(15, "REJECT CODE", self.server_check.first_rejection().map_or(0, |(e, _)| e.code()));
                display.hex(16, "REJECT OPCODE", rejected.map_or(u32::MAX, |r| r.opcode));
                display.hex(17, "REJECT BYTES", rejected.map_or(u32::MAX, |r| r.length));
                display.hex(18, "REJECT PEER", rejected.map_or(u32::MAX, |r| r.route.source.port));
                display.hex(19, "REJECT DEST", rejected.map_or(u32::MAX, |r| r.route.destination.port));
                for row in 20..=23 { display.replace_line(row, ""); }
            }
        }
        #[cfg(feature = "sunfish-wlan-startup-trace")]
        if self.error == 0 {
            if let Some(Stage::Services(owner)) = self.stage.as_ref() {
                let trace = owner.service_registry_trace();
                let q = self.wlan_query.snapshot(owner);
                let l = self.wlan_listener.snapshot(owner);
                let [wrq, rrq, wlan, data] = owner.tftp_wire_counts();
                let missing = owner.tftp_missing_reply_trace();
                let final_ack = owner.tftp_final_ack_trace();
                display.replace_line(1, "WLAN STARTUP TRACE");
                display.hex(4, "IPA COMPLETE", u32::from(self.ipa_complete_seen));
                display.hex(5, "LOC REQUESTS", trace.requests);
                display.hex(6, "LOC REPLIES", trace.replies_queued);
                display.hex(7, "LOC QUEUED MASK", trace.queued_names);
                display.hex(8, "LOC DONE MASK", trace.completed_names);
                for (index, label) in ["LOC TMS SERVREG", "LOC GPS SERVICE", "LOC ELF LOADER",
                    "LOC WLAN FW", "LOC PDR ENABLE", "LOC PDDUMP OFF", "LOC OTHER"].iter().enumerate()
                {
                    display.hex(9 + index, label, trace.name_counts[index]);
                }
                display.hex(16, "LOC TX WAITS", trace.reply_waits);
                display.hex(17, "TFTP WRQ RX", wrq);
                display.hex(18, "TFTP RRQ RX", rrq);
                display.hex(19, "WLAN RRQ RX", wlan);
                display.hex(20, "TFTP DATA RX", data);
                display.hex(21, "FIRST ACK FLAGS", final_ack.map_or(0, |t| t.flags));
                display.hex(22, "MISS RSP FLAGS", missing.map_or(0, |t| t.flags));
                display.hex(23, "QUERY PHASE", q[0]);
                display.hex(24, "PD QUERY STATE", q[1]);
                display.hex(25, "WLFW SERVERS", q[7]);
                display.hex(26, "LISTEN PHASE", l[0]);
                display.hex(27, "UPDATE STATE", l[5]);
                let (name, _) = trace.last_service_name_parts();
                display.replace_line(28, name);
            }
        }
        #[cfg(feature = "sunfish-wlan-elf-timing")]
        let elapsed = |start: Option<u64>, end: Option<u64>| {
            start.zip(end).map_or(u32::MAX, |(start, end)| {
                diagnostic::ticks_to_ms(end.wrapping_sub(start), self.hz)
            })
        };
        #[cfg(feature = "sunfish-wlan-elf-timing")]
        if self.error == 0 {
            if let Some(Stage::Services(owner)) = self.stage.as_ref() {
                let trace = owner.service_registry_trace();
                let elf = trace.elf;
                let received = elf.and_then(|e| e.received);
                let queued = elf.and_then(|e| e.queued);
                let published = elf.and_then(|e| e.published);
                let done = elf.and_then(|e| e.done);
                display.replace_line(1, "ELF LOADER REPLY TRACE");
                display.hex(9, "ELF PEER PORT", elf.map_or(u32::MAX, |e| e.peer_port));
                display.hex(10, "ELF OFFSET", elf.map_or(u32::MAX, |e| e.domain_offset));
                display.hex(11, "ELF TOTAL", elf.and_then(|e| e.total_domains).map_or(u32::MAX, u32::from));
                display.hex(12, "ELF ROWS", elf.and_then(|e| e.returned_rows).map_or(u32::MAX, u32::from));
                display.hex(13, "ELF QUEUE MS", elapsed(received, queued));
                display.hex(14, "ELF PUB MS", elapsed(queued, published));
                display.hex(15, "ELF DONE MS", elapsed(received, done));
                display.hex(16, "ELF TX WAITS", trace.reply_wait_name_counts[personal_os_rust_server::qcom_servreg::NAME_COUNTER_ELF_LOADER]);
                display.hex(17, "ELF REQ AGE MS", elapsed(Some(self.runtime_started), received));
            }
        }
        #[cfg(feature = "sunfish-wlan-elf-queue-trace")]
        if self.error == 0 {
            if let Some(Stage::Services(owner)) = self.stage.as_ref() {
                let elf = owner.service_registry_trace().elf;
                let waits = elf.map_or([0; 6], |trace| trace.waits_by_reason);
                let pressure = elf.and_then(|trace| trace.last_pressure);
                display.replace_line(1, "ELF REPLY QUEUE TRACE");
                display.hex(5, "WAIT SAME PEER", waits[1]);
                display.hex(6, "WAIT TX FULL", waits[2]);
                display.hex(7, "WAIT LEGACY MAP", waits[3]);
                display.hex(8, "WAIT REF CREDIT", waits[4]);
                display.hex(9, "WAIT REF CAP", waits[5]);
                display.hex(10, "WAIT UNKNOWN", waits[0]);
                display.hex(11, "LAST WAIT", pressure.map_or(0, |p| u32::from(p.reason)));
                display.hex(12, "TX SLOTS USED", pressure.map_or(0, |p| u32::from(p.used_slots)));
                display.hex(18, "TX UNPUBLISHED", pressure.map_or(0, |p| u32::from(p.unpublished_slots)));
                display.hex(20, "TX GLINK WAIT", pressure.map_or(0, |p| u32::from(p.glink_wait_slots)));
                display.hex(21, "TX QRTR WAIT", pressure.map_or(0, |p| u32::from(p.qrtr_wait_slots)));
                display.hex(22, "TX KIND MASK", pressure.map_or(0, |p| p.kind_mask));
                display.hex(26, "PEER TX KIND", pressure.map_or(0, |p| u32::from(p.peer_kind)));
                display.hex(27, "PEER TX FLAGS", pressure.map_or(0, |p| u32::from(p.peer_flags)));
                #[cfg(feature = "sunfish-wlan-elf-publish-trace")]
                {
                    let trace = pressure.and_then(|p| p.peer_publication);
                    display.replace_line(1, "ELF BLOCKER TX TRACE");
                    display.hex(7, "BLOCKER TXN", trace.map_or(u32::MAX, |t| u32::from(t.transaction)));
                    display.hex(8, "BLOCKER BYTES", trace.map_or(u32::MAX, |t| u32::from(t.bytes)));
                    display.hex(9, "BLOCKER CALLS", trace.map_or(u32::MAX, |t| t.calls));
                    display.hex(10, "BLOCKER MASK", trace.map_or(u32::MAX, |t| t.mask));
                }
                #[cfg(feature = "sunfish-wlan-registry-txn4-timing")]
                {
                    let tx4 = owner.service_registry_trace().txn4;
                    let received = tx4.and_then(|t| t.received);
                    let queued = tx4.and_then(|t| t.queued);
                    let published = tx4.and_then(|t| t.published);
                    let done = tx4.and_then(|t| t.done);
                    display.replace_line(1, "REGISTRY TXN 4 TIMING");
                    #[cfg(feature = "sunfish-wlan-registry-tx-turn")]
                    display.replace_line(1, "REGISTRY TX TURN");
                    display.hex(11, "TX4 NAME CODE", tx4.map_or(u32::MAX, |t| t.name_code));
                    display.hex(12, "TX4 RX AGE MS", elapsed(Some(self.runtime_started), received));
                    display.hex(18, "TX4 QUEUE MS", elapsed(received, queued));
                    display.hex(20, "TX4 PUB MS", elapsed(queued, published));
                    display.hex(21, "TX4 RXDONE MS", elapsed(published, done));
                    display.hex(22, "TX4 ELF WAITS", tx4.map_or(0, |t| t.elf_waits));
                }
                #[cfg(feature = "sunfish-wlan-elf-dispatch-timing")]
                {
                    let dispatch = elf.map(|trace| trace.dispatch);
                    let received = elf.and_then(|trace| trace.received);
                    let queued = elf.and_then(|trace| trace.queued);
                    let pass_end = dispatch.and_then(|timing| timing.receive_pass_end);
                    let registry = dispatch.and_then(|timing| timing.registry_entered);
                    let selected = dispatch.and_then(|timing| timing.selected);
                    display.replace_line(1, "ELF DISPATCH TIMING");
                    display.hex(7, "RX PASS MS", elapsed(received, pass_end));
                    display.hex(8, "PASS TO REG MS", elapsed(pass_end, registry));
                    display.hex(9, "REG TO ELF MS", elapsed(registry, selected));
                    display.hex(10, "ELF ENQUEUE MS", elapsed(selected, queued));
                    #[cfg(feature = "sunfish-wlan-elf-registry-turns")]
                    {
                        let turns = dispatch.map(|timing| timing.turns).unwrap_or_default();
                        display.replace_line(1, "ELF REGISTRY TURNS");
                        display.hex(5, "REG CALLS", turns.calls);
                        display.hex(6, "REG MAX GAP MS", diagnostic::ticks_to_ms(turns.max_gap, self.hz));
                        display.hex(11, "REG RUN 0", turns.runs[0]);
                        display.hex(12, "REG RUN 1", turns.runs[1]);
                        display.hex(18, "REG RUN 2", turns.runs[2]);
                        display.hex(22, "REG RUN 3", turns.runs[3]);
                        display.hex(26, "REG OMITTED", turns.omitted);
                        display.hex(27, "REG MAX CALL MS", diagnostic::ticks_to_ms(turns.max_call, self.hz));
                    }
                    #[cfg(feature = "sunfish-wlan-tx4-pressure")]
                    {
                        let tx4 = owner.service_registry_trace().txn4;
                        let waits = tx4.map_or([0; 6], |timing| timing.waits_by_reason);
                        let pressure = tx4.and_then(|timing| timing.last_pressure);
                        let peer = pressure.and_then(|p| p.peer_publication);
                        display.replace_line(1, "TX4 FLOWBUSY TRACE");
                        display.hex(5, "TX4 SAME PEER", waits[1]);
                        display.hex(6, "TX4 REF FLOW", waits[4]);
                        display.hex(11, "TX4 OTHER WAIT", waits[0].saturating_add(waits[2])
                            .saturating_add(waits[3]).saturating_add(waits[5]));
                        display.hex(12, "TX4 LAST REASON", pressure.map_or(u32::MAX, |p| u32::from(p.reason)));
                        display.hex(18, "TX4 BLOCK TXN", peer.map_or(u32::MAX, |t| u32::from(t.transaction)));
                        display.hex(22, "TX4 BLOCK MASK", peer.map_or(u32::MAX, |t| t.mask));
                        display.hex(26, "TX4 BLOCK KIND", pressure.map_or(u32::MAX, |p| u32::from(p.peer_kind)));
                        display.hex(27, "TX4 BLOCK FLAGS", pressure.map_or(u32::MAX, |p| u32::from(p.peer_flags)));
                        #[cfg(feature = "sunfish-wlan-registry-turn-gates")]
                        {
                            let gates = peer.map_or([u32::MAX; 8], |t| t.turn_gates);
                            display.replace_line(1, "TX4 BLOCKER TX GATES");
                            display.hex(7, "GATE COOLDOWN", gates[0]);
                            display.hex(8, "GATE TFTP", gates[1]);
                            display.hex(9, "GATE INTENT ACK", gates[2]);
                            display.hex(10, "GATE OWN DONE", gates[3]);
                            display.hex(11, "GATE NOT READY", gates[4]);
                            display.hex(12, "GATE OTHER REG", gates[5]);
                            display.hex(13, "GATE CREDIT", gates[6]);
                            display.hex(14, "GATE SELECTED", gates[7]);
                            display.hex(16, "BLOCKER CALLS", peer.map_or(u32::MAX, |t| t.calls));
                        }
                        #[cfg(feature = "sunfish-wlan-registry-ack-trace")]
                        {
                            let ack = peer.map(|t| t.ack);
                            let ms = |ticks: Option<u64>| ticks.map_or(u32::MAX, |ticks|
                                diagnostic::ticks_to_ms(ticks, self.hz));
                            display.replace_line(1, "TX4 BLOCKER ACK TRACE");
                            display.hex(7, "ACK SENT", ack.map_or(u32::MAX, |t| t.sent));
                            display.hex(8, "ACK FIFO BLOCK", ack.map_or(u32::MAX, |t| t.blocked));
                            display.hex(10, "DONE BEFORE ACK", ack.map_or(u32::MAX, |t| t.done_ahead));
                            display.hex(11, "ADS BEFORE ACK", ack.map_or(u32::MAX, |t| t.ads_ahead));
                            display.hex(12, "ACK MAX AGE MS", ms(ack.and_then(|t| t.max_age)));
                            display.hex(13, "ACK AT WAIT MS", ms(pressure.and_then(|p| p.pending_ack_age)));
                            #[cfg(feature = "sunfish-wlan-intent-ack-turn")]
                            {
                                let (sent, age) = owner.intent_ack_summary();
                                display.replace_line(1, "GLINK INTENT ACK TURN");
                                display.hex(6, "ACK TOTAL", sent);
                                display.hex(14, "ACK BOOT MAX MS", ms(age));
                                #[cfg(feature = "sunfish-wlan-locator-other-trace")]
                                {
                                    let trace = owner.service_registry_trace();
                                    let (first, second) = trace.first_other_service_name_parts();
                                    let other = trace.name_counts[
                                        personal_os_rust_server::qcom_servreg::NAME_COUNTER_OTHER
                                    ];
                                    display.replace_line(1, "LOC OTHER LOOKUP TRACE");
                                    display.hex(7, "LOC OTHER", other);
                                    display.hex(8, "AVS AUDIO RX", trace.avs_audio_queries);
                                    display.replace_line(9, "FIRST OTHER NAME");
                                    display.replace_line(10, first);
                                    display.replace_line(11, second);
                                }
                            }
                        }
                    }
                }
            }
        }
        #[cfg(feature = "sunfish-wlan-elf-phase-display")]
        if self.error == 0
            && let Some(Stage::Services(owner)) = self.stage.as_ref()
        {
            let trace = owner.service_registry_trace();
            if let Some(elf) = trace.elf {
                let dispatch = elf.dispatch;
                display.replace_line(1, "ELF REPLY PHASES");
                display.hex(7, "RX PASS MS", elapsed(elf.received, dispatch.receive_pass_end));
                display.hex(8, "PASS TO REG MS", elapsed(dispatch.receive_pass_end, dispatch.registry_entered));
                display.hex(9, "REG TO ELF MS", elapsed(dispatch.registry_entered, dispatch.selected));
                display.hex(10, "ELF ENQUEUE MS", elapsed(dispatch.selected, elf.queued));
                display.hex(11, "ELF PUB MS", elapsed(elf.queued, elf.published));
                display.hex(12, "ELF RXDONE MS", elapsed(elf.published, elf.done));
                display.hex(13, "REG MAX GAP MS", diagnostic::ticks_to_ms(dispatch.turns.max_gap, self.hz));
                display.hex(14, "REG CALLS", dispatch.turns.calls);
                display.hex(15, "ELF DONE MS", elapsed(elf.received, elf.done));
                display.hex(16, "ELF TX WAITS", trace.reply_wait_name_counts[
                    personal_os_rust_server::qcom_servreg::NAME_COUNTER_ELF_LOADER
                ]);
                display.hex(17, "ELF REQ AGE MS", elapsed(Some(self.runtime_started), elf.received));
                display.hex(18, "AVS AUDIO RX", trace.avs_audio_queries);
            }
        }
        #[cfg(feature = "sunfish-wlan-elf-registry-gap")]
        if !cfg!(feature = "sunfish-wlan-elf-six-phase-display") && self.error == 0
            && let Some(gap) = self.registry_gap.longest.as_ref()
        {
            let ms = |ticks| diagnostic::ticks_to_ms(ticks, self.hz);
            display.replace_line(1, "ELF REGISTRY GAP");
            display.hex(7, "REG GAP MS", ms(gap.elapsed));
            display.hex(8, "ROUTER MS", ms(gap.work[0]));
            display.hex(9, "RMTFS MS", ms(gap.work[1]));
            display.hex(10, "IPA MS", ms(gap.work[2]));
            display.hex(11, "PAINT MS", ms(gap.work[3]));
            display.hex(12, "REG WORK MS", ms(gap.work[4]));
            display.hex(13, "OUTSIDE CB MS", ms(gap.outside));
            display.hex(14, "UNATTRIB MS", ms(gap.unaccounted));
            display.hex(20, "GAP NOTIF COUNT", gap.notifications);
            display.hex(21, "NEXT BADGE LO", gap.next_badge as u32);
            display.hex(22, "NEXT BADGE HI", (gap.next_badge >> 32) as u32);
        }
        #[cfg(feature = "sunfish-wlan-elf-gap-io")]
        if !cfg!(feature = "sunfish-wlan-elf-six-phase-display") && self.error == 0
            && let Some(gap) = self.registry_gap.longest.as_ref()
        {
            let ms = |ticks| if ticks == u64::MAX { u32::MAX } else {
                diagnostic::ticks_to_ms(ticks, self.hz)
            };
            display.replace_line(1, "ELF GAP IO TIMING");
            for (row, label, index) in [
                (8, "IO OBS MS", 0), (9, "IO DESC MS", 1),
                (10, "IO READ MS", 2), (11, "IO OTHER MS", 3),
                (12, "IO LEN MS", 4), (13, "IO WRITE MS", 5),
                (14, "IO HEAD MS", 6), (16, "IO TAIL MS", 7),
                (17, "IO BARR MS", 8), (18, "IO NOTIFY MS", 9),
                (20, "DOMAIN PRE MS", 10), (21, "DOMAIN POST MS", 11),
            ] {
                display.hex(row, label, ms(gap.router_io[index]));
            }
            display.hex(22, "ROUTER MS", ms(gap.work[0]));
        }
        #[cfg(feature = "sunfish-wlan-elf-phase-io")]
        if !cfg!(feature = "sunfish-wlan-elf-six-phase-display") && self.error == 0
            && let Some(Stage::Services(owner)) = self.stage.as_ref()
            && let Some(elf) = owner.service_registry_trace().elf
        {
            let ms = |ticks| if ticks == u64::MAX { u32::MAX } else {
                diagnostic::ticks_to_ms(ticks, self.hz)
            };
            let receive = diagnostic::elf_io_delta(elf.io.received, elf.io.receive_pass_end);
            let publish = diagnostic::elf_io_delta(elf.io.queued, elf.io.published);
            display.replace_line(1, "ELF PHASE IO");
            display.hex(7, "RX PASS MS", elapsed(elf.received, elf.dispatch.receive_pass_end));
            display.hex(15, "ELF PUB MS", elapsed(elf.queued, elf.published));
            for (slot, rlabel, plabel, index) in [
                (0, "R OBS MS", "P OBS MS", 0), (1, "R DESC MS", "P DESC MS", 1),
                (2, "R READ MS", "P READ MS", 2), (3, "R OTHER MS", "P OTHER MS", 3),
                (4, "R BARR MS", "P BARR MS", 8),
                (5, "R DOM PRE MS", "P DOM PRE MS", 10),
                (6, "R DOM POST MS", "P DOM POST MS", 11),
            ] {
                display.hex(8 + slot, rlabel, ms(receive[index]));
                display.hex(16 + slot, plabel, ms(publish[index]));
            }
            display.hex(23, "ELF DONE MS", elapsed(elf.received, elf.done));
            #[cfg(feature = "sunfish-wlan-state-query")]
            {
                let q = self.wlan_query.snapshot(owner);
                display.hex(24, "QUERY PHASE", q[0]);
                display.hex(25, "PD QUERY STATE", q[1]);
                display.hex(26, "WLFW SERVERS", q[7]);
            }
            display.hex(27, "WLAN RRQ RX", owner.tftp_wire_counts()[2]);
            display.replace_line(28, "NO FLASH OR WLAN START");
        }
        #[cfg(feature = "sunfish-servreg-bounded-second-turn")]
        if self.error == 0
            && let Some(Stage::Services(owner)) = self.stage.as_ref()
            && let Some(elf) = owner.service_registry_trace().elf
        {
            display.hex(18, "REG EXTRA TRY", self.registry_extra_attempts);
            display.hex(20, "REG EXTRA OUT", self.registry_extra_outcome);
            display.hex(21, "ELF PEER PORT", elf.peer_port);
            display.hex(22, "LAST WAIT WHY", elf.last_pressure.map_or(0, |p| u32::from(p.reason)));
            display.hex(27, "LAST PEER FLAGS", elf.last_pressure.map_or(0, |p| u32::from(p.peer_flags)));
            display.replace_line(28, "ONE EXTRA TURN");
        }
        #[cfg(feature = "sunfish-wlan-elf-blocker-timing")]
        if self.error == 0
            && let Some(Stage::Services(owner)) = self.stage.as_ref()
            && let Some(elf) = owner.service_registry_trace().elf
        {
            let blocker = elf.blocker;
            display.replace_line(1, "ELF BLOCKER TIMING");
            display.hex(13, "BLOCK PUB MS", elapsed(blocker.and_then(|b| b.wait), blocker.and_then(|b| b.published)));
            display.hex(14, "BLOCK DONE MS", elapsed(blocker.and_then(|b| b.published), blocker.and_then(|b| b.done)));
            display.hex(17, "BLOCK TXN", blocker.map_or(u32::MAX, |b| u32::from(b.transaction)));
            display.hex(21, "DONE TO ELF MS", elapsed(blocker.and_then(|b| b.done), elf.queued));
            display.hex(23, "BLOCK FLAGS", blocker.map_or(u32::MAX, |b| u32::from(b.flags_at_wait)));
            display.hex(27, "BLOCK TURN MS", elapsed(blocker.and_then(|b| b.wait), blocker.and_then(|b| b.first_turn)));
            #[cfg(feature = "sunfish-wlan-elf-completion-turn")]
            {
                display.replace_line(1, "ELF COMPLETION TURN");
                display.hex(5, "YIELD ELF MS", elapsed(blocker.and_then(|b| b.yielded_at), elf.queued));
                display.hex(6, "ELF YIELDED", u32::from(blocker.is_some_and(|b| b.yielded)));
                display.hex(8, "DONE YIELD MS", elapsed(blocker.and_then(|b| b.done), blocker.and_then(|b| b.yielded_at)));
            }
        }
        #[cfg(feature = "sunfish-wlan-elf-exchange-trace")]
        if self.error == 0
            && let Some(Stage::Services(owner)) = self.stage.as_ref()
            && let trace = owner.service_registry_trace()
            && let (Some(elf), Some(exchange)) = (trace.elf, trace.elf_exchange)
        {
            display.replace_line(1, "ELF QRTR EXCHANGE");
            display.hex(2, "IPA COMPLETE", u32::from(self.ipa_complete_seen));
            display.hex(3, "ELF TXN", u32::from(exchange.transaction));
            display.hex(4, "ELF QRTR BYTES", u32::from(exchange.wire_bytes));
            display.hex(5, "ELF QMI MATCH", u32::from(exchange.qmi_matches_reference));
            display.hex(6, "ELF SRC PORT", exchange.source.port);
            display.hex(7, "ELF DST PORT", exchange.destination.port);
            display.hex(8, "ELF DST NODE", exchange.destination.node);
            display.hex(9, "ELF CONFIRM", exchange.confirm_rx);
            display.hex(10, "ELF RXDONE", u32::from(exchange.completed));
            display.hex(11, "ELF DONE MS", elapsed(elf.received, elf.done));
            display.hex(12, "DONE TO ELF MS", elapsed(elf.blocker.and_then(|b| b.done), elf.queued));
            display.hex(13, "WLAN RRQ RX", owner.tftp_wire_counts()[2]);
            let query = self.wlan_query.snapshot(owner);
            display.hex(14, "WLFW SERVERS", query[7]);
            for (index, labels) in [
                ["RX0 META", "RX0 SRC PORT", "RX0 DST PORT"],
                ["RX1 META", "RX1 SRC PORT", "RX1 DST PORT"],
                ["RX2 META", "RX2 SRC PORT", "RX2 DST PORT"],
                ["RX3 META", "RX3 SRC PORT", "RX3 DST PORT"],
            ].iter().enumerate() {
                let values = exchange.following[index].map_or([u32::MAX; 3], |header|
                    [header.meta, header.source_port, header.destination_port]);
                for (field, (label, value)) in labels.iter().zip(values).enumerate() {
                    display.hex(15 + index * 3 + field, label, value);
                }
            }
            display.hex(27, "PD QUERY STATE", query[1]);
            display.hex(28, "ELF SRC NODE", exchange.source.node);
        }
        #[cfg(feature = "sunfish-wlan-elf-credit-trace")]
        if self.error == 0
            && let Some(Stage::Services(owner)) = self.stage.as_ref()
            && let trace = owner.service_registry_trace()
            && let (Some(elf), Some(exchange)) = (trace.elf, trace.elf_exchange)
        {
            let credit = exchange.credit;
            display.replace_line(1, "ELF QRTR CREDIT");
            display.hex(3, "CREDIT APPLIED", credit.applied);
            display.hex(6, "CREDIT SRC PORT", credit.first_resume.map_or(u32::MAX, |resume| resume.source_port));
            display.hex(4, "ELF PUBLISHED", u32::from(credit.published));
            display.hex(8, "CREDIT PRE PUB", credit.resumes[0]);
            display.hex(11, "CREDIT PRE DONE", credit.resumes[1]);
            display.hex(12, "CREDIT AFT DONE", credit.resumes[2]);
            display.hex(15, "CREDIT DST PORT", credit.first_resume.map_or(u32::MAX, |resume| resume.local_port));
            display.hex(16, "CREDIT AGE MS", elapsed(elf.queued, credit.first_resume.and_then(|resume| resume.observed_at)));
            display.hex(17, "FIRST DATA META", credit.first_data.map_or(u32::MAX, |header| header.meta));
            display.hex(18, "FIRST DATA SRC", credit.first_data.map_or(u32::MAX, |header| header.source_port));
            display.hex(19, "FIRST DATA DST", credit.first_data.map_or(u32::MAX, |header| header.destination_port));
            for (index, labels) in [
                ["NEW0 SERVICE", "NEW0 PORT"],
                ["NEW1 SERVICE", "NEW1 PORT"],
                ["NEW2 SERVICE", "NEW2 PORT"],
                ["NEW3 SERVICE", "NEW3 PORT"],
            ].iter().enumerate() {
                let values = credit.servers[index].map_or([u32::MAX; 2], |server|
                    [server.service, server.address.port]);
                for (field, (label, value)) in labels.iter().zip(values).enumerate() {
                    display.hex(20 + index * 2 + field, label, value);
                }
            }
            display.hex(28, "PD QUERY STATE", self.wlan_query.snapshot(owner)[1]);
        }
        #[cfg(feature = "sunfish-wlan-elf-host-credit-trace")]
        if self.error == 0
            && let Some(Stage::Services(owner)) = self.stage.as_ref()
            && let Some(exchange) = owner.service_registry_trace().elf_exchange
        {
            let host = exchange.host_credit;
            let before = host.and_then(|credit| credit.before_reply);
            let tftp = exchange.first_tftp_request;
            display.replace_line(1, "ELF HOST CREDIT");
            display.hex(6, "REQ CONFIRM", host.map_or(u32::MAX, |credit| u32::from(credit.requested)));
            display.hex(8, "HOST PUBLISHED", host.map_or(u32::MAX, |credit| u32::from(credit.remote_intent.is_some())));
            display.hex(11, "HOST RXDONE", host.map_or(u32::MAX, |credit| u32::from(credit.completed)));
            display.hex(15, "PUB BEFORE ELF", before.map_or(u32::MAX, |flags| u32::from(flags[0])));
            display.hex(16, "DONE BEFORE ELF", before.map_or(u32::MAX, |flags| u32::from(flags[1])));
            display.hex(20, "HOST INTENT", host.and_then(|credit| credit.remote_intent).unwrap_or(u32::MAX));
            display.hex(21, "TFTP REQ OPCODE", tftp.map_or(u32::MAX, |request| u32::from(request.opcode)));
            display.hex(22, "TFTP REQ BYTES", tftp.map_or(u32::MAX, |request| request.body_bytes));
            display.hex(23, "TFTP REQ SRC", tftp.map_or(u32::MAX, |request| request.source_port));
            display.hex(24, "TFTP REQ DST", tftp.map_or(u32::MAX, |request| request.destination_port));
            display.replace_line(25, "FIRST TFTP REQUEST");
            display.replace_line(26, tftp.map_or("AWAITING RRQ OR WRQ", |request| request.path_kind));
            display.hex(27, "ELF TXN", u32::from(exchange.transaction));
        }
        #[cfg(feature = "sunfish-wlan-elf-server-check-trace")]
        if (self.error == 0 || cfg!(feature = "sunfish-wlan-elf-server-check-timing"))
            && let Some(Stage::Services(owner)) = self.stage.as_ref()
            && let Some(exchange) = owner.service_registry_trace().elf_exchange
            && let Some(write) = exchange.server_check
        {
            let elapsed_us = |start: Option<u64>, end: Option<u64>| {
                match start.zip(end).and_then(|(start, end)| end.checked_sub(start)) {
                    Some(ticks) if self.hz != 0 =>
                        ((u128::from(ticks) * 1_000_000 / u128::from(self.hz)).min(u128::from(u32::MAX))) as u32,
                    _ => u32::MAX,
                }
            };
            let age_us = |end| elapsed_us(write.received_at, end);
            display.replace_line(1, "ELF SERVER CHECK");
            display.hex(4, "WRQ PEER", write.peer.port);
            display.hex(5, "WRQ RESULT", write.wrq_result.unwrap_or(u32::MAX));
            display.hex(6, "WRQ COALESCED", u32::from(write.coalesced));
            display.hex(7, "TRANSFER PORT", write.transfer_port.unwrap_or(u32::MAX));
            display.hex(8, "OACK PHASE", u32::from(write.oack.phase));
            display.hex(9, "OACK PUB US", age_us(write.oack.published_at));
            display.hex(10, "OACK DONE US", age_us(write.oack.done_at));
            display.hex(11, "DATA AGE US", age_us(write.data.and_then(|data| data.received_at)));
            display.hex(12, "DATA RESULT", write.data.and_then(|data| data.result).unwrap_or(u32::MAX));
            display.hex(15, "DATA BLOCK", write.data.map_or(u32::MAX, |data| u32::from(data.block)));
            display.hex(16, "DATA BYTES", write.data.map_or(u32::MAX, |data| data.body_bytes));
            display.hex(17, "WRITE COMPLETE", u32::from(write.final_block.is_some()));
            display.hex(18, "FINAL ACK BLOCK", write.final_block.map_or(u32::MAX, u32::from));
            display.hex(19, "FINAL ACK PHASE", u32::from(write.ack.phase));
            display.hex(20, "ACK PUB US", age_us(write.ack.published_at));
            display.hex(21, "ACK DONE US", age_us(write.ack.done_at));
            #[cfg(feature = "sunfish-wlan-elf-server-check-timing")]
            {
                let timing = write.timing;
                display.replace_line(1, "ELF SERVER TIMING");
                if self.error != 0 {
                    display.hex(2, "RUNTIME ERROR", self.error);
                    display.hex(3, "ERROR DETAIL", self.detail);
                }
                display.hex(13, "WLAN RRQ RX", owner.tftp_wire_counts()[2]);
                let query = self.wlan_query.snapshot(owner);
                display.hex(14, "WLFW SERVERS", query[7]);
                display.hex(18, "CLOSE OACK PH", timing.first_close_oack_phase.map_or(u32::MAX, u32::from));
                display.hex(22, "DISPATCH US", age_us(timing.dispatch_at));
                display.hex(23, "OACK READY US", age_us(timing.ready_at));
                display.hex(24, "OACK QUEUE US", age_us(timing.queued_at));
                display.hex(25, "WRQ REPEATS", timing.repeats);
                display.hex(26, "FIRST REPT US", age_us(timing.first_repeat_at));
                display.hex(27, "PEER CLOSES", timing.closes);
                display.hex(28, "FIRST CLOSE US", age_us(timing.first_close_at));
            }
            #[cfg(feature = "sunfish-wlan-elf-oack-queue-trace")]
            {
                let queue = write.timing.queue;
                let ticks_us = |ticks| elapsed_us(Some(0), Some(ticks));
                display.replace_line(1, "ELF OACK TX WAIT");
                if self.error == 0 {
                    display.hex(2, "OACK WRITE US", ticks_us(queue.oack_writer_ticks));
                }
                display.hex(5, "TX CALLS", queue.calls);
                display.hex(6, "FIRST TX US", elapsed_us(write.timing.queued_at, queue.first_at));
                display.hex(7, "MAX TX GAP US", ticks_us(queue.max_gap_ticks));
                display.hex(8, "TX ACTIVE US", ticks_us(queue.active_ticks));
                display.hex(9, "TX WRITER US", ticks_us(queue.writer_ticks));
                display.hex(10, "NO FIT CALLS", queue.no_fit);
                display.hex(11, "INTENT WAIT", queue.intent_wait);
                display.hex(12, "OLDER FLOW", queue.older_flow);
                display.hex(15, "RX ADS AHEAD", queue.advertisements);
                display.hex(16, "INTENT ACKS", queue.intent_acks);
                display.hex(17, "RXDONE AHEAD", queue.receive_dones);
                display.hex(18, "OTHER TFTP", queue.other_tftp);
                display.hex(19, "OTHER DATA", queue.other_data);
                display.hex(20, "OACK BLOCKED", queue.oack_blocked);
                display.hex(21, "OTHER BLOCKED", queue.other_blocked);
                display.hex(22, "INTENT REQS", queue.intent_requests);
                display.hex(23, "IDLE CALLS", queue.idle);
                display.hex(24, "OACK QUEUE US", age_us(write.timing.queued_at));
                display.hex(25, "OACK PUB US", age_us(write.oack.published_at));
                display.hex(26, "OACK DONE US", age_us(write.oack.done_at));
                display.hex(27, "PEER CLOSES", write.timing.closes);
                display.hex(28, "WRITE COMPLETE", u32::from(write.final_block.is_some()));
            }
            #[cfg(feature = "sunfish-wlan-elf-oack-router-turn")]
            {
                let (entry, end) = self.router.elf_oack_turn().unzip();
                let first_tx = write.timing.queue.first_at;
                display.replace_line(1, "ELF OACK EARLY TURN");
                display.hex(7, "QUEUE ROUTER US", elapsed_us(write.timing.queued_at, entry));
                display.hex(8, "ROUTER TO TX US", elapsed_us(entry, first_tx));
                display.hex(9, "EARLY TURN US", elapsed_us(entry, end));
                display.hex(15, "TX IN TURN", entry.zip(end).map_or(u32::MAX, |(start, end)| {
                    u32::from(first_tx.is_some_and(|tx| (start..=end).contains(&tx)))
                }));
                display.hex(17, "DATA RESULT", write.data.and_then(|data| data.result).unwrap_or(u32::MAX));
                display.hex(18, "DATA BLOCK", write.data.map_or(u32::MAX, |data| u32::from(data.block)));
                display.hex(19, "FINAL ACK PHASE", u32::from(write.ack.phase));
            }
            #[cfg(feature = "sunfish-wlan-elf-oack-peer-trace")]
            {
                use personal_os_rust_server::qcom_glink_qmi::ElfPeerPacket;

                let timing = write.timing;
                let peer_rx = timing.peer_rx;
                let port = |packet: Option<ElfPeerPacket>| {
                    packet.map_or(u32::MAX, |packet| packet.destination_port)
                };
                let metadata = |packet: Option<ElfPeerPacket>| {
                    packet.map_or(u32::MAX, |packet|
                        (u32::from(packet.body_bytes) << 16)
                            | u32::from(packet.block_or_error.unwrap_or(u16::MAX)))
                };
                display.replace_line(1, "ELF OACK PEER RX");
                if self.error == 0 {
                    display.hex(2, "FINAL ACK PHASE", u32::from(write.ack.phase));
                }
                display.hex(5, "DATA COUNT", peer_rx.data.count);
                display.hex(8, "CLOSE OACK PH", timing.first_close_oack_phase.map_or(u32::MAX, u32::from));
                display.hex(9, "TRANSFER PORT", write.transfer_port.unwrap_or(u32::MAX));
                display.hex(10, "DATA DST PORT", port(peer_rx.data.first));
                display.hex(11, "DATA LEN BLOCK", metadata(peer_rx.data.first));
                display.hex(12, "DATA AGE US", age_us(peer_rx.data.first.and_then(|packet| packet.received_at)));
                display.hex(16, "ERROR COUNT", peer_rx.error.count);
                display.hex(17, "ERROR DST PORT", port(peer_rx.error.first));
                display.hex(18, "ERROR LEN CODE", metadata(peer_rx.error.first));
                display.hex(19, "ERROR AGE US", age_us(peer_rx.error.first.and_then(|packet| packet.received_at)));
                display.hex(20, "REPEAT COUNT", peer_rx.repeat.count);
                display.hex(21, "REPEAT DST PORT", port(peer_rx.repeat.first));
                display.hex(22, "REPEAT AGE US", age_us(peer_rx.repeat.first.and_then(|packet| packet.received_at)));
                display.hex(23, "CLOSE AGE US", age_us(timing.first_close_at));
            }
        }
        #[cfg(feature = "sunfish-wlan-first-server-check-trace")]
        if let Some(Stage::Services(owner)) = self.stage.as_ref() {
            #[cfg(not(feature = "sunfish-wlan-elf-tftp-timeline"))]
            self.show_first_server_check(display, owner);
            #[cfg(all(feature = "sunfish-wlan-tftp-progress-display", not(feature = "sunfish-wlan-elf-tftp-timeline")))]
            {
                let ack = owner.tftp_final_ack_trace();
                let query = self.wlan_query.snapshot(owner);
                display.replace_line(1, "TFTP WLAN PROGRESS");
                display.hex(18, "WRITES COMPLETE", self.server_check.completed_count() as u32);
                display.hex(19, "FINAL ACK FLAGS", ack.map_or(0, |trace| trace.flags));
                display.hex(23, "ACK PEER PORT", ack.map_or(u32::MAX, |trace| trace.destination.port));
                display.hex(24, "ACK LOCAL PORT", ack.map_or(u32::MAX, |trace| trace.source.port));
                display.hex(25, "PD QUERY PHASE", query[0]);
                display.hex(26, "PD QUERY STATE", query[1]);
                display.hex(27, "WLAN RRQ RX", owner.tftp_wire_counts()[2]);
                display.hex(28, "WLFW SERVERS", query[7]);
            }
            #[cfg(all(feature = "sunfish-wlan-elf-tftp-timeline", not(feature = "sunfish-wlan-tftp-oack-compare")))]
            self.show_elf_tftp_timeline(display, owner);
            #[cfg(all(feature = "sunfish-wlan-tftp-oack-compare", not(feature = "sunfish-wlan-first-wrq-window")))]
            self.show_tftp_oack_comparison(display, owner);
            #[cfg(all(feature = "sunfish-wlan-first-wrq-window", not(feature = "sunfish-ipa-registration-trace")))]
            self.show_first_wrq_window(display, owner);
            #[cfg(all(feature = "sunfish-ipa-registration-trace", not(feature = "sunfish-ipa-init-continuation")))]
            self.show_ipa_registration(display, owner);
            #[cfg(all(feature = "sunfish-ipa-init-continuation", not(feature = "sunfish-ipa-init-tx-trace")))]
            self.show_ipa_init(display, owner);
            #[cfg(all(feature = "sunfish-ipa-init-tx-trace", not(any(feature = "sunfish-ipa-schedule-trace", feature = "sunfish-ipa-independent-server-runtime"))))]
            self.show_ipa_init_tx(display, owner);
            #[cfg(feature = "sunfish-ipa-schedule-trace")]
            self.show_ipa_schedule(display, owner);
            #[cfg(all(feature = "sunfish-ipa-independent-server-runtime", not(feature = "sunfish-ipa-registration-tx-trace")))]
            self.show_ipa_independent(display, owner);
            #[cfg(any(feature = "sunfish-ipa-registration-cold-handoff", all(feature = "sunfish-ipa-registration-tx-trace", not(feature = "sunfish-ipa-registration-prep-trace"))))]
            self.show_ipa_registration_tx(display, owner);
            #[cfg(all(feature = "sunfish-ipa-registration-prep-trace", not(feature = "sunfish-ipa-registration-cold-handoff")))]
            self.show_registration_prep(display, owner);
        }
        #[cfg(feature = "sunfish-tftp-oack-ahead-headers")]
        if let Some(Stage::Services(owner)) = self.stage.as_ref() {
            self.show_oack_resources(display, owner);
        }
        display.replace_line(29, "RESTART WHEN DONE");
        display.flush();
    }

    #[cfg(feature = "sunfish-wlan-first-server-check-trace")]
    fn time_from_us(&self, origin: Option<u64>, event: Option<u64>) -> u32 {
        match origin.zip(event).and_then(|(start, end)| end.checked_sub(start)) {
            Some(ticks) if self.hz != 0 =>
                (u128::from(ticks) * 1_000_000 / u128::from(self.hz)).min(u128::from(u32::MAX)) as u32,
            _ => u32::MAX,
        }
    }

    #[cfg(all(feature = "sunfish-wlan-elf-tftp-timeline", not(feature = "sunfish-wlan-tftp-oack-compare")))]
    fn show_elf_tftp_timeline(&self, display: &Display, owner: &ModemOwner) {
        let registry = owner.service_registry_trace();
        let elf = registry.elf;
        let exchange = registry.elf_exchange;
        let ack = owner.tftp_final_ack_trace();
        let post_tftp = exchange.and_then(|trace| trace.first_tftp_request);
        let first = registry.first_server_check;
        let query = self.wlan_query.snapshot(owner);
        let (wlan_rrq, wlfw_server) = owner.wlan_discovery_times();
        let age = |event| self.time_from_us(Some(self.runtime_started), event);

        display.replace_line(1, "ELF WLAN TIMELINE");
        display.hex(2, "RUNTIME ERROR", self.error);
        display.hex(3, "ERROR DETAIL", self.detail);
        display.hex(4, "ELF TXN", exchange.map_or(u32::MAX, |trace| u32::from(trace.transaction)));
        display.hex(5, "ELF QMI MATCH", exchange.map_or(u32::MAX, |trace| u32::from(trace.qmi_matches_reference)));
        display.hex(6, "ELF RX US", age(elf.and_then(|trace| trace.received)));
        display.hex(7, "ELF PUB US", age(elf.and_then(|trace| trace.published)));
        display.hex(8, "ELF DONE US", age(elf.and_then(|trace| trace.done)));
        display.hex(9, "ELF ROUND US", elf.map_or(u32::MAX, |trace| self.time_from_us(trace.received, trace.done)));
        display.hex(10, "WRITES COMPLETE", self.server_check.completed_count() as u32);
        display.hex(11, "ACK ACCEPT US", age(ack.and_then(|trace| trace.accepted_at)));
        display.hex(12, "ACK PUB US", age(ack.and_then(|trace| trace.published_at)));
        display.hex(13, "ACK DONE US", age(ack.and_then(|trace| trace.done_at)));
        display.hex(14, "FINAL ACK FLAGS", ack.map_or(0, |trace| trace.flags));
        display.hex(15, "ACK PEER PORT", ack.map_or(u32::MAX, |trace| trace.destination.port));
        display.hex(16, "ACK LOCAL PORT", ack.map_or(u32::MAX, |trace| trace.source.port));
        display.hex(17, "POST TFTP OP", post_tftp.map_or(u32::MAX, |request| u32::from(request.opcode)));
        display.hex(18, "POST TFTP PEER", post_tftp.map_or(u32::MAX, |request| request.source_port));
        display.replace_line(19, post_tftp.map_or("POST TFTP PENDING", |request| request.path_kind));
        display.hex(20, "WLAN RRQ US", age(wlan_rrq));
        display.hex(21, "WLAN RRQ RX", owner.tftp_wire_counts()[2]);
        display.hex(22, "WLFW FIRST US", age(wlfw_server));
        display.hex(23, "WLFW SERVERS", query[7]);
        display.hex(24, "PD QUERY PHASE", query[0]);
        display.hex(25, "PD QUERY STATE", query[1]);
        display.hex(26, "FIRST WRQ US", age(first.and_then(|trace| trace.received_at)));
        display.hex(27, "FIRST OACK US", age(first.and_then(|trace| trace.oack.published_at)));
        display.hex(28, "FIRST DATA RX", first.map_or(u32::MAX, |trace| trace.timing.peer_rx.data.count));
    }

    #[cfg(feature = "sunfish-ipa-registration-prep-trace")]
    fn show_registration_prep(&self, display: &Display, owner: &ModemOwner) {
        let reg = owner.ipa_registration_trace();
        let route = owner.ipa_registration_owner_trace();
        let step = route.and_then(|r| r.first_step);
        let [yielded, early] = owner.ipa_registration_turn_times();
        let (init, transport) = owner.ipa_init_trace();
        let [first, _] = owner.server_check_oacks();
        let query = self.wlan_query.snapshot(owner);
        let age = |event| self.time_from_us(Some(self.runtime_started), event);
        let work = |slot: usize| self.time_from_us(Some(0), Some(self.registration_prep_work[slot]));
        let step_code = step.map_or(u32::MAX, |step| {
            use qcom_modem_owner::LinuxIpaProgress::*;
            match step.progress {
                None => 0, Some(WaitingPeer) => 1, Some(Discovered) => 2, Some(Prepared) => 3,
                Some(Queued) => 4, Some(WaitingQueue) => 5, Some(WaitingCompletion) => 6,
                Some(Committed) => 7, Some(ResponseAccepted) => 8,
                Some(WaitingHandshake) => 9, Some(Complete) => 10,
            }
        });
        display.replace_line(1, if cfg!(feature = "sunfish-ipa-registration-cold-handoff") {
            "IPA REG COLD HANDOFF"
        } else {
            "IPA REG PREP WORK"
        });
        for (row, (label, value)) in [
            ("RUNTIME ERROR", self.error),
            ("ERROR DETAIL", self.detail),
            ("REG RX US", age(reg.and_then(|r| r.received_at))),
            ("REG ROUTE US", age(reg.and_then(|r| r.routed_at))),
            ("FIRST CLOSE US", age(first.and_then(|t| t.first_close_at))),
            ("REG PREP US", age(reg.and_then(|r| r.prepared_at))),
            ("REG Q TRY US", age(reg.and_then(|r| r.first_queue_attempt_at))),
            ("REG QUEUE US", age(reg.and_then(|r| r.queued_at))),
            ("ROUTE IPA FLAGS", route.map_or(u32::MAX, |r| r.route_flags)),
            ("ROUTE IPA QMI", route.and_then(|r| r.route_pending_id).map_or(u32::MAX, u32::from)),
            ("REG YIELD US", age(yielded)),
            ("IPA STEP US", age(step.map(|r| r.started_at))),
            ("IPA STEP END US", age(step.and_then(|r| r.ended_at))),
            ("IPA STEP RESULT", step_code),
            ("INIT DISC US", age(init.and_then(|t| t.discovered_at))),
            ("INIT QUEUE US", age(transport.and_then(|t| t.queued_at))),
            ("REG EARLY US", age(early)),
            ("ROUTER WORK US", work(0)),
            ("RMTFS WORK US", work(1)),
            ("IPA WORK US", work(2)),
            ("PAINT WORK US", work(3)),
            ("LOC WORK US", work(4)),
            ("TFTP WORK US", work(5)),
            ("REG DONE US", age(reg.and_then(|r| r.completed_at))),
            ("PD QUERY STATE", query[1]),
            ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
            ("WLFW SERVERS", query[7]),
        ].into_iter().enumerate() { display.hex(row + 2, label, value); }
    }

    #[cfg(feature = "sunfish-ipa-registration-tx-trace")]
    fn show_ipa_registration_tx(&self, display: &Display, owner: &ModemOwner) {
        #[cfg(not(feature = "sunfish-tftp-first-oack-headers"))]
        let reg = owner.ipa_registration_trace();
        #[cfg(not(feature = "sunfish-ipa-registration-reply-headers"))]
        let tx = reg.map(|r| r.tx).unwrap_or_default();
        #[cfg(all(feature = "sunfish-ipa-registration-rxdone-turn", not(feature = "sunfish-ipa-registration-rxdone-trace")))]
        let (_, init_transport) = owner.ipa_init_trace();
        #[cfg(feature = "sunfish-ipa-registration-rxdone-turn")]
        let [first_oack, completed_oack] = owner.server_check_oacks();
        #[cfg(not(feature = "sunfish-tftp-first-oack-unheld"))]
        let _ = completed_oack;
        let query = self.wlan_query.snapshot(owner);
        #[cfg(not(feature = "sunfish-ipa-registration-reply-headers"))]
        let pair = diagnostic::packed_counts;
        let age = |event| self.time_from_us(Some(self.runtime_started), event);
        #[cfg(feature = "sunfish-tftp-first-oack-headers")]
        {
            let headers = owner.first_oack_headers().unwrap_or_default();
            let rx = owner.first_oack_rx_done();
            let peer_tail = match owner.service_registry_trace().first_server_check_tail {
                qcom_glink_fifo::ServerCheckTail::Consumed(at) => at,
                _ => None,
            };
            display.replace_line(1, "FIRST OACK CLIENT");
            for (row, (label, value)) in [
                ("RUNTIME ERROR", self.error),
                ("MIDDLE OMITTED", headers.omitted),
                ("FIRST OACK US", age(first_oack.and_then(|t| t.oack_at))),
                ("FIRST CLOSE US", age(first_oack.and_then(|t| t.first_close_at))),
                ("PEER TAIL US", age(peer_tail)),
                ("RX ABSENT US", age(rx.and_then(|t| t.last_absent))),
                ("RX VISIBLE US", age(rx.and_then(|t| t.first_visible))),
                ("OACK DONE US", age(first_oack.and_then(|t| t.oack_done_at))),
                ("DONE OACK US", age(completed_oack.and_then(|t| t.oack_at))),
                ("DONE GLINK US", age(completed_oack.and_then(|t| t.oack_done_at))),
                ("FIRST DATA US", age(first_oack.and_then(|t| t.first_data_at))),
            ].into_iter().enumerate() { display.hex(row + 2, label, value); }
            for (index, header) in headers.recent.into_iter().chain([headers.completion]).enumerate() {
                let row = index + 13;
                if let Some(header) = header {
                    let [kind, source, destination] = header.qrtr.unwrap_or([u32::MAX; 3]);
                    let (qmi, transaction, message) = header.qmi.unwrap_or((0xf, u16::MAX, u16::MAX));
                    display.header_row(row, false, &[
                        (age(header.at), 8), (u32::from(header.tx), 1),
                        (u32::from(header.command), 4), (u32::from(header.channel), 4),
                        (header.parameter, 8), (u32::from(header.bytes), 4),
                    ]);
                    if let Some([node, port]) = header.del_client {
                        display.header_row(row, true, &[
                            (kind, 2), (source, 8), (destination, 8), (node, 8), (port, 8),
                        ]);
                    } else {
                        display.header_row(row, true, &[
                            (kind, 2), (source, 8), (destination, 8), (u32::from(qmi), 1),
                            (u32::from(transaction), 4), (u32::from(message), 4),
                        ]);
                    }
                } else {
                    display.replace_line(row, "");
                }
            }
            display.hex(26, "PD QUERY STATE", query[1]);
            display.hex(27, "WLAN RRQ RX", owner.tftp_wire_counts()[2]);
            display.hex(28, "WLFW SERVERS", query[7]);
        }
        #[cfg(all(feature = "sunfish-ipa-registration-rx-first", feature = "sunfish-wlan-elf-oack-queue-trace"))]
        {
            let registry = owner.service_registry_trace();
            let write = registry.first_server_check;
            let timing = write.map(|t| t.timing);
            #[cfg(not(all(feature = "sunfish-ipa-reply-before-oack", feature = "sunfish-wlan-first-oack-rx-fifo")))]
            let queue = timing.map_or_else(Default::default, |t| t.queue);
            #[cfg(all(feature = "sunfish-first-oack-owner-turn", feature = "sunfish-wlan-first-oack-rx-fifo"))]
            let rows = {
                #[cfg(not(feature = "sunfish-ipa-reply-fifo-visibility"))]
                let rx = owner.first_oack_rx_done();
                #[cfg(not(feature = "sunfish-ipa-reply-fifo-visibility"))]
                let peer_tail = match registry.first_server_check_tail {
                    qcom_glink_fifo::ServerCheckTail::Consumed(at) => at,
                    _ => None,
                };
                #[cfg(feature = "sunfish-ipa-reply-before-oack")]
                let rows = {
                    let tx = reg.map_or_else(Default::default, |trace| trace.tx);
                    let pair = diagnostic::packed_counts;
                    let duration = |ticks| self.time_from_us(Some(0), Some(ticks));
                    #[cfg(feature = "sunfish-ipa-reply-fifo-visibility")]
                    let fifo = owner.registration_reply_fifo().unwrap_or_default();
                    #[cfg(feature = "sunfish-ipa-reply-fifo-visibility")]
                    let done = fifo.completion_visibility;
                    display.replace_line(1, if cfg!(feature = "sunfish-ipa-reply-fifo-visibility") {
                        "IPA REPLY FIFO GRANT"
                    } else if cfg!(feature = "sunfish-ipa-reply-before-grant") {
                        "IPA REPLY BEFORE GRANT"
                    } else {
                        "IPA REPLY WRITER"
                    });
                    [
                        ("RUNTIME ERROR", self.error),
                        ("ERROR DETAIL", self.detail),
                        ("REG QUEUE US", age(reg.and_then(|t| t.queued_at))),
                        ("REQ RXDONE US", age(reg.and_then(|t| t.tx.request_done_at))),
                        ("REG PASS US", age(tx.first_pass_at)),
                        ("REG SELECT US", age(tx.selected_at)),
                        ("REG PUB US", age(reg.and_then(|t| t.published_at))),
                        ("REG DONE US", age(reg.and_then(|t| t.completed_at))),
                        ("REG PASSES", tx.passes),
                        ("ACTIVE US", duration(tx.active_ticks)),
                        ("WRITER US", duration(tx.writer_ticks)),
                        ("MAX GAP US", duration(tx.max_gap_ticks)),
                        ("AD ACK", pair(tx.commands[0], tx.commands[3])),
                        ("DONE REUSE", pair(tx.commands[1], tx.commands[4])),
                        #[cfg(feature = "sunfish-ipa-reply-fifo-visibility")]
                        ("REG RX ABSENT", age(done.and_then(|t| t.last_absent))),
                        #[cfg(not(feature = "sunfish-ipa-reply-fifo-visibility"))]
                        ("DATA OTHER", pair(tx.data_ahead, tx.commands[5])),
                        ("WAIT NO FIT", pair(tx.gates[0], tx.gates[1])),
                        ("FLOW CREDIT", pair(tx.gates[2], tx.gates[7])),
                        ("REG OTHER BLOCK", pair(tx.target_blocked, tx.other_blocked)),
                        ("FIRST CLOSE US", age(timing.and_then(|t| t.first_close_at))),
                        ("FIRST OACK US", age(first_oack.and_then(|t| t.oack_at))),
                        #[cfg(feature = "sunfish-ipa-reply-fifo-visibility")]
                        ("REG TAIL US", age(fifo.peer_tail_at)),
                        #[cfg(not(feature = "sunfish-ipa-reply-fifo-visibility"))]
                        ("PEER TAIL US", age(peer_tail)),
                        #[cfg(feature = "sunfish-ipa-reply-fifo-visibility")]
                        ("REG RX VISIBLE", age(done.and_then(|t| t.first_visible))),
                        #[cfg(not(feature = "sunfish-ipa-reply-fifo-visibility"))]
                        ("RX VISIBLE US", age(rx.and_then(|t| t.first_visible))),
                        ("OACK DONE US", age(first_oack.and_then(|t| t.oack_done_at))),
                        ("FIRST DATA US", age(first_oack.and_then(|t| t.first_data_at))),
                        ("PD QUERY STATE", query[1]),
                        ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                        ("WLFW SERVERS", query[7]),
                    ]
                };
                #[cfg(not(feature = "sunfish-ipa-reply-before-oack"))]
                let rows = {
                    display.replace_line(1, "OACK IPA FIFO");
                    [
                        ("RUNTIME ERROR", self.error),
                        ("ERROR DETAIL", self.detail),
                        ("FIRST WRQ US", age(first_oack.and_then(|t| t.wrq_at))),
                        ("WRQ RXDONE US", age(first_oack.and_then(|t| t.wrq_done_at))),
                        ("OACK QUEUE US", age(timing.and_then(|t| t.queued_at))),
                        ("FIRST TURN US", age(queue.first_at)),
                        ("REG RX US", age(reg.and_then(|t| t.received_at))),
                        ("REQ RXDONE US", age(reg.and_then(|t| t.tx.request_done_at))),
                        ("REG PUB US", age(reg.and_then(|t| t.published_at))),
                        ("REG DONE US", age(reg.and_then(|t| t.completed_at))),
                        ("FIRST OACK US", age(first_oack.and_then(|t| t.oack_at))),
                        ("PEER TAIL US", age(peer_tail)),
                        ("RX ABSENT US", age(rx.and_then(|t| t.last_absent))),
                        ("RX VISIBLE US", age(rx.and_then(|t| t.first_visible))),
                        ("RX AT CLOSE", rx.and_then(|t| t.visible_at_close).map_or(u32::MAX, u32::from)),
                        ("RX BYTES AHEAD", rx.and_then(|t| t.bytes_ahead).unwrap_or(u32::MAX)),
                        ("OACK DONE US", age(first_oack.and_then(|t| t.oack_done_at))),
                        ("FIRST DATA US", age(first_oack.and_then(|t| t.first_data_at))),
                        ("FIRST CLOSE US", age(timing.and_then(|t| t.first_close_at))),
                        ("OACK AT CLOSE", timing.and_then(|t| t.first_close_oack_phase).map_or(u32::MAX, u32::from)),
                        ("DONE OACK US", age(completed_oack.and_then(|t| t.oack_at))),
                        ("DONE GLINK US", age(completed_oack.and_then(|t| t.oack_done_at))),
                        ("WRITER PASSES", queue.calls),
                        ("OACK BLOCKED", queue.oack_blocked),
                        ("PD QUERY STATE", query[1]),
                        ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                        ("WLFW SERVERS", query[7]),
                    ]
                };
                rows
            };
            #[cfg(not(all(feature = "sunfish-first-oack-owner-turn", feature = "sunfish-wlan-first-oack-rx-fifo")))]
            let rows = {
                display.replace_line(1, "FIRST OACK WRITER");
                [
                    ("RUNTIME ERROR", self.error),
                    ("ERROR DETAIL", self.detail),
                    ("FIRST WRQ US", age(first_oack.and_then(|t| t.wrq_at))),
                    ("OACK QUEUE US", age(timing.and_then(|t| t.queued_at))),
                    ("FIRST TURN US", age(queue.first_at)),
                    ("REG RX US", age(reg.and_then(|t| t.received_at))),
                    ("REQ RXDONE US", age(reg.and_then(|t| t.tx.request_done_at))),
                    ("FIRST CLOSE US", age(timing.and_then(|t| t.first_close_at))),
                    ("OACK AT CLOSE", timing.and_then(|t| t.first_close_oack_phase).map_or(u32::MAX, u32::from)),
                    ("FIRST OACK US", age(first_oack.and_then(|t| t.oack_at))),
                    ("OACK DONE US", age(first_oack.and_then(|t| t.oack_done_at))),
                    ("FIRST DATA US", age(first_oack.and_then(|t| t.first_data_at))),
                    ("WRITER PASSES", queue.calls),
                    ("NO FIT", queue.no_fit),
                    ("INTENT WAIT", queue.intent_wait),
                    ("OLDER FLOW", queue.older_flow),
                    ("ADS INTENT ACKS", (queue.advertisements.min(0xffff) << 16) | queue.intent_acks.min(0xffff)),
                    ("RXDONE TFTP", (queue.receive_dones.min(0xffff) << 16) | queue.other_tftp.min(0xffff)),
                    ("OTHER DATA", queue.other_data),
                    ("INTENT REQUESTS", queue.intent_requests),
                    ("OACK BLOCKED", queue.oack_blocked),
                    ("OTHER BLOCKED", queue.other_blocked),
                    ("IDLE", queue.idle),
                    ("EXTRA RX PASSES", owner.first_oack_rx_passes()),
                    ("PD QUERY STATE", query[1]),
                    ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                    ("WLFW SERVERS", query[7]),
                ]
            };
            for (row, (label, value)) in rows.into_iter().enumerate() {
                display.hex(row + 2, label, value);
            }
        }
        #[cfg(all(feature = "sunfish-ipa-registration-rx-visibility", not(all(feature = "sunfish-ipa-registration-rx-first", feature = "sunfish-wlan-elf-oack-queue-trace"))))]
        {
            let rx = owner.registration_rx_visibility();
            let elf_done = owner.service_registry_trace().elf.and_then(|trace| trace.done);
            display.replace_line(1, if cfg!(feature = "sunfish-ipa-registration-rx-first") {
                "IPA REG RX FIRST"
            } else {
                "IPA REG RX VISIBILITY"
            });
            for (row, (label, value)) in [
                ("RUNTIME ERROR", self.error),
                ("ERROR DETAIL", self.detail),
                #[cfg(feature = "sunfish-ipa-registration-rx-first")]
                ("EXTRA RX PASSES", owner.first_oack_rx_passes()),
                ("FIRST WRQ US", age(first_oack.and_then(|t| t.wrq_at))),
                ("FIRST OACK US", age(first_oack.and_then(|t| t.oack_at))),
                ("REG ABSENT US", age(rx.and_then(|t| t.last_absent))),
                ("REG VISIBLE US", age(rx.and_then(|t| t.first_visible))),
                ("REG RX US", age(reg.and_then(|t| t.received_at))),
                ("REG RX AHEAD", rx.and_then(|t| t.bytes_ahead).unwrap_or(u32::MAX)),
                ("REG ROUTE US", age(reg.and_then(|t| t.routed_at))),
                ("REG QUEUE US", age(reg.and_then(|t| t.queued_at))),
                ("REQ RXDONE US", age(reg.and_then(|t| t.tx.request_done_at))),
                ("REG PUB US", age(reg.and_then(|t| t.published_at))),
                ("REG DONE US", age(reg.and_then(|t| t.completed_at))),
                ("OACK DONE US", age(first_oack.and_then(|t| t.oack_done_at))),
                ("FIRST DATA US", age(first_oack.and_then(|t| t.first_data_at))),
                ("FIRST CLOSE US", age(first_oack.and_then(|t| t.first_close_at))),
                ("ELF DONE US", age(elf_done)),
                ("DONE OACK US", age(completed_oack.and_then(|t| t.oack_at))),
                ("DONE GLINK US", age(completed_oack.and_then(|t| t.oack_done_at))),
                ("PD QUERY STATE", query[1]),
                ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                ("WLFW SERVERS", query[7]),
            ].into_iter().enumerate() { display.hex(row + 2, label, value); }
        }
        #[cfg(all(feature = "sunfish-tftp-first-oack-unheld", not(any(feature = "sunfish-tftp-first-oack-headers", feature = "sunfish-ipa-registration-rx-visibility"))))]
        {
            let ack = owner.tftp_final_ack_trace();
            let registry = owner.service_registry_trace();
            let elf_done = registry.elf.and_then(|trace| trace.done);
            let rx = owner.first_oack_rx_done();
            let peer_tail = match registry.first_server_check_tail {
                qcom_glink_fifo::ServerCheckTail::Consumed(at) => at,
                _ => None,
            };
            let ack_done = ack.and_then(|trace| trace.done_at);
            let before = ack_done.zip(elf_done)
                .map_or(u32::MAX, |(ack, elf)| u32::from(ack < elf));
            display.replace_line(1, "FIRST TFTP FIFO");
            for (row, (label, value)) in [
                ("RUNTIME ERROR", self.error),
                ("ERROR DETAIL", self.detail),
                ("FIRST WRQ US", age(first_oack.and_then(|t| t.wrq_at))),
                ("FIRST PEER", first_oack.map_or(u32::MAX, |t| t.peer.port)),
                ("TRANSFER PORT", first_oack.map_or(u32::MAX, |t| t.transfer_port)),
                ("FIRST OACK US", age(first_oack.and_then(|t| t.oack_at))),
                ("OACK DONE US", age(first_oack.and_then(|t| t.oack_done_at))),
                ("FIRST DATA US", age(first_oack.and_then(|t| t.first_data_at))),
                ("FIRST CLOSE US", age(first_oack.and_then(|t| t.first_close_at))),
                ("FINAL ACK FLAGS", ack.map_or(0, |t| t.flags)),
                ("FINAL ACK US", age(ack.and_then(|t| t.published_at))),
                ("ACK DONE US", age(ack_done)),
                ("ELF DONE US", age(elf_done)),
                ("ACK BEFORE ELF", before),
                ("REG DONE US", age(reg.and_then(|t| t.completed_at))),
                ("PEER TAIL US", age(peer_tail)),
                ("RX ABSENT US", age(rx.and_then(|t| t.last_absent))),
                ("RX VISIBLE US", age(rx.and_then(|t| t.first_visible))),
                ("RX AT CLOSE", rx.and_then(|t| t.visible_at_close).map_or(u32::MAX, u32::from)),
                ("RX BYTES AHEAD", rx.and_then(|t| t.bytes_ahead).unwrap_or(u32::MAX)),
                ("DONE PEER", completed_oack.map_or(u32::MAX, |t| t.peer.port)),
                ("DONE OACK US", age(completed_oack.and_then(|t| t.oack_at))),
                ("DONE GLINK US", age(completed_oack.and_then(|t| t.oack_done_at))),
                ("WRITES COMPLETE", self.server_check.completed_count() as u32),
                ("PD QUERY STATE", query[1]),
                ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                ("WLFW SERVERS", query[7]),
            ].into_iter().enumerate() { display.hex(row + 2, label, value); }
        }
        #[cfg(all(feature = "sunfish-glink-intent-denial-snapshot", not(feature = "sunfish-tftp-first-oack-unheld")))]
        {
            let fifo = owner.registration_reply_fifo().unwrap_or_default();
            let denial = owner.intent_denial_window();
            display.replace_line(1, if cfg!(feature = "sunfish-glink-denial-rxdone-turn") {
                "IPA DENIAL RXDONE TURN"
            } else {
                "IPA INTENT DENIAL"
            });
            for (row, (label, value)) in [
                ("RUNTIME ERROR", self.error),
                ("ERROR DETAIL", self.detail),
                ("DENY SIZE", denial.map_or(u32::MAX, |d| d.requested)),
                ("DENY REQ US", age(denial.and_then(|d| d.request.at))),
                ("DENY ACK US", age(denial.and_then(|d| d.ack.at))),
                ("REG PUB US", age(reg.and_then(|r| r.published_at))),
                ("PEER TAIL US", age(fifo.peer_tail_at)),
                ("REG DONE US", age(reg.and_then(|r| r.completed_at))),
                ("FIRST CLOSE US", age(first_oack.and_then(|t| t.first_close_at))),
                ("FIRST OACK US", age(first_oack.and_then(|t| t.oack_at))),
                ("PD QUERY STATE", query[1]),
                ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                ("WLFW SERVERS", query[7]),
            ].into_iter().enumerate() { display.hex(row + 2, label, value); }
            display.replace_line(15, "SLOT REQ ID ACK ID");
            if let Some(denial) = denial {
                for (index, ((req_id, req_flags), (ack_id, ack_flags))) in
                    denial.request.slots.into_iter().zip(denial.ack.slots).enumerate()
                {
                    display.header_row(16 + index, false, &[
                        (index as u32, 1), (req_id, 8), (req_flags, 4),
                        (ack_id, 8), (ack_flags, 4),
                    ]);
                }
            }
        }
        #[cfg(all(feature = "sunfish-ipa-registration-reply-headers",
            not(feature = "sunfish-ipa-tftp-slot-snapshot"),
            any(not(feature = "sunfish-glink-intent-denial-snapshot"), feature = "sunfish-ipa-reply-grant-headers")))]
        {
            let fifo = owner.registration_reply_fifo().unwrap_or_default();
            #[cfg(feature = "sunfish-ipa-reply-grant-headers")]
            let (row_start, header_count, tail_start) = {
                let kept = fifo.headers.iter().flatten().count();
                let hidden = kept.saturating_sub(15);
                let visibility = fifo.completion_visibility;
                display.replace_line(1, if cfg!(feature = "sunfish-ipa-tftp-queue-block") {
                    "IPA TFTP QUEUE BLOCK"
                } else if cfg!(feature = "sunfish-ipa-tftp-original-trace") {
                    "IPA TFTP ORIGINAL"
                } else if cfg!(feature = "sunfish-ipa-tftp-dispatch-trace") {
                    "IPA TFTP DISPATCH"
                } else if cfg!(feature = "sunfish-ipa-grant-tftp-peer") {
                    "IPA TFTP PEER"
                } else {
                    "IPA GRANT HEADERS"
                });
                for (row, (label, value)) in [
                    ("RUNTIME ERROR", self.error),
                    ("REG PUB US", age(reg.and_then(|r| r.published_at))),
                    ("REG TAIL US", age(fifo.peer_tail_at)),
                    ("REG RX ABSENT", age(visibility.and_then(|v| v.last_absent))),
                    ("REG RX VISIBLE", age(visibility.and_then(|v| v.first_visible))),
                    ("REG DONE US", age(reg.and_then(|r| r.completed_at))),
                    ("FIRST CLOSE US", age(first_oack.and_then(|t| t.first_close_at))),
                    #[cfg(feature = "sunfish-ipa-grant-tftp-peer")]
                    ("ELF DONE US", age(owner.service_registry_trace().elf.and_then(|t| t.done))),
                    #[cfg(not(feature = "sunfish-ipa-grant-tftp-peer"))]
                    ("FIRST OACK US", age(first_oack.and_then(|t| t.oack_at))),
                    ("RECORDS OMITTED", fifo.dropped.saturating_add(hidden as u32)),
                    ("PD QUERY STATE", query[1]),
                    ("WLAN RRQ WLFW", diagnostic::packed_counts(owner.tftp_wire_counts()[2], query[7])),
                ].into_iter().enumerate() {
                    display.hex(row + 2, label, value);
                }
                (13, 16, hidden + 1)
            };
            #[cfg(not(feature = "sunfish-ipa-reply-grant-headers"))]
            let (row_start, header_count, tail_start) = {
                display.replace_line(1, if cfg!(feature = "sunfish-glink-registry-second-host-entry") {
                    "IPA REG TWO ENTRIES"
                } else if cfg!(feature = "sunfish-glink-registry-early-release") {
                    "IPA REG SLOT RELEASE"
                } else if cfg!(feature = "sunfish-glink-retry-full-intent") {
                    "IPA INTENT SLOT RETRY"
                } else {
                    "IPA REPLY HEAD TAIL"
                });
                display.hex(2, "RUNTIME ERROR", self.error);
                display.hex(3, "MIDDLE OMITTED", fifo.dropped);
                display.hex(4, "PEER TAIL US", age(fifo.peer_tail_at));
                display.hex(5, "FIRST CLOSE US", age(first_oack.and_then(|t| t.first_close_at)));
                display.hex(6, "PD QUERY STATE", query[1]);
                display.hex(7, "REG DONE US", age(reg.and_then(|r| r.completed_at)));
                (8, 20, 1)
            };
            let records = fifo.headers.into_iter().enumerate()
                .filter(|(index, _)| *index == 0 || *index >= tail_start)
                .map(|(_, header)| header);
            for (index, header) in records.take(header_count - 1)
                .chain([fifo.completion]).enumerate()
            {
                let row = index + row_start;
                if let Some(header) = header {
                    let [kind, source, destination] = header.qrtr.unwrap_or([u32::MAX; 3]);
                    let (qmi, transaction, message) = header.qmi.unwrap_or((0xf, u16::MAX, u16::MAX));
                    display.header_row(row, false, &[
                        (age(header.at), 8), (u32::from(header.tx), 1),
                        (u32::from(header.command), 4), (u32::from(header.channel), 4),
                        (header.parameter, 8), (u32::from(header.bytes), 4),
                    ]);
                    #[cfg(feature = "sunfish-ipa-reply-grant-headers")]
                    if let Some([node, port]) = header.del_client {
                        #[cfg(feature = "sunfish-ipa-grant-tftp-peer")]
                        if let Some(request) = header.tftp_request {
                            #[cfg(feature = "sunfish-ipa-tftp-original-trace")]
                            if let Some(original) = request.original {
                                #[cfg(feature = "sunfish-ipa-tftp-queue-block")]
                                {
                                    let block = original.queue_block;
                                    display.header_row(row, false, &[
                                        (age(header.at), 8), (u32::from(block.check), 2),
                                        (u32::from(block.kind), 2), (u32::from(block.opcode), 2),
                                        (u32::from(block.flags), 1),
                                    ]);
                                }
                                display.header_row(row, true, &[
                                    (kind, 2), (node, 8), (port, 8), (u32::from(original.source), 1),
                                    (original.id, 8), (u32::from(original.result), 2),
                                    (u32::from(original.occupied), 2), (u32::from(original.phases), 1),
                                ]);
                                continue;
                            }
                            display.header_row(row, true, &[
                                (kind, 2), (node, 8), (port, 8), (u32::from(request.opcode), 2),
                                (u32::from(request.kind), 2), (u32::from(request.bytes), 4),
                                #[cfg(feature = "sunfish-ipa-tftp-dispatch-trace")]
                                (u32::from(request.server_result), 2),
                                #[cfg(feature = "sunfish-ipa-tftp-dispatch-trace")]
                                (u32::from(request.occupied), 2),
                                #[cfg(feature = "sunfish-ipa-tftp-dispatch-trace")]
                                (u32::from(request.oack_queued), 1),
                            ]);
                            continue;
                        }
                        display.header_row(row, true, &[
                            (kind, 2), (source, 8), (destination, 8), (node, 8), (port, 8),
                        ]);
                        continue;
                    }
                    display.header_row(row, true, &[
                        (kind, 2), (source, 8), (destination, 8), (u32::from(qmi), 1),
                        (u32::from(transaction), 4), (u32::from(message), 4),
                    ]);
                } else {
                    display.replace_line(row, "");
                }
            }
        }
        #[cfg(all(feature = "sunfish-ipa-tftp-slot-snapshot", not(feature = "sunfish-tftp-oack-reserve")))]
        {
            let fifo = owner.registration_reply_fifo().unwrap_or_default();
            let matched = fifo.headers.iter().rev().flatten().filter_map(|header| {
                let peer = header.del_client?;
                let original = header.tftp_request?.original?;
                Some((header.at, peer, original))
            });
            let selected = matched.clone().find(|(_, _, original)| original.full_queue.is_some())
                .or_else(|| matched.into_iter().next());
            let snapshot = selected.and_then(|(_, _, original)| original.full_queue);
            let missing = u32::MAX;
            display.replace_line(1, "IPA TFTP TX SLOTS");
            for (row, (label, value)) in [
                ("RUNTIME ERROR", self.error),
                ("PEER NODE", selected.map_or(missing, |(_, peer, _)| peer[0])),
                ("PEER PORT", selected.map_or(missing, |(_, peer, _)| peer[1])),
                ("ORIGINAL WRQ", selected.map_or(missing, |(_, _, req)| req.id)),
                ("SERVER RESULT", selected.map_or(missing, |(_, _, req)| u32::from(req.result))),
                ("OACK PHASES", selected.map_or(missing, |(_, _, req)| u32::from(req.phases))),
                ("LAST CHECK", selected.map_or(missing, |(_, _, req)| u32::from(req.queue_block.check))),
                ("FULL QUEUE US", age(snapshot.and_then(|value| value.at))),
                ("PEER CLOSE US", age(selected.and_then(|(at, _, _)| at))),
                ("FIRST FREE MASK", snapshot.map_or(missing, |value| u32::from(value.first_free_mask))),
                ("FIRST FREE US", age(snapshot.and_then(|value| value.first_free_at))),
                ("REG DONE US", age(reg.and_then(|trace| trace.completed_at))),
                ("ELF DONE US", age(owner.service_registry_trace().elf.and_then(|trace| trace.done))),
                ("PD QUERY STATE", query[1]),
                ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                ("WLFW SERVERS", query[7]),
            ].into_iter().enumerate() {
                display.hex(row + 2, label, value);
            }
            display.replace_line(18, "SLOT TYPE OP FLAGS");
            display.replace_line(19, "PEER PORT INTENT");
            for index in 0..8 {
                if let Some(snapshot) = snapshot {
                    let slot = snapshot.slots[index];
                    display.header_row(20 + index, false, &[
                        (index as u32, 1), (u32::from(slot.kind), 2), (u32::from(slot.opcode), 2),
                        (u32::from(slot.flags), 1), (slot.peer_port, 8), (slot.remote_intent, 8),
                    ]);
                } else {
                    display.replace_line(20 + index, "");
                }
            }
            display.replace_line(28, "");
        }
        #[cfg(all(feature = "sunfish-tftp-oack-resources", not(feature = "sunfish-tftp-oack-ahead-headers")))]
        self.show_oack_resources(display, owner);
        #[cfg(all(feature = "sunfish-tftp-oack-reserve", not(feature = "sunfish-tftp-oack-resources")))]
        {
            let reserved = owner.reserved_oack();
            let oack = reserved.map(|(value, _)| value);
            let peer = reserved.and_then(|(_, transfer)| transfer);
            let ack = owner.tftp_final_ack_trace().filter(|ack|
                oack.is_some_and(|value| ack.destination == value.peer && ack.source.port == value.transfer_port));
            let registry = owner.service_registry_trace();
            display.replace_line(1, "TFTP OACK RESERVE");
            for (row, (label, value)) in [
                ("RUNTIME ERROR", self.error),
                ("ERROR DETAIL", self.detail),
                ("RESERVE PEER", oack.map_or(u32::MAX, |value| value.peer.port)),
                ("TRANSFER PORT", oack.map_or(u32::MAX, |value| value.transfer_port)),
                ("WRQ US", age(peer.and_then(|value| value.wrq_at))),
                ("WRQ RXDONE US", age(peer.and_then(|value| value.wrq_done_at))),
                ("OACK QUEUE US", age(oack.and_then(|value| value.queued_at))),
                ("OACK PUB US", age(oack.and_then(|value| value.published_at))),
                ("OACK DONE US", age(oack.and_then(|value| value.done_at))),
                ("PEER DATA US", age(peer.and_then(|value| value.first_data_at))),
                ("PEER CLOSE US", age(peer.and_then(|value| value.first_close_at))),
                ("FINAL ACK US", age(ack.and_then(|value| value.published_at))),
                ("ACK DONE US", age(ack.and_then(|value| value.done_at))),
                ("FIRST CLOSE US", age(first_oack.and_then(|value| value.first_close_at))),
                ("REG PUB US", age(reg.and_then(|value| value.published_at))),
                ("REG DONE US", age(reg.and_then(|value| value.completed_at))),
                ("ELF DONE US", age(registry.elf.and_then(|value| value.done))),
                ("WRITES COMPLETE", self.server_check.completed_count() as u32),
                ("TFTP WRQ RX", owner.tftp_wire_counts()[0]),
                ("TFTP DATA RX", owner.tftp_wire_counts()[3]),
                ("PD QUERY STATE", query[1]),
                ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                ("WLFW SERVERS", query[7]),
            ].into_iter().enumerate() {
                display.hex(row + 2, label, value);
            }
            for row in 25..29 { display.replace_line(row, ""); }
        }
        #[cfg(all(feature = "sunfish-ipa-registration-reply-path", not(feature = "sunfish-ipa-registration-reply-headers")))]
        {
            let fifo = owner.registration_reply_fifo().unwrap_or_default();
            let us = |ticks| self.time_from_us(Some(0), Some(ticks));
            display.replace_line(1, "IPA REPLY FIFO TRACE");
            for (row, (label, value)) in [
                ("RUNTIME ERROR", self.error),
                ("ERROR DETAIL", self.detail),
                ("REG QUEUE US", age(reg.and_then(|r| r.queued_at))),
                ("REQ RXDONE US", age(tx.request_done_at)),
                ("FIRST PASS US", age(tx.first_pass_at)),
                ("SELECTED US", age(tx.selected_at)),
                ("FIRST CLOSE US", age(first_oack.and_then(|t| t.first_close_at))),
                ("REG PUB US", age(reg.and_then(|r| r.published_at))),
                ("PEER TAIL US", age(fifo.peer_tail_at)),
                ("LAST EMPTY US", age(fifo.last_empty_at)),
                ("REG DONE US", age(reg.and_then(|r| r.completed_at))),
                ("MAX RX BYTES", fifo.max_unread as u32),
                ("RX POLLS", fifo.polls),
                ("MAX RX GAP US", us(fifo.max_poll_gap)),
                ("TX PASSES", tx.passes),
                ("TX ACTIVE US", us(tx.active_ticks)),
                ("TX WRITER US", us(tx.writer_ticks)),
                ("MAX TX GAP US", us(tx.max_gap_ticks)),
                ("ADV RXDONE", pair(tx.commands[0], tx.commands[1])),
                ("IREQ IACK", pair(tx.commands[2], tx.commands[3])),
                ("REUSE OTHER", pair(tx.commands[4], tx.commands[5])),
                ("DATA IDLE", pair(tx.data_ahead, tx.idle)),
                ("BLOCK REG OTHER", pair(tx.target_blocked, tx.other_blocked)),
                ("FIRST OACK US", age(first_oack.and_then(|t| t.oack_at))),
                ("PD QUERY STATE", query[1]),
                ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                ("WLFW SERVERS", query[7]),
            ].into_iter().enumerate() { display.hex(row + 2, label, value); }
        }
        #[cfg(all(feature = "sunfish-ipa-registration-rxdone-trace", not(feature = "sunfish-ipa-registration-reply-path")))]
        {
            let done = tx.request_done;
            display.replace_line(1, if cfg!(feature = "sunfish-ipa-registration-rxdone-before-grant") {
                "IPA RXDONE BEFORE GRANT"
            } else {
                "IPA RXDONE TURN TRACE"
            });
            for (row, (label, value)) in [
                ("RUNTIME ERROR", self.error),
                ("ERROR DETAIL", self.detail),
                ("REG RX US", age(reg.and_then(|r| r.received_at))),
                ("REG QUEUE US", age(reg.and_then(|r| r.queued_at))),
                ("FIRST PASS US", age(done.first_pass_at)),
                ("ELIGIBLE US", age(done.first_eligible_at)),
                ("REQ RXDONE US", age(tx.request_done_at)),
                ("FIRST CLOSE US", age(first_oack.and_then(|t| t.first_close_at))),
                ("FIRST WRQ US", age(first_oack.and_then(|t| t.wrq_at))),
                ("FIRST OACK US", age(first_oack.and_then(|t| t.oack_at))),
                ("PASSES AT CLOSE", done.passes_at_close.unwrap_or(u32::MAX)),
                ("REG PUB US", age(reg.and_then(|r| r.published_at))),
                ("REG DONE US", age(reg.and_then(|r| r.completed_at))),
                ("G DATA WRQ", pair(done.gates[0], done.gates[1])),
                ("G GRANT CREDIT", pair(done.gates[2], done.gates[3])),
                ("G NOQUEUE NRDY", pair(done.gates[4], done.gates[5])),
                ("RXDONE PASSES", done.passes),
                ("TURNS 00 09", done.turns[0]),
                ("TURNS 10 19", done.turns[1]),
                ("TURNS 20 29", done.turns[2]),
                ("TURNS 30 39", done.turns[3]),
                ("FAIL LOW", done.failed_turns as u32),
                ("FAIL HIGH", (done.failed_turns >> 32) as u32),
                ("WRITER FAILS", done.failures),
                ("PD QUERY STATE", query[1]),
                ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                ("WLFW SERVERS", query[7]),
            ].into_iter().enumerate() { display.hex(row + 2, label, value); }
        }
        #[cfg(not(feature = "sunfish-ipa-registration-rxdone-trace"))]
        {
            let ms = |ticks: Option<u64>| ticks.filter(|_| self.hz != 0 && !tx.clock_invalid)
                .map_or(u32::MAX, |ticks| (u128::from(ticks) * 1000 / u128::from(self.hz))
                    .min(u128::from(u32::MAX)) as u32);
            let interval = |start: Option<u64>, end: Option<u64>| ms(start.zip(end).and_then(|(a,b)| b.checked_sub(a)));
            let measured = tx.first_pass_at.map(|_| ());
            let flags = owner.linux_ipa_report().and_then(|r| r.protocol.map(|s|
                u32::from(s.modem_ready) | (u32::from(s.qmi_uc_ready) << 1)
                | (u32::from(s.indication_requested) << 2) | (u32::from(s.indication_sent) << 3)
                | (u32::from(r.protocol_complete) << 4) | (u32::from(r.failed) << 5))).unwrap_or(u32::MAX);
            let title = if cfg!(feature = "sunfish-ipa-registration-reply-between-grants") {
                "IPA REPLY GRANT TURN"
            } else if cfg!(feature = "sunfish-ipa-registration-between-grants") {
                "IPA RXDONE GRANT TURN"
            } else if cfg!(feature = "sunfish-ipa-registration-cold-handoff") {
                "IPA HANDOFF TX TRACE"
            } else if cfg!(feature = "sunfish-ipa-before-first-oack") {
                "IPA REG RX TO QUEUE"
            } else if cfg!(feature = "sunfish-ipa-registration-rxdone-turn") {
                "IPA STARTUP ORDER"
            } else if cfg!(feature = "sunfish-ipa-registration-tx-turn") {
                "IPA REG TX GUARDS"
            } else {
                "IPA REG TX WAIT"
            };
            display.replace_line(1, title);
            for (row, (label, value)) in [
                ("RUNTIME ERROR", self.error),
                ("ERROR DETAIL", self.detail),
                ("REG QUEUE US", age(reg.and_then(|r| r.queued_at))),
                ("REG PUB US", age(reg.and_then(|r| r.published_at))),
                ("QUEUE WAIT MS", interval(reg.and_then(|r| r.queued_at), reg.and_then(|r| r.published_at))),
                {
                    #[cfg(feature = "sunfish-ipa-registration-tx-turn")]
                    { ("REQ RXDONE US", age(tx.request_done_at)) }
                    #[cfg(not(feature = "sunfish-ipa-registration-tx-turn"))]
                    { ("FIRST PASS MS", interval(reg.and_then(|r| r.queued_at), tx.first_pass_at)) }
                },
                {
                    #[cfg(feature = "sunfish-ipa-registration-tx-turn")]
                    { ("PRIO SELECTED", u32::from(tx.priority_selected)) }
                    #[cfg(not(feature = "sunfish-ipa-registration-tx-turn"))]
                    { ("MAX GAP MS", ms(measured.map(|_| tx.max_gap_ticks))) }
                },
                {
                    #[cfg(feature = "sunfish-ipa-registration-rxdone-turn")]
                    { ("RXDONE TURN", u32::from(tx.request_done_attempted)) }
                    #[cfg(not(feature = "sunfish-ipa-registration-rxdone-turn"))]
                    { ("ACTIVE MS", ms(measured.map(|_| tx.active_ticks))) }
                },
                { #[cfg(feature = "sunfish-ipa-before-first-oack")] { ("REG RX US", age(reg.and_then(|r| r.received_at))) } #[cfg(not(feature = "sunfish-ipa-before-first-oack"))] { ("WRITER MS", ms(measured.map(|_| tx.writer_ticks))) } },
                { #[cfg(feature = "sunfish-ipa-before-first-oack")] { ("REG ROUTE US", age(reg.and_then(|r| r.routed_at))) } #[cfg(not(feature = "sunfish-ipa-before-first-oack"))] { ("REG TX PASSES", tx.passes) } },
                {
                    #[cfg(feature = "sunfish-ipa-registration-tx-turn")]
                    { ("DEFER DATA WRQ", pair(tx.priority_deferrals[0], tx.priority_deferrals[1])) }
                    #[cfg(not(feature = "sunfish-ipa-registration-tx-turn"))]
                    { ("G REQ NOFIT", pair(tx.gates[0], tx.gates[1])) }
                },
                {
                    #[cfg(feature = "sunfish-ipa-registration-tx-turn")]
                    { ("DEFER ACK QRTR", pair(tx.priority_deferrals[2], tx.priority_deferrals[3])) }
                    #[cfg(not(feature = "sunfish-ipa-registration-tx-turn"))]
                    { ("G FLOW READY", pair(tx.gates[2], tx.gates[3])) }
                },
                {
                    #[cfg(feature = "sunfish-ipa-registration-tx-turn")]
                    { { #[cfg(feature = "sunfish-ipa-before-first-oack")] { if cfg!(feature = "sunfish-ipa-registration-cold-handoff") { ("DEFER DONE ETC", pair(tx.priority_deferrals[4], tx.priority_deferrals[5])) } else { ("REG CAP WAITS", reg.map_or(0, |r| r.capacity_waits)) } } #[cfg(not(feature = "sunfish-ipa-before-first-oack"))] { ("DEFER DONE ETC", pair(tx.priority_deferrals[4], tx.priority_deferrals[5])) } } }
                    #[cfg(not(feature = "sunfish-ipa-registration-tx-turn"))]
                    { ("G ACK ADV", pair(tx.gates[4], tx.gates[5])) }
                },
                {
                    #[cfg(feature = "sunfish-ipa-registration-rxdone-turn")]
                    { if cfg!(feature = "sunfish-ipa-registration-cold-handoff") { ("INIT QUEUE US", age(init_transport.and_then(|trace| trace.queued_at))) } else { ("INIT RETIRE US", age(init_transport.and_then(|trace| trace.retired_at))) } }
                    #[cfg(not(feature = "sunfish-ipa-registration-rxdone-turn"))]
                    { ("G DONE CREDIT", pair(tx.gates[6], tx.gates[7])) }
                },
                {
                    #[cfg(feature = "sunfish-ipa-before-first-oack")]
                    { ("FIRST OACK US", age(first_oack.and_then(|t| t.oack_at))) }
                    #[cfg(not(feature = "sunfish-ipa-before-first-oack"))]
                    { ("CMD 5 6", pair(tx.commands[0], tx.commands[1])) }
                },
                {
                    #[cfg(feature = "sunfish-ipa-before-first-oack")]
                    { ("OACK DONE US", age(first_oack.and_then(|t| t.oack_done_at))) }
                    #[cfg(not(feature = "sunfish-ipa-before-first-oack"))]
                    { ("CMD 7 8", pair(tx.commands[2], tx.commands[3])) }
                },
                {
                    #[cfg(feature = "sunfish-ipa-registration-rxdone-turn")]
                    { ("FIRST CLOSE US", age(first_oack.and_then(|trace| trace.first_close_at))) }
                    #[cfg(not(feature = "sunfish-ipa-registration-rxdone-turn"))]
                    { ("CMD 14 OTHER", pair(tx.commands[4], tx.commands[5])) }
                },
                {
                    #[cfg(feature = "sunfish-ipa-before-first-oack")]
                    { ("FIRST DATA US", age(first_oack.and_then(|t| t.first_data_at))) }
                    #[cfg(not(feature = "sunfish-ipa-before-first-oack"))]
                    { ("DATA AHEAD", tx.data_ahead) }
                },
                {
                    #[cfg(feature = "sunfish-ipa-before-first-oack")]
                    { ("FIRST WRQ US", age(first_oack.and_then(|t| t.wrq_at))) }
                    #[cfg(not(feature = "sunfish-ipa-before-first-oack"))]
                    { ("IDLE PASSES", tx.idle) }
                },
                { #[cfg(feature = "sunfish-ipa-before-first-oack")] { if cfg!(feature = "sunfish-ipa-registration-cold-handoff") { ("G REQ NOFIT", pair(tx.gates[0], tx.gates[1])) } else { ("REG PREP US", age(reg.and_then(|r| r.prepared_at))) } } #[cfg(not(feature = "sunfish-ipa-before-first-oack"))] { ("BLOCK REG OTHER", pair(tx.target_blocked, tx.other_blocked)) } },
                { #[cfg(feature = "sunfish-ipa-before-first-oack")] { if cfg!(feature = "sunfish-ipa-registration-cold-handoff") { ("G FLOW READY", pair(tx.gates[2], tx.gates[3])) } else { ("REG Q TRY US", age(reg.and_then(|r| r.first_queue_attempt_at))) } } #[cfg(not(feature = "sunfish-ipa-before-first-oack"))] { ("UNCERTAIN", tx.uncertain) } },
                { #[cfg(feature = "sunfish-ipa-before-first-oack")] { if cfg!(feature = "sunfish-ipa-registration-cold-handoff") { ("BLOCK REG OTHER", pair(tx.target_blocked, tx.other_blocked)) } else { ("REG FLOW WAITS", reg.map_or(0, |r| r.flow_waits)) } } #[cfg(not(feature = "sunfish-ipa-before-first-oack"))] { ("CLOCK INVALID", u32::from(tx.clock_invalid)) } },
                ("IPA FLAGS", flags),
                ("REG DONE US", age(reg.and_then(|r| r.completed_at))),
                ("PD QUERY STATE", query[1]),
                ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                ("WLFW SERVERS", query[7]),
            ].into_iter().enumerate() { display.hex(row + 2, label, value); }
        }
    }

    #[cfg(feature = "sunfish-ipa-independent-server-runtime")]
    fn show_ipa_independent(&self, display: &Display, owner: &ModemOwner) {
        let (init, transport) = owner.ipa_init_trace();
        let registration = owner.ipa_registration_trace();
        let report = owner.linux_ipa_report();
        let query = self.wlan_query.snapshot(owner);
        let age = |event| self.time_from_us(Some(self.runtime_started), event);
        let flags = report.and_then(|r| r.protocol).map_or(u32::MAX, |s|
            u32::from(s.modem_ready) | (u32::from(s.qmi_uc_ready) << 1)
            | (u32::from(s.indication_requested) << 2) | (u32::from(s.indication_sent) << 3)
            | (u32::from(report.is_some_and(|r| r.protocol_complete)) << 4)
            | (u32::from(report.is_some_and(|r| r.failed)) << 5));
        let pending = report.map_or(u32::MAX, |r|
            u32::from(r.client_pending_record) | (u32::from(r.client_record_queued) << 1)
            | (u32::from(r.server_pending_record) << 2) | (u32::from(r.server_record_queued) << 3)
            | (u32::from(r.held_response) << 4));
        let [first, _] = owner.server_check_oacks();
        display.replace_line(1, "IPA INDEPENDENT");
        for (row, (label, value)) in [
            ("RUNTIME ERROR", self.error),
            ("ERROR DETAIL", self.detail),
            ("IPA FLAGS", flags),
            ("PENDING FLAGS", pending),
            ("INIT DISC US", age(init.and_then(|t| t.discovered_at))),
            ("INIT PREP US", age(init.and_then(|t| t.prepared_at))),
            ("INIT QUEUE US", age(transport.and_then(|t| t.queued_at))),
            ("INIT PUB US", age(transport.and_then(|t| t.published_at))),
            ("INIT RETIRE US", age(transport.and_then(|t| t.retired_at))),
            ("INIT COMMIT US", age(init.and_then(|t| t.committed_at))),
            ("INIT RSP US", age(init.and_then(|t| t.response_at))),
            ("REG RX US", age(registration.and_then(|t| t.received_at))),
            ("REG ROUTE US", age(registration.and_then(|t| t.routed_at))),
            ("REG PREP US", age(registration.and_then(|t| t.prepared_at))),
            ("REG QUEUE US", age(registration.and_then(|t| t.queued_at))),
            ("REG PUB US", age(registration.and_then(|t| t.published_at))),
            ("REG DONE US", age(registration.and_then(|t| t.completed_at))),
            ("REG PRIOR MSG", registration.and_then(|t| t.prior_message).map_or(u32::MAX, u32::from)),
            ("INIT TX TURN", transport.map_or(0, |t| t.tx.priority_turn)),
            ("CONTROL AHEAD", transport.map_or(0, |t| t.tx.control_ahead)),
            ("DATA AHEAD", transport.map_or(0, |t| t.tx.data_ahead)),
            ("COMMIT COUNT", report.map_or(0, |r| r.publications_committed)),
            ("RSP ACCEPTED", report.map_or(0, |r| r.responses_accepted)),
            ("FIRST CLOSE US", age(first.and_then(|t| t.first_close_at))),
            ("PD QUERY STATE", query[1]),
            ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
            ("WLFW SERVERS", query[7]),
        ].into_iter().enumerate() { display.hex(row + 2, label, value); }
    }

    #[cfg(feature = "sunfish-ipa-schedule-trace")]
    fn show_ipa_schedule(&self, display: &Display, owner: &ModemOwner) {
        let tx = owner.ipa_init_trace().1.map(|trace| trace.tx).unwrap_or_default();
        let query = self.wlan_query.snapshot(owner);
        let pair = diagnostic::packed_counts;
        let work = |slot| self.ipa_schedule.packed_ms(slot, self.hz);
        display.replace_line(1, "IPA SCHEDULE TRACE");
        for (row, (label, value)) in [
            ("RUNTIME ERROR", self.error),
            ("ERROR DETAIL", self.detail),
            ("G ACK WRQ", pair(tx.gates[0], tx.gates[1])),
            ("G TFTP REG", pair(tx.gates[2], tx.gates[3])),
            ("G CREDIT UNRDY", pair(tx.gates[4], tx.gates[5])),
            ("G USED ELIGIBLE", pair(tx.gates[6], tx.gates[7])),
            ("CMD 5 6", pair(tx.commands[0], tx.commands[1])),
            ("CMD 7 8", pair(tx.commands[2], tx.commands[3])),
            ("CMD 14 OTHER", pair(tx.commands[4], tx.commands[5])),
            ("TP ROUTER MS", work(0)),
            ("TP STORAGE MS", work(1)),
            ("TP IPA MS", work(2)),
            ("TP PAINT MS", work(3)),
            ("TP REGISTRY MS", work(4)),
            ("TP TFTP MS", work(5)),
            ("TP OUTSIDE MS", work(6)),
            ("TP OTHER MS", work(7)),
            ("QUEUE PUB MS", self.ipa_schedule.phase_ms(0, self.hz)),
            ("PUB RETIRE MS", self.ipa_schedule.phase_ms(1, self.hz)),
            ("RET COMMIT MS", self.ipa_schedule.phase_ms(2, self.hz)),
            ("COMMIT PREP MS", self.ipa_schedule.phase_ms(3, self.hz)),
            ("INIT TX TURN", tx.priority_turn),
            ("TRACE INVALID", u32::from(self.ipa_schedule.invalid)),
            ("TX FLAGS", tx.flags),
            ("PD QUERY STATE", query[1]),
            ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
            ("WLFW SERVERS", query[7]),
        ].into_iter().enumerate() {
            display.hex(row + 2, label, value);
        }
    }

    #[cfg(all(feature = "sunfish-ipa-init-tx-trace", not(any(feature = "sunfish-ipa-schedule-trace", feature = "sunfish-ipa-independent-server-runtime"))))]
    fn show_ipa_init_tx(&self, display: &Display, owner: &ModemOwner) {
        let registration = owner.ipa_registration_trace();
        #[cfg(not(feature = "sunfish-ipa-init-tx-turn"))]
        let route = owner.ipa_registration_owner_trace();
        let (init, transport) = owner.ipa_init_trace();
        let tx = transport.map(|trace| trace.tx);
        let [first, _] = owner.server_check_oacks();
        let query = self.wlan_query.snapshot(owner);
        let age = |event| self.time_from_us(Some(self.runtime_started), event);
        #[cfg(feature = "sunfish-ipa-init-tx-turn")]
        display.replace_line(1, "IPA INIT TX TURN");
        #[cfg(not(feature = "sunfish-ipa-init-tx-turn"))]
        display.replace_line(1, "IPA INIT TX TRACE");
        for (row, (label, value)) in [
            ("RUNTIME ERROR", self.error),
            ("ERROR DETAIL", self.detail),
            #[cfg(feature = "sunfish-ipa-init-tx-turn")]
            ("INIT TX TURN", tx.map_or(0, |trace| trace.priority_turn)),
            #[cfg(not(feature = "sunfish-ipa-init-tx-turn"))]
            ("ROUTE IPA FLAGS", route.map_or(u32::MAX, |trace| trace.route_flags)),
            ("REG ROUTE US", age(registration.and_then(|trace| trace.routed_at))),
            ("INIT QUEUE US", age(transport.and_then(|trace| trace.queued_at))),
            ("TX ROUTER US", age(tx.and_then(|trace| trace.router_at))),
            ("TX PASS US", age(tx.and_then(|trace| trace.first_pass_at))),
            ("TX WRITER US", age(tx.and_then(|trace| trace.first_writer_at))),
            ("TX SELECT US", age(tx.and_then(|trace| trace.selected_at))),
            ("INIT PUB US", age(transport.and_then(|trace| trace.published_at))),
            ("INIT RETIRE US", age(transport.and_then(|trace| trace.retired_at))),
            ("INIT COMMIT US", age(init.and_then(|trace| trace.committed_at))),
            ("TX PASSES", tx.map_or(0, |trace| trace.passes)),
            ("CONTROL AHEAD", tx.map_or(0, |trace| trace.control_ahead)),
            ("DATA AHEAD", tx.map_or(0, |trace| trace.data_ahead)),
            ("TX NO FIT", tx.map_or(0, |trace| trace.no_fit)),
            ("TX FIFO BLOCK", tx.map_or(0, |trace| trace.target_blocked)),
            ("TX FLAGS", tx.map_or(0, |trace| trace.flags)),
            ("REG PREP US", age(registration.and_then(|trace| trace.prepared_at))),
            ("REG DONE US", age(registration.and_then(|trace| trace.completed_at))),
            ("FIRST CLOSE US", age(first.and_then(|trace| trace.first_close_at))),
            ("FIRST DATA US", age(first.and_then(|trace| trace.first_data_at))),
            ("WRITES COMPLETE", self.server_check.completed_count() as u32),
            ("PD QUERY STATE", query[1]),
            ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
            ("WLFW SERVERS", query[7]),
            ("IPA TIME CAUSE", owner.linux_ipa_report().map_or(0, |report| report.timeout_causes)),
        ].into_iter().enumerate() {
            display.hex(row + 2, label, value);
        }
    }

    #[cfg(all(feature = "sunfish-ipa-init-continuation", not(feature = "sunfish-ipa-init-tx-trace")))]
    fn show_ipa_init(&self, display: &Display, owner: &ModemOwner) {
        let registration = owner.ipa_registration_trace();
        let route = owner.ipa_registration_owner_trace();
        let [_, early] = owner.ipa_registration_turn_times();
        let (init, transport) = owner.ipa_init_trace();
        let [first, _] = owner.server_check_oacks();
        let query = self.wlan_query.snapshot(owner);
        let age = |event| self.time_from_us(Some(self.runtime_started), event);
        display.replace_line(1, "IPA INIT CONTINUATION");
        for (row, (label, value)) in [
            ("RUNTIME ERROR", self.error),
            ("ERROR DETAIL", self.detail),
            ("ROUTE IPA FLAGS", route.map_or(u32::MAX, |trace| trace.route_flags)),
            ("REG RX US", age(registration.and_then(|trace| trace.received_at))),
            ("REG ROUTE US", age(registration.and_then(|trace| trace.routed_at))),
            ("INIT DISC US", age(init.and_then(|trace| trace.discovered_at))),
            ("INIT PREP US", age(init.and_then(|trace| trace.prepared_at))),
            ("INIT QUEUE US", age(transport.and_then(|trace| trace.queued_at))),
            ("INIT PUB US", age(transport.and_then(|trace| trace.published_at))),
            ("INIT RETIRE US", age(transport.and_then(|trace| trace.retired_at))),
            ("INIT COMMIT US", age(init.and_then(|trace| trace.committed_at))),
            ("INIT EXTRA", init.map_or(0, |trace| trace.extra_steps)),
            ("INIT QUEUE WAIT", init.map_or(0, |trace| trace.queue_waits)),
            ("REG EARLY US", age(early)),
            ("REG PREP US", age(registration.and_then(|trace| trace.prepared_at))),
            ("REG QUEUE US", age(registration.and_then(|trace| trace.queued_at))),
            ("REG PUB US", age(registration.and_then(|trace| trace.published_at))),
            ("REG DONE US", age(registration.and_then(|trace| trace.completed_at))),
            ("IPA TIME CAUSE", owner.linux_ipa_report().map_or(0, |report| report.timeout_causes)),
            ("FIRST WRQ US", age(first.and_then(|trace| trace.wrq_at))),
            ("FIRST OACK US", age(first.and_then(|trace| trace.oack_at))),
            ("FIRST CLOSE US", age(first.and_then(|trace| trace.first_close_at))),
            ("FIRST DATA US", age(first.and_then(|trace| trace.first_data_at))),
            ("WRITES COMPLETE", self.server_check.completed_count() as u32),
            ("PD QUERY STATE", query[1]),
            ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
            ("WLFW SERVERS", query[7]),
        ].into_iter().enumerate() {
            display.hex(row + 2, label, value);
        }
    }

    #[cfg(all(feature = "sunfish-ipa-registration-trace", not(feature = "sunfish-ipa-init-continuation")))]
    fn show_ipa_registration(&self, display: &Display, owner: &ModemOwner) {
        let trace = owner.ipa_registration_trace();
        let age = |event| self.time_from_us(Some(self.runtime_started), event);
        let [first, _] = owner.server_check_oacks();
        let report = owner.linux_ipa_report();
        #[cfg(not(feature = "sunfish-ipa-registration-owner-trace"))]
        let transmit = owner.linux_ipa_tx_diagnostic();
        let query = self.wlan_query.snapshot(owner);
        #[cfg(feature = "sunfish-ipa-registration-early-turn")]
        let [yielded, early] = owner.ipa_registration_turn_times();
        #[cfg(all(feature = "sunfish-ipa-registration-early-turn", not(feature = "sunfish-ipa-registration-owner-trace")))]
        display.replace_line(1, "IPA REG EARLY TURN");
        #[cfg(not(feature = "sunfish-ipa-registration-early-turn"))]
        display.replace_line(1, "IPA REG REPLY");
        #[cfg(feature = "sunfish-ipa-registration-owner-trace")]
        let route = owner.ipa_registration_owner_trace();
        #[cfg(feature = "sunfish-ipa-registration-owner-trace")]
        let step = route.and_then(|route| route.first_step);
        #[cfg(feature = "sunfish-ipa-registration-owner-trace")]
        let step_code = step.map_or(u32::MAX, |step| {
            use qcom_modem_owner::LinuxIpaProgress::*;
            match step.progress {
                None => 0,
                Some(WaitingPeer) => 1,
                Some(Discovered) => 2,
                Some(Prepared) => 3,
                Some(Queued) => 4,
                Some(WaitingQueue) => 5,
                Some(WaitingCompletion) => 6,
                Some(Committed) => 7,
                Some(ResponseAccepted) => 8,
                Some(WaitingHandshake) => 9,
                Some(Complete) => 10,
            }
        });
        #[cfg(feature = "sunfish-ipa-registration-owner-trace")]
        display.replace_line(1, "IPA REG OWNER TRACE");
        for (row, (label, value)) in [
            ("RUNTIME ERROR", self.error),
            ("ERROR DETAIL", self.detail),
            ("REG PEER PORT", trace.map_or(u32::MAX, |trace| trace.peer_port)),
            #[cfg(not(feature = "sunfish-ipa-registration-owner-trace"))]
            ("REG LOCAL PORT", trace.map_or(u32::MAX, |trace| trace.local_port)),
            ("REG TXN", trace.map_or(u32::MAX, |trace| u32::from(trace.transaction))),
            ("REG RX US", age(trace.and_then(|trace| trace.received_at))),
            ("REG ROUTE US", age(trace.and_then(|trace| trace.routed_at))),
            ("REG PREP US", age(trace.and_then(|trace| trace.prepared_at))),
            ("REG QUEUE US", age(trace.and_then(|trace| trace.queued_at))),
            ("REG PUB US", age(trace.and_then(|trace| trace.published_at))),
            ("REG DONE US", age(trace.and_then(|trace| trace.completed_at))),
            #[cfg(not(feature = "sunfish-ipa-registration-owner-trace"))]
            ("REG INTENT", trace.and_then(|trace| trace.remote_intent).unwrap_or(u32::MAX)),
            ("REG FLOW WAITS", trace.map_or(0, |trace| trace.flow_waits)),
            ("REG CAP WAITS", trace.map_or(0, |trace| trace.capacity_waits)),
            ("REG PRIOR QMI", trace.and_then(|trace| trace.prior_message).map_or(u32::MAX, u32::from)),
            #[cfg(feature = "sunfish-ipa-registration-owner-trace")]
            ("ROUTE IPA FLAGS", route.map_or(u32::MAX, |trace| trace.route_flags)),
            #[cfg(feature = "sunfish-ipa-registration-owner-trace")]
            ("ROUTE IPA QMI", route.and_then(|trace| trace.route_pending_id).map_or(u32::MAX, u32::from)),
            #[cfg(feature = "sunfish-ipa-registration-owner-trace")]
            ("IPA STEP US", age(step.map(|step| step.started_at))),
            #[cfg(feature = "sunfish-ipa-registration-owner-trace")]
            ("IPA STEP END US", age(step.and_then(|step| step.ended_at))),
            #[cfg(feature = "sunfish-ipa-registration-owner-trace")]
            ("IPA STEP RESULT", step_code),
            #[cfg(not(feature = "sunfish-ipa-registration-owner-trace"))]
            ("IPA PENDING ID", report.and_then(|report| report.pending_message_id).map_or(u32::MAX, u32::from)),
            #[cfg(not(feature = "sunfish-ipa-registration-owner-trace"))]
            ("IPA TX FLAGS", transmit.map_or(u32::MAX, |info| info.flags)),
            #[cfg(feature = "sunfish-ipa-registration-early-turn")]
            ("REG YIELD US", age(yielded)),
            #[cfg(feature = "sunfish-ipa-registration-early-turn")]
            ("REG EARLY US", age(early)),
            #[cfg(not(feature = "sunfish-ipa-registration-early-turn"))]
            ("IPA TX SENT", transmit.map_or(u32::MAX, |info| info.sent)),
            #[cfg(not(feature = "sunfish-ipa-registration-early-turn"))]
            ("IPA TX LENGTH", transmit.map_or(u32::MAX, |info| info.len)),
            ("IPA TIME CAUSE", report.map_or(0, |report| report.timeout_causes)),
            #[cfg(not(feature = "sunfish-ipa-registration-owner-trace"))]
            ("FIRST WRQ US", age(first.and_then(|trace| trace.wrq_at))),
            ("FIRST OACK US", age(first.and_then(|trace| trace.oack_at))),
            ("FIRST CLOSE US", age(first.and_then(|trace| trace.first_close_at))),
            ("FIRST DATA US", age(first.and_then(|trace| trace.first_data_at))),
            ("PD QUERY STATE", query[1]),
            ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
            ("WLFW SERVERS", query[7]),
        ].into_iter().enumerate() {
            display.hex(row + 2, label, value);
        }
    }

    #[cfg(all(feature = "sunfish-wlan-first-wrq-window", not(feature = "sunfish-ipa-registration-trace")))]
    fn show_first_wrq_window(&self, display: &Display, owner: &ModemOwner) {
        use personal_os_rust_server::qcom_glink_qmi::WrqNeighbor;

        let [first, _] = owner.server_check_oacks();
        let window = owner.first_wrq_window();
        let age = |event| self.time_from_us(Some(self.runtime_started), event);
        let fields = |neighbor: Option<WrqNeighbor>| match neighbor {
            Some(n) => [
                n.source_port, n.destination_port, n.qmi_header.unwrap_or(u32::MAX),
                age(n.at), age(n.done_at),
            ],
            None => [u32::MAX; 5],
        };
        display.replace_line(1, "FIRST WRQ WINDOW");
        display.hex(2, "RUNTIME ERROR", self.error);
        if self.error != 0 {
            display.hex(3, "ERROR DETAIL", self.detail);
        } else {
            display.hex(3, "RX44 TX48 CNT", (u32::from(window.rx_after_count) << 16)
                | u32::from(window.tx_after_count));
        }
        display.hex(4, "FIRST WRQ US", age(first.and_then(|trace| trace.wrq_at)));
        display.hex(5, "FIRST OACK US", age(first.and_then(|trace| trace.oack_at)));
        display.hex(6, "FIRST CLOSE US", age(first.and_then(|trace| trace.first_close_at)));
        display.hex(7, "FIRST DATA US", age(first.and_then(|trace| trace.first_data_at)));
        for (start, labels, neighbor) in [
            (8, ["PRE44 SRC PORT", "PRE44 DST PORT", "PRE44 QMI", "PRE44 RX US"], window.rx_before),
            (12, ["POST44 SRC PORT", "POST44 DST PORT", "POST44 QMI", "POST44 RX US"], window.rx_after),
        ] {
            for (i, (label, value)) in labels.into_iter().zip(fields(neighbor)).enumerate() {
                display.hex(start + i, label, value);
            }
        }
        for (start, labels, neighbor) in [
            (16, ["PRE48 SRC PORT", "PRE48 DST PORT", "PRE48 QMI", "PRE48 PUB US", "PRE48 DONE US"], window.tx_before),
            (21, ["POST48 SRC PORT", "POST48 DST PORT", "POST48 QMI", "POST48 PUB US", "POST48 DONE US"], window.tx_after),
        ] {
            for (i, (label, value)) in labels.into_iter().zip(fields(neighbor)).enumerate() {
                display.hex(start + i, label, value);
            }
        }
        display.hex(26, "POST48 INTENT", window.tx_after.map_or(u32::MAX, |trace| trace.intent));
        display.hex(27, "WLAN RRQ RX", owner.tftp_wire_counts()[2]);
        display.hex(28, "WLFW SERVERS", self.wlan_query.snapshot(owner)[7]);
    }

    #[cfg(feature = "sunfish-first-oack-grant-window")]
    fn show_first_grant_window(&self, display: &Display, owner: &ModemOwner) {
        let [first, completed] = owner.server_check_oacks();
        let reg = owner.ipa_registration_trace();
        let grant = owner.first_grant_trace();
        let gates = owner.first_oack_eligibility();
        let query = self.wlan_query.snapshot(owner);
        let age = |stamp| self.time_from_us(Some(self.runtime_started), stamp);
        let tail = match owner.first_oack_tail() {
            qcom_glink_fifo::ServerCheckTail::Consumed(at) => at,
            _ => None,
        };
        #[cfg(not(feature = "sunfish-first-oack-yield-rx"))]
        {
        display.replace_line(1, "FIRST OACK GRANT WINDOW");
        for (row, label, value) in [
            (2, "RUNTIME ERROR", self.error),
            (3, "ERROR DETAIL", self.detail),
            (4, "FIRST PEER", first.map_or(u32::MAX, |t| t.peer.port)),
            (5, "FIRST WRQ US", age(first.and_then(|t| t.wrq_at))),
            (6, "OACK QUEUE US", age(gates.queued_at)),
            (7, "IPA PUB US", age(reg.and_then(|t| t.published_at))),
            (8, "TRIGGER CMD", grant.trigger),
            (9, "BACKING AD US", age(grant.advertisement_at)),
            (10, "GRANT ACK US", age(grant.ack_at)),
            (11, "FOLLOW RESULT", grant.outcome),
            (12, "RX BYTES", grant.unread_bytes),
            (13, "EARLY ATTEMPT", first.map_or(0, |t| u32::from(t.early_oack_attempted))),
            (14, "FIRST OACK US", age(first.and_then(|t| t.oack_at))),
            (15, "OACK TAIL US", age(tail)),
            (16, "FIRST DATA US", age(first.and_then(|t| t.first_data_at))),
            (17, "FIRST CLOSE US", age(first.and_then(|t| t.first_close_at))),
            (18, "OACK DONE US", age(first.and_then(|t| t.oack_done_at))),
            (19, "IPA DONE US", age(reg.and_then(|t| t.completed_at))),
            (20, "IPA OPEN PASSES", gates.ipa_passes),
            (21, "BLOCK ANY", u32::from(gates.seen)),
            (22, "CLOSE BLOCK", gates.close_mask.map_or(u32::MAX, u32::from)),
            (23, "WRITES COMPLETE", self.server_check.completed_count() as u32),
            (24, "COMPLETED PEER", completed.map_or(u32::MAX, |t| t.peer.port)),
            (25, "PD QUERY STATE", query[1]),
            (26, "WLAN RRQ RX", owner.tftp_wire_counts()[2]),
            (27, "WLFW SERVERS", query[7]),
            (28, "IPA COMPLETE", u32::from(self.ipa_complete_seen)),
        ] { display.hex(row, label, value); }
        }
        #[cfg(all(feature = "sunfish-grant-rx-open", not(feature = "sunfish-wrq-return-headers")))]
        {
            let rx = owner.grant_receive_trace();
            let visible = owner.ipa_wrq_timing().1.and_then(|bounds| bounds.first_visible);
            let head = owner.wrq_prefix().oack_head;
            let returned = match rx.return_tail {
                Some(qcom_glink_fifo::ServerCheckTail::Consumed(at)) => at, _ => None,
            };
            #[cfg(feature = "sunfish-wrq-grant-turns")]
            display.replace_line(1, "WRQ GRANT RX TURNS");
            #[cfg(not(feature = "sunfish-wrq-grant-turns"))]
            display.replace_line(1, "GRANT RX OPEN WINDOW");
            for (row, (label, value)) in [
                ("RUNTIME ERROR", self.error),
                #[cfg(feature = "sunfish-wrq-grant-turns")]
                ("52 ACK COUNT", rx.ack_count),
                #[cfg(not(feature = "sunfish-wrq-grant-turns"))]
                ("ERROR DETAIL", self.detail),
                ("FIRST PEER", first.map_or(u32::MAX, |t| t.peer.port)),
                ("GRANT ACK US", age(rx.ack_at)),
                #[cfg(feature = "sunfish-wrq-grant-turns")]
                ("RX TURN COUNT", rx.turn_count),
                #[cfg(not(feature = "sunfish-wrq-grant-turns"))]
                ("RX TURN COUNT", u32::from(rx.taken)),
                ("RX TURN US", age(rx.turn_at)),
                ("WRQ VISIBLE US", age(visible)),
                ("WRQ RXDONE US", age(first.and_then(|t| t.wrq_done_at))),
                ("WRQ TAIL US", age(returned)),
                ("RETURN TURN", u32::from(grant.wrq_return_outcome)),
                ("PENDING BYTES", owner.wrq_return_pending().map_or(u32::MAX, |t| t.bytes)),
                ("OPEN CHANNEL", rx.open.map_or(u32::MAX, |(id, _)| u32::from(id))),
                ("OPEN NAME HEX", rx.open.map_or(u32::MAX, |(_, name)| u32::from(name))),
                #[cfg(feature = "sunfish-wrq-grant-turns")]
                ("PRIMARY ACK NUM", rx.primary_checked),
                #[cfg(not(feature = "sunfish-wrq-grant-turns"))]
                ("BYTE ABSENT US", age(rx.byte_bounds.and_then(|b| b.last_absent))),
                #[cfg(feature = "sunfish-wrq-grant-turns")]
                ("TURN ACK NUM", rx.turn_ack),
                #[cfg(not(feature = "sunfish-wrq-grant-turns"))]
                ("BYTE VISIBLE US", age(rx.byte_bounds.and_then(|b| b.first_visible))),
                ("OPEN ABSENT US", age(rx.open_bounds.and_then(|b| b.last_absent))),
                ("OPEN VISIBLE US", age(rx.open_bounds.and_then(|b| b.first_visible))),
                ("OACK HEAD US", age(head)),
                ("OACK TAIL US", age(tail)),
                ("WRQ TO HEAD US", self.time_from_us(visible, head)),
                ("WRQ TO TAIL US", self.time_from_us(visible, tail)),
                ("REG PUB US", age(reg.and_then(|t| t.published_at))),
                ("FIRST DATA US", age(first.and_then(|t| t.first_data_at))),
                ("FIRST CLOSE US", age(first.and_then(|t| t.first_close_at))),
                ("IPA COMPLETE", u32::from(self.ipa_complete_seen)),
                ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                ("WLFW SERVERS", query[7]),
            ].into_iter().enumerate() { display.hex(row + 2, label, value); }
        }
        #[cfg(all(feature = "sunfish-wrq-return-headers", not(feature = "sunfish-oack-grant-outcome")))]
        {
            let pending = owner.wrq_return_pending();
            display.replace_line(1, "WRQ RETURN HEADERS");
            for (row, (label, value)) in [
                ("RUNTIME ERROR", self.error),
                ("PENDING BYTES", pending.map_or(u32::MAX, |t| t.bytes)),
                ("EARLY RECORDS", pending.map_or(u32::MAX, |t| t.early)),
                ("HEADER COUNT", pending.map_or(0, |t| t.header_count)),
                ("HEADERS OMITTED", pending.map_or(0, |t| t.header_omitted)),
                ("WRQ VISIBLE US", age(owner.ipa_wrq_timing().1.and_then(|b| b.first_visible))),
                ("WRQ RXDONE US", age(first.and_then(|t| t.wrq_done_at))),
                ("BACKLOG CLR US", age(pending.and_then(|t| t.backlog_clear))),
            ].into_iter().enumerate() { display.hex(row + 2, label, value); }
            display.replace_line(10, "RX WINDOW HEADERS");
            let mut records = pending.into_iter().flat_map(|t| t.headers());
            for row in 11..24 {
                if let Some(entry) = records.next() {
                    let h = &entry.header;
                    let q = h.qrtr.unwrap_or([u32::MAX; 3]);
                    display.header_row(row, false, &[
                        (entry.offset, 4), (u32::from(h.command), 2),
                        (u32::from(entry.channel), 4), (u32::from(h.bytes), 4),
                        (h.parameter, 8), (age(entry.checked), 8), (age(entry.clear), 8),
                    ]);
                    display.header_row(row, true, &[
                        (q[0], 2), (q[1], 8), (q[2], 8), (h.extra, 8),
                        (u32::from(entry.qmi_kind), 2), (entry.name, 8),
                    ]);
                } else {
                    display.replace_line(row, "");
                }
            }
            for (row, label, value) in [
                (24, "OACK HEAD US", age(owner.wrq_prefix().oack_head)),
                (25, "FIRST DATA US", age(first.and_then(|t| t.first_data_at))),
                (26, "FIRST CLOSE US", age(first.and_then(|t| t.first_close_at))),
                (27, "WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                (28, "WLFW SERVERS", query[7]),
            ] { display.hex(row, label, value); }
        }
        #[cfg(all(feature = "sunfish-oack-grant-outcome", not(feature = "sunfish-oack-retry-close")))]
        {
            let trace = owner.oack_grant_outcome();
            let tail = match owner.first_oack_tail() {
                qcom_glink_fifo::ServerCheckTail::Consumed(at) => at, _ => None,
            };
            #[cfg(not(feature = "sunfish-oack-grant-ack-turn"))]
            let (title, pending_label, pending_value) = ("OACK 108 GRANT OUTCOME", "PRE TX STATES",
                if trace.oack_captured {trace.states_before_writer} else {u32::MAX});
            #[cfg(feature = "sunfish-oack-grant-ack-turn")]
            let (title, pending_label, pending_value) = ("OACK 108 ACK TURN", "PENDING CLR US", age(trace.pending_clear));
            display.replace_line(1, title);
            for (row, (label, value)) in [
                ("RUNTIME ERROR", self.error),
                ("OACK HEAD US", age(trace.oack_at)),
                ("OACK TAIL US", age(tail)),
                ("OACK DONE US", age(first.and_then(|t| t.oack_done_at))),
                ("REQ 108 US", age(trace.request_at)),
                ("GRANT INTENT", trace.intent.unwrap_or(u32::MAX)),
                ("INITIAL GRANT", trace.initial_granted.map_or(u32::MAX, u32::from)),
                ("AD HEAD US", age(trace.advertisement.and_then(|a| a.published))),
                ("AD TAIL US", age(trace.advertisement.and_then(|a| a.first_read))),
                ("ACK HEAD US", age(trace.ack.and_then(|a| a.published))),
                ("ACK TAIL US", age(trace.ack.and_then(|a| a.first_read))),
                ("ACK GRANTED", trace.ack_granted.map_or(u32::MAX, u32::from)),
                (pending_label, pending_value),
                ("OACK RX BYTES", if trace.oack_captured {trace.rx_at_oack} else {u32::MAX}),
            ].into_iter().enumerate() { display.hex(row + 2, label, value); }
            display.replace_line(16, "ID SIZE OP PUB READ SEEN");
            for (slot, offer) in trace.available.iter().enumerate() {
                let row = 17 + slot;
                if let Some(offer) = offer {
                    display.header_row(row, false, &[
                        (offer.id, 8), (offer.size, 4), (u32::from(offer.command), 2),
                        (age(offer.published), 8), (age(offer.first_read), 8),
                        ((trace.read_at_oack >> slot) & 1, 1),
                    ]);
                    #[cfg(not(feature = "sunfish-oack-grant-ack-turn"))]
                    display.header_row(row, true, &[]);
                    #[cfg(feature = "sunfish-oack-grant-ack-turn")]
                    if let Some(used) = trace.used[slot] {
                        let [kind, source, destination] = used.header.qrtr.unwrap_or([u32::MAX; 3]);
                        display.header_row(row, true, &[
                            (used.offset, 4), (u32::from(used.header.command), 2),
                            (u32::from(used.header.bytes), 4), (trace.use_prefix_bytes[slot], 4),
                            (kind, 2), (source, 8), (destination, 8), (age(used.clear), 8),
                        ]);
                    } else {
                        display.header_row(row, true, &[]);
                    }
                } else {
                    display.replace_line(row, "");
                }
            }
            for (row, label, value) in [
                (25, "FIRST DATA US", age(first.and_then(|t| t.first_data_at))),
                (26, "FIRST CLOSE US", age(first.and_then(|t| t.first_close_at))),
                (27, "WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                (28, "WLFW SERVERS", query[7]),
            ] { display.hex(row, label, value); }
        }
        #[cfg(all(feature = "sunfish-oack-retry-close", not(feature = "sunfish-first-oack-spare-buffer")))]
        {
            let trace = owner.oack_grant_outcome();
            let packet = trace.pending_tftp.iter().enumerate().find_map(|(slot, meta)| meta.map(|meta| (slot, meta)));
            let used = packet.and_then(|(slot, _)| trace.used[slot]);
            let repeat = packet.filter(|(_, meta)| meta.first_repeat).and_then(|_| owner.service_registry_trace()
                .first_server_check.and_then(|write| write.repeat));
            let close = owner.init_close_fifo().bounds;
            let tail = match owner.first_oack_tail() {
                qcom_glink_fifo::ServerCheckTail::Consumed(at) => at, _ => None,
            };
            display.replace_line(1, "OACK RETRY CLOSE");
            for (index, (label, value)) in [
                ("RUNTIME ERROR", self.error),
                ("OACK HEAD US", age(trace.oack_at)),
                ("OACK READ US", age(tail)),
                ("ACK READ US", age(trace.ack.and_then(|ack| ack.first_read))),
                ("PKT INTENT", packet.and_then(|(slot, _)| trace.available[slot]).map_or(u32::MAX, |offer| offer.id)),
                ("PKT OP KIND", packet.map_or(u32::MAX, |(_, meta)| u32::from(meta.opcode) << 16 | u32::from(meta.path_kind))),
                ("PKT BYTES", packet.map_or(u32::MAX, |(_, meta)| u32::from(meta.bytes))),
                ("PKT EXACT WRQ", packet.map_or(u32::MAX, |(_, meta)| u32::from(meta.exact_wrq))),
                ("PKT IS RETRY", packet.map_or(u32::MAX, |(_, meta)| u32::from(meta.first_repeat))),
                ("PKT CHECKED US", age(used.and_then(|used| used.checked))),
                ("PKT CLEARED US", age(used.and_then(|used| used.clear))),
                ("RETRY COALESCED", repeat.map_or(u32::MAX, |repeat| u32::from(repeat.coalesced))),
                ("RETRY DISP US", age(repeat.and_then(|repeat| repeat.dispatch_at))),
                ("RETRY RESULT", repeat.and_then(|repeat| repeat.result).unwrap_or(u32::MAX)),
                ("RETRY QUEUE US", age(repeat.and_then(|repeat| repeat.queued_at))),
                ("RETRY HEAD US", age(repeat.and_then(|repeat| repeat.oack.published_at))),
                ("RETRY DONE US", age(repeat.and_then(|repeat| repeat.oack.done_at))),
                ("RETRY FLOW BUSY", repeat.map_or(u32::MAX, |repeat| repeat.flow_busy)),
                ("RETRY NO CAP", repeat.map_or(u32::MAX, |repeat| repeat.no_capacity)),
                ("RETRY CLOSE", repeat.map_or(u32::MAX, |repeat| (u32::from(repeat.cancelled) << 8)
                    | u32::from(repeat.close_phase.unwrap_or(0xff)))),
                ("CLOSE ABSENT US", age(close.and_then(|close| close.last_absent))),
                ("CLOSE VIS US", age(close.and_then(|close| close.first_visible))),
                ("FIRST CLOSE US", age(first.and_then(|first| first.first_close_at))),
                ("OACK DONE US", age(first.and_then(|first| first.oack_done_at))),
                ("FIRST DATA US", age(first.and_then(|first| first.first_data_at))),
                ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                ("WLFW SERVERS", query[7]),
            ].into_iter().enumerate() { display.hex(index + 2, label, value); }
        }
        #[cfg(all(feature = "sunfish-first-oack-spare-buffer", not(feature = "sunfish-wrq-stage-costs")))]
        {
            let trace = owner.oack_grant_outcome();
            let spare = trace.primed_offer;
            let slot = spare.and_then(|offer| trace.available.iter().position(|entry|
                entry.is_some_and(|entry| entry.id == offer.id)));
            let used = slot.and_then(|slot| trace.used[slot]);
            let pending = if used.is_some() { 1 }
                else if slot.is_some() && trace.pending_clear.is_some() { 0 }
                else { u32::MAX };
            let close = owner.init_close_fifo().bounds;
            let resources = first.and_then(|first| owner.oack_resources(first.transfer_port));
            let final_ack = owner.tftp_final_ack_trace().filter(|ack|
                ack.destination.node == 0 && ack.destination.port == 0x1f && ack.source.port == 0x5001);
            let read = match owner.first_oack_tail() {
                qcom_glink_fifo::ServerCheckTail::Consumed(at) => at, _ => None,
            };
            display.replace_line(1, "FIRST OACK SPARE");
            for (index, (label, value)) in [
                ("RUNTIME ERROR", self.error),
                ("FIRST PEER", first.map_or(u32::MAX, |first| first.peer.port)),
                ("SPARE ID", spare.map_or(u32::MAX, |offer| offer.id)),
                ("SPARE SIZE", spare.map_or(u32::MAX, |offer| offer.size)),
                ("SPARE HEAD US", age(spare.and_then(|offer| offer.published))),
                ("SPARE READ US", age(spare.and_then(|offer| offer.first_read))),
                ("SPARE PENDING", pending),
                ("SPARE USE CMD", used.map_or(u32::MAX, |used| u32::from(used.header.command))),
                ("SPARE USE SRC", used.and_then(|used| used.header.qrtr).map_or(u32::MAX, |q| q[1])),
                ("WRQ VISIBLE US", age(owner.ipa_wrq_timing().1.and_then(|bounds| bounds.first_visible))),
                ("WRQ RXDONE US", age(first.and_then(|first| first.wrq_done_at))),
                ("OACK HEAD US", age(trace.oack_at)),
                ("OACK READ US", age(read)),
                ("GRANT 108 ID", trace.intent.unwrap_or(u32::MAX)),
                ("ACK READ US", age(trace.ack.and_then(|ack| ack.first_read))),
                ("CLOSE ABSENT US", age(close.and_then(|close| close.last_absent))),
                ("CLOSE VIS US", age(close.and_then(|close| close.first_visible))),
                ("FIRST CLOSE US", age(first.and_then(|first| first.first_close_at))),
                ("OACK DONE US", age(first.and_then(|first| first.oack_done_at))),
                ("FIRST DATA US", age(first.and_then(|first| first.first_data_at))),
                ("DATA INTENT", resources.and_then(|resources| resources.data_intent).unwrap_or(u32::MAX)),
                ("FINAL ACK FLAGS", final_ack.map_or(0, |ack| ack.flags)),
                ("FINAL ACK US", age(final_ack.and_then(|ack| ack.published_at))),
                ("ACK DONE US", age(final_ack.and_then(|ack| ack.done_at))),
                ("IPA COMPLETE", u32::from(self.ipa_complete_seen)),
                ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                ("WLFW SERVERS", query[7]),
            ].into_iter().enumerate() { display.hex(index + 2, label, value); }
        }
        #[cfg(all(feature = "sunfish-wrq-stage-costs",not(feature = "sunfish-wrq-validation-path")))]
        {
            let work = owner.wrq_receive_work();
            let visible = owner.ipa_wrq_timing().1.and_then(|b| b.first_visible);
            let accepted = first.and_then(|t| t.wrq_at);
            let queued = gates.queued_at;
            let returned = work.finished;
            let close = owner.init_close_fifo().bounds;
            let read = match owner.first_oack_tail() {
                qcom_glink_fifo::ServerCheckTail::Consumed(at) => at, _ => None,
            };
            display.replace_line(1, "WRQ STAGE COSTS");
            for (i, (label, value)) in [
                ("RUNTIME ERROR", self.error), ("WRQ VISIBLE US", age(visible)),
                ("WRQ ACCEPT US", age(accepted)), ("OACK QUEUE US", age(queued)),
                ("WRQ RXDONE US", age(returned)),
                ("OACK HEAD US", age(owner.oack_grant_outcome().oack_at)),
                ("OACK READ US", age(read)),
                ("CLOSE ABSENT US", age(close.and_then(|c| c.last_absent))),
                ("CLOSE VIS US", age(close.and_then(|c| c.first_visible))),
            ].into_iter().enumerate() { display.hex(i+2,label,value); }
            display.replace_line(11, "PHASE TOTAL CK RX TX");
            let mut lost = 0;
            for (i, (from,to)) in [(visible,accepted),(accepted,queued),(queued,returned)].into_iter().enumerate() {
                if returned.is_some() && let Some((a,b)) = from.zip(to).filter(|(a,b)| a<=b) {
                    let c = work.cost(a,b); lost |= c.lost;
                    let us = |ticks| self.time_from_us(Some(0),Some(ticks));
                    display.header_row(i+12,false,&[(i as u32+1,2),(us(c.total),8),
                        (us(c.check_ticks),8),(us(c.rx),8),(us(c.tx),8)]);
                    let other = c.rx.checked_add(c.tx).and_then(|sum| c.total.checked_sub(sum));
                    display.header_row(i+12,true,&[(c.check_count,8),(c.lost,2),
                        (other.map_or(u32::MAX,us),8),(age(from),8),(age(to),8)]);
                } else {
                    lost = u32::MAX;
                    display.header_row(i+12,false,&[(i as u32+1,2),(u32::MAX,8),
                        (u32::MAX,8),(u32::MAX,8),(u32::MAX,8)]);
                    display.header_row(i+12,true,&[]);
                }
            }
            display.replace_line(15, "RECORD HEADERS");
            let mut count=0;
            if returned.is_some() && let Some((a,b))=visible.zip(returned).filter(|(a,b)| a<=b) {
                for r in work.records.records(a,b) {
                    if count<8 {
                        let q=r.protocol.unwrap_or([u32::MAX;4]);
                        display.header_row(count+16,false,&[
                            ((u32::from(r.kind)<<8)|u32::from(r.cmd),4),(u32::from(r.size),4),
                            (r.detail,8),(q[0],2),(q[1],8),(q[2],8),(q[3],8)]);
                        display.header_row(count+16,true,&[(age(Some(r.start)),8),(age(Some(r.end)),8)]);
                    }
                    count+=1;
                }
            }
            for i in count.min(8)..8 { display.replace_line(i+16,""); }
            for (row,label,value) in [
                (24,"HISTORY LOST",lost),
                (25,"RECORDS OMITTED",if returned.is_some(){count.saturating_sub(8) as u32}else{u32::MAX}),
                (26,"FIRST DATA US",age(first.and_then(|t|t.first_data_at))),
                (27,"WLAN RRQ RX",owner.tftp_wire_counts()[2]), (28,"WLFW SERVERS",query[7]),
            ] { display.hex(row,label,value); }
        }
        #[cfg(feature = "sunfish-wrq-validation-path")]
        {
            let work=owner.wrq_receive_work();let path=&work.validation;
            let visible=owner.ipa_wrq_timing().1.and_then(|b|b.first_visible);
            let returned=work.finished;let head=owner.oack_grant_outcome().oack_at;
            let read=match owner.first_oack_tail(){qcom_glink_fifo::ServerCheckTail::Consumed(t)=>t,_=>None};
            let close=owner.init_close_fifo().bounds;
            display.replace_line(1,"WRQ VALIDATION PATH");
            for(i,(label,value))in [
                ("RUNTIME ERROR",self.error),("WRQ VISIBLE US",age(visible)),
                ("WRQ RXDONE US",age(returned)),("REG PREP US",age(reg.and_then(|r|r.prepared_at))),
                ("REG QUEUE US",age(reg.and_then(|r|r.queued_at))),("REG PUB US",age(reg.and_then(|r|r.published_at))),
                ("OACK HEAD US",age(head)),("OACK READ US",age(read)),
                ("CLOSE ABSENT US",age(close.and_then(|c|c.last_absent))),
                ("CLOSE VIS US",age(close.and_then(|c|c.first_visible))),
            ].into_iter().enumerate(){display.hex(i+2,label,value);}
            display.replace_line(12,"PHASE TOTAL CK RX TX");
            #[cfg(not(feature="sunfish-wrq-domain-costs"))]
            display.replace_line(15,"CHECK OBS CUR ACQ");
            let mut lost=0;
            for(i,(from,to))in [(visible,returned),(returned,head)].into_iter().enumerate(){
                if path.finished.is_some() && let Some((a,b))=from.zip(to).filter(|(a,b)|a<=b){
                    let c=path.cost(a,b);lost|=c.lost;
                    let us=|t|self.time_from_us(Some(0),Some(t));
                    let checks=c.operations.iter().sum();
                    display.header_row(i+13,false,&[(i as u32+1,2),(us(c.total),8),(us(checks),8),(us(c.rx),8),(us(c.tx),8)]);
                    let other=c.rx.checked_add(c.tx).and_then(|t|c.total.checked_sub(t));
                    display.header_row(i+13,true,&[(c.count,4),(c.lost,2),(other.map_or(u32::MAX,us),8),(age(from),8),(age(to),8)]);
                    #[cfg(not(feature="sunfish-wrq-domain-costs"))]
                    {
                    display.header_row(i+16,false,&[(i as u32+1,2),(us(c.operations[0]),8),(us(c.operations[1]),8),(us(c.operations[2]),8)]);
                    display.header_row(i+16,true,&[(us(c.domains[0]),8),(us(c.domains[1]),8),(us(c.domains[2]),8),(c.missing,4),(c.domain_count,4)]);
                    }
                }else{
                    lost=u32::MAX;
                    for row in [i+13,i+16]{display.header_row(row,false,&[(i as u32+1,2),(u32::MAX,8),(u32::MAX,8),(u32::MAX,8),(u32::MAX,8)]);display.header_row(row,true,&[]);}
                }
            }
            let mut count:usize=0;
            #[cfg(not(feature="sunfish-wrq-domain-costs"))]
            {
            display.replace_line(18,"POST RETURN RECORDS");

            if path.finished.is_some() && let Some((a,b))=returned.zip(head).filter(|(a,b)|a<=b){
                for r in path.records.records(a,b){
                    if count<7 {let q=r.protocol.unwrap_or([u32::MAX;4]);
                        display.header_row(count+19,false,&[((u32::from(r.kind)<<8)|u32::from(r.cmd),4),(u32::from(r.size),4),(r.detail,8),(q[0],2),(q[1],8),(q[2],8),(q[3],8)]);
                        display.header_row(count+19,true,&[(age(Some(r.start)),8),(age(Some(r.end)),8)]);
                    }count+=1;
                }
            }
            for row in count.min(7)..7{display.replace_line(row+19,"");}
            }
            #[cfg(feature="sunfish-wrq-domain-costs")]
            {
                #[cfg(not(feature="sunfish-wrq-table-normal-nc"))]
                display.replace_line(1,"WRQ DOMAIN COSTS");
                #[cfg(feature="sunfish-wrq-table-normal-nc")]
                display.replace_line(1,"WRQ TABLE NORMAL NC");
                display.replace_line(15,"FAULT ASID ROUTE REG");
                display.replace_line(18,"DOMAIN PRE BODY POST");
                for (i,(from,to)) in [(visible,returned),(returned,head)].into_iter().enumerate(){
                    if path.finished.is_some() && let Some((a,b))=from.zip(to).filter(|(a,b)|a<=b){
                        let c=path.cost(a,b);let us=|t|self.time_from_us(Some(0),Some(t));
                        display.header_row(i+16,false,&[(i as u32+1,2),(us(c.parts[0]),8),(us(c.parts[1]),8),(us(c.parts[2]),8),(us(c.parts[3]),8)]);
                        display.header_row(i+16,true,&[(us(c.parts[4]),8),(us(c.parts[5]),8),(c.parts_missing,4),(c.parts_count,4)]);
                        display.header_row(i+19,false,&[(i as u32+1,2),(us(c.domains[0]),8),(us(c.domains[1]),8),(us(c.domains[2]),8)]);
                        let outer=c.domains[0].saturating_add(c.domains[2]);
                        let parts=c.parts.iter().sum::<u64>();
                        display.header_row(i+19,true,&[(us(parts),8),(us(outer),8),(outer.checked_sub(parts).map_or(u32::MAX,us),8),(c.missing,4)]);
                    }else{
                        for row in [i+16,i+19]{display.header_row(row,false,&[(i as u32+1,2),(u32::MAX,8),(u32::MAX,8),(u32::MAX,8),(u32::MAX,8)]);display.header_row(row,true,&[]);}
                    }
                }
                let late=|at:Option<u64>| at.zip(close.and_then(|c|c.first_visible)).and_then(|(a,b)|a.checked_sub(b))
                    .map_or(u32::MAX,|t|self.time_from_us(Some(0),Some(t)));
                for(row,label,value) in [(21,"OACK LATE US",late(head)),(22,"READ LATE US",late(read)),
                    (23,"IPA COMPLETE",u32::from(self.ipa_complete_seen)),(24,"WLAN RRQ RX",owner.tftp_wire_counts()[2]),(25,"WLFW SERVERS",query[7])]
                {display.hex(row,label,value);}
            }
            display.hex(26,"HISTORY LOST",lost);
            display.hex(27,"FIRST DATA US",age(first.and_then(|f|f.first_data_at)));
            display.replace_line(28,"");
            display.header_row(28,false,&[(owner.tftp_wire_counts()[2],8),(query[7],8),(u32::from(self.ipa_complete_seen),2),(count.saturating_sub(7) as u32,4)]);
            display.header_row(28,true,&[]);
        }
        #[cfg(all(feature = "sunfish-wrq-return-pending", not(feature = "sunfish-grant-rx-open")))]
        {
            let pending = owner.wrq_return_pending();
            let record = pending.and_then(|trace| trace.first);
            let qrtr = record.and_then(|header| header.qrtr);
            let visible = owner.ipa_wrq_timing().1.and_then(|bounds| bounds.first_visible);
            let returned = first.and_then(|trace| trace.wrq_done_at);
            let cleared = pending.and_then(|trace| trace.first_clear);
            let head = owner.wrq_prefix().oack_head;
            display.replace_line(1, "WRQ RETURN PENDING RX");
            for (row, (label, value)) in [
                ("RUNTIME ERROR", self.error),
                ("ERROR DETAIL", self.detail),
                ("FIRST PEER", first.map_or(u32::MAX, |trace| trace.peer.port)),
                ("WRQ VISIBLE US", age(visible)),
                ("WRQ RXDONE US", age(returned)),
                ("RETURN TURN", u32::from(grant.wrq_return_outcome)),
                ("PENDING BYTES", pending.map_or(u32::MAX, |trace| trace.bytes)),
                ("EARLY RECORDS", pending.map_or(u32::MAX, |trace| trace.early)),
                ("GLINK CMD", record.map_or(u32::MAX, |h| u32::from(h.command))),
                ("GLINK PARAM", record.map_or(u32::MAX, |h| h.parameter)),
                ("RECORD BYTES", record.map_or(u32::MAX, |h| u32::from(h.bytes))),
                ("RECORD DETAIL", record.map_or(u32::MAX, |h| h.extra)),
                ("QRTR TYPE", qrtr.map_or(u32::MAX, |q| q[0])),
                ("QRTR SRC PORT", qrtr.map_or(u32::MAX, |q| q[1])),
                ("QRTR DST PORT", qrtr.map_or(u32::MAX, |q| q[2])),
                ("CHECKED US", age(pending.and_then(|trace| trace.first_at))),
                ("RECORD CLEAR US", age(cleared)),
                ("RETURN TO CLEAR", self.time_from_us(returned, cleared)),
                ("BACKLOG CLR US", age(pending.and_then(|trace| trace.backlog_clear))),
                ("OACK HEAD US", age(head)),
                ("OACK TAIL US", age(tail)),
                ("WRQ TO HEAD US", self.time_from_us(visible, head)),
                ("FIRST DATA US", age(first.and_then(|trace| trace.first_data_at))),
                ("FIRST CLOSE US", age(first.and_then(|trace| trace.first_close_at))),
                ("IPA COMPLETE", u32::from(self.ipa_complete_seen)),
                ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                ("WLFW SERVERS", query[7]),
            ].into_iter().enumerate() { display.hex(row + 2, label, value); }
        }
        #[cfg(all(feature = "sunfish-first-wrq-return-oack", not(feature = "sunfish-wrq-return-pending")))]
        {
            let visible = owner.ipa_wrq_timing().1.and_then(|bounds| bounds.first_visible);
            let head = owner.wrq_prefix().oack_head;
            let close = owner.init_close_fifo().bounds;
            let data_at = first.and_then(|trace| trace.first_data_at);
            let close_at = first.and_then(|trace| trace.first_close_at);
            display.replace_line(1, "WRQ RETURN EARLY OACK");
            for (row, (label, value)) in [
                ("RUNTIME ERROR", self.error),
                ("ERROR DETAIL", self.detail),
                ("FIRST PEER", first.map_or(u32::MAX, |trace| trace.peer.port)),
                ("TRANSFER PORT", first.map_or(u32::MAX, |trace| trace.transfer_port)),
                ("WRQ VISIBLE US", age(visible)),
                ("FIRST WRQ US", age(first.and_then(|trace| trace.wrq_at))),
                ("OACK QUEUE US", age(gates.queued_at)),
                ("WRQ RXDONE US", age(first.and_then(|trace| trace.wrq_done_at))),
                ("RETURN TURN", u32::from(grant.wrq_return_outcome)),
                ("BLOCK MASK", grant.wrq_return_mask.map_or(u32::MAX, u32::from)),
                ("FITTING INTENTS", grant.wrq_return_fitting.map_or(u32::MAX, u32::from)),
                ("OACK HEAD US", age(head)),
                ("OACK TAIL US", age(tail)),
                ("WRQ TO HEAD US", self.time_from_us(visible, head)),
                ("WRQ TO TAIL US", self.time_from_us(visible, tail)),
                ("REG PUB US", age(reg.and_then(|trace| trace.published_at))),
                ("OACK DONE US", age(first.and_then(|trace| trace.oack_done_at))),
                ("REPEAT WRQ US", age(first.and_then(|trace| trace.first_repeat_at))),
                ("FIRST DATA US", age(data_at)),
                ("CLOSE ABSENT US", age(close.and_then(|bounds| bounds.last_absent))),
                ("CLOSE VIS US", age(close.and_then(|bounds| bounds.first_visible))),
                ("FIRST CLOSE US", age(close_at)),
                ("DATA BEFORE CLOSE", close_at.map_or(u32::MAX, |closed|
                    u32::from(data_at.is_some_and(|data| data < closed)))),
                ("WRITES COMPLETE", self.server_check.completed_count() as u32),
                ("IPA COMPLETE", u32::from(self.ipa_complete_seen)),
                ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                ("WLFW SERVERS", query[7]),
            ].into_iter().enumerate() { display.hex(row + 2, label, value); }
        }
        #[cfg(all(feature = "sunfish-spare-wrq-ingress", not(feature = "sunfish-first-wrq-return-oack")))]
        {
            let work = owner.wrq_receive_work();
            let bounds = owner.ipa_wrq_timing().1;
            let visible = bounds.and_then(|b| b.first_visible);
            let accepted = first.and_then(|t| t.wrq_at);
            let window = visible.zip(accepted).filter(|(a,b)| a <= b);
            let checks = window.map(|(a,b)| work.checks(a,b));
            display.replace_line(1, "SPARE WRQ INGRESS");
            for (row,(label,value)) in [
                ("RUNTIME ERROR", self.error),
                ("FIRST PEER", first.map_or(u32::MAX, |t| t.peer.port)),
                ("WRQ VISIBLE US", age(visible)),
                ("BYTES AHEAD", bounds.and_then(|b| b.bytes_ahead).unwrap_or(u32::MAX)),
                ("FIRST WRQ US", age(accepted)),
                ("CHECK COUNT", checks.map_or(u32::MAX, |c| c.0)),
                ("CHECK US", self.time_from_us(Some(0), checks.map(|c| c.1))),
                ("HISTORY LOST", window.zip(checks).map_or(u32::MAX, |((a,_),c)|
                    u32::from(work.records.lost(a)) | (u32::from(c.2) << 1))),
                ("REG PUB US", age(reg.and_then(|r| r.published_at))),
                ("OACK HEAD US", age(owner.wrq_prefix().oack_head)),
                ("OACK TAIL US", age(tail)),
            ].into_iter().enumerate() { display.hex(row + 2, label, value); }
            display.replace_line(13, "K CMD LEN ARG START END");
            let mut records = window.into_iter().flat_map(|(a,b)| work.records.records(a,b));
            for i in 0..24 {
                let row = 14 + i / 2;
                if let Some(e) = records.next() {
                    display.header_row(row, i % 2 != 0, &[(e.kind.into(),1),(e.cmd.into(),2),
                        (e.size.into(),4),(e.detail,8),(age(Some(e.start)),8),(age(Some(e.end)),8)]);
                } else { display.header_row(row, i % 2 != 0, &[]); }
            }
            display.hex(26, "FIRST DATA US", age(first.and_then(|t| t.first_data_at)));
            display.hex(27, "WLAN RRQ RX", owner.tftp_wire_counts()[2]);
            display.hex(28, "WLFW SERVERS", query[7]);
        }
        #[cfg(all(feature = "sunfish-spare-oack-completions", not(feature = "sunfish-spare-wrq-ingress")))]
        {
            let (reg_tail, reg_done) = owner.registration_completion_window();
            let oack_done = owner.first_oack_rx_done();
            let close = owner.init_close_fifo().bounds;
            let wrq = owner.ipa_wrq_timing().1;
            display.replace_line(1, "SPARE OACK COMPLETIONS");
            for (index, (label, value)) in [
                ("RUNTIME ERROR", self.error),
                ("FIRST PEER", first.map_or(u32::MAX, |t| t.peer.port)),
                ("TRANSFER PORT", first.map_or(u32::MAX, |t| t.transfer_port)),
                ("WRQ VISIBLE US", age(wrq.and_then(|s| s.first_visible))),
                ("FIRST WRQ US", age(first.and_then(|t| t.wrq_at))),
                ("WRQ RXDONE US", age(first.and_then(|t| t.wrq_done_at))),
                ("REG PUB US", age(reg.and_then(|r| r.published_at))),
                ("REG TAIL US", age(reg_tail)),
                ("REG DONE ABS US", age(reg_done.and_then(|s| s.last_absent))),
                ("REG DONE VIS US", age(reg_done.and_then(|s| s.first_visible))),
                ("REG DONE US", age(reg.and_then(|r| r.completed_at))),
                ("OACK HEAD US", age(owner.wrq_prefix().oack_head)),
                ("OACK TAIL US", age(tail)),
                ("OACK DONE ABS US", age(oack_done.and_then(|s| s.last_absent))),
                ("OACK DONE VIS US", age(oack_done.and_then(|s| s.first_visible))),
                ("OACK DONE US", age(first.and_then(|t| t.oack_done_at))),
                ("REPEAT WRQ US", age(first.and_then(|t| t.first_repeat_at))),
                ("FIRST DATA US", age(first.and_then(|t| t.first_data_at))),
                ("CLOSE ABSENT US", age(close.and_then(|s| s.last_absent))),
                ("CLOSE VIS US", age(close.and_then(|s| s.first_visible))),
                ("FIRST CLOSE US", age(first.and_then(|t| t.first_close_at))),
                ("REQ108 RX US", age(grant.request_108_at)),
                ("SAME TURN US", age(owner.registration_admission_at())),
                ("WRITES COMPLETE", self.server_check.completed_count() as u32),
                ("IPA COMPLETE", u32::from(self.ipa_complete_seen)),
                ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                ("WLFW SERVERS", query[7]),
            ].into_iter().enumerate() { display.hex(index + 2, label, value); }
        }
        #[cfg(all(feature = "sunfish-spare-reg-timeline", not(feature = "sunfish-spare-oack-completions")))]
        {
            let rx = owner.registration_rx_visibility();
            let close = owner.init_close_fifo().bounds;
            let prefix = owner.wrq_prefix();
            display.replace_line(1, if cfg!(feature = "sunfish-spare-reg-admission") {
                "SPARE IPA REG ADMISSION"
            } else {
                "SPARE IPA REG TIMELINE"
            });
            for (index, (label, value)) in [
                ("RUNTIME ERROR", self.error),
                ("ERROR DETAIL", self.detail),
                ("FIRST PEER", first.map_or(u32::MAX, |t| t.peer.port)),
                ("FIRST WRQ US", age(first.and_then(|t| t.wrq_at))),
                ("OACK QUEUE US", age(gates.queued_at)),
                ("REG ABSENT US", age(rx.and_then(|b| b.last_absent))),
                ("REG VISIBLE US", age(rx.and_then(|b| b.first_visible))),
                ("REG RX AHEAD", rx.and_then(|b| b.bytes_ahead).unwrap_or(u32::MAX)),
                ("REG RX US", age(reg.and_then(|r| r.received_at))),
                ("REG ROUTE US", age(reg.and_then(|r| r.routed_at))),
                ("REG PREP US", age(reg.and_then(|r| r.prepared_at))),
                #[cfg(feature = "sunfish-spare-reg-admission")]
                ("SAME TURN US", age(owner.registration_admission_at())),
                ("REG Q TRY US", age(reg.and_then(|r| r.first_queue_attempt_at))),
                ("REG QUEUE US", age(reg.and_then(|r| r.queued_at))),
                ("REQ RXDONE US", age(reg.and_then(|r| r.tx.request_done_at))),
                ("REG PUB US", age(reg.and_then(|r| r.published_at))),
                ("CLOSE ABSENT US", age(close.and_then(|b| b.last_absent))),
                ("CLOSE VIS US", age(close.and_then(|b| b.first_visible))),
                ("FIRST CLOSE US", age(first.and_then(|t| t.first_close_at))),
                ("REQ108 RX US", age(grant.request_108_at)),
                ("DIRECT TURN", grant.direct_turn),
                ("OACK HEAD US", age(prefix.oack_head)),
                ("OACK TAIL US", age(tail)),
                ("FIRST DATA US", age(first.and_then(|t| t.first_data_at))),
                #[cfg(not(feature = "sunfish-spare-reg-admission"))]
                ("REG DONE US", age(reg.and_then(|r| r.completed_at))),
                ("IPA COMPLETE", u32::from(self.ipa_complete_seen)),
                ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                ("WLFW SERVERS", query[7]),
            ].into_iter().enumerate() { display.hex(index + 2, label, value); }
        }
        #[cfg(all(feature = "sunfish-first-wrq-spare-oack", not(feature = "sunfish-spare-reg-timeline")))]
        {
            let prefix = owner.wrq_prefix();
            let close = owner.init_close_fifo().bounds;
            let ack = owner.tftp_final_ack_trace().filter(|ack| first.is_some_and(|t|
                ack.destination == t.peer && ack.source.port == t.transfer_port && ack.block == 1));
            display.replace_line(1, "FIRST WRQ SPARE OACK");
            for (index, (label, value)) in [
                ("RUNTIME ERROR", self.error),
                ("ERROR DETAIL", self.detail),
                ("FIRST PEER", first.map_or(u32::MAX, |t| t.peer.port)),
                ("FIRST WRQ US", age(first.and_then(|t| t.wrq_at))),
                ("OACK QUEUE US", age(gates.queued_at)),
                ("REQ108 RX US", age(grant.request_108_at)),
                ("GRANT MATCH", u32::from(owner.grant_buffer_reads().matched)),
                ("REQ BYTES", owner.grant_buffer_reads().requested.unwrap_or(u32::MAX)),
                ("BACKING AD US", age(grant.backing.advertised_at)),
                ("GRANT ACK US", age(grant.ack_at)),
                ("REG PUB US", age(reg.and_then(|t| t.published_at))),
                ("DIRECT TURN", grant.direct_turn),
                ("OACK HEAD US", age(prefix.oack_head)),
                ("OACK TAIL US", age(tail)),
                ("OACK DONE US", age(first.and_then(|t| t.oack_done_at))),
                ("CLOSE ABSENT US", age(close.and_then(|b| b.last_absent))),
                ("CLOSE VIS US", age(close.and_then(|b| b.first_visible))),
                ("FIRST CLOSE US", age(first.and_then(|t| t.first_close_at))),
                ("FIRST DATA US", age(first.and_then(|t| t.first_data_at))),
                ("FINAL ACK US", age(ack.and_then(|t| t.published_at))),
                ("ACK DONE US", age(ack.and_then(|t| t.done_at))),
                ("INIT HEAD US", age(prefix.init_head)),
                ("WRITES COMPLETE", self.server_check.completed_count() as u32),
                ("IPA COMPLETE", u32::from(self.ipa_complete_seen)),
                ("PD QUERY STATE", query[1]),
                ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                ("WLFW SERVERS", query[7]),
            ].into_iter().enumerate() { display.hex(index + 2, label, value); }
        }
        #[cfg(all(feature = "sunfish-grant-buffer-reads", not(feature = "sunfish-first-wrq-spare-oack")))]
        {
            let pool = owner.grant_buffer_reads();
            let close = owner.init_close_fifo().bounds;
            let prefix = owner.wrq_prefix();
            display.replace_line(1, "GRANT BUFFER READS");
            for (index, (label, value)) in [
                ("RUNTIME ERROR", self.error),
                ("FIRST PEER", first.map_or(u32::MAX, |t| t.peer.port)),
                ("GRANT MATCH", u32::from(pool.matched)),
                ("REQUEST BYTES", pool.requested.unwrap_or(u32::MAX)),
                ("BACKING ID", pool.backing_id.unwrap_or(u32::MAX)),
                ("REQ ABSENT US", age(pool.absent)),
                ("REQ VISIBLE US", age(pool.visible)),
                ("REQ DECODE US", age(pool.decoded)),
                ("FIT AT DECODE", pool.fits.iter().flatten().count() as u32),
                ("BACKING AD US", age(grant.backing.advertised_at)),
                ("OACK HEAD US", age(prefix.oack_head)),
                ("CLOSE VIS US", age(close.and_then(|b| b.first_visible))),
            ].into_iter().enumerate() { display.hex(index + 2, label, value); }
            display.replace_line(14, "ID SIZE OP AD US");
            for (i, entry) in pool.fits.iter().enumerate() {
                if let Some(row) = entry {
                    display.header_row(15+i, false, &[(row.id,8), (row.size,4),
                        (u32::from(row.command),2), (age(row.published),8)]);
                    display.header_row(15+i, true, &[(age(row.last_short),8),
                        (age(row.first_read),8), (row.order(pool.absent, pool.visible),1)]);
                } else {
                    display.header_row(15+i, false, &[]);
                    display.header_row(15+i, true, &[]);
                }
            }
            display.replace_line(23, "SHORT US READ US ORDER");
            display.hex(24, "FIRST DATA US", age(first.and_then(|t| t.first_data_at)));
            display.hex(25, "INIT HEAD US", age(prefix.init_head));
            display.hex(26, "IPA COMPLETE", u32::from(self.ipa_complete_seen));
            display.hex(27, "WLAN RRQ RX", owner.tftp_wire_counts()[2]);
            display.hex(28, "WLFW SERVERS", query[7]);
        }
        #[cfg(all(feature = "sunfish-post-reg-oack-turn", not(feature = "sunfish-grant-buffer-reads")))]
        {
            let turns = owner.post_reg_oack_turns();
            let entries = turns.entries();
            let initial = entries.first();
            let close = owner.init_close_fifo().bounds;
            let visible = close.and_then(|bounds| bounds.first_visible);
            let prefix = owner.wrq_prefix();
            display.replace_line(1, "POST REG OACK TURNS");
            for (index, (label, value)) in [
                ("RUNTIME ERROR", self.error),
                ("FIRST PEER", first.map_or(u32::MAX, |t| t.peer.port)),
                ("OACK QUEUE US", age(gates.queued_at)),
                ("REG PUB US", age(reg.and_then(|r| r.published_at))),
                ("CLOSE ABSENT US", age(close.and_then(|bounds| bounds.last_absent))),
                ("CLOSE VIS US", age(visible)),
                ("FIRST CLOSE US", age(first.and_then(|t| t.first_close_at))),
                ("FIRST TURN US", age(initial.map(|turn| turn.at))),
                ("FIRST MASK", initial.map_or(u32::MAX, |turn| u32::from(turn.mask))),
                ("SAVED PRE VIS", visible.map_or(u32::MAX, |at| entries.iter().filter(|t| t.at < at).count() as u32)),
                ("TURNS OMITTED", turns.omitted),
                ("BACKING AD US", age(grant.backing.advertised_at)),
                ("GRANT ACK US", age(grant.ack_at)),
                ("OACK HEAD US", age(prefix.oack_head)),
                ("INIT HEAD US", age(prefix.init_head)),
            ].into_iter().enumerate() { display.hex(index + 2, label, value); }
            display.replace_line(17, "US PATH MASK CMD RESULT");
            for i in 0..12 {
                let row = 18 + i / 2;
                if let Some(turn) = entries.get(i) {
                    display.header_row(row, i % 2 != 0, &[(age(Some(turn.at)), 8),
                        (turn.path.into(), 1), (turn.mask.into(), 3), (turn.command.into(), 3),
                        (turn.result.into(), 1)]);
                } else { display.header_row(row, i % 2 != 0, &[]); }
            }
            display.hex(24, "FIRST DATA US", age(first.and_then(|t| t.first_data_at)));
            display.hex(25, "REG DONE US", age(reg.and_then(|r| r.completed_at)));
            display.hex(26, "IPA COMPLETE", u32::from(self.ipa_complete_seen));
            display.hex(27, "WLAN RRQ RX", owner.tftp_wire_counts()[2]);
            display.hex(28, "WLFW SERVERS", query[7]);
        }
        #[cfg(all(feature = "sunfish-wrq-prefix-close", not(feature = "sunfish-post-reg-oack-turn")))]
        {
            let prefix = owner.wrq_prefix();
            let (ready, wrq) = owner.ipa_wrq_timing();
            let close = owner.init_close_fifo().bounds;
            display.replace_line(1, if cfg!(feature = "sunfish-smmu-grouped-table-check") {
                "WRQ CHECKED TABLES"
            } else {
                "WRQ PREFIX AND CLOSE"
            });
            for (index, (label, value)) in [
                ("RUNTIME ERROR", self.error),
                ("FIRST PEER", first.map_or(u32::MAX, |t| t.peer.port)),
                ("ENDPT READY US", age(ready)),
                ("WRQ VISIBLE US", age(wrq.and_then(|t| t.first_visible))),
                ("WRQ BYTES AHEAD", wrq.and_then(|t| t.bytes_ahead).unwrap_or(u32::MAX)),
                ("FIRST WRQ US", age(first.and_then(|t| t.wrq_at))),
                ("PREFIX COMPLETE", u32::from(prefix.complete)),
                ("PREFIX RECORDS", prefix.records),
                ("PREFIX BYTES", prefix.bytes),
                ("OACK QUEUE US", age(gates.queued_at)),
                ("REG PUB US", age(reg.and_then(|r| r.published_at))),
                ("OACK HEAD US", age(prefix.oack_head)),
                ("INIT HEAD US", age(prefix.init_head)),
                ("CLOSE ABSENT US", age(close.and_then(|c| c.last_absent))),
                ("CLOSE VIS US", age(close.and_then(|c| c.first_visible))),
                ("FIRST CLOSE US", age(first.and_then(|t| t.first_close_at))),
            ].into_iter().enumerate() { display.hex(index + 2, label, value); }
            display.replace_line(18, "CMD LEN ARG K SRC DST X");
            for i in 0..12 {
                let row = 19 + i / 2;
                if let Some(h) = prefix.headers().get(i) {
                    let [kind, src, dst] = h.qrtr.unwrap_or([u32::MAX; 3]);
                    display.header_row(row, i % 2 != 0, &[(h.command.into(),2), (h.bytes.into(),4),
                        (h.parameter,8), (kind,2), (src,8), (dst,8), (h.extra,8)]);
                } else { display.header_row(row, i % 2 != 0, &[]); }
            }
            display.hex(25, "FIRST DATA US", age(first.and_then(|t| t.first_data_at)));
            display.hex(26, "IPA COMPLETE", u32::from(self.ipa_complete_seen));
            display.hex(27, "WLAN RRQ RX", owner.tftp_wire_counts()[2]);
            display.hex(28, "WLFW SERVERS", query[7]);
        }
        #[cfg(all(feature = "sunfish-ipa-ingress-work", not(feature = "sunfish-wrq-prefix-close")))]
        {
            let work = owner.ipa_ingress_work();
            let bounds = owner.ipa_announcement_rx();
            let visible = bounds.and_then(|b| b.first_visible);
            let accepted = owner.first_ipa_announcement().and_then(|a| a.1);
            let (ready, wrq) = owner.ipa_wrq_timing();
            display.replace_line(1, "IPA INGRESS WORK");
            for (index, (label, value)) in [
                ("RUNTIME ERROR", self.error),
                ("IPA VISIBLE US", age(visible)),
                ("IPA BYTES AHEAD", bounds.and_then(|b| b.bytes_ahead).unwrap_or(u32::MAX)),
                ("NEW SERVER US", age(accepted)),
                ("ENDPT READY US", age(ready)),
                ("WRQ ABSENT US", age(wrq.and_then(|b| b.last_absent))),
                ("WRQ VISIBLE US", age(wrq.and_then(|b| b.first_visible))),
                ("WRQ BYTES AHEAD", wrq.and_then(|b| b.bytes_ahead).unwrap_or(u32::MAX)),
                ("INIT PUB US", age(owner.ipa_init_trace().1.and_then(|t| t.published_at))),
                ("FIRST CLOSE US", age(first.and_then(|t| t.first_close_at))),
                ("FIRST DATA US", age(first.and_then(|t| t.first_data_at))),
                ("HISTORY LOST", visible.map_or(u32::MAX, |t| u32::from(work.lost(t)))),
            ].into_iter().enumerate() { display.hex(index + 2, label, value); }
            display.replace_line(14, "K CMD LEN ID DIST FROM TO");
            let mut records = visible.zip(accepted).into_iter().flat_map(|(lo, hi)| work.records(lo, hi));
            for i in 0..24 {
                let row = 15 + i / 2;
                if let Some(e) = records.next() {
                    display.header_row(row, i % 2 != 0, &[(e.kind.into(),1), (e.cmd.into(),2),
                        (e.size.into(),4), (e.detail,8), (work.distance(e).unwrap_or(0xffff) as u32,4),
                        (age(Some(e.start)),8), (age(Some(e.end)),8)]);
                } else { display.header_row(row, i % 2 != 0, &[]); }
            }
            display.hex(27, "WLAN RRQ RX", owner.tftp_wire_counts()[2]);
            display.hex(28, "WLFW SERVERS", query[7]);
        }
        #[cfg(all(feature = "sunfish-ipa-announcement-fifo", not(feature = "sunfish-ipa-ingress-work")))]
        {
            let announcement = owner.first_ipa_announcement();
            let bounds = owner.ipa_announcement_rx();
            let (ready, wrq) = owner.ipa_wrq_timing();
            let (init, tx) = owner.ipa_init_trace();
            display.replace_line(1, "IPA ANNOUNCEMENT FIFO");
            for (index, (label, value)) in [
                ("RUNTIME ERROR", self.error),
                ("ERROR DETAIL", self.detail),
                ("IPA MODEM PORT", announcement.map_or(u32::MAX, |a| a.0)),
                ("IPA ABSENT US", age(bounds.and_then(|b| b.last_absent))),
                ("IPA VISIBLE US", age(bounds.and_then(|b| b.first_visible))),
                ("IPA BYTES AHEAD", bounds.and_then(|b| b.bytes_ahead).unwrap_or(u32::MAX)),
                ("NEW SERVER US", age(announcement.and_then(|a| a.1))),
                ("ENDPT READY US", age(ready)),
                ("SVC PASS US", age(self.ipa_ready_service_at)),
                ("IPA DISC US", age(init.and_then(|t| t.discovered_at))),
                ("INIT PREP US", age(init.and_then(|t| t.prepared_at))),
                ("INIT QUEUE US", age(tx.and_then(|t| t.queued_at))),
                ("INIT PUB US", age(tx.and_then(|t| t.published_at))),
                ("FIRST WRQ PEER", first.map_or(u32::MAX, |t| t.peer.port)),
                ("WRQ ABSENT US", age(wrq.and_then(|b| b.last_absent))),
                ("WRQ VISIBLE US", age(wrq.and_then(|b| b.first_visible))),
                ("WRQ BYTES AHEAD", wrq.and_then(|b| b.bytes_ahead).unwrap_or(u32::MAX)),
                ("FIRST WRQ US", age(first.and_then(|t| t.wrq_at))),
                ("FIRST OACK US", age(first.and_then(|t| t.oack_at))),
                ("FIRST DATA US", age(first.and_then(|t| t.first_data_at))),
                ("FIRST CLOSE US", age(first.and_then(|t| t.first_close_at))),
                ("REG PUB US", age(reg.and_then(|t| t.published_at))),
                ("INIT RETIRE US", age(tx.and_then(|t| t.retired_at))),
                ("IPA COMPLETE", u32::from(self.ipa_complete_seen)),
                ("PD QUERY STATE", query[1]),
                ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                ("WLFW SERVERS", query[7]),
            ].into_iter().enumerate() {
                display.hex(index + 2, label, value);
            }
        }
        #[cfg(all(feature = "sunfish-reg-writer-timeline", not(feature = "sunfish-ipa-announcement-fifo")))]
        {
            let close = owner.init_close_fifo();
            let (_, init) = owner.ipa_init_trace();
            display.replace_line(1, "REG WRITER TIMELINE");
            let rows = [
                ("RUNTIME ERROR", self.error),
                ("ERROR DETAIL", self.detail),
                ("FIRST PEER", first.map_or(u32::MAX, |t| t.peer.port)),
                ("FIRST WRQ US", age(first.and_then(|t| t.wrq_at))),
                ("REG QUEUE US", age(reg.and_then(|r| r.queued_at))),
                ("REQUEST BYTES", grant.backing.intent.map_or(u32::MAX, |_| grant.backing.requested)),
                ("GRANT INTENT", grant.backing.intent.unwrap_or(u32::MAX)),
                ("RESERVED US", age(grant.backing.reserved_at)),
                ("REQ RXDONE US", age(reg.and_then(|r| r.tx.request_done_at))),
                ("REG PASS US", age(reg.and_then(|r| r.tx.first_pass_at))),
                ("REG SELECT US", age(reg.and_then(|r| r.tx.selected_at))),
                ("REG WRITE US", age(reg.and_then(|r| r.tx.committed_write_start))),
                ("WRITER END US", age(reg.filter(|r| r.published_at.is_some()).and_then(|r| r.tx.last_end))),
                ("REG PUB US", age(reg.and_then(|r| r.published_at))),
                ("REG BLOCKS", reg.map_or(u32::MAX, |r| r.tx.target_blocked)),
                ("AD WRITER", grant.backing.writer.map_or(0, |w| w as u32)),
                ("BACKING AD US", age(grant.backing.advertised_at)),
                ("GRANT ACK US", age(grant.ack_at)),
                ("LAST ABSENT US", age(close.bounds.and_then(|b| b.last_absent))),
                ("CLOSE VIS US", age(close.bounds.and_then(|b| b.first_visible))),
                ("FIRST CLOSE US", age(first.and_then(|t| t.first_close_at))),
                ("FIRST OACK US", age(first.and_then(|t| t.oack_at))),
                ("FIRST DATA US", age(first.and_then(|t| t.first_data_at))),
                ("INIT PUB US", age(init.and_then(|t| t.published_at))),
                ("IPA COMPLETE", u32::from(self.ipa_complete_seen)),
                ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                ("WLFW SERVERS", query[7]),
            ];
            for (index, (label, value)) in rows.into_iter().enumerate() {
                display.hex(index + 2, label, value);
            }
        }
        #[cfg(all(feature = "sunfish-ipa-post-init-wrq", not(feature = "sunfish-reg-writer-timeline")))]
        {
            let (init, transport) = owner.ipa_init_trace();
            #[cfg(not(feature = "sunfish-ipa-init-close-visibility"))]
            let reads = owner.read_sequence();
            #[cfg(feature = "sunfish-ipa-init-close-visibility")]
            let close = owner.init_close_fifo();
            #[cfg(feature = "sunfish-grant-ad-writer-trace")]
            display.replace_line(1, "GRANT RESERVE AND AD");
            #[cfg(all(feature = "sunfish-paired-oack-init", not(feature = "sunfish-grant-ad-writer-trace")))]
            display.replace_line(1, "PAIRED OACK AND INIT");
            #[cfg(all(feature = "sunfish-ipa-init-close-visibility", not(feature = "sunfish-paired-oack-init")))]
            display.replace_line(1, "INIT CLOSE VISIBILITY");
            #[cfg(not(feature = "sunfish-ipa-init-close-visibility"))]
            display.replace_line(1, "POST INIT FIRST WRQ");
            for (row, label, value) in [
                (2, "RUNTIME ERROR", self.error),
                (3, "ERROR DETAIL", self.detail),
                (4, "FIRST PEER", first.map_or(u32::MAX, |t| t.peer.port)),
                (5, "FIRST WRQ US", age(first.and_then(|t| t.wrq_at))),
                #[cfg(feature = "sunfish-grant-ad-writer-trace")]
                (6, "GRANT INTENT", grant.backing.intent.unwrap_or(u32::MAX)),
                #[cfg(all(feature = "sunfish-ipa-init-close-visibility", not(feature = "sunfish-grant-ad-writer-trace")))]
                (6, "FRESH RX BYTES", close.fresh_unread.unwrap_or(u32::MAX)),
                #[cfg(not(feature = "sunfish-ipa-init-close-visibility"))]
                (6, "OACK QUEUE US", age(gates.queued_at)),
                #[cfg(feature = "sunfish-grant-ad-writer-trace")]
                (7, "RESERVED US", age(grant.backing.reserved_at)),
                #[cfg(all(feature = "sunfish-ipa-init-close-visibility", not(feature = "sunfish-grant-ad-writer-trace")))]
                (7, "SAMPLE START US", age(close.sample_start)),
                #[cfg(not(feature = "sunfish-ipa-init-close-visibility"))]
                (7, "INIT QUEUE US", age(transport.and_then(|t| t.queued_at))),
                #[cfg(feature = "sunfish-grant-ad-writer-trace")]
                (8, "REG PUB US", age(reg.and_then(|t| t.published_at))),
                #[cfg(all(feature = "sunfish-ipa-init-close-visibility", not(feature = "sunfish-grant-ad-writer-trace")))]
                (8, "SAMPLE END US", age(close.sample_end)),
                #[cfg(not(feature = "sunfish-ipa-init-close-visibility"))]
                (8, "REG PUB US", age(reg.and_then(|t| t.published_at))),
                #[cfg(feature = "sunfish-grant-ad-writer-trace")]
                (9, "AD WRITER", grant.backing.writer.map_or(0, |w| w as u32)),
                #[cfg(all(feature = "sunfish-ipa-init-close-visibility", not(feature = "sunfish-grant-ad-writer-trace")))]
                (9, "CLOSE IN SAMPLE", close.visible_at_sample().map_or(u32::MAX, u32::from)),
                #[cfg(not(feature = "sunfish-ipa-init-close-visibility"))]
                (9, "REG DONE US", age(reg.and_then(|t| t.completed_at))),
                (10, "INIT PUB US", age(transport.and_then(|t| t.published_at))),
                (11, "INIT RETIRE US", age(transport.and_then(|t| t.retired_at))),
                #[cfg(feature = "sunfish-ipa-init-close-visibility")]
                (12, "LAST ABSENT US", age(close.bounds.and_then(|b| b.last_absent))),
                #[cfg(not(feature = "sunfish-ipa-init-close-visibility"))]
                (12, "INIT RESP US", age(init.and_then(|t| t.response_at))),
                #[cfg(feature = "sunfish-grant-ad-writer-trace")]
                (13, "BACKING AD US", age(grant.backing.advertised_at)),
                #[cfg(not(feature = "sunfish-grant-ad-writer-trace"))]
                (13, "BACKING AD US", age(grant.advertisement_at)),
                (14, "GRANT ACK US", age(grant.ack_at)),
                (15, "FOLLOW RESULT", grant.outcome),
                (16, "FIRST OACK US", age(first.and_then(|t| t.oack_at))),
                (17, "OACK TAIL US", age(tail)),
                (18, "OACK DONE US", age(first.and_then(|t| t.oack_done_at))),
                (19, "FIRST DATA US", age(first.and_then(|t| t.first_data_at))),
                (20, "FIRST CLOSE US", age(first.and_then(|t| t.first_close_at))),
                (21, "CLOSE BLOCK", gates.close_mask.map_or(u32::MAX, u32::from)),
                #[cfg(feature = "sunfish-ipa-init-close-visibility")]
                (22, "CLOSE VIS US", age(close.bounds.and_then(|b| b.first_visible))),
                #[cfg(not(feature = "sunfish-ipa-init-close-visibility"))]
                (22, "READ KIND MASK", reads.kinds),
                #[cfg(feature = "sunfish-grant-ad-writer-trace")]
                (23, "REQUEST BYTES", grant.backing.intent.map_or(u32::MAX, |_| grant.backing.requested)),
                #[cfg(all(feature = "sunfish-ipa-init-close-visibility", not(feature = "sunfish-grant-ad-writer-trace")))]
                (23, "RX BYTES AHEAD", close.bounds.and_then(|b| b.bytes_ahead).unwrap_or(u32::MAX)),
                #[cfg(not(feature = "sunfish-ipa-init-close-visibility"))]
                (23, "FIRST READ US", age(reads.first[0].and_then(|r| r.2))),
                (24, "COMPLETED PEER", completed.map_or(u32::MAX, |t| t.peer.port)),
                (25, "IPA COMPLETE", u32::from(self.ipa_complete_seen)),
                (26, "PD QUERY STATE", query[1]),
                (27, "WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                (28, "WLFW SERVERS", query[7]),
            ] { display.hex(row, label, value); }
        }
        #[cfg(all(feature = "sunfish-ipa-init-open-writer", not(feature = "sunfish-ipa-post-init-wrq")))]
        {
            let (init, transport) = owner.ipa_init_trace();
            let tx = transport.map(|t| t.tx).unwrap_or_default();
            let open = tx.at_first_close;
            let pair = diagnostic::packed_counts;
            #[cfg(not(feature = "sunfish-ipa-init-before-ad"))]
            display.replace_line(1, "IPA INIT OPEN WRITER");
            #[cfg(feature = "sunfish-ipa-init-before-ad")]
            display.replace_line(1, "IPA INIT BEFORE AD");
            for (row, label, value) in [
                (2, "RUNTIME ERROR", self.error),
                (3, "ERROR DETAIL", self.detail),
                (4, "REG PUB US", age(reg.and_then(|r| r.published_at))),
                (5, "POST REG TURN", init.map_or(u32::MAX, |t| t.post_registration as u32)),
                (6, "INIT QUEUE US", age(transport.and_then(|t| t.queued_at))),
                (7, "QUEUE STATE", transport.map_or(u32::MAX, |t| t.tx.queue_state)),
                (8, "FIRST PASS US", age(tx.first_pass_at)),
                (9, "FIRST FIT US", age(tx.first_fit_at)),
                (10, "INIT SELECT US", age(tx.selected_at)),
                (11, "FIRST CLOSE US", age(first.and_then(|t| t.first_close_at))),
                (12, "OPEN PASSES", open.map_or(u32::MAX, |v| v[0])),
                (13, "OPEN NOFIT", open.map_or(u32::MAX, |v| v[1])),
                (14, "OPEN CTRL DATA", open.map_or(u32::MAX, |v| pair(v[2], v[3]))),
                (15, "OPEN GATES", open.map_or(u32::MAX, |v| v[4])),
                (16, "ALL PASS NOFIT", pair(tx.passes, tx.no_fit)),
                (17, "CTRL DATA AHEAD", pair(tx.control_ahead, tx.data_ahead)),
                (18, "AD RXDONE", pair(tx.commands[0], tx.commands[1])),
                (19, "REQ GRANTACK", pair(tx.commands[2], tx.commands[3])),
                (20, "CREDIT TFTP", pair(tx.data_kinds[0], tx.data_kinds[5])),
                (21, "G ACK CREDIT", pair(tx.gates[0], tx.gates[4])),
                (22, "TURN BLOCKED", pair(tx.priority_turn, tx.target_blocked)),
                (23, "INIT PUB US", age(transport.and_then(|t| t.published_at))),
                (24, "FIRST DATA US", age(first.and_then(|t| t.first_data_at))),
                (25, "IPA COMPLETE", u32::from(self.ipa_complete_seen)),
                (26, "PD QUERY STATE", query[1]),
                (27, "WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                (28, "WLFW SERVERS", query[7]),
            ] { display.hex(row, label, value); }
        }
        #[cfg(all(feature = "sunfish-ipa-reg-close-fifo", not(feature = "sunfish-ipa-init-open-writer")))]
        {
            let trace = owner.registration_close_fifo();
            let v = trace.unwrap_or_default();
            let (_, transport) = owner.ipa_init_trace();
            #[cfg(not(feature = "sunfish-ipa-init-empty-rx"))]
            display.replace_line(1, "IPA REG CLOSE FIFO");
            #[cfg(feature = "sunfish-ipa-init-empty-rx")]
            display.replace_line(1, "IPA INIT EMPTY RX");
            for (row, label, value) in [
                (2, "RUNTIME ERROR", self.error),
                (3, "ERROR DETAIL", self.detail),
                (4, "REG PUB US", age(reg.and_then(|r| r.published_at))),
                (5, "VIS AT REG HEAD", trace.map_or(u32::MAX, |t| t.status())),
                (6, "RX PRE BYTES", v.pre.unwrap_or(u32::MAX)),
                (7, "RX POST BYTES", v.post.unwrap_or(u32::MAX)),
                (8, "RX FRESH BYTES", v.fresh.unwrap_or(u32::MAX)),
                (9, "FRESH START US", age(v.fresh_start)),
                (10, "FRESH END US", age(v.fresh_end)),
                (11, "CLOSE START", v.end.zip(v.record_bytes).and_then(|(end, len)| end.checked_sub(len)).unwrap_or(u32::MAX)),
                (12, "CLOSE END", v.end.unwrap_or(u32::MAX)),
                (13, "RECORD BYTES", v.record_bytes.unwrap_or(u32::MAX)),
                (14, "AT FRESH SAMPLE", v.end.zip(v.fresh).map_or(u32::MAX, |(end, fresh)| u32::from(end <= fresh))),
                (15, "FIRST WRQ PEER", first.map_or(u32::MAX, |t| t.peer.port)),
                (16, "FIRST WRQ US", age(first.and_then(|t| t.wrq_at))),
                (17, "FIRST CLOSE US", age(first.and_then(|t| t.first_close_at))),
                (18, "POST REG TURN", owner.ipa_init_trace().0.map_or(u32::MAX, |t| t.post_registration as u32)),
                (19, "INIT QUEUE US", age(transport.and_then(|t| t.queued_at))),
                (20, "INIT PUB US", age(transport.and_then(|t| t.published_at))),
                (21, "INIT RETIRE US", age(transport.and_then(|t| t.retired_at))),
                (22, "REG DONE US", age(reg.and_then(|t| t.completed_at))),
                (23, "FIRST OACK US", age(first.and_then(|t| t.oack_at))),
                (24, "FIRST DATA US", age(first.and_then(|t| t.first_data_at))),
                (25, "IPA COMPLETE", u32::from(self.ipa_complete_seen)),
                (26, "PD QUERY STATE", query[1]),
                (27, "WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                (28, "WLFW SERVERS", query[7]),
            ] { display.hex(row, label, value); }
        }
        #[cfg(all(feature = "sunfish-ipa-init-writer-fit", not(feature = "sunfish-ipa-reg-close-fifo")))]
        {
            let (_, transport) = owner.ipa_init_trace();
            let tx = transport.map(|t| t.tx).unwrap_or_default();
            let pair = diagnostic::packed_counts;
            #[cfg(not(feature = "sunfish-ipa-init-after-registration"))]
            display.replace_line(1, "IPA INIT WRITER FIT");
            #[cfg(feature = "sunfish-ipa-init-after-registration")]
            display.replace_line(1, "IPA INIT AFTER REG");
            for (row, label, value) in [
                (2, "RUNTIME ERROR", self.error),
                (3, "INIT QUEUE US", age(transport.and_then(|t| t.queued_at))),
                (4, "FIRST WRITER US", age(tx.first_writer_at)),
                (5, "FIRST FIT US", age(tx.first_fit_at)),
                (6, "INIT SELECT US", age(tx.selected_at)),
                (7, "INIT PUB US", age(transport.and_then(|t| t.published_at))),
                (8, "INIT RETIRE US", age(transport.and_then(|t| t.retired_at))),
                (9, "FIT NOFIT", pair(tx.passes.saturating_sub(tx.no_fit), tx.no_fit)),
                (10, "CTRL DATA AHEAD", pair(tx.control_ahead, tx.data_ahead)),
                (11, "ADV RXDONE", pair(tx.commands[0], tx.commands[1])),
                (12, "INTREQ ACK", pair(tx.commands[2], tx.commands[3])),
                #[cfg(not(feature = "sunfish-ipa-init-after-registration"))]
                (13, "REUSE OTHER", pair(tx.commands[4], tx.commands[5])),
                #[cfg(feature = "sunfish-ipa-init-after-registration")]
                (13, "REG PUB US", age(reg.and_then(|t| t.published_at))),
                (14, "CREDIT QRTRCTL", pair(tx.data_kinds[0], tx.data_kinds[1])),
                (15, "RMTFS REGISTRY", pair(tx.data_kinds[2], tx.data_kinds[3])),
                (16, "REPLY TFTP", pair(tx.data_kinds[4], tx.data_kinds[5])),
                #[cfg(not(feature = "sunfish-ipa-init-after-registration"))]
                (17, "IPA OTHER DATA", pair(tx.data_kinds[6], tx.data_kinds[7])),
                #[cfg(feature = "sunfish-ipa-init-after-registration")]
                (17, "POST REG TURN", owner.ipa_init_trace().0.map_or(u32::MAX, |t| t.post_registration as u32)),
                (18, "G ACK WRQ", pair(tx.gates[0], tx.gates[1])),
                #[cfg(not(feature = "sunfish-ipa-init-after-registration"))]
                (19, "G TFTP PRIORITY", pair(tx.gates[2], tx.gates[3])),
                #[cfg(feature = "sunfish-ipa-init-after-registration")]
                (19, "FIRST OACK US", age(first.and_then(|t| t.oack_at))),
                (20, "G CREDIT UNRDY", pair(tx.gates[4], tx.gates[5])),
                (21, "TURN BLOCKED", pair(tx.priority_turn, tx.target_blocked)),
                #[cfg(not(feature = "sunfish-ipa-init-after-registration"))]
                (22, "TX FLAGS", tx.flags),
                #[cfg(feature = "sunfish-ipa-init-after-registration")]
                (22, "FIRST DATA US", age(first.and_then(|t| t.first_data_at))),
                (23, "FIRST WRQ US", age(first.and_then(|t| t.wrq_at))),
                (24, "FIRST CLOSE US", age(first.and_then(|t| t.first_close_at))),
                (25, "IPA COMPLETE", u32::from(self.ipa_complete_seen)),
                (26, "PD QUERY STATE", query[1]),
                (27, "WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                (28, "WLFW SERVERS", query[7]),
            ] { display.hex(row, label, value); }
        }
        #[cfg(all(feature = "sunfish-ipa-announcement-timeline", not(feature = "sunfish-ipa-init-writer-fit")))]
        {
            let announcement = owner.first_ipa_announcement();
            let (init, transport) = owner.ipa_init_trace();
            let reads = owner.read_sequence();
            #[cfg(not(feature = "sunfish-ipa-wrq-ready-fifo"))]
            display.replace_line(1, "IPA DISCOVERY AND WRQ");
            #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
            display.replace_line(1, "IPA READY AND WRQ FIFO");
            for (row, label, value) in [
                (2, "RUNTIME ERROR", self.error),
                (3, "ERROR DETAIL", self.detail),
                (4, "IPA MODEM PORT", announcement.map_or(u32::MAX, |a| a.0)),
                (5, "NEW SERVER US", age(announcement.and_then(|a| a.1))),
                (6, "IPA DISC US", age(init.and_then(|t| t.discovered_at))),
                (7, "INIT PREP US", age(init.and_then(|t| t.prepared_at))),
                #[cfg(not(feature = "sunfish-ipa-wrq-ready-fifo"))]
                (8, "REG PUB US", age(reg.and_then(|t| t.published_at))),
                #[cfg(not(feature = "sunfish-ipa-wrq-ready-fifo"))]
                (9, "REG DONE US", age(reg.and_then(|t| t.completed_at))),
                #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
                (8, "ENDPT READY US", age(owner.ipa_wrq_timing().0)),
                #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
                (9, "SVC PASS US", age(self.ipa_ready_service_at)),
                (10, "INIT QUEUE US", age(transport.and_then(|t| t.queued_at))),
                (11, "INIT PUB US", age(transport.and_then(|t| t.published_at))),
                (12, "INIT RETIRE US", age(transport.and_then(|t| t.retired_at))),
                #[cfg(not(feature = "sunfish-ipa-wrq-ready-fifo"))]
                (13, "INIT RESP US", age(init.and_then(|t| t.response_at))),
                #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
                (13, "INIT WRITER US", age(transport.and_then(|t| t.tx.first_writer_at))),
                (14, "FIRST WRQ PEER", first.map_or(u32::MAX, |t| t.peer.port)),
                (15, "FIRST WRQ US", age(first.and_then(|t| t.wrq_at))),
                (16, "FIRST OACK US", age(first.and_then(|t| t.oack_at))),
                (17, "FIRST DATA US", age(first.and_then(|t| t.first_data_at))),
                (18, "FIRST CLOSE US", age(first.and_then(|t| t.first_close_at))),
                #[cfg(not(feature = "sunfish-ipa-wrq-ready-fifo"))]
                (19, "REG RX US", age(reg.and_then(|t| t.received_at))),
                #[cfg(not(feature = "sunfish-ipa-wrq-ready-fifo"))]
                (20, "WRITES COMPLETE", self.server_check.completed_count() as u32),
                #[cfg(not(feature = "sunfish-ipa-wrq-ready-fifo"))]
                (21, "COMPLETED PEER", completed.map_or(u32::MAX, |t| t.peer.port)),
                #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
                (19, "WRQ ABSENT US", age(owner.ipa_wrq_timing().1.and_then(|t| t.last_absent))),
                #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
                (20, "WRQ VISIBLE US", age(owner.ipa_wrq_timing().1.and_then(|t| t.first_visible))),
                #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
                (21, "WRQ BYTES AHEAD", owner.ipa_wrq_timing().1.and_then(|t| t.bytes_ahead).unwrap_or(u32::MAX)),
                (22, "SERVICE RRQS", reads.total),
                (23, "READ KIND MASK", reads.kinds),
                (24, "FIRST READ US", age(reads.first[0].and_then(|r| r.2))),
                (25, "IPA COMPLETE", u32::from(self.ipa_complete_seen)),
                (26, "PD QUERY STATE", query[1]),
                (27, "WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                (28, "WLFW SERVERS", query[7]),
            ] { display.hex(row, label, value); }
        }
        #[cfg(all(feature = "sunfish-wlan-read-sequence", not(feature = "sunfish-ipa-announcement-timeline")))]
        {
            let reads = owner.read_sequence();
            let ack = owner.tftp_final_ack_trace();
            display.replace_line(1, "WLAN STARTUP READS");
            for (row, label, value) in [
                (2, "RUNTIME ERROR", self.error),
                (3, "ERROR DETAIL", self.detail),
                (4, "WRITES COMPLETE", self.server_check.completed_count() as u32),
                (5, "SERVICE RRQS", reads.total),
                (6, "READ KIND MASK", reads.kinds),
                (7, "FINAL ACK PEER", ack.map_or(u32::MAX, |a| a.destination.port)),
                (8, "FINAL ACK PUB US", age(ack.and_then(|a| a.published_at))),
                (9, "FINAL ACK DONE US", age(ack.and_then(|a| a.done_at))),
                (10, "WRITE CLOSE US", age(completed.filter(|t| Some(t.peer.port) == ack.map(|a| a.destination.port)).and_then(|t| t.first_close_at))),
                (26, "PD QUERY STATE", query[1]),
                (27, "WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                (28, "WLFW SERVERS", query[7]),
            ] { display.hex(row, label, value); }
            for (i, record) in reads.first.iter().copied().enumerate() {
                let row = 11 + 3 * i;
                display.hex(row, "READ KIND", record.map_or(u32::MAX, |r| r.0));
                display.hex(row + 1, "PEER PORT", record.map_or(u32::MAX, |r| r.1));
                display.hex(row + 2, "READ US", age(record.and_then(|r| r.2)));
            }
        }
        #[cfg(all(feature = "sunfish-first-oack-yield-rx", not(feature = "sunfish-wlan-read-sequence")))]
        {
            let window = grant.window;
            display.replace_line(1, "FIRST OACK YIELD RX");
            for (row, label, value) in [
                (2, "RUNTIME ERROR", self.error),
                (3, "ERROR DETAIL", self.detail),
                (4, "FIRST PEER", first.map_or(u32::MAX, |t| t.peer.port)),
                (5, "TRIGGER CMD", grant.trigger),
                (6, "BACKING AD US", age(grant.advertisement_at)),
                (7, "GRANT ACK US", age(grant.ack_at)),
                (8, "FOLLOW RESULT", grant.outcome),
                (9, "YIELD RX BYTES", grant.unread_bytes),
                (10, "BYTES LEFT", window.remaining),
                (11, "WHOLE RECORDS", window.records),
                (12, "GLINK CMD MASK", window.commands),
                (13, "QRTR TYPE MASK", window.qrtr_types),
                (14, "DEL NODE", window.first_close.map_or(u32::MAX, |v| v[0])),
                (15, "DEL PORT", window.first_close.map_or(u32::MAX, |v| v[1])),
                (16, "MATCHED CLOSE", u32::from(window.matched_close)),
                (17, "PARTIAL FRAME", u32::from(window.partial)),
                (18, "CURSOR LOST", u32::from(window.lost_cursor)),
                (19, "FIRST CLOSE US", age(first.and_then(|t| t.first_close_at))),
                (20, "FIRST OACK US", age(first.and_then(|t| t.oack_at))),
                (21, "OACK TAIL US", age(tail)),
                (22, "FIRST DATA US", age(first.and_then(|t| t.first_data_at))),
                (23, "CLOSE BLOCK", gates.close_mask.map_or(u32::MAX, u32::from)),
                (24, "IPA DONE US", age(reg.and_then(|t| t.completed_at))),
                (25, "COMPLETED PEER", completed.map_or(u32::MAX, |t| t.peer.port)),
                (26, "PD QUERY STATE", query[1]),
                (27, "WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                (28, "WLFW SERVERS", query[7]),
            ] { display.hex(row, label, value); }
        }
        #[cfg(feature="sunfish-wrq-post-return-callers")]
        self.show_wrq_callers(display,owner);
    }

    #[cfg(feature = "sunfish-first-oack-after-ipa-publish")]
    fn show_first_oack_after_ipa_publish(&self, display: &Display, owner: &ModemOwner) {
        let [first, completed] = owner.server_check_oacks();
        let reg = owner.ipa_registration_trace();
        let gates = owner.first_oack_eligibility();
        let ack = owner.tftp_final_ack_trace();
        let query = self.wlan_query.snapshot(owner);
        let age = |stamp| self.time_from_us(Some(self.runtime_started), stamp);
        display.replace_line(1, "FIRST OACK ELIGIBILITY");
        for (row, label, value) in [
            (2, "RUNTIME ERROR", self.error),
            (3, "ERROR DETAIL", self.detail),
            (4, "FIRST PEER", first.map_or(u32::MAX, |t| t.peer.port)),
            (5, "FIRST WRQ US", age(first.and_then(|t| t.wrq_at))),
            (6, "IPA PUB US", age(reg.and_then(|t| t.published_at))),
            (7, "EARLY ATTEMPT", first.map_or(0, |t| u32::from(t.early_oack_attempted))),
            (8, "FIRST OACK US", age(first.and_then(|t| t.oack_at))),
            (9, "FIRST DATA US", age(first.and_then(|t| t.first_data_at))),
            (10, "FIRST CLOSE US", age(first.and_then(|t| t.first_close_at))),
            (11, "OACK DONE US", age(first.and_then(|t| t.oack_done_at))),
            (12, "IPA DONE US", age(reg.and_then(|t| t.completed_at))),
            (13, "OACK QUEUE US", age(gates.queued_at)),
            (14, "WRITER PASSES", gates.passes),
            (15, "WRITES COMPLETE", self.server_check.completed_count() as u32),
            (16, "COMPLETED PEER", completed.map_or(u32::MAX, |t| t.peer.port)),
            (17, "WLAN RRQ RX", owner.tftp_wire_counts()[2]),
            (18, "PD QUERY STATE", query[1]),
            (19, "WLFW SERVERS", query[7]),
            (20, "IPA OPEN PASSES", gates.ipa_passes),
            (21, "ELIGIBLE PASSES", gates.ready_passes),
            (22, "BLOCK ANY", u32::from(gates.seen)),
            (23, "BLOCK ALL", u32::from(gates.common)),
            (24, "CLOSE BLOCK", gates.close_mask.map_or(u32::MAX, u32::from)),
            (25, "READY CMD", gates.ready_command.map_or(u32::MAX, u32::from)),
            (26, "ACK PUB US", age(ack.and_then(|t| t.published_at))),
            (27, "ACK DONE US", age(ack.and_then(|t| t.done_at))),
            (28, "IPA COMPLETE", u32::from(self.ipa_complete_seen)),
        ] { display.hex(row, label, value); }
    }

    #[cfg(feature = "sunfish-wlan-mux-elf-phases")]
    fn show_mux_elf_phases(&self, display: &Display, owner: &ModemOwner) {
        let trace = owner.service_registry_trace();
        let elf = trace.elf;
        let received = elf.and_then(|t| t.received);
        let queued = elf.and_then(|t| t.queued);
        let published = elf.and_then(|t| t.published);
        let pass_end = elf.and_then(|t| t.dispatch.receive_pass_end);
        let entered = elf.and_then(|t| t.dispatch.registry_entered);
        let selected = elf.and_then(|t| t.dispatch.selected);
        let blocker = elf.and_then(|t| t.blocker);
        let turns = elf.map(|t| t.dispatch.turns).unwrap_or_default();
        let gap = self.registry_gap.longest.as_ref();
        let query = self.wlan_query.snapshot(owner);
        let ms = |start: Option<u64>, end: Option<u64>| start.zip(end).and_then(|(a, b): (u64, u64)| b.checked_sub(a))
            .map_or(u32::MAX, |ticks| diagnostic::ticks_to_ms(ticks, self.hz));
        let post = trace.elf_exchange.and_then(|t| t.first_tftp_request);

        display.replace_line(1, "WLAN MUX ELF PHASES");
        for (row, label, value) in [
            (2, "RUNTIME ERROR", self.error),
            (3, "ERROR DETAIL", self.detail),
            (4, "RX TO PUB MS", ms(received, published)),
            (5, "RX PASS MS", ms(received, pass_end)),
            (6, "PASS TO REG MS", ms(pass_end, entered)),
            (7, "REG TO ELF MS", ms(entered, selected)),
            (8, "ELF ENQUEUE MS", ms(selected, queued)),
            (9, "QUEUE TO PUB MS", ms(queued, published)),
            (10, "PUB TO DONE MS", ms(published, elf.and_then(|t| t.done))),
            (11, "PRESEL RUN 0", turns.runs[0]),
            (12, "PRESEL RUN 1", turns.runs[1]),
            (13, "PRESEL CALLS", turns.calls),
            (14, "TX4 PEER WAITS", trace.txn4.map_or(u32::MAX, |t| t.waits_by_reason[1])),
            (15, "ELF PEER WAITS", elf.map_or(u32::MAX, |t| t.waits_by_reason[1])),
            (16, "BLOCK TXN", blocker.map_or(u32::MAX, |b| u32::from(b.transaction))),
            (17, "BLOCK WAIT MS", ms(blocker.and_then(|b| b.wait), blocker.and_then(|b| b.done))),
            (18, "DONE TO Q MS", ms(blocker.and_then(|b| b.done), queued)),
            (19, "REG GAP MS", gap.map_or(u32::MAX, |g| diagnostic::ticks_to_ms(g.elapsed, self.hz))),
            (20, "GAP ROUTER MS", gap.map_or(u32::MAX, |g| diagnostic::ticks_to_ms(g.work[0], self.hz))),
            (21, "RX PUB IO MS", elf.map_or(u32::MAX, |t| {
                let io = diagnostic::elf_io_delta(t.io.received, t.io.published);
                let ticks = io[..4].iter().copied().fold(0u64, u64::saturating_add);
                if ticks == u64::MAX { u32::MAX } else { diagnostic::ticks_to_ms(ticks, self.hz) }
            })),
            (22, "IPA COMPLETE", u32::from(self.ipa_complete_seen)),
            (23, "WRITES COMPLETE", self.server_check.completed_count() as u32),
            (24, "POST ELF PEER", post.map_or(u32::MAX, |p| p.source_port)),
            (26, "WLAN RRQ RX", owner.tftp_wire_counts()[2]),
            (27, "PD QUERY STATE", query[1]),
            (28, "WLFW SERVERS", query[7]),
        ] { display.hex(row, label, value); }
        display.replace_line(25, post.map_or("AWAITING POST ELF TFTP", |p| p.path_kind));
    }

    #[cfg(feature = "sunfish-wlan-mux-startup-panel")]
    fn show_mux_startup(&self, display: &Display, owner: &ModemOwner) {
        let registry = owner.service_registry_trace();
        let elf = registry.elf;
        let exchange = registry.elf_exchange;
        let post = exchange.and_then(|trace| trace.first_tftp_request);
        let ack = owner.tftp_final_ack_trace();
        let (wlan_rrq, wlfw) = owner.wlan_discovery_times();
        let query = self.wlan_query.snapshot(owner);
        let [wrq, rrq, wlan, _] = owner.tftp_wire_counts();
        let age = |event| self.time_from_us(Some(self.runtime_started), event);

        display.replace_line(1, "WLAN MUX STARTUP");
        for (row, label, value) in [
            (2, "RUNTIME ERROR", self.error),
            (3, "ERROR DETAIL", self.detail),
            (4, "IPA COMPLETE", u32::from(self.ipa_complete_seen)),
            (5, "TFTP WRQ RX", wrq),
            (6, "WRITES COMPLETE", self.server_check.completed_count() as u32),
            (7, "FINAL ACK PEER", ack.map_or(u32::MAX, |t| t.destination.port)),
            (8, "FINAL ACK DONE", age(ack.and_then(|t| t.done_at))),
            (9, "TFTP RRQ RX", rrq),
            (10, "FIRST MISS KIND", u32::from(self.server_check.first_missing_kind())),
            (11, "MISS RSP FLAGS", owner.tftp_missing_reply_trace().map_or(0, |t| t.flags)),
            (12, "LOC QUEUED MASK", registry.queued_names),
            (13, "LOC DONE MASK", registry.completed_names),
            (14, "ELF REQ US", age(elf.and_then(|t| t.received))),
            (15, "ELF PUB US", age(elf.and_then(|t| t.published))),
            (16, "ELF DONE US", age(elf.and_then(|t| t.done))),
            (17, "ELF QMI MATCH", exchange.map_or(u32::MAX, |t| u32::from(t.qmi_matches_reference))),
            (18, "POST ELF OP", post.map_or(u32::MAX, |t| u32::from(t.opcode))),
            (19, "POST ELF PEER", post.map_or(u32::MAX, |t| t.source_port)),
            (21, "WLAN RRQ RX", wlan),
            (22, "WLAN RRQ US", age(wlan_rrq)),
            (23, "FW ACTIVE", self.firmware.active_transfers() as u32),
            (24, "FW PORT NEXT", self.firmware.next_transfer_port()),
            (25, "PD QUERY PHASE", query[0]),
            (26, "PD QUERY STATE", query[1]),
            (27, "WLFW FIRST US", age(wlfw)),
            (28, "WLFW SERVERS", query[7]),
        ] {
            display.hex(row, label, value);
        }
        display.replace_line(20, post.map_or("AWAITING POST ELF TFTP", |t| t.path_kind));
    }

    #[cfg(feature = "sunfish-tftp-oack-resources")]
    fn show_oack_resources(&self, display: &Display, owner: &ModemOwner) {
        use personal_os_rust_server::qcom_glink_qmi::{MAX_DATAGRAM_BYTES, OackIntentObservation};

        #[cfg(feature = "sunfish-first-oack-grant-window")]
        {
            self.show_first_grant_window(display, owner);
            return;
        }
        #[cfg(all(feature = "sunfish-first-oack-after-ipa-publish", not(feature = "sunfish-first-oack-grant-window")))]
        {
            self.show_first_oack_after_ipa_publish(display, owner);
            return;
        }
        #[cfg(all(feature = "sunfish-wlan-mux-elf-phases", not(feature = "sunfish-first-oack-after-ipa-publish")))]
        {
            self.show_mux_elf_phases(display, owner);
            return;
        }
        #[cfg(all(feature = "sunfish-wlan-mux-startup-panel", not(feature = "sunfish-wlan-mux-elf-phases")))]
        {
            self.show_mux_startup(display, owner);
            return;
        }
        #[cfg(all(feature = "sunfish-tftp-reserved-writer-io", not(feature = "sunfish-wlan-mux-startup-panel")))]
        {
            let age = |event| self.time_from_us(Some(self.runtime_started), event);
            let ticks = |value| self.time_from_us(Some(0), Some(value));
            let pair = owner.reserved_oack().filter(|(oack, _)| oack.peer.port == 0x49);
            let reserved = pair.map(|(oack, _)| oack);
            let peer = pair.and_then(|(_, peer)| peer);
            let writer = reserved.map(|p| p.writer_trace).unwrap_or_default();
            let io = owner.reserved_writer_io();
            let broad_total = io.totals[..4].iter().fold(0u64, |a, b| a.saturating_add(*b));
            let query = self.wlan_query.snapshot(owner);
            #[cfg(feature = "sunfish-tftp-wlan-mux")]
            display.replace_line(1, "WLAN TFTP MUX");
            #[cfg(not(feature = "sunfish-tftp-wlan-mux"))]
            display.replace_line(1, "TFTP WRITER IO");
            for (row, label, value) in [
                (2, "RUNTIME ERROR", self.error), (3, "ERROR DETAIL", self.detail),
                (4, "QUEUE US", age(reserved.and_then(|p| p.queued_at))),
                (5, "OACK PUB US", age(reserved.and_then(|p| p.published_at))),
                (6, "PASS COUNT", writer.calls), (7, "WRITER US", ticks(writer.writer_ticks)),
                (8, "IO PROFILED", io.profiled), (9, "IO UNAVAILABLE", io.unavailable),
                (10, "IO INVALID", io.invalid),
                (11, "OBSERVE US", ticks(io.totals[0])), (12, "DESCRIPTOR US", ticks(io.totals[1])),
                (13, "FIFO READ US", ticks(io.totals[2])), (14, "OTHER IO US", ticks(io.totals[3])),
                (15, "IO TOTAL US", ticks(broad_total)),
                (17, "FIFO WRITE US", ticks(io.totals[5])), (18, "TX HEAD US", ticks(io.totals[6])),
                (19, "BARRIER US", ticks(io.totals[8])), (20, "NOTIFY US", ticks(io.totals[9])),
                (21, "DOMAIN PRE US", ticks(io.totals[10])), (22, "DOMAIN POST US", ticks(io.totals[11])),
                (23, "RX LENGTH US", ticks(io.totals[4])), (24, "RX TAIL US", ticks(io.totals[7])),
                (25, "MAX GAP US", ticks(writer.max_gap_ticks)),
                (26, "PEER DATA US", age(peer.and_then(|p| p.first_data_at))),
                (27, "WLAN RRQ RX", owner.tftp_wire_counts()[2]), (28, "WLFW SERVERS", query[7]),
            ] { display.hex(row, label, value); }
            #[cfg(feature = "sunfish-tftp-wlan-mux")]
            {
                display.hex(8, "FW ACTIVE", self.firmware.active_transfers() as u32);
                display.hex(9, "FW PORT NEXT", self.firmware.next_transfer_port());
            }
            display.replace_line(16, "NESTED IO DETAIL");
            return;
        }

        #[cfg(feature = "sunfish-tftp-reserved-writer-trace")]
        {
            let age = |event| self.time_from_us(Some(self.runtime_started), event);
            let ticks = |value| self.time_from_us(Some(0), Some(value));
            let pair = owner.reserved_oack().filter(|(oack, _)| oack.peer.port == 0x49);
            let reserved = pair.map(|(oack, _)| oack);
            let peer = pair.and_then(|(_, peer)| peer);
            let trace = reserved.map(|oack| oack.writer_trace).unwrap_or_default();
            let query = self.wlan_query.snapshot(owner);
            display.replace_line(1, "TFTP RESERVED WRITER");
            for (row, label, value) in [
                (2, "RUNTIME ERROR", self.error), (3, "ERROR DETAIL", self.detail),
                (4, "WRQ US", age(peer.and_then(|p| p.wrq_at))),
                (5, "QUEUE US", age(reserved.and_then(|p| p.queued_at))),
                (6, "FIRST PASS US", age(trace.first_at)),
                (7, "WRQ DONE US", age(peer.and_then(|p| p.wrq_done_at))),
                (8, "OACK PUB US", age(reserved.and_then(|p| p.published_at))),
                (9, "PASS COUNT", trace.calls),
                (10, "MAX GAP US", ticks(trace.max_gap_ticks)),
                (11, "ACTIVE US", ticks(trace.active_ticks)),
                (12, "WRITER US", ticks(trace.writer_ticks)),
                (13, "NO FIT", trace.no_fit), (14, "READY", trace.ready),
                (15, "INTENT WAIT", trace.intent_wait), (16, "OLDER FLOW", trace.older_flow),
                (17, "ADVERTISEMENT", trace.advertisements), (18, "INTENT ACK", trace.intent_acks),
                (19, "RX DONE", trace.receive_dones), (20, "OTHER TFTP", trace.other_tftp),
                (21, "OTHER DATA", trace.other_data), (22, "OACK SELECTED", trace.oack_selected),
                (23, "OACK BLOCKED", trace.oack_blocked), (24, "OTHER BLOCKED", trace.other_blocked),
                (25, "FOLLOWUP YIELD", trace.followup_yield), (26, "IDLE REQUEST", (trace.idle.min(0xffff) << 16) | trace.intent_requests.min(0xffff)),
                (27, "WLAN RRQ RX", owner.tftp_wire_counts()[2]), (28, "WLFW SERVERS", query[7]),
            ] { display.hex(row, label, value); }
            return;
        }

        #[cfg(feature = "sunfish-tftp-oack-ahead-headers")]
        {
            let age = |event| self.time_from_us(Some(self.runtime_started), event);
            let peer = owner.server_check_49();
            let reserved = owner.reserved_oack().map(|(oack, _)| oack)
                .filter(|oack| oack.peer.port == 0x49);
            let tail = owner.oack_peer_tail();
            let close = owner.oack_close_fifo();
            let completion = owner.reserved_oack_rx();
            let capture = owner.reserved_oack_ahead();
            let query = self.wlan_query.snapshot(owner);
            let matched_peer = owner.reserved_oack().and_then(|(_, peer)| peer);
            display.replace_line(1, "TFTP OACK DELAY WIRE");
            for (row, label, value) in [
                (2, "RUNTIME ERROR", self.error),
                (3, "ERROR DETAIL", self.detail),
                (4, "PEER PORT", reserved.map_or(u32::MAX, |p| p.peer.port)),
                (5, "TRANSFER PORT", reserved.map_or(u32::MAX, |p| p.transfer_port)),
                (6, "WRQ US", age(matched_peer.and_then(|p| p.wrq_at))),
                (7, "WRQ DONE US", age(matched_peer.and_then(|p| p.wrq_done_at))),
                (8, "OACK QUEUE US", age(reserved.and_then(|p| p.queued_at))),
                (9, "OACK PUB US", age(reserved.and_then(|p| p.published_at))),
                (10, "TAIL CROSSED US", age(tail.and_then(|p| p.first_crossed))),
                (11, "CLOSE SEEN US", age(close.and_then(|p| p.first_visible))),
                (12, "CLOSE DONE US", age(matched_peer.and_then(|p| p.first_close_at))),
                (13, "DONE SEEN US", age(completion.and_then(|p| p.first_visible))),
                (14, "DONE TAKEN US", age(reserved.and_then(|p| p.done_at))),
                (15, "PEER DATA US", age(matched_peer.and_then(|p| p.first_data_at))),
                (16, "WIRE BYTES", reserved.map_or(u32::MAX, |p| p.wire_len)),
                (17, "WIRE INTENT", reserved.map_or(u32::MAX, |p| p.wire_intent)),
                (18, "DONE INTENT", capture.and_then(|p| p.completion_id).unwrap_or(u32::MAX)),
                (19, "WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                (20, "WLFW SERVERS", query[7]),
            ] { display.hex(row, label, value); }
            display.replace_line(21, "QRTR OACK WORDS LE");
            for row in 22..28 {
                if let Some(packet) = reserved.filter(|p| p.wire_len != 0) {
                    let at = (row - 22) * 12;
                    let word = |i: usize| u32::from_le_bytes(packet.wire_bytes[i..i+4].try_into().unwrap());
                    display.header_row(row, false, &[(word(at), 8), (word(at+4), 8), (word(at+8), 8)]);
                    display.header_row(row, true, &[]);
                } else { display.replace_line(row, ""); }
            }
            display.replace_line(28, "");
            return;
        }

        #[cfg(feature = "sunfish-tftp-oack-fifo-outcome")]
        {
            let age = |event| self.time_from_us(Some(self.runtime_started), event);
            let peer = owner.server_check_49();
            let reserved = owner.reserved_oack().map(|(oack, _)| oack)
                .filter(|oack| oack.peer.port == 0x49);
            let tail = owner.oack_peer_tail();
            let close = owner.oack_close_fifo();
            let completion = owner.reserved_oack_rx();
            let query = self.wlan_query.snapshot(owner);
            display.replace_line(1, "TFTP OACK FIFO OUTCOME");
            display.hex(2, "RUNTIME ERROR", self.error);
            display.hex(3, "ERROR DETAIL", self.detail);
            for (row, label, value) in [
                (4, "PEER PORT", peer.map_or(u32::MAX, |item| item.peer.port)),
                (5, "TRANSFER PORT", peer.map_or(u32::MAX, |item| item.transfer_port)),
                (6, "WRQ US", age(peer.and_then(|item| item.wrq_at))),
                (7, "OACK US", age(peer.and_then(|item| item.oack_at))),
                (8, "TAIL BEFORE US", age(tail.and_then(|item| item.last_before))),
                (9, "TAIL CROSSED US", age(tail.and_then(|item| item.first_crossed))),
                (10, "TAIL AT CLOSE", tail.and_then(|item| item.at_close).map_or(u32::MAX, |n| n as u32)),
                (11, "CLOSE ABSENT US", age(close.and_then(|item| item.last_absent))),
                (12, "CLOSE VISIBLE US", age(close.and_then(|item| item.first_visible))),
                (13, "CLOSE AHEAD", close.and_then(|item| item.bytes_ahead).unwrap_or(u32::MAX)),
                (14, "CLOSE PROCESSED US", age(peer.and_then(|item| item.first_close_at))),
                (15, "DATA US", age(peer.and_then(|item| item.first_data_at))),
                (16, "DONE ABSENT US", age(completion.and_then(|item| item.last_absent))),
                (17, "DONE VISIBLE US", age(completion.and_then(|item| item.first_visible))),
                (18, "DONE AHEAD", completion.and_then(|item| item.bytes_ahead).unwrap_or(u32::MAX)),
                (19, "DONE AT CLOSE", completion.and_then(|item| item.visible_at_close).map_or(u32::MAX, u32::from)),
                (20, "DONE PROCESSED US", age(reserved.and_then(|item| item.done_at))),
                (21, "WLAN RRQ RX", owner.tftp_wire_counts()[2]),
                (22, "WLFW SERVERS", query[7]),
                (23, "PD QUERY STATE", query[1]),
                (24, "TFTP DATA RX", owner.tftp_wire_counts()[3]),
            ] {
                display.hex(row, label, value);
            }
            return;
        }

        let peers = [owner.server_check_49(), owner.server_check_oacks()[1]];
        let age = |event| self.time_from_us(Some(self.runtime_started), event);
        #[cfg(feature = "sunfish-tftp-oack-early-reserve")]
        display.replace_line(1, "TFTP EARLY OACK GUARDS");
        #[cfg(all(feature = "sunfish-tftp-oack-close-fifo", not(feature = "sunfish-tftp-oack-early-reserve")))]
        display.replace_line(1, "TFTP OACK CLOSE FIFO");
        #[cfg(all(feature = "sunfish-tftp-oack-peer-tail", not(feature = "sunfish-tftp-oack-close-fifo")))]
        display.replace_line(1, "TFTP OACK PEER TAIL");
        #[cfg(all(feature = "sunfish-tftp-oack-grant-followup", not(feature = "sunfish-tftp-oack-peer-tail")))]
        display.replace_line(1, "TFTP OACK GRANT FOLLOW");
        #[cfg(all(feature = "sunfish-tftp-oack-guard", not(feature = "sunfish-tftp-oack-grant-followup")))]
        display.replace_line(1, "TFTP OACK GUARD RECORD");
        #[cfg(all(feature = "sunfish-tftp-oack-ordinary-ad", not(feature = "sunfish-tftp-oack-guard")))]
        display.replace_line(1, "TFTP OACK ORDINARY AD");
        #[cfg(all(feature = "sunfish-tftp-oack-followup-reason", not(feature = "sunfish-tftp-oack-ordinary-ad")))]
        display.replace_line(1, "TFTP OACK FOLLOWUP WHY");
        #[cfg(all(feature = "sunfish-tftp-oack-followup", not(feature = "sunfish-tftp-oack-followup-reason")))]
        display.replace_line(1, "TFTP OACK FOLLOWUP");
        #[cfg(all(feature = "sunfish-tftp-oack-repeat-turn", not(feature = "sunfish-tftp-oack-followup")))]
        display.replace_line(1, "TFTP OACK REPEAT TURN");
        #[cfg(all(feature = "sunfish-tftp-oack-slot-follow", not(feature = "sunfish-tftp-oack-repeat-turn")))]
        display.replace_line(1, "TFTP OACK SLOT FOLLOW");
        #[cfg(all(feature = "sunfish-tftp-oack-prime-rx", not(feature = "sunfish-tftp-oack-slot-follow")))]
        display.replace_line(1, "TFTP OACK RX TURN");
        #[cfg(not(feature = "sunfish-tftp-oack-prime-rx"))]
        display.replace_line(1, "TFTP OACK RX CREDIT");
        display.hex(2, "RUNTIME ERROR", self.error);
        display.hex(3, "ERROR DETAIL", self.detail);
        display.replace_line(4, "PEER PORT CONF FLOW");
        display.replace_line(6, "OACK DONE WRQDONE CREDIT");
        #[cfg(feature = "sunfish-tftp-oack-prime-rx")]
        display.replace_line(8, "DATA CLOSE CREDIT P D");
        #[cfg(not(feature = "sunfish-tftp-oack-prime-rx"))]
        display.replace_line(8, "DATA CLOSE PEER CREDIT");
        display.replace_line(10, "DATA CLOSE IDS R A OMIT");
        #[cfg(not(feature = "sunfish-tftp-server-inventory"))]
        display.replace_line(12, "ID SIZE FROM US USED US");
        #[cfg(feature = "sunfish-tftp-server-inventory")]
        display.replace_line(12, "PEER TRANSFER WRQ OACK CLOSE");
        for (second, peer) in peers.into_iter().enumerate() {
            let second = second != 0;
            let empty = Default::default();
            let resource = peer.and_then(|trace| owner.oack_resources(trace.transfer_port)).unwrap_or(&empty);
            let publication = peer.and_then(|trace| trace.oack_at);
            let view = |value: Option<u64>| age(value);
            display.header_row(5, second, &[
                (peer.map_or(u32::MAX, |trace| trace.peer.port), 8),
                (peer.map_or(u32::MAX, |trace| trace.transfer_port), 8),
                (peer.map_or(u32::MAX, |trace| u32::from(trace.confirm_requested)), 1),
                (publication.map_or(u32::MAX, |_| u32::from(resource.oack_confirm)), 1),
                (resource.flow_pending.map_or(u32::MAX, u32::from), 2),
            ]);
            display.header_row(7, second, &[
                (view(publication), 8),
                (view(peer.and_then(|trace| trace.oack_done_at)), 8),
                (view(peer.and_then(|trace| trace.wrq_done_at)), 8),
                (view(peer.and_then(|trace| trace.resume_at)), 8),
            ]);
            display.header_row(9, second, &[
                (view(peer.and_then(|trace| trace.first_data_at)), 8),
                (view(peer.and_then(|trace| trace.first_close_at)), 8),
                (view(resource.peer_resume_at), 8),
                #[cfg(feature = "sunfish-tftp-oack-prime-rx")]
                (resource.primed_intent.unwrap_or(u32::MAX), 8),
                #[cfg(feature = "sunfish-tftp-oack-prime-rx")]
                (resource.prime_released.unwrap_or(u32::MAX), 8),
            ]);
            display.header_row(11, second, &[
                (resource.data_intent.unwrap_or(u32::MAX), 8),
                (resource.close_intent.unwrap_or(u32::MAX), 8),
                (resource.peer_resumes, 8),
                (resource.applied_resumes, 8),
                (resource.omitted.saturating_add(if cfg!(feature = "sunfish-tftp-oack-slot-follow") {
                    let shown = if cfg!(feature = "sunfish-tftp-oack-peer-tail") { 0 }
                        else if cfg!(feature = "sunfish-tftp-oack-guard") { 2 } else { 4 };
                    resource.added[shown..].iter().flatten().count() as u32
                } else { 0 }), 8),
            ]);
            #[cfg(not(feature = "sunfish-tftp-server-inventory"))]
            let observations = resource.initial.into_iter().chain(resource.added);
            #[cfg(all(feature = "sunfish-tftp-oack-slot-follow", not(feature = "sunfish-tftp-server-inventory")))]
            let observations = observations.take(if cfg!(feature = "sunfish-tftp-oack-close-fifo") { 6 }
                else if cfg!(feature = "sunfish-tftp-oack-peer-tail") { 8 }
                else if cfg!(feature = "sunfish-tftp-oack-guard") { 10 } else { 12 });
            #[cfg(not(feature = "sunfish-tftp-server-inventory"))]
            for (row, entry) in observations.enumerate() {
                let fields = |entry: Option<OackIntentObservation>| match entry {
                    Some(entry) => [
                        (entry.id, 8), (MAX_DATAGRAM_BYTES as u32, 4),
                        (view(entry.available_at.or(publication)), 8), (view(entry.consumed_at), 8),
                    ],
                    None => [(u32::MAX, 8), (u32::MAX, 4), (u32::MAX, 8), (u32::MAX, 8)],
                };
                display.header_row(13 + row, second, &fields(entry));
            }
        }
        #[cfg(feature = "sunfish-tftp-server-inventory")]
        {
            for row in 0..6 {
                for (second, index) in [(false, row * 2), (true, row * 2 + 1)] {
                    let trace = owner.server_check_inventory().get(index).copied().flatten();
                    display.header_row(13 + row, second, &[
                        (trace.map_or(u32::MAX, |item| item.peer.port), 8),
                        (trace.map_or(u32::MAX, |item| item.transfer_port), 8),
                        (age(trace.and_then(|item| item.wrq_at)), 8),
                        (age(trace.and_then(|item| item.oack_at)), 8),
                        (age(trace.and_then(|item| item.first_close_at)), 8),
                    ]);
                }
            }
        }
        #[cfg(feature = "sunfish-tftp-oack-slot-follow")]
        {
            let trace = owner.oack_slot_follow();
            let packet = trace.and_then(|trace| trace.packet);
            let missing = u32::MAX;
            #[cfg(feature = "sunfish-tftp-oack-close-fifo")]
            {
                let close = owner.oack_close_fifo();
                #[cfg(not(feature = "sunfish-tftp-oack-early-reserve"))]
                display.replace_line(19, "CLOSE ABSENT VISIBLE AHEAD");
                #[cfg(feature = "sunfish-tftp-oack-early-reserve")]
                display.replace_line(19, "QUEUE ST CMD MASK REPEAT");
                display.header_row(20, false, &[
                    (age(close.and_then(|trace| trace.last_absent)), 8),
                    (age(close.and_then(|trace| trace.first_visible)), 8),
                    (close.and_then(|trace| trace.bytes_ahead).unwrap_or(missing), 8),
                ]);
                #[cfg(feature = "sunfish-tftp-oack-early-reserve")]
                {
                    let reserved = owner.reserved_oack().map(|(oack, _)| oack)
                        .filter(|oack| oack.peer.port == 0x49);
                    display.header_row(20, true, &[
                        (age(reserved.and_then(|oack| oack.queued_at)), 8),
                        (reserved.map_or(0, |oack| u32::from(oack.early_status)), 2),
                        (reserved.map_or(0, |oack| u32::from(oack.early_command)), 2),
                        (reserved.map_or(0, |oack| u32::from(oack.early_guards)), 4),
                        (age(owner.server_check_49().and_then(|trace| trace.first_repeat_at)), 8),
                    ]);
                }
            }
            #[cfg(feature = "sunfish-tftp-oack-peer-tail")]
            {
                let tail = owner.oack_peer_tail();
                #[cfg(not(feature = "sunfish-tftp-oack-credit-retry"))]
                display.replace_line(21, "TAIL BEFORE CROSS CLOSE");
                #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
                display.replace_line(21, "RETRY ACK CREDIT ATTEMPT");
                #[cfg(not(feature = "sunfish-tftp-oack-credit-retry"))]
                display.header_row(22, false, &[
                    (age(tail.and_then(|trace| trace.last_before)), 8),
                    (age(tail.and_then(|trace| trace.first_crossed)), 8),
                    (tail.and_then(|trace| trace.at_close).map_or(missing, |bytes| bytes as u32), 8),
                ]);
                #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
                {
                    let reserved = owner.reserved_oack().map(|(oack, _)| oack)
                        .filter(|oack| oack.peer.port == 0x49);
                    display.header_row(22, false, &[
                        (age(reserved.and_then(|oack| oack.retry_ack_at)), 8),
                        (age(reserved.and_then(|oack| oack.retry_credit_at)), 8),
                        (age(reserved.and_then(|oack| oack.retry_attempt_at)), 8),
                    ]);
                    display.header_row(22, true, &[
                        (reserved.map_or(missing, |oack| u32::from(oack.retry_state)), 2),
                        (reserved.map_or(missing, |oack| u32::from(oack.retry_result)), 2),
                        #[cfg(feature = "sunfish-tftp-oack-credit-early")]
                        (reserved.map_or(missing, |oack| u32::from(oack.credit_turn)), 2),
                    ]);
                }
            }
            #[cfg(feature = "sunfish-tftp-oack-guard")]
            {
                let guard = trace.and_then(|trace| trace.advertisement_guard);
                let record = guard.and_then(|guard| guard.first_record);
                #[cfg(not(feature = "sunfish-tftp-oack-credit-retry"))]
                display.replace_line(23, "SLOT TYPE LEN SENT OACK");
                #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
                #[cfg(not(feature = "sunfish-tftp-oack-credit-early"))]
                display.replace_line(23, "CREDIT PASS AD ACK DONE DATA");
                #[cfg(feature = "sunfish-tftp-oack-credit-early")]
                display.replace_line(23, "RETRY CHOICE ID GATES");
                #[cfg(not(feature = "sunfish-tftp-oack-credit-retry"))]
                display.header_row(24, false, &[
                    (record.map_or(missing, |record| u32::from(record.slot)), 2),
                    (record.map_or(missing, |record| u32::from(record.kind)), 2),
                    (record.map_or(missing, |record| record.len), 8),
                    (record.map_or(missing, |record| record.sent), 8),
                    (age(guard.and_then(|guard| guard.oack_at)), 8),
                ]);
                #[cfg(not(feature = "sunfish-tftp-oack-credit-retry"))]
                display.header_row(24, true, &[
                    (guard.map_or(missing, |guard| u32::from(guard.matching)), 2),
                    (guard.map_or(missing, |guard| u32::from(guard.unsent)), 2),
                    (guard.map_or(missing, |guard| u32::from(guard.ready_unsent)), 2),
                    (guard.map_or(missing, |guard| u32::from(guard.grant_pending)), 1),
                    (guard.map_or(missing, |guard| u32::from(guard.peer_closed)), 1),
                    (guard.map_or(missing, |guard| u32::from(guard.allowed)), 1),
                ]);
            }
            #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
            {
                let reserved = owner.reserved_oack().map(|(oack, _)| oack)
                    .filter(|oack| oack.peer.port == 0x49);
                let counts = reserved.map_or([0; 7], |oack| oack.credit_counts);
                #[cfg(not(feature = "sunfish-tftp-oack-credit-early"))]
                display.header_row(24, false, &[
                    (age(reserved.and_then(|oack| oack.credit_pass_at)), 8),
                    (u32::from(counts[0]), 4), (u32::from(counts[1]), 4),
                    (u32::from(counts[2]), 4), (u32::from(counts[4]), 4),
                ]);
                #[cfg(not(feature = "sunfish-tftp-oack-credit-early"))]
                display.header_row(24, true, &[
                    (u32::from(counts[3]), 4), (u32::from(counts[5]), 4),
                    (u32::from(counts[6]), 4),
                ]);
                #[cfg(feature = "sunfish-tftp-oack-credit-early")]
                display.header_row(24, false, &[
                    (reserved.map_or(missing, |oack| u32::from(oack.retry_choice)), 2),
                    (reserved.map_or(missing, |oack| oack.retry_choice_id), 8),
                    (reserved.map_or(missing, |oack| oack.retry_gates), 8),
                ]);
                #[cfg(not(feature = "sunfish-tftp-server-inventory"))]
                display.replace_line(25, "CREDIT IDLE BLOCKED");
                #[cfg(feature = "sunfish-tftp-server-inventory")]
                #[cfg(not(feature = "sunfish-tftp-oack-after-rxdone"))]
                display.replace_line(25, "RESERVED PEER PORT TOTAL");
                #[cfg(feature = "sunfish-tftp-oack-after-rxdone")]
                display.replace_line(25, "DONE RETRY STATE RESULT");
                #[cfg(feature = "sunfish-tftp-server-inventory")]
                {
                    let reserved = owner.reserved_oack().map(|(oack, _)| oack);
                    #[cfg(not(feature = "sunfish-tftp-oack-after-rxdone"))]
                    display.header_row(26, false, &[
                        (reserved.map_or(missing, |oack| oack.peer.port), 8),
                        (reserved.map_or(missing, |oack| oack.transfer_port), 8),
                        (owner.server_check_inventory().iter().flatten().count() as u32, 4),
                    ]);
                    #[cfg(feature = "sunfish-tftp-oack-after-rxdone")]
                    {
                        display.header_row(26, false, &[
                            (age(reserved.and_then(|oack| oack.after_done_at)), 8),
                            (age(reserved.and_then(|oack| oack.after_done_retry_at)), 8),
                            (reserved.map_or(missing, |oack| u32::from(oack.after_done_state)), 2),
                            (reserved.map_or(missing, |oack| u32::from(oack.after_done_result)), 2),
                            #[cfg(feature = "sunfish-tftp-after-done-grant-turn")]
                            (reserved.map_or(missing, |oack| u32::from(oack.after_done_ad)), 2),
                            #[cfg(feature = "sunfish-tftp-after-done-grant-turn")]
                            (reserved.map_or(missing, |oack| u32::from(oack.after_done_ack)), 2),
                        ]);
                        display.header_row(26, true, &[
                            (reserved.map_or(missing, |oack| oack.peer.port), 8),
                            (reserved.map_or(missing, |oack| oack.transfer_port), 8),
                            (owner.server_check_inventory().iter().flatten().count() as u32, 4),
                        ]);
                    }
                }
            }
            #[cfg(not(feature = "sunfish-tftp-oack-credit-retry"))]
            display.replace_line(25, "SLOT OLD NEW MODE US");
            #[cfg(not(feature = "sunfish-tftp-server-inventory"))]
            display.header_row(26, false, &[
                (trace.map_or(missing, |trace| trace.slot as u32), 2),
                (trace.map_or(missing, |trace| trace.released), 8),
                (trace.and_then(|trace| trace.next_intent).unwrap_or(missing), 8),
                (trace.map_or(missing, |trace| trace.availability_kind), 2),
                (age(trace.and_then(|trace| trace.released_at)), 8),
                (age(trace.and_then(|trace| trace.available_at)), 8),
            ]);
            #[cfg(not(feature = "sunfish-tftp-server-inventory"))]
            display.header_row(26, true, &[
                (age(trace.and_then(|trace| trace.started_at)), 8),
                (age(trace.and_then(|trace| trace.received_at)), 8),
                (packet.map_or(missing, |packet| packet.bytes), 8),
                (packet.map_or(missing, |packet| packet.version), 2),
                (packet.and_then(|packet| packet.kind).unwrap_or(missing), 8),
            ]);
            #[cfg(all(feature = "sunfish-tftp-oack-repeat-turn", not(feature = "sunfish-tftp-after-done-trace")))]
            display.replace_line(27, "SRC DST OP RXDONE US");
            #[cfg(feature = "sunfish-tftp-after-done-trace")]
            display.replace_line(27, "FOLLOWUP CHOICE ID GATES");
            #[cfg(feature = "sunfish-tftp-after-done-trace")]
            {
                let oack = owner.reserved_oack().map(|(oack, _)| oack)
                    .filter(|oack| oack.peer.port == 0x49);
                display.header_row(28, false, &[
                    (oack.map_or(missing, |oack| u32::from(oack.after_done_choice)), 2),
                    (oack.map_or(missing, |oack| oack.after_done_choice_id), 8),
                    (oack.map_or(missing, |oack| oack.after_done_gates), 8),
                    (oack.map_or(missing, |oack| u32::from(oack.after_done_origin)), 2),
                ]);
                display.header_row(28, true, &[
                    (age(oack.and_then(|oack| oack.after_done_first_check)), 8),
                    (age(oack.and_then(|oack| oack.after_done_grant_at)), 8),
                    (oack.map_or(missing, |oack| oack.after_done_pending_checks), 8),
                ]);
            }
            #[cfg(not(feature = "sunfish-tftp-oack-repeat-turn"))]
            display.replace_line(27, "SRC NODE PORT DST OP");
            let source = packet.and_then(|packet| packet.source);
            let destination = packet.and_then(|packet| packet.destination);
            #[cfg(not(feature = "sunfish-tftp-after-done-trace"))]
            display.header_row(28, false, &[
                (source.map_or(missing, |address| address.node), 8),
                (source.map_or(missing, |address| address.port), 8),
                (destination.map_or(missing, |address| address.node), 8),
                (destination.map_or(missing, |address| address.port), 8),
                (packet.and_then(|packet| packet.opcode).unwrap_or(missing), 8),
                #[cfg(feature = "sunfish-tftp-oack-repeat-turn")]
                (age(trace.and_then(|trace| trace.exact_repeat_done_at)), 8),
            ]);
            #[cfg(all(feature = "sunfish-tftp-oack-followup", not(feature = "sunfish-tftp-after-done-trace")))]
            display.header_row(28, true, &[
                (trace.map_or(0, |trace| trace.followup_ad), 2),
                #[cfg(feature = "sunfish-tftp-oack-grant-followup")]
                (trace.map_or(0, |trace| trace.followup_grant), 2),
                (trace.map_or(0, |trace| trace.followup_reply), 2),
                #[cfg(feature = "sunfish-tftp-oack-followup-reason")]
                (trace.map_or(0, |trace| trace.followup_choice), 2),
                #[cfg(feature = "sunfish-tftp-oack-followup-reason")]
                (trace.and_then(|trace| trace.followup_candidate).unwrap_or(missing), 8),
                #[cfg(feature = "sunfish-tftp-oack-followup-reason")]
                (trace.and_then(|trace| trace.followup_selected).unwrap_or(missing), 8),
                #[cfg(feature = "sunfish-tftp-oack-followup-reason")]
                (trace.map_or(0, |trace| trace.followup_gates), 8),
                #[cfg(feature = "sunfish-tftp-oack-followup-reason")]
                (trace.map_or(0, |trace| trace.followup_slots), 8),
            ]);
        }
    }

    #[cfg(all(feature = "sunfish-wlan-tftp-oack-compare", not(feature = "sunfish-wlan-first-wrq-window")))]
    fn show_tftp_oack_comparison(&self, display: &Display, owner: &ModemOwner) {
        let [first, completed] = owner.server_check_oacks();
        let fields = |trace: Option<personal_os_rust_server::qcom_glink_qmi::ServerCheckOackTrace>| {
            let Some(trace) = trace else { return [u32::MAX; 11]; };
            let age = |event| self.time_from_us(trace.wrq_at, event);
            [
                trace.peer.port,
                trace.transfer_port,
                u32::from(trace.confirm_requested),
                trace.ready_at_oack.map_or(u32::MAX, u32::from),
                age(trace.wrq_done_at),
                #[cfg(not(feature = "sunfish-wlan-first-wrq-reuse"))]
                age(trace.resume_at),
                #[cfg(feature = "sunfish-wlan-first-wrq-reuse")]
                u32::from(trace.wrq_reused),
                age(trace.oack_at),
                age(trace.oack_done_at),
                age(trace.first_data_at),
                age(trace.first_repeat_at),
                age(trace.first_close_at),
            ]
        };
        let query = self.wlan_query.snapshot(owner);
        #[cfg(not(feature = "sunfish-wlan-first-wrq-rxdone-turn"))]
        display.replace_line(1, "TFTP OACK COMPARE");
        #[cfg(all(feature = "sunfish-wlan-first-wrq-rxdone-turn", not(feature = "sunfish-wlan-first-oack-rx-fifo")))]
        display.replace_line(1, "TFTP WRQ RXDONE");
        #[cfg(all(feature = "sunfish-wlan-first-oack-rx-fifo", not(feature = "sunfish-wlan-first-wrq-reuse")))]
        display.replace_line(1, "TFTP OACK RX FIFO");
        #[cfg(feature = "sunfish-wlan-first-wrq-reuse")]
        display.replace_line(1, "TFTP WRQ REUSE");
        display.hex(2, "RUNTIME ERROR", self.error);
        display.hex(3, "ERROR DETAIL", self.detail);
        for (i, (label, value)) in [
            "FIRST PEER", "FIRST XFER PORT", "FIRST CONFIRM", "FIRST RX READY",
            "FIRST RXDONE US",
            #[cfg(not(feature = "sunfish-wlan-first-wrq-reuse"))]
            "FIRST RESUME US",
            #[cfg(feature = "sunfish-wlan-first-wrq-reuse")]
            "FIRST WRQ REUSE",
            "FIRST OACK US", "FIRST DONE US",
            "FIRST DATA US", "FIRST RETRY US", "FIRST CLOSE US",
        ].into_iter().zip(fields(first)).enumerate() {
            display.hex(4 + i, label, value);
        }
        #[cfg(not(feature = "sunfish-wlan-first-oack-rx-fifo"))]
        for (i, (label, value)) in [
            "DONE PEER", "DONE XFER PORT", "DONE CONFIRM", "DONE RX READY",
            "DONE RXDONE US", "DONE RESUME US", "DONE OACK US", "DONE GLINK US",
            "DONE DATA US",
        ].into_iter().zip(fields(completed)).enumerate() {
            display.hex(15 + i, label, value);
        }
        #[cfg(feature = "sunfish-wlan-first-oack-rx-fifo")]
        {
            let rx = owner.first_oack_rx_done();
            let tail = match owner.service_registry_trace().first_server_check_tail {
                qcom_glink_fifo::ServerCheckTail::Consumed(at) => at,
                _ => None,
            };
            let age = |event| self.time_from_us(first.and_then(|trace| trace.wrq_at), event);
            let [peer, _, _, _, _, _, oack, glink, data, _, _] = fields(completed);
            for (i, (label, value)) in [
                ("PEER TAIL US", age(tail)),
                ("RX ABSENT US", age(rx.and_then(|trace| trace.last_absent))),
                ("RX VISIBLE US", age(rx.and_then(|trace| trace.first_visible))),
                ("RX AT CLOSE", rx.and_then(|trace| trace.visible_at_close).map_or(u32::MAX, u32::from)),
                ("RX BYTES AHEAD", rx.and_then(|trace| trace.bytes_ahead).unwrap_or(u32::MAX)),
                ("DONE PEER", peer), ("DONE OACK US", oack), ("DONE GLINK US", glink),
                ("DONE DATA US", data),
            ].into_iter().enumerate() {
                display.hex(15 + i, label, value);
            }
        }
        let ack = owner.tftp_final_ack_trace().filter(|ack|
            completed.is_some_and(|trace| trace.peer == ack.destination && trace.transfer_port == ack.source.port));
        display.hex(24, "FINAL ACK FLAGS", ack.map_or(u32::MAX, |ack| ack.flags));
        #[cfg(not(feature = "sunfish-wlan-first-wrq-rxdone-turn"))]
        display.hex(25, "WRITES COMPLETE", self.server_check.completed_count() as u32);
        #[cfg(feature = "sunfish-wlan-first-wrq-rxdone-turn")]
        {
            display.hex(25, "RXDONE PRIORITY", first.map_or(u32::MAX, |trace| u32::from(trace.wrq_priority_published)));
            if self.error == 0 {
                let first_ack = ack.filter(|ack| first.is_some_and(|trace|
                    trace.peer == ack.destination && trace.transfer_port == ack.source.port));
                let elf_done = owner.service_registry_trace().elf.and_then(|elf| elf.done);
                let before = first_ack.and_then(|ack| ack.done_at).zip(elf_done)
                    .map_or(u32::MAX, |(ack_done, elf_done)| u32::from(ack_done < elf_done));
                display.hex(3, "ACK BEFORE ELF", before);
            }
        }
        display.hex(26, "PD QUERY STATE", query[1]);
        display.hex(27, "WLAN RRQ RX", owner.tftp_wire_counts()[2]);
        display.hex(28, "WLFW SERVERS", query[7]);
    }

    #[cfg(all(feature = "sunfish-wlan-first-server-check-trace", not(feature = "sunfish-wlan-elf-tftp-timeline")))]
    fn show_first_server_check(&self, display: &Display, owner: &ModemOwner) {
        let trace = owner.service_registry_trace();
        display.replace_line(1, "TFTP FIRST WRQ");
        #[cfg(feature = "sunfish-wlan-first-server-check-tail")]
        display.replace_line(1, "TFTP FIRST TAIL");
        #[cfg(feature = "sunfish-wlan-first-server-check-repeat")]
        display.replace_line(1, "TFTP WRQ REPEAT");
        #[cfg(feature = "sunfish-wlan-first-server-check-retry")]
        display.replace_line(1, "TFTP OACK RETRY");
        #[cfg(feature = "sunfish-wlan-first-server-check-retry-trace")]
        display.replace_line(1, "TFTP RETRY TX");
        #[cfg(feature = "sunfish-wlan-first-server-check-early-turn")]
        display.replace_line(1, "TFTP FIRST OACK");
        let Some(write) = trace.first_server_check else {
            display.replace_line(4, "AWAITING SERVER CHECK");
            return;
        };
        let elapsed_us = |end| self.time_from_us(write.received_at, end);
        let timing = write.timing;
        let data = timing.peer_rx.data.first;
        let error = timing.peer_rx.error.first;
        #[cfg(feature = "sunfish-wlan-tftp-progress-display")]
        let (data_label, write_label, ack_label) = ("FIRST DATA RX", "FIRST WR DONE", "FIRST ACK PHASE");
        #[cfg(not(feature = "sunfish-wlan-tftp-progress-display"))]
        let (data_label, write_label, ack_label) = ("DATA COUNT", "WRITE COMPLETE", "FINAL ACK PHASE");
        if self.error == 0 {
            display.hex(2, "WRQ RESULT", write.wrq_result.unwrap_or(u32::MAX));
            #[cfg(not(feature = "sunfish-wlan-first-server-check-retry"))]
            display.hex(3, "PRE ELF", u32::from(write.before_elf));
            #[cfg(feature = "sunfish-wlan-first-server-check-retry")]
            display.hex(3, "RETRY FLAGS", u32::from(write.repeat.is_some_and(|repeat| repeat.overlapped))
                | (u32::from(self.router.retry_oack_turn()) << 1)
                | (u32::from(write.repeat.is_some_and(|repeat| repeat.cancelled)) << 2)
                | (u32::from(write.before_elf) << 3));
        } else {
            display.hex(2, "RUNTIME ERROR", self.error);
            display.hex(3, "ERROR DETAIL", self.detail);
        }
        display.hex(4, "FIRST WRQ PEER", write.peer.port);
        display.hex(5, "TRANSFER PORT", write.transfer_port.unwrap_or(u32::MAX));
        display.hex(6, "OACK PHASE", u32::from(write.oack.phase));
        display.hex(7, "OACK QUEUE US", elapsed_us(timing.queued_at));
        display.hex(8, "OACK PUB US", elapsed_us(write.oack.published_at));
        display.hex(9, "OACK DONE US", elapsed_us(write.oack.done_at));
        display.hex(10, data_label, timing.peer_rx.data.count);
        display.hex(11, "DATA DST PORT", data.map_or(u32::MAX, |packet| packet.destination_port));
        display.hex(12, "DATA LEN BLOCK", data.map_or(u32::MAX, |packet|
            (u32::from(packet.body_bytes) << 16) | u32::from(packet.block_or_error.unwrap_or(u16::MAX))));
        display.hex(13, "DATA AGE US", elapsed_us(data.and_then(|packet| packet.received_at)));
        display.hex(14, "DATA RESULT", write.data.and_then(|packet| packet.result).unwrap_or(u32::MAX));
        display.hex(15, write_label, u32::from(write.final_block.is_some()));
        display.hex(16, ack_label, u32::from(write.ack.phase));
        display.hex(17, "ACK DONE US", elapsed_us(write.ack.done_at));
        display.hex(18, "ERROR COUNT", timing.peer_rx.error.count);
        display.hex(19, "ERROR LEN CODE", error.map_or(u32::MAX, |packet|
            (u32::from(packet.body_bytes) << 16) | u32::from(packet.block_or_error.unwrap_or(u16::MAX))));
        display.hex(20, "ERROR AGE US", elapsed_us(error.and_then(|packet| packet.received_at)));
        display.hex(21, "WRQ REPEATS", timing.repeats);
        display.hex(22, "CLOSE AGE US", elapsed_us(timing.first_close_at));
        display.hex(23, "CLOSE OACK PH", timing.first_close_oack_phase.map_or(u32::MAX, u32::from));
        display.hex(24, "ELF DONE US", elapsed_us(trace.elf.and_then(|elf| elf.done)));
        let post = trace.elf_exchange.and_then(|exchange| exchange.server_check);
        display.hex(25, "POST WRQ PEER", post.map_or(u32::MAX, |write| write.peer.port));
        display.hex(26, "POST OACK PH", post.map_or(u32::MAX, |write| u32::from(write.oack.phase)));
        #[cfg(not(feature = "sunfish-wlan-tftp-progress-display"))]
        display.hex(27, "WLAN RRQ RX", owner.tftp_wire_counts()[2]);
        #[cfg(all(feature = "sunfish-wlan-state-query", not(feature = "sunfish-wlan-tftp-progress-display")))]
        display.hex(28, "WLFW SERVERS", self.wlan_query.snapshot(owner)[7]);
        #[cfg(feature = "sunfish-wlan-first-server-check-tail")]
        {
            use qcom_glink_fifo::ServerCheckTail;

            let (remaining, consumed_at) = match trace.first_server_check_tail {
                ServerCheckTail::Untracked => (u32::MAX, None),
                ServerCheckTail::Pending(bytes) => (bytes as u32, None),
                ServerCheckTail::Consumed(at) => (0, at),
            };
            display.hex(17, "PEER TAIL US", elapsed_us(consumed_at));
            display.hex(19, "TAIL BYTES LEFT", remaining);
            display.hex(20, "REPEAT AGE US", elapsed_us(timing.first_repeat_at));
        }
        #[cfg(feature = "sunfish-wlan-first-server-check-repeat")]
        {
            let repeat = write.repeat;
            display.hex(7, "RPT DONE US", elapsed_us(repeat.and_then(|trace| trace.oack.done_at)));
            display.hex(11, "REPEAT COALESCE", repeat.map_or(u32::MAX, |trace| u32::from(trace.coalesced)));
            display.hex(12, "REPEAT DISP US", elapsed_us(repeat.and_then(|trace| trace.dispatch_at)));
            display.hex(13, "REPEAT RESULT", repeat.and_then(|trace| trace.result).unwrap_or(u32::MAX));
            display.hex(14, "REPEAT OACK PH", repeat.map_or(u32::MAX, |trace| u32::from(trace.oack.phase)));
            display.hex(18, "RPT NO CAP", repeat.map_or(0, |trace| trace.no_capacity));
            display.hex(19, "REPEAT FLOWBUSY", repeat.map_or(0, |trace| trace.flow_busy));
            display.hex(24, "RPT CLOSE PH", repeat.and_then(|trace| trace.close_phase).map_or(u32::MAX, u32::from));
            display.hex(25, "RPT QUEUE US", elapsed_us(repeat.and_then(|trace| trace.queued_at)));
            display.hex(26, "RPT PUB US", elapsed_us(repeat.and_then(|trace| trace.oack.published_at)));
        }
        #[cfg(feature = "sunfish-wlan-first-server-check-retry-trace")]
        {
            let tx = write.repeat.map(|repeat| repeat.tx).unwrap_or_default();
            display.hex(11, "RPT TURN US", elapsed_us(tx.turn_at));
            display.hex(12, "RPT RX END US", elapsed_us(tx.first_poll_end));
            display.hex(13, "RPT TX US", elapsed_us(tx.first_call));
            display.hex(18, "RPT TX CALLS", tx.calls);
            display.hex(19, "RPT TX FIRST", tx.first.unwrap_or(u32::MAX));
            display.hex(21, "RPT TX LAST", tx.last.unwrap_or(u32::MAX));
            display.hex(23, "RPT TX SEEN", tx.seen);
        }
        #[cfg(feature = "sunfish-wlan-first-server-check-early-turn")]
        {
            let (start, end) = self.router.first_oack_turn().unzip();
            display.hex(7, "OACK QUEUE US", elapsed_us(timing.queued_at));
            display.hex(11, "OACK TURN US", elapsed_us(start));
            display.hex(12, "OACK TURN END", elapsed_us(end));
            display.hex(13, "DATA AGE US", elapsed_us(write.data.and_then(|data| data.received_at)));
            display.hex(14, "DATA RESULT", write.data.and_then(|data| data.result).unwrap_or(u32::MAX));
            display.hex(18, "ERROR COUNT", timing.peer_rx.error.count);
            display.hex(21, "WRQ REPEATS", timing.repeats);
            display.hex(23, "CLOSE OACK PH", timing.first_close_oack_phase.map_or(u32::MAX, u32::from));
        }
    }
}

fn counter() -> u64 {
    let value;
    unsafe {
        core::arch::asm!("isb", "mrs {value}, cntpct_el0", value = out(reg) value,
            options(nostack, preserves_flags));
    }
    value
}

fn badge(channels: sel4_microkit::ChannelSet) -> u64 {
    let mut result = 0u64;
    for channel in channels.iter() {
        result |= 1u64 << channel.index();
    }
    result
}

#[cfg(not(feature="sunfish-runtime-handoff-stack-split"))]
pub(super) fn start(display: &Display, state: &mut Startup) {
    if state._mpss_runtime.is_some() || !pixel_mpss_auth::runtime_ready(state) {
        return;
    }
    let parts = match pixel_mpss_auth::take_runtime(state) {
        Ok(parts) => parts,
        Err(_) => {
            pixel_mpss_auth::show(display, state);
            return;
        }
    };
    state._mpss_runtime = Some(RuntimeDomain::new(parts));
    let mut runtime = state._mpss_runtime.take().unwrap();
    runtime.begin(display, state);
    state._mpss_runtime = Some(runtime);
}

// Keep large construction temporaries out of the frame that begins GLINK and
// services. The init-time path still holds Startup's initialization frame.
#[cfg(feature="sunfish-runtime-handoff-stack-split")]
#[inline(never)]
fn construct_runtime(display: &Display, state: &mut Startup) -> bool {
    let parts = match pixel_mpss_auth::take_runtime(state) {
        Ok(parts) => parts,
        Err(_) => {
            pixel_mpss_auth::show(display, state);
            return false;
        }
    };
    state._mpss_runtime = Some(RuntimeDomain::new(parts));
    true
}

#[cfg(feature="sunfish-runtime-handoff-stack-split")]
#[inline(never)]
fn begin_prepared_runtime(display: &Display, state: &mut Startup) {
    display.hex(2,"HANDOFF STEP",3);
    display.flush();
    let mut runtime = state._mpss_runtime.take().unwrap();
    runtime.begin(display, state);
    state._mpss_runtime = Some(runtime);
}

#[cfg(feature="sunfish-runtime-handoff-stack-split")]
#[inline(never)]
pub(super) fn start(display: &Display, state: &mut Startup) {
    if state._mpss_runtime.is_some() || !pixel_mpss_auth::runtime_ready(state) {
        return;
    }
    display.replace_line(1,"RUNTIME HANDOFF START");
    display.hex(2,"HANDOFF STEP",1);
    display.flush();
    if !construct_runtime(display,state) {return;}
    display.hex(2,"HANDOFF STEP",2);
    display.flush();
    begin_prepared_runtime(display,state);
}

pub(super) fn notified(state: &mut Startup, channels: sel4_microkit::ChannelSet) {
    let Some(display_pointer) = state._irq_registers.map(|r| r.pixels) else {
        if let Some(mut runtime) = state._mpss_runtime.take() {
            runtime.fail(state, 0x80);
            state._mpss_runtime = Some(runtime);
        }
        return;
    };
    let display = unsafe { Display::new(display_pointer) };
    let Some(mut runtime) = state._mpss_runtime.take() else {
        return;
    };
    runtime.notified(&display, state, badge(channels));
    state._mpss_runtime = Some(runtime);
}
