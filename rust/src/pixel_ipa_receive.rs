//! Native binding for the retained receive/UC/GSI owners. Diagnostic RX
//! aggregates are bounded and counted, not delivered as network frames.
use super::*;
use personal_os_rust_server::{
    sunfish_ipa_linux_buffers::Pool, sunfish_ipa_linux_buffers_native::NativeRam,
    sunfish_ipa_linux_command as cmd, sunfish_ipa_linux_config as config,
    sunfish_ipa_linux_endpoint as ep, sunfish_ipa_linux_gsi as gsi, sunfish_ipa_linux_irq as irq,
    sunfish_ipa_linux_receive as rx, sunfish_ipa_linux_receive_dispatch as dispatch,
    sunfish_ipa_linux_receive_native::NativeReceiveRam, sunfish_ipa_linux_receive_pool::DataPool,
    sunfish_ipa_linux_receive_runtime as runtime, sunfish_ipa_linux_receive_start as start,
    sunfish_ipa_linux_table_transfer::TableOnce, sunfish_ipa_linux_transfer as dma,
    sunfish_ipa_linux_uc as uc,
};
#[path = "pixel_ipa_receive_policy.rs"]
mod policy;
type Rings = Pool<NativeRam<'static>>;
type Data = DataPool<NativeReceiveRam<'static>>;
pub(super) type Once = start::ReceiveOnce<NativeRam<'static>, NativeReceiveRam<'static>>;
pub(super) type Runtime = dispatch::Runtime<NativeRam<'static>, NativeReceiveRam<'static>>;
#[derive(Default)]
pub(super) struct Stats {
    cold_route: Option<u32>,
    terminal: bool,
    error: u32,
    reads: u32,
    writes: u32,
    notices: u32,
    acks_gsi: u32,
    acks_ipa: u32,
    last_read: usize,
    last_write: usize,
    last_value: u32,
    badge: u64,
    guard_failure: Option<policy::GuardFailure>,
    config_trace: Option<policy::ConfigTrace>,
    config_last_good_clkon: Option<u32>,
    config_good_snapshots: u32,
    postmodem_pair_accepted: u32,
}
#[derive(Default)]
pub(super) struct Sink {
    aggregates: u32,
    bytes: u64,
    last_length: u32,
    digest: u32,
}
impl runtime::Consumer for Sink {
    fn consume(&mut self, packet: &mut impl runtime::PacketRead) -> Result<(), runtime::Error> {
        let length = packet.completion().length();
        let mut bytes = [0; 256];
        let mut at = 0;
        let mut digest = 2166136261u32;
        while at < length {
            let n = bytes.len().min(length - at);
            packet.read_into(at, &mut bytes[..n])?;
            for byte in &bytes[..n] {
                digest = (digest ^ u32::from(*byte)).wrapping_mul(16777619);
            }
            at += n;
        }
        self.aggregates = self.aggregates.saturating_add(1);
        self.bytes = self.bytes.saturating_add(length as u64);
        self.last_length = length as u32;
        self.digest = digest;
        Ok(())
    }
}
struct Native<'a> {
    registers: IrqRegisters,
    expected: config::Snapshot,
    cold_route: u32,
    post_modem_ready: bool,
    domain: pixel_smmu_install::Access<'a>,
    firmware: &'a pixel_ipa_firmware::NativeLoader,
    power: &'a Report,
    scm: &'a personal_os_rust_server::sunfish_scm_linux_power::HoldOnce,
    setup: &'a gsi::SetupOnce,
    endpoints: &'a ep::ConfigureOnce,
    command: &'a cmd::StartOnce,
    memory: &'a dma::TransferOnce,
    tables: &'a TableOnce,
    stats: &'a mut Stats,
}
fn counter() -> u64 {
    let value;
    unsafe {
        core::arch::asm!("isb","mrs {v}, cntpct_el0",v=out(reg)value,options(nostack,preserves_flags));
    }
    value
}
fn frequency() -> u64 {
    let value;
    unsafe {
        core::arch::asm!("mrs {v}, cntfrq_el0",v=out(reg)value,options(nostack,preserves_flags));
    }
    value
}
fn barrier() {
    unsafe {
        core::arch::asm!("dsb sy", options(nostack, preserves_flags));
    }
}
fn pending() -> u64 {
    #[cfg(feature = "sunfish-native-startup-mpss-auth")]
    {
        return pixel_mpss_auth::receive_badge();
    }
    #[cfg(not(feature = "sunfish-native-startup-mpss-auth"))]
    {
        let (_, badge) =
            sel4::Cap::<sel4::cap_type::Endpoint>::from_bits(1)
                .nb_recv(sel4::Cap::<sel4::cap_type::Reply>::from_bits(4));
        badge
    }
}
#[cfg(feature = "sunfish-native-startup-mpss-auth")]
pub(super) fn delivered_badge(state: &mut Startup, b: u64) {
    badge(state, b);
}
impl Native<'_> {
    fn guard_failure(&mut self, stage: u32, space: u32, offset: u32, observed: u32, expected: u32, mask: u32) -> start::Error {
        policy::first_guard_failure(&mut self.stats.guard_failure,
            policy::GuardFailure { stage, space, offset, observed, expected, mask });
        start::Error::Guard
    }
    fn store(&mut self, space: rx::Space, offset: usize, value: u32) {
        self.stats.writes = self.stats.writes.saturating_add(1);
        self.stats.last_write = offset;
        self.stats.last_value = value;
        let base = if space == rx::Space::Gsi {
            self.registers.gsi
        } else {
            self.registers.ipa
        };
        // SAFETY: the generated capability audit covers these uncached RW/NX
        // apertures. Each caller independently validates a closed request.
        // No raw RAM pointer or host-supplied register is accepted here.
        unsafe {
            core::arch::asm!("dsb sy", options(nostack, preserves_flags));
            base.cast_mut()
                .add(offset / 4)
                .write_volatile(value.to_le());
            core::arch::asm!("dsb sy", options(nostack, preserves_flags));
        }
    }
    fn load(&mut self, space: rx::Space, offset: usize) -> u32 {
        self.stats.reads = self.stats.reads.saturating_add(1);
        self.stats.last_read = offset;
        let value = word(
            if space == rx::Space::Gsi {
                self.registers.gsi
            } else {
                self.registers.ipa
            },
            offset,
        );
        self.stats.last_value = value;
        value
    }
}
impl start::Io<NativeRam<'static>, NativeReceiveRam<'static>> for Native<'_> {
    fn ticks(&mut self) -> u64 {
        counter()
    }
    fn guard(
        &mut self,
        pool: &Rings,
        data: &Data,
        state: start::GuardState,
    ) -> Result<(), start::Error> {
        macro_rules! require_guard {
            ($bad:expr, $index:expr) => {
                if $bad { return Err(self.guard_failure(1, u32::MAX, $index, u32::MAX, u32::MAX, 0)); }
            };
        }
        require_guard!(self.power.error.is_some(), 1);
        require_guard!(self.power.submitted != 0x7ff, 2);
        require_guard!(self.power.completed != 0x7ff, 3);
        require_guard!(self.scm.report().error.is_some(), 4);
        require_guard!(self.scm.report().completed != ((1 << personal_os_rust_server::sunfish_scm_linux_power::COUNT) - 1), 5);
        require_guard!(self.setup.report().phase != gsi::SetupPhase::Complete, 6);
        require_guard!(self.setup.report().error.is_some(), 7);
        require_guard!(self.setup.report().mask_sent, 8);
        require_guard!(self.endpoints.report().phase != ep::Phase::Complete, 9);
        require_guard!(self.endpoints.report().error.is_some(), 10);
        require_guard!(self.endpoints.report().containment_sent, 11);
        require_guard!(self.command.report().phase != cmd::Phase::Complete, 12);
        require_guard!(self.command.report().error.is_some(), 13);
        require_guard!(!self.command.report().ack_done, 14);
        require_guard!(self.command.report().mask_sent, 15);
        require_guard!(self.memory.report().phase != dma::Phase::Complete, 16);
        require_guard!(self.memory.report().error.is_some(), 17);
        require_guard!(self.memory.report().containment_sent, 18);
        require_guard!(!self.memory.report().ack_done, 19);
        require_guard!(self.tables.report().phase != dma::Phase::Complete, 20);
        require_guard!(self.tables.report().error.is_some(), 21);
        require_guard!(self.tables.report().containment_sent, 22);
        require_guard!(!self.tables.report().ack_done, 23);
        require_guard!(!pool.table_setup_complete(), 24);
        require_guard!(!data.prepared(), 25);
        require_guard!(self.firmware.report().phase != personal_os_rust_server::sunfish_ipa_linux_loader::Phase::AuthReturned, 26);
        require_guard!(!self.firmware.pas_report().passed(), 27);
        let retained = IrqIo {
            registers: self.registers,
            expected: self.expected,
            domain: self.domain,
            pool: Some(pool),
            firmware: Some(self.firmware),
        };
        if let Err(error) = retained.retained_prerequisites() {
            return Err(self.guard_failure(2, u32::MAX, u32::MAX, error.code(), 0, 0));
        }
        for _ in 0..2 {
            let actual = config::Snapshot {
                config: config::PROFILE.map(|s| self.load(rx::Space::Ipa, s.offset)),
                guards: [
                    self.load(rx::Space::Ipa, 0x210),
                    self.load(rx::Space::Ipa, 0x54),
                    self.load(rx::Space::Ipa, 0xb4),
                    self.load(rx::Space::Ipa, 0x300c),
                    self.load(rx::Space::Gsi, 0x1f000),
                    self.load(rx::Space::Gsi, 0x1f040),
                ],
            };
            let strict = policy::config(self.expected, actual, state);
            let reference_pair = !strict && self.post_modem_ready
                && policy::post_modem_config(self.expected, actual, state, true);
            if !strict && !reference_pair {
                if self.stats.config_trace.is_none() {
                    self.stats.config_trace = Some(policy::config_trace(self.expected, actual,
                        self.stats.config_last_good_clkon, self.stats.config_good_snapshots));
                }
                let failure = policy::config_failure(self.expected, actual, state);
                policy::first_guard_failure(&mut self.stats.guard_failure, failure);
                return Err(start::Error::Guard);
            }
            if reference_pair {
                self.stats.postmodem_pair_accepted = self.stats.postmodem_pair_accepted.saturating_add(1);
            }
            self.stats.config_last_good_clkon = Some(actual.config[2]);
            self.stats.config_good_snapshots = self.stats.config_good_snapshots.saturating_add(1);
            let suspend = self.load(rx::Space::Ipa, 0x3034);
            if suspend != state.suspend_mask() {
                return Err(self.guard_failure(4, 0, 0x3034, suspend, state.suspend_mask(), u32::MAX));
            }
            let route = self.load(rx::Space::Ipa, 0x48);
            let wanted = if state.route_set() { rx::DEFAULT_ROUTE.value } else { self.cold_route & 0x013fffff };
            if route & 0x013fffff != wanted {
                return Err(self.guard_failure(5, 0, 0x48, route, wanted, 0x013fffff));
            }
        }
        for spec in ep::PROFILE {
            let observed = self.load(rx::Space::Ipa, spec.offset());
            if observed & spec.register.mask() != spec.value & spec.register.mask() {
                return Err(self.guard_failure(6, 0, spec.offset() as u32, observed, spec.value, spec.register.mask()));
            }
        }
        for (r, want) in [
            (cmd::Read::TypeMask, state.gsi_types()),
            (cmd::Read::IeobMask, state.gsi_ieob()),
            (cmd::Read::ChannelMask, state.gsi_channel_mask()),
            (cmd::Read::EventMask, 0),
            (cmd::Read::GlobalEnable, 1),
            (cmd::Read::GeneralEnable, 14),
        ] {
            let observed = self.load(rx::Space::Gsi, r.offset());
            if observed != want {
                return Err(self.guard_failure(7, 1, r.offset() as u32, observed, want, u32::MAX));
            }
        }
        for (index, e) in ep::ENDPOINTS.into_iter().enumerate() {
            let channel = self.load(rx::Space::Gsi, cmd::Read::Channel(e).offset());
            let observed = (channel >> 20) & 15;
            if (e.channel() == 2 && !policy::rx_state(state, observed))
                || (e.channel() != 2 && observed != if e.channel() == 1 { 2 } else { 1 }) {
                let wanted = if e.channel() == 2 { u32::MAX } else if e.channel() == 1 { 2 << 20 } else { 1 << 20 };
                return Err(self.guard_failure(8, 1, cmd::Read::Channel(e).offset() as u32, channel, wanted, 0x00f00000));
            }
            let event = self.load(rx::Space::Gsi, cmd::Read::Event(e).offset());
            if (event >> 20) & 15 != 1 {
                return Err(self.guard_failure(9, 1, cmd::Read::Event(e).offset() as u32, event, 1 << 20, 0x00f00000));
            }
            // Only immutable context words: scratch and hardware pointer
            // fields are deliberately not treated as reset invariants.
            let channel_words = self
                .setup
                .plan()
                .channel_words(index, 0)
                .map_err(|_| self.guard_failure(10, u32::MAX, u32::MAX, index as u32, u32::MAX, 0))?;
            let event_words = self
                .setup
                .plan()
                .event_words(index)
                .map_err(|_| self.guard_failure(11, u32::MAX, u32::MAX, index as u32, u32::MAX, 0))?;
            for spec in channel_words
                .into_iter()
                .take(5)
                .chain(event_words.into_iter().take(5))
            {
                let observed = self.load(rx::Space::Gsi, spec.offset);
                if observed & spec.readback_mask != spec.value & spec.readback_mask {
                    return Err(self.guard_failure(12, 1, spec.offset as u32, observed, spec.value, spec.readback_mask));
                }
            }
        }
        let types = self.load(rx::Space::Gsi, gsi::TYPE_STATUS);
        let channels = self.load(rx::Space::Gsi, gsi::CHANNEL_STATUS);
        let allowed = if state.gsi_channel_mask() == 4 { 9 } else { 8 };
        if types & !allowed != 0 { return Err(self.guard_failure(13, 1, gsi::TYPE_STATUS as u32, types, 0, !allowed)); }
        if channels & !state.gsi_channel_mask() != 0 { return Err(self.guard_failure(14, 1, gsi::CHANNEL_STATUS as u32, channels, 0, !state.gsi_channel_mask())); }
        let ieob = self.load(rx::Space::Gsi, cmd::IEOB_STATUS);
        if ieob & !2 != 0 { return Err(self.guard_failure(15, 1, cmd::IEOB_STATUS as u32, ieob, 0, !2)); }
        for offset in [gsi::EVENT_STATUS, gsi::GLOBAL_STATUS, gsi::GENERAL_STATUS] {
            let observed = self.load(rx::Space::Gsi, offset);
            if observed != 0 { return Err(self.guard_failure(16, 1, offset as u32, observed, 0, u32::MAX)); }
        }
        Ok(())
    }
    fn read(&mut self, r: start::Read) -> Result<u32, start::Error> {
        Ok(self.load(r.space(), r.offset()))
    }
    fn write(&mut self, r: &start::Request) -> Result<(), start::Error> {
        let w = r.word();
        if !policy::startup(w) {
            return Err(start::Error::Io);
        }
        self.store(w.space, w.offset, w.value);
        Ok(())
    }
    fn acknowledge_start(&mut self) -> Result<(), start::Error> {
        Channel::new(pixel_ipa_gsi::CHANNEL)
            .irq_ack()
            .map_err(|_| start::Error::Acknowledge)?;
        self.stats.acks_gsi = self.stats.acks_gsi.saturating_add(1);
        Ok(())
    }
}
impl runtime::Io<NativeRam<'static>, NativeReceiveRam<'static>> for Native<'_> {
    fn acquire(&mut self) -> Result<(), runtime::Error> {
        barrier();
        Ok(())
    }
    fn release(&mut self) -> Result<(), runtime::Error> {
        barrier();
        Ok(())
    }
    fn write_receive(&mut self, r: &runtime::Request) -> Result<(), runtime::Error> {
        if !policy::receive(r.register(), r.value()) {
            return Err(runtime::Error::Io);
        }
        self.store(rx::Space::Gsi, r.register().offset(), r.value());
        Ok(())
    }
    fn acknowledge_receive(&mut self) -> Result<(), runtime::Error> {
        Channel::new(pixel_ipa_gsi::CHANNEL)
            .irq_ack()
            .map_err(|_| runtime::Error::Acknowledge)?;
        self.stats.acks_gsi = self.stats.acks_gsi.saturating_add(1);
        Ok(())
    }
}
impl dispatch::Io<NativeRam<'static>, NativeReceiveRam<'static>> for Native<'_> {
    fn ipa_read(&mut self, r: irq::Read) -> Result<u32, uc::Error> {
        Ok(self.load(rx::Space::Ipa, r.offset()))
    }
    fn ipa_write(&mut self, r: &uc::Write) -> Result<(), uc::Error> {
        if !((r.offset() == uc::CLEAR_OFFSET && r.value() != 0)
            || (r.offset() == irq::ENABLE_OFFSET && r.value() == 0))
        {
            return Err(uc::Error::Io);
        }
        self.store(rx::Space::Ipa, r.offset(), r.value());
        Ok(())
    }
    fn uc_byte(&mut self, f: irq::Shared) -> Result<u8, uc::Error> {
        let value = unsafe { self.registers.shared.add(f.offset()).read_volatile() };
        barrier();
        Ok(value)
    }
    fn suspend_info(&mut self) -> Result<u32, uc::Error> {
        Ok(self.load(rx::Space::Ipa, uc::SUSPEND_INFO))
    }
    fn clear_suspend(&mut self, r: &uc::SuspendWrite) -> Result<(), uc::Error> {
        if r.value() == 0 {
            return Err(uc::Error::Io);
        }
        self.store(rx::Space::Ipa, uc::SUSPEND_CLEAR, r.value());
        Ok(())
    }
    fn acknowledge_ipa(&mut self) -> Result<(), uc::Error> {
        Channel::new(irq::CHANNEL as usize)
            .irq_ack()
            .map_err(|_| uc::Error::Acknowledge)?;
        self.stats.acks_ipa = self.stats.acks_ipa.saturating_add(1);
        Ok(())
    }
}
fn with_once<T>(state: &mut Startup, f: impl FnOnce(&mut Once, &mut Native<'_>) -> T) -> Option<T> {
    let mut io = Native {
        registers: state._irq_registers?,
        expected: state._config_report.as_ref()?.after?,
        cold_route: state._receive_stats.cold_route?,
        post_modem_ready: false,
        domain: pixel_smmu_install::Access::Probe(state._domain.as_ref()?),
        firmware: state._firmware.as_ref()?,
        power: state._report.as_ref()?,
        scm: &state._scm_hold,
        setup: state._gsi.as_ref()?,
        endpoints: state._endpoint.as_ref()?,
        command: state._command.as_ref()?,
        memory: state._transfer.as_ref()?,
        tables: state._tables.as_ref()?,
        stats: &mut state._receive_stats,
    };
    Some(f(state._receive_once.as_mut()?, &mut io))
}
fn with_runtime<T>(
    state: &mut Startup,
    f: impl FnOnce(&mut Runtime, &mut Native<'_>, &mut Sink) -> T,
) -> Option<T> {
    let mut io = Native {
        registers: state._irq_registers?,
        expected: state._config_report.as_ref()?.after?,
        cold_route: state._receive_stats.cold_route?,
        post_modem_ready: false,
        domain: pixel_smmu_install::Access::Probe(state._domain.as_ref()?),
        firmware: state._firmware.as_ref()?,
        power: state._report.as_ref()?,
        scm: &state._scm_hold,
        setup: state._gsi.as_ref()?,
        endpoints: state._endpoint.as_ref()?,
        command: state._command.as_ref()?,
        memory: state._transfer.as_ref()?,
        tables: state._tables.as_ref()?,
        stats: &mut state._receive_stats,
    };
    Some(f(
        state._receive_runtime.as_mut()?,
        &mut io,
        &mut state._receive_sink,
    ))
}

/// Dispatch through a runtime-owned GLINK gate. The adapter reference is
/// borrowed only inside the enclosing `NativeGate::try_borrow_mut` closure;
/// this function stores neither it nor any reference into `Startup`.
#[cfg(feature = "sunfish-native-startup-mpss-glink")]
pub(super) fn validate_runtime_setup(
    state: &mut Startup,
    runtime: &Runtime,
    adapter: &pixel_smmu_install::Adapter,
    ready: &personal_os_rust_server::sunfish_mpss_linux_smp2p::ReadyObservation,
) -> Result<personal_os_rust_server::qcom_ipa_service::ApReadyPermit, dispatch::Error> {
    if state._receive_stats.terminal {
        return Err(dispatch::Error::Quarantined);
    }
    if ready.generation() != 1 {
        return Err(dispatch::Error::Startup);
    }
    let result = (|| {
        let mut io = Native {
            registers: state._irq_registers?,
            expected: state._config_report.as_ref()?.after?,
            cold_route: state._receive_stats.cold_route?,
            post_modem_ready: cfg!(feature = "sunfish-ipa-postmodem-reference-experiment") && ready.generation() == 1,
            domain: pixel_smmu_install::Access::Borrowed(adapter),
            firmware: state._firmware.as_ref()?,
            power: state._report.as_ref()?,
            scm: &state._scm_hold,
            setup: state._gsi.as_ref()?,
            endpoints: state._endpoint.as_ref()?,
            command: state._command.as_ref()?,
            memory: state._transfer.as_ref()?,
            tables: state._tables.as_ref()?,
            stats: &mut state._receive_stats,
        };
        Some(runtime.linux_ap_service_permit(&mut io, ready))
    })().unwrap_or(Err(dispatch::Error::Startup));
    if result.is_err() {
        state._receive_stats.terminal = true;
        state._receive_stats.error = 0x4b;
    }
    result
}

/// Dispatch through a runtime-owned GLINK gate. The adapter reference is
/// borrowed only inside the enclosing `NativeGate::try_borrow_mut` closure.
#[cfg(feature = "sunfish-native-startup-mpss-glink")]
pub(super) fn notify_runtime_badge(
    state: &mut Startup,
    runtime: &mut Runtime,
    adapter: &pixel_smmu_install::Adapter,
    ready: &personal_os_rust_server::sunfish_mpss_linux_smp2p::ReadyObservation,
    allow_reference_pair: bool,
    badge: u64,
) -> Result<usize, dispatch::Error> {
    state._receive_stats.badge = badge;
    notify_runtime(state, runtime, adapter, ready, allow_reference_pair, policy::notice(badge))
}

#[cfg(feature = "sunfish-native-startup-mpss-glink")]
pub(super) fn quarantine_runtime(state: &mut Startup, _runtime: &mut Runtime, code: u32) {
    state._receive_stats.terminal = true;
    if state._receive_stats.error == 0 {
        state._receive_stats.error = code;
    }
}

#[cfg(feature = "sunfish-native-startup-mpss-glink")]
pub(super) fn notify_runtime(
    state: &mut Startup,
    runtime: &mut Runtime,
    adapter: &pixel_smmu_install::Adapter,
    ready: &personal_os_rust_server::sunfish_mpss_linux_smp2p::ReadyObservation,
    allow_reference_pair: bool,
    notice: dispatch::Notice,
) -> Result<usize, dispatch::Error> {
    if state._receive_stats.terminal {
        return Err(dispatch::Error::Quarantined);
    }
    if ready.generation() != 1 { return Err(dispatch::Error::Startup); }
    let Some(registers) = state._irq_registers else {
        state._receive_stats.terminal = true;
        state._receive_stats.error = 0x40;
        return Err(dispatch::Error::Startup);
    };
    let Some(expected) = state._config_report.as_ref().and_then(|r| r.after) else {
        state._receive_stats.terminal = true;
        state._receive_stats.error = 0x41;
        return Err(dispatch::Error::Startup);
    };
    let Some(cold_route) = state._receive_stats.cold_route else {
        state._receive_stats.terminal = true;
        state._receive_stats.error = 0x42;
        return Err(dispatch::Error::Startup);
    };
    state._receive_stats.notices = state._receive_stats.notices.saturating_add(1);
    let mut io = Native {
        registers,
        expected,
        cold_route,
        post_modem_ready: cfg!(feature = "sunfish-ipa-postmodem-reference-experiment") && allow_reference_pair,
        domain: pixel_smmu_install::Access::Borrowed(adapter),
        firmware: match state._firmware.as_ref() {
            Some(value) => value,
            None => {
                state._receive_stats.terminal = true;
                state._receive_stats.error = 0x43;
                return Err(dispatch::Error::Startup);
            }
        },
        power: match state._report.as_ref() {
            Some(value) => value,
            None => {
                state._receive_stats.terminal = true;
                state._receive_stats.error = 0x44;
                return Err(dispatch::Error::Startup);
            }
        },
        scm: &state._scm_hold,
        setup: match state._gsi.as_ref() {
            Some(value) => value,
            None => {
                state._receive_stats.terminal = true;
                state._receive_stats.error = 0x45;
                return Err(dispatch::Error::Startup);
            }
        },
        endpoints: match state._endpoint.as_ref() {
            Some(value) => value,
            None => {
                state._receive_stats.terminal = true;
                state._receive_stats.error = 0x46;
                return Err(dispatch::Error::Startup);
            }
        },
        command: match state._command.as_ref() {
            Some(value) => value,
            None => {
                state._receive_stats.terminal = true;
                state._receive_stats.error = 0x47;
                return Err(dispatch::Error::Startup);
            }
        },
        memory: match state._transfer.as_ref() {
            Some(value) => value,
            None => {
                state._receive_stats.terminal = true;
                state._receive_stats.error = 0x48;
                return Err(dispatch::Error::Startup);
            }
        },
        tables: match state._tables.as_ref() {
            Some(value) => value,
            None => {
                state._receive_stats.terminal = true;
                state._receive_stats.error = 0x49;
                return Err(dispatch::Error::Startup);
            }
        },
        stats: &mut state._receive_stats,
    };
    let result = runtime.notification(notice, &mut io, &mut state._receive_sink);
    drop(io);
    if result.is_err() {
        state._receive_stats.terminal = true;
        state._receive_stats.error = 0x4a;
    }
    result
}

fn stopped(state: &mut Startup, error: u32) {
    state._receive_stats.terminal = true;
    state._receive_stats.error = error;
    if let Some(d) = state._domain.as_mut() {
        d.stop_on_fault();
    }
}
pub(super) fn show(d: &Display, state: &Startup) {
    d.clear();
    d.replace_line(0, "NATIVE IPA RECEIVE");
    let r = state
        ._receive_runtime
        .as_ref()
        .map(|v| v.startup_report())
        .or_else(|| state._receive_once.as_ref().map(Once::report));
    let armed = state
        ._receive_runtime
        .as_ref()
        .is_some_and(|v| v.error().is_none())
        && !state._receive_stats.terminal;
    d.replace_line(
        1,
        if armed {
            "RX ARMED DIAGNOSTIC"
        } else {
            "RX STARTUP STOP"
        },
    );
    if let Some(r) = r {
        d.hex(2, "RX PHASE", r.phase as u32);
        d.hex(3, "TRE WORDS", r.cpu_checked as u32);
        d.hex(4, "POSTED BUFFERS", r.published_buffers as u32);
        d.hex(5, "POST DOORBELLS", r.doorbells as u32);
        d.hex(6, "ROUTE SET", r.route_done as u32);
        d.hex(7, "START ACK", r.ack_done as u32);
        d.hex(8, "FINAL SAMPLES", r.final_samples as u32);
        d.hex(16, "CORE ERROR", r.error.map_or(0, |e| e as u32 + 1));
    }
    d.hex(9, "IRQ NOTICES", state._receive_stats.notices);
    d.hex(10, "GSI ACKS", state._receive_stats.acks_gsi);
    d.hex(11, "IPA ACKS", state._receive_stats.acks_ipa);
    d.hex(12, "LAST BADGE", state._receive_stats.badge as u32);
    d.hex(13, "LAST READ", state._receive_stats.last_read as u32);
    d.hex(14, "LAST VALUE", state._receive_stats.last_value);
    d.hex(15, "LAST WRITE", state._receive_stats.last_write as u32);
    d.hex(17, "NATIVE ERROR", state._receive_stats.error);
    d.hex(18, "RX AGGREGATES", state._receive_sink.aggregates);
    d.hex(19, "RX BYTES LOW", state._receive_sink.bytes as u32);
    if let Some(v) = state._receive_runtime.as_ref() {
        let (uc, s) = v.ipa_reports();
        d.hex(20, "UC INIT", uc.init_observed);
        d.hex(21, "SUSPEND NOTICES", s.notifications);
    }
    if let Some((g, c)) = state._domain.as_ref().and_then(|v| v.fault_status()) {
        d.hex(22, "GLOBAL FSR", g);
        d.hex(23, "CONTEXT FSR", c);
    }
    d.replace_line(24, "DIAGNOSTIC AGGREGATES ONLY");
    d.replace_line(25, "NO NETWORK DELIVERY");
    d.replace_line(26, "NO MODEM OR WLAN START");
    d.replace_line(27, "RETAINED UNTIL REBOOT");
    d.replace_line(28, "NO FLASH OR RETRY");
    d.replace_line(29, "RESTART WHEN DONE");
    d.flush();
}
fn pending_ipa(state: &mut Startup) -> Option<Result<(), uc::Error>> {
    let registers = state._irq_registers?;
    let expected = state._config_report.as_ref()?.after?;
    let domain = state._domain.as_ref()?;
    let pool = state._receive_once.as_ref()?.pools().0;
    let mut io = IrqIo {
        registers,
        expected,
        domain: pixel_smmu_install::Access::Probe(domain),
        pool: Some(pool),
        firmware: state._firmware.as_ref(),
    };
    let result = state._uc.as_mut()?.notification(&mut io);
    if result.is_ok() {
        state._receive_stats.acks_ipa = state._receive_stats.acks_ipa.saturating_add(1);
    }
    Some(result)
}
fn badge(state: &mut Startup, badge: u64) {
    if state._receive_stats.terminal {
        return;
    }
    state._receive_stats.badge = badge;
    state._receive_stats.notices = state._receive_stats.notices.saturating_add(1);
    let notice = policy::notice(badge);
    if state._receive_runtime.is_some() {
        let result = with_runtime(state, |r, i, s| r.notification(notice, i, s));
        if result.is_none_or(|r| r.is_err()) {
            stopped(state, 8);
        }
        return;
    }
    if state._receive_once.is_none() {
        return;
    }
    if notice == dispatch::Notice::Fault {
        let _ = with_once(state, |o, i| o.external_fault(i));
        stopped(state, 4);
        return;
    }
    if matches!(notice, dispatch::Notice::Ipa | dispatch::Notice::Both) {
        if pending_ipa(state).is_none_or(|r| r.is_err()) {
            let _ = with_once(state, |o, i| o.ipa_dispatch_failed(i));
            stopped(state, 5);
            return;
        }
    }
    if matches!(notice, dispatch::Notice::Gsi | dispatch::Notice::Both) {
        let now = counter();
        if with_once(state, |o, i| o.notified(i, now)).is_none_or(|r| r.is_err()) {
            stopped(state, 6);
            return;
        }
        if state
            ._receive_once
            .as_ref()
            .is_some_and(|o| o.report().phase == start::Phase::Armed)
        {
            if state._uc.is_none() {
                let _ = with_once(state, |o, i| o.external_fault(i));
                stopped(state, 7);
                return;
            }
            let once = state._receive_once.take().unwrap();
            let uc = state._uc.take().unwrap();
            state._receive_runtime = Some(Runtime::new(runtime::Runtime::new(once), uc));
            if state
                ._receive_runtime
                .as_ref()
                .is_some_and(|r| r.error().is_some())
            {
                stopped(state, 7);
            }
        }
    }
}
pub(super) fn owns(state: &Startup) -> bool {
    state._receive_once.is_some() || state._receive_runtime.is_some()
}
#[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
pub(super) fn healthy(state: &Startup) -> bool {
    !state._receive_stats.terminal
        && state
            ._receive_runtime
            .as_ref()
            .is_some_and(|r| r.error().is_none() && r.startup_report().phase == start::Phase::Armed)
}
#[cfg(feature = "sunfish-native-startup-mpss-auth")]
pub(super) fn diagnostic_snapshot(state: &Startup) -> (u32, u64) {
    (state._receive_stats.error, state._receive_stats.badge)
}
#[cfg(feature = "sunfish-native-startup-mpss-auth")]
pub(super) fn guard_diagnostic(state: &Startup) -> Option<(u32, u32, u32, u32, u32, u32)> {
    state._receive_stats.guard_failure.map(|f| (f.stage, f.space, f.offset, f.observed, f.expected, f.mask))
}
#[cfg(feature = "sunfish-native-startup-mpss-auth")]
pub(super) fn postmodem_pair_count(state: &Startup) -> u32 {
    state._receive_stats.postmodem_pair_accepted
}
#[cfg(feature = "sunfish-native-startup-mpss-auth")]
pub(super) fn config_diagnostic(state: &Startup) -> Option<([u32; 9], u32, u32, u32, u32)> {
    state._receive_stats.config_trace.map(|t| (t.actual, t.mismatch, t.clkon_baseline, t.last_good_clkon, t.successful_snapshots))
}
#[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
pub(super) fn poll_pending(state: &mut Startup) {
    let b = pending();
    if b != 0 {
        badge(state, b);
    }
}
pub(super) fn notified(state: &mut Startup, channels: ChannelSet) -> bool {
    if !owns(state) {
        return false;
    }
    let ipa = channels.contains(Channel::new(irq::CHANNEL as usize));
    let gsi = channels.contains(Channel::new(pixel_ipa_gsi::CHANNEL));
    let foreign = channels.iter().any(|c| {
        c != Channel::new(irq::CHANNEL as usize) && c != Channel::new(pixel_ipa_gsi::CHANNEL)
    });
    badge(
        state,
        if foreign {
            1
        } else {
            (ipa as u64) * 4 + (gsi as u64) * 8
        },
    );
    if !super::diagnostic_display_owned() {
        if let Some(r) = state._irq_registers {
            show(&unsafe { Display::new(r.pixels) }, state);
        }
    }
    true
}
pub(super) fn quarantine(state: &mut Startup) {
    if !owns(state) || state._receive_stats.terminal {
        return;
    }
    badge(state, 1);
}
pub(super) fn start(d: &Display, state: &mut Startup) {
    if owns(state)
        || state._receive_stats.terminal
        || state._receive_data.as_ref().is_none_or(|d| !d.prepared())
        || state._uc.is_none()
    {
        return;
    }
    if state._buffers.is_none()
        || state
            ._config_report
            .as_ref()
            .and_then(|r| r.after)
            .is_none()
        || state._domain.is_none()
        || state._firmware.is_none()
        || state._report.is_none()
        || state._gsi.is_none()
        || state._endpoint.is_none()
        || state._command.is_none()
        || state._transfer.is_none()
        || state._tables.is_none()
    {
        pixel_ipa_command::quarantine(state);
        return;
    }
    let hz = frequency();
    if !(1_000_000..=1_000_000_000).contains(&hz) {
        pixel_ipa_command::quarantine(state);
        return;
    }
    if pending() != 0 {
        pixel_ipa_command::quarantine(state);
        d.replace_line(1, "RX STALE NOTICE STOP");
        d.flush();
        return;
    }
    let Some(registers) = state._irq_registers else {
        return;
    };
    state._receive_stats.cold_route = Some(word(registers.ipa, 0x48));
    let rings = state._buffers.take().unwrap();
    let data = state._receive_data.take().unwrap();
    state._receive_once = Some(Once::new(rings, data, hz));
    d.clear();
    d.replace_line(0, "NATIVE IPA RECEIVE");
    d.replace_line(1, "STAGE TRE AND START RX");
    d.flush();
    let result = {
        let mut io = Native {
            registers,
            expected: state._config_report.as_ref().and_then(|r| r.after).unwrap(),
            cold_route: state._receive_stats.cold_route.unwrap(),
            post_modem_ready: false,
            domain: pixel_smmu_install::Access::Probe(state._domain.as_ref().unwrap()),
            firmware: state._firmware.as_ref().unwrap(),
            power: state._report.as_ref().unwrap(),
            scm: &state._scm_hold,
            setup: state._gsi.as_ref().unwrap(),
            endpoints: state._endpoint.as_ref().unwrap(),
            command: state._command.as_ref().unwrap(),
            memory: state._transfer.as_ref().unwrap(),
            tables: state._tables.as_ref().unwrap(),
            stats: &mut state._receive_stats,
        };
        state._receive_once.as_mut().unwrap().begin(
            io.setup,
            io.endpoints,
            io.command,
            io.memory,
            io.tables,
            &mut io,
        )
    };
    if result.is_err() {
        stopped(state, 2);
        show(d, state);
        return;
    }
    while state
        ._receive_once
        .as_ref()
        .is_some_and(|o| o.report().phase == start::Phase::Pending)
        && !state._receive_stats.terminal
    {
        let b = pending();
        if b != 0 {
            badge(state, b);
        } else {
            let now = counter();
            if with_once(state, |o, i| o.timeout(i, now)).is_none_or(|r| r.is_err()) {
                stopped(state, 3);
            }
        }
    }
    show(d, state);
}
