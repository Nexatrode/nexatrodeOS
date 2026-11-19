//! Native AP endpoint setup and one empty command-channel START; no transfer.
use super::*;
use personal_os_rust_server::{
    sunfish_ipa_linux_command as cmd, sunfish_ipa_linux_endpoint as ep,
    sunfish_ipa_linux_gsi as gsi, sunfish_ipa_linux_uc as uc,
};

#[derive(Default)]
pub(super) struct Stats {
    ep_reads: u32,
    ep_writes: u32,
    command_reads: u32,
    command_writes: u32,
    notices: u32,
    acks: u32,
    badge: u64,
    last_read: usize,
    last_write: usize,
    last_value: u32,
}
fn endpoint_word_allowed(word: ep::Word) -> bool {
    ep::PROFILE.contains(&word)
}
fn command_word_allowed(register: cmd::Register, value: u32) -> bool {
    match register {
        cmd::Register::TypeMask => matches!(value, 0 | 0x4c | 0x4d),
        cmd::Register::ChannelMask => matches!(value, 0 | 2),
        cmd::Register::ChannelClear => matches!(value, 2 | u32::MAX),
        cmd::Register::Command => value == cmd::START,
        cmd::Register::IeobMask => value == 1,
    }
}
fn counter() -> u64 {
    let v: u64;
    unsafe {
        core::arch::asm!("isb","mrs {v}, cntpct_el0",v=out(reg)v,options(nostack,preserves_flags));
    }
    v
}
fn frequency() -> u64 {
    let v: u64;
    unsafe {
        core::arch::asm!("mrs {v}, cntfrq_el0",v=out(reg)v,options(nostack,preserves_flags));
    }
    v
}
struct Native<'a> {
    retained: IrqIo<'a>,
    setup: &'a gsi::SetupOnce,
    endpoints: Option<&'a ep::ConfigureOnce>,
    stats: &'a mut Stats,
}
impl Native<'_> {
    fn guard(&mut self) -> Result<(), ()> {
        if self.setup.report().phase != gsi::SetupPhase::Complete {
            return Err(());
        }
        uc::Io::guard(&mut self.retained).map_err(|_| ())?;
        if let Some(endpoints) = self.endpoints {
            let r = endpoints.report();
            if r.phase != ep::Phase::Complete || r.readback != ep::COMPLETE || r.error.is_some() {
                return Err(());
            }
            for w in ep::PROFILE {
                if word(self.retained.registers.ipa, w.offset()) & w.register.mask()
                    != w.value & w.register.mask()
                {
                    return Err(());
                }
            }
        }
        Ok(())
    }
    fn store(&mut self, base: *const u32, offset: usize, value: u32) {
        self.stats.last_write = offset;
        // SAFETY: retained audited CapDL provides the same IPA first/IRQ
        // pages and five GSI RW/NX pages. The two sealed request types and
        // pure whitelists restrict every normal store. No raw client address,
        // transfer doorbell, reset, DE_ALLOCATE or arbitrary opcode is accepted.
        unsafe {
            core::arch::asm!("dsb sy", options(nostack, preserves_flags));
            base.cast_mut()
                .add(offset / 4)
                .write_volatile(value.to_le());
            core::arch::asm!("dsb sy", options(nostack, preserves_flags));
        }
    }
}
impl ep::Io for Native<'_> {
    fn guard(&mut self) -> Result<(), ep::Error> {
        self.guard().map_err(|_| ep::Error::Guard)
    }
    fn read(&mut self, r: ep::Read) -> Result<u32, ep::Error> {
        if let ep::Read::Endpoint(w) = r {
            if !endpoint_word_allowed(w) {
                return Err(ep::Error::Io);
            }
        }
        self.stats.ep_reads = self.stats.ep_reads.saturating_add(1);
        self.stats.last_read = r.offset();
        let value = word(
            if r.is_gsi() {
                self.retained.registers.gsi
            } else {
                self.retained.registers.ipa
            },
            r.offset(),
        );
        self.stats.last_value = value;
        Ok(value)
    }
    fn write(&mut self, r: &ep::Request) -> Result<(), ep::Error> {
        let w = r.word();
        if !endpoint_word_allowed(w) {
            return Err(ep::Error::Io);
        }
        self.stats.ep_writes = self.stats.ep_writes.saturating_add(1);
        self.store(self.retained.registers.ipa, w.offset(), w.value);
        Ok(())
    }
    fn contain(&mut self) -> Result<(), ep::Error> {
        // The outer failure path contains UC separately, once this borrow
        // ends. Preserve all pending sources and every retained owner.
        self.store(self.retained.registers.gsi, gsi::TYPE_MASK, 0);
        Ok(())
    }
}
impl cmd::Io for Native<'_> {
    fn ticks(&mut self) -> u64 {
        counter()
    }
    fn guard(&mut self) -> Result<(), cmd::Error> {
        self.guard().map_err(|_| cmd::Error::Guard)
    }
    fn read(&mut self, r: cmd::Read) -> Result<u32, cmd::Error> {
        self.stats.command_reads = self.stats.command_reads.saturating_add(1);
        self.stats.last_read = r.offset();
        let v = word(self.retained.registers.gsi, r.offset());
        self.stats.last_value = v;
        Ok(v)
    }
    fn write(&mut self, r: &cmd::Request) -> Result<(), cmd::Error> {
        if !command_word_allowed(r.register(), r.value()) {
            return Err(cmd::Error::Io);
        }
        self.stats.command_writes = self.stats.command_writes.saturating_add(1);
        self.store(
            self.retained.registers.gsi,
            r.register().offset(),
            r.value(),
        );
        Ok(())
    }
    fn acknowledge(&mut self) -> Result<(), cmd::Error> {
        Channel::new(pixel_ipa_gsi::CHANNEL)
            .irq_ack()
            .map_err(|_| cmd::Error::Acknowledge)?;
        self.stats.acks = self.stats.acks.saturating_add(1);
        Ok(())
    }
}
fn endpoints(state: &mut Startup) -> Option<Result<(), ep::Error>> {
    let registers = state._irq_registers?;
    let expected = state._config_report.as_ref()?.after?;
    let mut io = Native {
        retained: IrqIo {
            registers,
            expected,
            domain: pixel_smmu_install::Access::Probe(state._domain.as_ref()?),
            pool: Some(state._buffers.as_ref()?),
            firmware: Some(state._firmware.as_ref()?),
        },
        setup: state._gsi.as_ref()?,
        endpoints: None,
        stats: &mut state._command_stats,
    };
    let setup = io.setup;
    Some(state._endpoint.as_mut()?.execute(setup, &mut io))
}
fn with_command<T>(
    state: &mut Startup,
    f: impl FnOnce(&mut cmd::StartOnce, &mut Native<'_>) -> T,
) -> Option<T> {
    let registers = state._irq_registers?;
    let expected = state._config_report.as_ref()?.after?;
    let mut io = Native {
        retained: IrqIo {
            registers,
            expected,
            domain: pixel_smmu_install::Access::Probe(state._domain.as_ref()?),
            pool: Some(state._buffers.as_ref()?),
            firmware: Some(state._firmware.as_ref()?),
        },
        setup: state._gsi.as_ref()?,
        endpoints: Some(state._endpoint.as_ref()?),
        stats: &mut state._command_stats,
    };
    Some(f(state._command.as_mut()?, &mut io))
}
pub(super) fn quarantine(state: &mut Startup) {
    #[cfg(feature = "sunfish-native-startup-ipa-receive")]
    if pixel_ipa_receive::owns(state) {
        pixel_ipa_receive::quarantine(state);
        return;
    }
    #[cfg(feature = "sunfish-native-startup-ipa-receive-memory")]
    if let Some(data) = state._receive_data.as_mut() {
        data.quarantine();
    }
    if let Some(r) = state._irq_registers {
        // Normal coordinator failures already mask this source. Do not
        // replay their store; only contain failures before a coordinator ran.
        if word(r.gsi, gsi::TYPE_MASK) != 0 {
            unsafe {
                core::arch::asm!("dsb sy", options(nostack, preserves_flags));
                r.gsi.cast_mut().add(gsi::TYPE_MASK / 4).write_volatile(0);
                core::arch::asm!("dsb sy", options(nostack, preserves_flags));
            }
        }
    }
    dispatch_uc(state, true);
    if let Some(pool) = state._buffers.as_mut() {
        pool.quarantine();
    }
    if let Some(domain) = state._domain.as_mut() {
        domain.stop_on_fault();
    }
}
fn show(d: &Display, state: &Startup) {
    d.clear();
    d.replace_line(0, "NATIVE IPA COMMAND");
    let e = state._endpoint.as_ref().map(|o| o.report());
    let c = state._command.as_ref().map(|o| o.report());
    let passed = state
        ._gsi
        .as_ref()
        .is_some_and(|g| g.report().phase == gsi::SetupPhase::Complete)
        && c.is_some_and(|c| c.phase == cmd::Phase::Complete)
        && e.is_some_and(|e| e.phase == ep::Phase::Complete);
    d.replace_line(
        1,
        if passed {
            "COMMAND START PASS"
        } else {
            "COMMAND START STOP"
        },
    );
    if let Some(e) = e {
        d.hex(2, "ENDPOINT PHASE", e.phase as u32);
        d.hex(3, "ENDPOINTS", e.endpoints_done as u32);
        d.hex(4, "EP SENT LO", e.writes_sent as u32);
        d.hex(5, "EP SENT HI", (e.writes_sent >> 32) as u32);
        d.hex(6, "EP DONE LO", e.writes_done as u32);
        d.hex(7, "EP DONE HI", (e.writes_done >> 32) as u32);
        d.hex(8, "EP CHECK LO", e.readback as u32);
        d.hex(9, "EP CHECK HI", (e.readback >> 32) as u32);
        d.hex(10, "FINAL SAMPLES", e.final_samples as u32);
        if let Some(err) = e.error {
            d.hex(11, "ENDPOINT ERROR", err as u32 + 1);
        }
    }
    if let Some(c) = c {
        d.hex(12, "COMMAND PHASE", c.phase as u32);
        d.hex(
            13,
            "CHANNEL STATE",
            c.state.map(u32::from).unwrap_or(u32::MAX),
        );
        d.hex(14, "CMD WRITES", u32::from(c.writes_done));
        if let Some(err) = c.error {
            d.hex(11, "COMMAND ERROR", err as u32 + 1);
        }
    }
    let s = &state._command_stats;
    d.hex(15, "CMD NOTICES", s.notices);
    d.hex(16, "CMD IRQ ACKS", s.acks);
    d.hex(17, "LAST BADGE", s.badge as u32);
    d.hex(18, "LAST WRITE", s.last_write as u32);
    d.hex(19, "LAST READ", s.last_read as u32);
    d.hex(20, "LAST VALUE", s.last_value);
    if let Some(f) = state._domain.as_ref().and_then(|d| d.fault_status()) {
        d.hex(21, "GLOBAL FSR", f.0);
        d.hex(22, "CONTEXT FSR", f.1);
    }
    if let Some(f) = state._firmware.as_ref() {
        d.hex(23, "PAS RETURNED", f.pas_report().returned_mask);
    }
    d.replace_line(24, "NO TRE OR TRANSFER DB");
    d.replace_line(25, "NO MODEM OR WLAN START");
    d.replace_line(26, "RETAINED UNTIL REBOOT");
    d.replace_line(27, "NO FLASH OR RETRY");
    if let Some(gsi) = state._gsi.as_ref() {
        d.hex(28, "GSI PHASE", gsi.report().phase as u32);
    }
    d.replace_line(29, "RESTART WHEN DONE");
    d.flush();
}
fn receive() -> u64 {
    let (_, badge) =
        sel4::Cap::<sel4::cap_type::Endpoint>::from_bits(1)
            .nb_recv(sel4::Cap::<sel4::cap_type::Reply>::from_bits(4));
    badge
}
pub(super) fn start(d: &Display, state: &mut Startup) {
    if !state
        ._gsi
        .as_ref()
        .is_some_and(|g| g.report().phase == gsi::SetupPhase::Complete)
    {
        return;
    }
    state._endpoint = Some(ep::ConfigureOnce::new());
    d.clear();
    d.replace_line(0, "NATIVE IPA COMMAND");
    d.replace_line(1, "CONFIGURE AP ENDPOINTS");
    d.flush();
    if !matches!(endpoints(state), Some(Ok(()))) {
        quarantine(state);
        show(d, state);
        return;
    }
    // A queued notification before START cannot establish its completion.
    let badge = receive();
    if badge != 0 {
        state._command_stats.notices = state._command_stats.notices.saturating_add(1);
        state._command_stats.badge = badge;
        pixel_ipa_gsi::late_notification(state);
        quarantine(state);
        show(d, state);
        return;
    }
    let hz = frequency();
    if !(1_000_000..=1_000_000_000).contains(&hz) {
        quarantine(state);
        d.replace_line(1, "COMMAND COUNTER STOP");
        d.flush();
        return;
    }
    state._command = Some(cmd::StartOnce::new(gsi::command_timeout_ticks(hz).unwrap()).unwrap());
    d.replace_line(1, "START EMPTY COMMAND CH");
    d.flush();
    let began = with_command(state, |once, io| {
        let setup = io.setup;
        let endpoints = io.endpoints.unwrap();
        once.begin(setup, endpoints, io)
    });
    if !matches!(began, Some(Ok(()))) {
        quarantine(state);
        show(d, state);
        return;
    }
    loop {
        let phase = state._command.as_ref().unwrap().report().phase;
        if matches!(phase, cmd::Phase::Complete | cmd::Phase::Failed) {
            break;
        }
        let badge = receive();
        let now = counter();
        if badge != 0 {
            state._command_stats.notices = state._command_stats.notices.saturating_add(1);
            state._command_stats.badge = badge;
            if badge != 1 << pixel_ipa_gsi::CHANNEL {
                let _ = with_command(state, |once, io| once.external_fault(io));
                quarantine(state);
                break;
            }
            if !matches!(
                with_command(state, |once, io| once.notified(io, now)),
                Some(Ok(()))
            ) {
                break;
            }
        } else if !matches!(
            with_command(state, |once, io| once.timeout(io, now)),
            Some(Ok(()))
        ) {
            break;
        }
        core::hint::spin_loop();
    }
    if state
        ._command
        .as_ref()
        .is_some_and(|c| c.report().phase == cmd::Phase::Failed)
    {
        quarantine(state);
    }
    show(d, state);
}
pub(super) fn late_notification(state: &mut Startup) {
    state._command_stats.notices = state._command_stats.notices.saturating_add(1);
    state._command_stats.badge = 1 << pixel_ipa_gsi::CHANNEL;
    pixel_ipa_gsi::late_notification(state);
    quarantine(state);
    if super::diagnostic_display_owned() {
        return;
    }
    if let Some(r) = state._irq_registers {
        let d = unsafe { Display::new(r.pixels) };
        show(&d, state);
    }
}
