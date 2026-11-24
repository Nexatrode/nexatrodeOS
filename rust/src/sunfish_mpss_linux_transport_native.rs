//! Own the checked SMMU gate for the live transport.
//! No raw mapping, cloned native writer or status-as-authority constructor.
#![forbid(unsafe_code)]
use crate::{
    qcom_glink_fifo as fifo,
    qcom_glink_qmi::Session,
    sunfish_mpss_linux_loader::AuthReturned,
    sunfish_mpss_linux_smp2p as smp,
    sunfish_mpss_live_domain::{GateError, NativeGate},
    sunfish_smmu_native::{self as domain, Native},
};
use core::mem::ManuallyDrop;
#[cfg(feature = "sunfish-native-startup-mpss-runtime")]
#[path = "sunfish_mpss_io_timing.rs"]
mod io_timing;
#[cfg(feature = "sunfish-native-startup-mpss-runtime")]
pub use io_timing::IoTiming;
/// Low byte is the GLINK failure; the next byte preserves the first
/// controller failure when one exists. Zero in the next byte means absent.
pub(crate) fn encode_glink_diagnostic(error: smp::GlinkError, peer: Option<smp::Error>) -> u32 {
    1 + error as u32 | peer.map_or(0, |value| (1 + value as u32) << 8)
}
type Owner<'a> = smp::Controller<
    crate::sunfish_ipa_smem_native::Window<'a>,
    crate::sunfish_mpss_linux_smem_native::Native<
        'a,
        fn(u64) -> Result<(), crate::sunfish_mpss_linux_smem::Error>,
    >,
    crate::sunfish_mpss_linux_smp2p_native::Platform<'a>,
>;
pub struct Retained<'memory, F: FnMut(usize) -> Result<(), domain::Error>> {
    gate: NativeGate<Native<'memory, F>>,
    #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
    clock: Option<fn() -> u64>,
    #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
    timing: IoTiming,
    #[cfg(all(feature="sunfish-registry-prearm-io-trace",feature="sunfish-native-startup-mpss-runtime"))]
    prearm_timing: io_timing::WindowTiming,
    #[cfg(feature = "sunfish-wrq-validation-path")]
    validation_clock: Option<fn()->u64>,
    #[cfg(feature = "sunfish-wrq-validation-path")]
    validation_samples: [[Option<u64>;4];3],
    #[cfg(feature = "sunfish-wrq-validation-path")]
    validation_count: usize,
    #[cfg(feature="sunfish-wrq-domain-costs")]
    validation_parts:[[fifo::DomainEdges;2];3],
    auth: Option<ManuallyDrop<AuthReturned>>,
    failed: bool,
    diagnostic: u32,
    #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
    crash_evidence: Option<crate::qcom_modem_crash::Evidence>,
}
/// The original prepared session, actual AUTH receipt, and sole checked
/// native gate move into the transport. No borrow into `Startup` survives.
pub fn start_live<'m, F: FnMut(usize) -> Result<(), domain::Error>>(
    gate: NativeGate<Native<'m, F>>,
    auth: AuthReturned,
    session: Session,
    hz: u64,
) -> fifo::BootTransport<Retained<'m, F>> {
    let generation = auth.generation();
    fifo::BootTransport::new(
        Retained {
            gate,
            #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
            clock: None,
            #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
            timing: IoTiming::default(),
            #[cfg(all(feature="sunfish-registry-prearm-io-trace",feature="sunfish-native-startup-mpss-runtime"))]
            prearm_timing: io_timing::WindowTiming::default(),
            #[cfg(feature = "sunfish-wrq-validation-path")] validation_clock:None,
            #[cfg(feature = "sunfish-wrq-validation-path")] validation_samples:[[None;4];3],
            #[cfg(feature = "sunfish-wrq-validation-path")] validation_count:0,
            #[cfg(feature="sunfish-wrq-domain-costs")] validation_parts:[[fifo::DomainEdges::default();2];3],
            auth: Some(ManuallyDrop::new(auth)),
            failed: false,
            diagnostic: 0,
            #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
            crash_evidence: None,
        },
        session,
        generation,
        hz,
    )
}
impl<'m, F: FnMut(usize) -> Result<(), domain::Error>> Retained<'m, F> {
    pub fn with_native<T>(
        &mut self,
        action: impl FnOnce(&mut Native<'m, F>) -> T,
    ) -> Result<T, GateError> {
        if self.failed {
            return Err(GateError::Quarantined);
        }
        self.gate.try_borrow_mut(action)
    }

    pub(crate) fn gate_quarantined(&self) -> bool {
        self.gate.is_quarantined()
    }

    #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
    pub fn owner_timing_clock(&mut self, clock: fn() -> u64) { self.clock = Some(clock); }
    #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
    pub fn owner_timing_enable(&mut self, enabled: bool) { self.timing.enable(enabled); }
    #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
    pub fn owner_timing(&self) -> IoTiming { self.timing }

    fn run_profiled<T>(
        &mut self,
        category: usize,
        detail: Option<usize>,
        action: impl FnOnce(&mut Owner<'m>) -> Result<T, smp::GlinkError>,
    ) -> Result<T, fifo::Fault> {
        #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
        let enabled = self.timing.enabled();
        #[cfg(all(feature="sunfish-registry-prearm-io-trace",feature="sunfish-native-startup-mpss-runtime"))]
        let enabled = enabled || self.prearm_timing.enabled();
        #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
        let started = self.clock.filter(|_| enabled).map(|clock| (clock, clock()));
        #[cfg(not(feature = "sunfish-native-startup-mpss-runtime"))]
        let _ = category;
        #[cfg(not(feature = "sunfish-tftp-publish-op-observer"))]
        let _ = detail;
        #[cfg(feature = "sunfish-tftp-barrier-domain-observer")]
        let domain_clock = if detail == Some(4) && self.timing.enabled() { started.map(|(clock, _)| clock) } else { None };
        let result = self.run(action,
            #[cfg(feature = "sunfish-tftp-barrier-domain-observer")]
            domain_clock,
        );
        #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
        if let Some((clock, started)) = started {
            let elapsed = clock().wrapping_sub(started);
            self.timing.record(category, elapsed);
            #[cfg(feature="sunfish-registry-prearm-io-trace")]
            self.prearm_timing.record(category, elapsed);
            #[cfg(feature = "sunfish-tftp-publish-op-observer")]
            if let Some(detail) = detail { self.timing.record_detail(detail, elapsed); }
        }
        result
    }
    fn run<T>(
        &mut self,
        action: impl FnOnce(&mut Owner<'m>) -> Result<T, smp::GlinkError>,
        #[cfg(feature = "sunfish-tftp-barrier-domain-observer")]
        domain_clock: Option<fn() -> u64>,
    ) -> Result<T, fifo::Fault> {
        if self.failed {
            return Err(fifo::Fault);
        }
        self.failed = true;
        let mut peer_error = None;
        #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
        let mut crash_evidence = None;
        #[cfg(feature = "sunfish-tftp-barrier-domain-observer")]
        let mut samples = [None; 4];
        #[cfg(feature = "sunfish-wrq-validation-path")]
        let validation_clock=self.validation_clock;
        #[cfg(feature = "sunfish-wrq-validation-path")]
        let mut validation_samples=[None;4];
        #[cfg(feature="sunfish-wrq-domain-costs")]
        let mut parts=[fifo::DomainEdges::default();2];
        let result = self.gate.try_borrow_mut(|native| {
            let action = |owner: &mut Owner<'m>| {
                let result = action(owner);
                if result.is_err() {
                    peer_error = owner.report().error;
                    #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
                    { crash_evidence = Some(owner.crash_evidence()); }
                }
                result
            };
            #[cfg(feature = "sunfish-wrq-validation-path")]
            if validation_clock.is_some() {
                #[cfg(feature="sunfish-wrq-domain-costs")]
                let stage_capture=|which:usize,stage:domain::RetainedDomainStage,_index:usize| {
                    use domain::RetainedDomainStage as S;
                    let category=match stage {
                        S::Entry|S::GlobalFaultStatus|S::ContextControl|S::ContextControlValue|S::ContextFaultStatus|S::ContextFaultStatusValue=>0,
                        S::AsidBank=>1,S::RouteSlot=>2,S::Register=>3,S::TableSetup=>4,S::TablePage=>5,S::Complete=>6,
                        _=>return,
                    };
                    if parts[which].needs_stamp(category) && let Some(clock)=validation_clock {parts[which].mark(category,clock());}
                };
                return native.with_glink_observed(action,|index| {
                    if let Some(clock)=domain_clock {samples[index]=Some(clock());}
                    if let Some(clock)=validation_clock {validation_samples[index]=Some(clock());}
                },
                #[cfg(feature="sunfish-wrq-domain-costs")]
                stage_capture,
                );
            }
            #[cfg(feature = "sunfish-tftp-barrier-domain-observer")]
            if let Some(clock) = domain_clock {
                #[cfg(feature="sunfish-wrq-domain-costs")]
                let no_stage=|_,_,_|{};
                return native.with_glink_observed(action, |index| samples[index] = Some(clock()),
                    #[cfg(feature="sunfish-wrq-domain-costs")]
                    no_stage,
                );
            }
            native.with_glink(action)
        });
        #[cfg(feature = "sunfish-tftp-barrier-domain-observer")]
        self.timing.record_barrier_domains(samples);
        #[cfg(feature = "sunfish-wrq-validation-path")]
        if self.validation_clock.is_some() {
            if self.validation_count<3 {
                self.validation_samples[self.validation_count]=validation_samples;
                #[cfg(feature="sunfish-wrq-domain-costs")]
                {self.validation_parts[self.validation_count]=parts;}
            }
            self.validation_count=self.validation_count.saturating_add(1);
        }
        #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
        { if crash_evidence.is_some() { self.crash_evidence = crash_evidence; } }
        match result {
            Ok(Ok(value)) => {
                self.failed = false;
                Ok(value)
            }
            Ok(Err(e)) => {
                self.gate.quarantine();
                self.diagnostic = match e {
                    domain::Error::SharedMemory(
                        crate::sunfish_ipa_shared_memory::Error::Glink(e),
                    ) => encode_glink_diagnostic(e, peer_error),
                    domain::Error::Fault(_) => 0x101,
                    _ => 0x100,
                };
                Err(fifo::Fault)
            }
            Err(GateError::BorrowConflict | GateError::Quarantined) => {
                self.failed = true;
                self.diagnostic = 0x102;
                Err(fifo::Fault)
            }
        }
    }
}
impl<F: FnMut(usize) -> Result<(), domain::Error>> fifo::BootIo for Retained<'_, F> {
    fn fault_boot(&mut self) {
        self.failed = true;
        let _ = self.gate.try_borrow_mut(|native| native.quarantine_glink());
        self.gate.quarantine();
        if self.diagnostic == 0 {
            self.diagnostic = 0x200;
        }
    }
    fn claim_boot(&mut self) -> Result<(), fifo::Fault> {
        let auth = self.auth.take().ok_or(fifo::Fault)?;
        self.run(|c| c.claim_glink(ManuallyDrop::into_inner(auth)),
            #[cfg(feature = "sunfish-tftp-barrier-domain-observer")]
            None,
        )
    }
}
impl<F: FnMut(usize) -> Result<(), domain::Error>> fifo::Io for Retained<'_, F> {
    #[cfg(feature = "sunfish-wlan-elf-phase-io")]
    fn elf_io_totals(&self) -> Option<[u64; 12]> {
        self.clock.map(|_| [
            self.timing.total[0], self.timing.total[1], self.timing.total[2], self.timing.total[3],
            self.timing.detail_total[0], self.timing.detail_total[1], self.timing.detail_total[2],
            self.timing.detail_total[3], self.timing.detail_total[4], self.timing.detail_total[5],
            self.timing.barrier_domain_total[0], self.timing.barrier_domain_total[1],
        ])
    }
    #[cfg(all(feature="sunfish-registry-prearm-io-trace",feature="sunfish-native-startup-mpss-runtime"))]
    fn registry_prearm_timing(&mut self,enabled:bool){if enabled{self.prearm_timing.begin();}else{self.prearm_timing.finish();}}
    #[cfg(all(feature="sunfish-registry-prearm-io-trace",feature="sunfish-native-startup-mpss-runtime"))]
    fn registry_prearm_totals(&self)->Option<[u64;12]>{self.clock.and(self.prearm_timing.snapshot())}
    #[cfg(feature = "sunfish-tftp-reserved-writer-io")]
    fn reserved_io_totals(&self) -> Option<[u64; 12]> {
        self.timing.enabled().then(|| self.elf_io_totals()).flatten()
    }
    #[cfg(feature = "sunfish-wrq-validation-path")]
    fn validation_cost_begin(&mut self, clock:Option<fn()->u64>) {
        self.validation_clock=clock;self.validation_samples=[[None;4];3];self.validation_count=0;
        #[cfg(feature="sunfish-wrq-domain-costs")]
        {self.validation_parts=[[fifo::DomainEdges::default();2];3];}
    }
    #[cfg(feature="sunfish-wrq-domain-costs")]
    fn validation_cost_parts(&self)->Option<[[[u64;7];2];3]> {
        if self.validation_clock.is_none() || self.validation_count!=3 {return None;}
        let mut out=[[[0;7];2];3];
        for (i,groups) in self.validation_parts.iter().enumerate(){for (j,g) in groups.iter().enumerate(){out[i][j]=g.complete()?;}}
        Some(out)
    }
    #[cfg(feature = "sunfish-wrq-validation-path")]
    fn validation_cost_end(&mut self)->Option<[[u64;4];3]> {
        let active=self.validation_clock.take().is_some();
        if !active || self.validation_count!=3 {return None;}
        let mut result=[[0;4];3];
        for (i,group) in self.validation_samples.iter().enumerate() {
            for (j,t) in group.iter().enumerate() {result[i][j]=(*t)?;}
        }
        Some(result)
    }
    fn diagnostic(&self) -> u32 {
        self.diagnostic
    }
    #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
    fn crash_evidence(&self) -> Option<crate::qcom_modem_crash::Evidence> {
        self.crash_evidence
    }
    fn observe(&mut self) -> Result<fifo::Observation, fifo::Fault> {
        self.run_profiled(0, None, |c| {
            let p = c.glink_poll()?;
            let r = c.glink_report().ok_or(smp::GlinkError::Predecessor)?;
            Ok(fifo::Observation {
                generation: r.generation,
                identity: r.identity,
                ready: p != smp::GlinkPoll::Unstable,
                fatal: false,
            })
        })
    }
    fn descriptors(&mut self) -> Result<[u32; 8], fifo::Fault> {
        self.run_profiled(1, None, |c| c.glink_descriptor())
    }
    fn descriptor(&mut self, n: usize) -> Result<u32, fifo::Fault> {
        if n >= 8 {
            self.failed = true;
            return Err(fifo::Fault);
        }
        Ok(self.descriptors()?[n])
    }
    fn rx_length(&mut self) -> Result<Option<usize>, fifo::Fault> {
        self.run_profiled(3, Some(0), |c| {
            let r = c.glink_report().ok_or(smp::GlinkError::Predecessor)?;
            if r.error.is_some() {
                return Err(smp::GlinkError::Consumed);
            }
            Ok(r.rx.map(|i| i.size))
        })
    }
    #[cfg(feature = "sunfish-glink-combined-cursor-sample")]
    fn cursor_snapshot(&mut self) -> Result<(Option<usize>, [u32; 8]), fifo::Fault> {
        // glink_report is local immutable state, with no shared-memory access.
        // Keep its original order before glink_descriptor, but share the latter's
        // full pre/post domain validation. The descriptor action retains its
        // own live-peer, SMEM lock/directory and owned-cursor checks. Barriers,
        // FIFO accesses and publication remain separately checked operations.
        // The whole interval is classified as descriptor I/O by the profiler.
        self.run_profiled(1, None, |c| c.glink_cursor_snapshot())
    }
    fn read_rx(&mut self, at: usize, out: &mut [u8]) -> Result<(), fifo::Fault> {
        self.run_profiled(2, None, |c| c.glink_read_rx(at, out))
    }
    fn read_tx(&mut self, at: usize, out: &mut [u8]) -> Result<(), fifo::Fault> {
        self.run_profiled(2, None, |c| c.glink_read_tx(at, out))
    }
    fn write_tx(&mut self, at: usize, data: &[u8]) -> Result<(), fifo::Fault> {
        self.run_profiled(3, Some(1), |c| c.glink_write_tx(at, data))
    }
    fn store_tx_head(&mut self, v: u32) -> Result<(), fifo::Fault> {
        self.run_profiled(3, Some(2), |c| c.glink_store_tx_head(v))
    }
    fn store_rx_tail(&mut self, v: u32) -> Result<(), fifo::Fault> {
        self.run_profiled(3, Some(3), |c| c.glink_store_rx_tail(v))
    }
    fn barrier(&mut self, _: fifo::Barrier) -> Result<(), fifo::Fault> {
        self.run_profiled(3, Some(4), |c| c.glink_barrier())
    }
    #[cfg(feature = "sunfish-glink-barrier-observe-pair")]
    fn barrier_observe(&mut self, _: fifo::Barrier) -> Result<fifo::Observation, fifo::Fault> {
        // Both existing controller operations retain their own live-peer,
        // lock, directory and cursor checks. The shared outer domain checks
        // bracket this bounded pair, including any existing SMP acknowledgement.
        // No FIFO head/tail or payload store
        // can occur here. The whole interval is profiled as barrier work.
        self.run_profiled(3, Some(4), |c| {
            c.glink_barrier()?;
            let p = c.glink_poll()?;
            let r = c.glink_report().ok_or(smp::GlinkError::Predecessor)?;
            Ok(fifo::Observation {
                generation: r.generation,
                identity: r.identity,
                ready: p != smp::GlinkPoll::Unstable,
                fatal: false,
            })
        })
    }
    fn notify_peer(&mut self) -> Result<(), fifo::Fault> {
        self.run_profiled(3, Some(5), |c| c.glink_notify())
    }
}
