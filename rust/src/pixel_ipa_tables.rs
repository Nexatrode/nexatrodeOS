//! Six fixed route/filter commands after this boot’s actual memory DMA pass.
use super::*;
use personal_os_rust_server::{
    sunfish_ipa_linux_buffers as buffers, sunfish_ipa_linux_buffers_native::NativeRam,
    sunfish_ipa_linux_command as cmd, sunfish_ipa_linux_endpoint as ep,
    sunfish_ipa_linux_gsi as gsi, sunfish_ipa_linux_table_transfer::TableOnce,
    sunfish_ipa_linux_transfer as dma, sunfish_ipa_linux_uc as uc,
};

type Pool = buffers::Pool<NativeRam<'static>>;
#[derive(Default)]
pub(super) struct Stats {
    reads: u32,
    writes: u32,
    notices: u32,
    acks: u32,
    badge: u64,
    last_read: usize,
    last_write: usize,
    last_value: u32,
}
fn allowed(register: dma::Register, value: u32) -> bool {
    match register {
        dma::Register::ChannelDoorbell => value == 0x800040a0,
        dma::Register::EventDoorbell => value == 0x80003010,
        dma::Register::IeobClear => value == 1,
        dma::Register::IeobMask => matches!(value, 0 | 1),
        dma::Register::TypeMask => matches!(value, 0 | 0x44 | 0x4c),
        dma::Register::LocalContext => false,
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
fn receive() -> u64 {
    let (_, badge) =
        sel4::Cap::<sel4::cap_type::Endpoint>::from_bits(1)
            .nb_recv(sel4::Cap::<sel4::cap_type::Reply>::from_bits(4));
    badge
}
struct Native<'a> {
    registers: IrqRegisters,
    expected: personal_os_rust_server::sunfish_ipa_linux_config::Snapshot,
    domain: &'a pixel_smmu_install::Probe,
    firmware: &'a pixel_ipa_firmware::NativeLoader,
    setup: &'a gsi::SetupOnce,
    endpoints: &'a ep::ConfigureOnce,
    command: &'a cmd::StartOnce,
    memory: &'a dma::TransferOnce,
    stats: &'a mut Stats,
}
impl dma::Io<NativeRam<'static>> for Native<'_> {
    fn ticks(&mut self) -> u64 {
        counter()
    }
    fn guard(&mut self, pool: &Pool) -> Result<(), dma::Error> {
        if self.setup.report().phase != gsi::SetupPhase::Complete
            || self.endpoints.report().phase != ep::Phase::Complete
            || self.endpoints.report().readback != ep::COMPLETE
            || self.command.report().phase != cmd::Phase::Complete
            || !self.command.report().ack_done
            || self.memory.report().phase != dma::Phase::Complete
            || self.memory.report().error.is_some()
            || !self.memory.report().ack_done
            || self.memory.report().containment_sent
            || !pool.memory_setup_complete()
        {
            return Err(dma::Error::Guard);
        }
        let mut retained = IrqIo {
            registers: self.registers,
            expected: self.expected,
            domain: pixel_smmu_install::Access::Probe(self.domain),
            pool: Some(pool),
            firmware: Some(self.firmware),
        };
        uc::Io::guard(&mut retained).map_err(|_| dma::Error::Guard)?;
        for w in ep::PROFILE {
            if word(self.registers.ipa, w.offset()) & w.register.mask()
                != w.value & w.register.mask()
            {
                return Err(dma::Error::Guard);
            }
        }
        Ok(())
    }
    fn read(&mut self, r: dma::Read) -> Result<u32, dma::Error> {
        let base = match r {
            dma::Read::Gsi(_) | dma::Read::EventPointer => self.registers.gsi,
            dma::Read::Shared(offset) => {
                if offset % 4 != 0 || offset >= buffers::ZERO_BYTES {
                    return Err(dma::Error::Io);
                }
                self.registers.shared.cast::<u32>()
            }
            _ => self.registers.ipa,
        };
        self.stats.reads = self.stats.reads.saturating_add(1);
        self.stats.last_read = r.offset();
        // word() performs an ordered volatile load followed by DSB SY.
        // Sampling the producer therefore precedes the subsequent CPU event
        // loads from the retained uncached pool; no cache alias is present.
        let value = word(base, r.offset());
        self.stats.last_value = value;
        Ok(value)
    }
    fn write(&mut self, r: &dma::Request) -> Result<(), dma::Error> {
        if !allowed(r.register(), r.value()) {
            return Err(dma::Error::Io);
        }
        let base = self.registers.gsi;
        self.stats.writes = self.stats.writes.saturating_add(1);
        self.stats.last_write = r.register().offset();
        // SAFETY: the same audited five GSI RW/NX pages as the proven
        // memory transaction. The compiled whitelist permits only the next
        // six-command transaction's doorbells/masks; no IPA context store.
        // DSB SY orders every payload/TRE store before channel publication.
        unsafe {
            core::arch::asm!("dsb sy", options(nostack, preserves_flags));
            base.cast_mut()
                .add(r.register().offset() / 4)
                .write_volatile(r.value().to_le());
            core::arch::asm!("dsb sy", options(nostack, preserves_flags));
        }
        Ok(())
    }
    fn acknowledge(&mut self) -> Result<(), dma::Error> {
        Channel::new(pixel_ipa_gsi::CHANNEL)
            .irq_ack()
            .map_err(|_| dma::Error::Acknowledge)?;
        self.stats.acks = self.stats.acks.saturating_add(1);
        Ok(())
    }
}
fn with_tables<T>(
    state: &mut Startup,
    f: impl FnOnce(&mut TableOnce, &mut Pool, &mut Native<'_>) -> T,
) -> Option<T> {
    let mut io = Native {
        registers: state._irq_registers?,
        expected: state._config_report.as_ref()?.after?,
        domain: state._domain.as_ref()?,
        firmware: state._firmware.as_ref()?,
        setup: state._gsi.as_ref()?,
        endpoints: state._endpoint.as_ref()?,
        command: state._command.as_ref()?,
        memory: state._transfer.as_ref()?,
        stats: &mut state._table_stats,
    };
    Some(f(
        state._tables.as_mut()?,
        state._buffers.as_mut()?,
        &mut io,
    ))
}
fn show(d: &Display, state: &Startup) {
    d.clear();
    d.replace_line(0, "NATIVE IPA TABLE DMA");
    let report = state._tables.as_ref().map(|o| o.report());
    let passed = report.is_some_and(|r| r.phase == dma::Phase::Complete)
        && state
            ._buffers
            .as_ref()
            .is_some_and(|p| p.table_setup_complete());
    d.replace_line(
        1,
        if passed {
            "TABLE DMA PASS"
        } else {
            "TABLE DMA STOP"
        },
    );
    if let Some(r) = report {
        d.hex(2, "TABLE PHASE", r.phase as u32);
        d.hex(3, "CPU SENT", r.cpu_sent as u32);
        d.hex(4, "CPU DONE", r.cpu_done as u32);
        d.hex(5, "CPU CHECK", r.cpu_verified as u32);
        d.hex(6, "PUBLISHED", r.published as u32);
        d.hex(7, "TABLE WRITES", r.writes_done as u32);
        d.hex(8, "ZERO WORDS", r.zero_source_words);
        d.hex(9, "TABLE WORDS", r.table_source_words);
        d.hex(10, "TARGET WORDS", r.target_words);
        d.hex(11, "CANARY CHECKS", r.canary_checks);
        d.hex(12, "FINAL SAMPLES", r.final_samples as u32);
        d.hex(13, "EVENT PRODUCER", r.event_pointer.unwrap_or(u32::MAX));
        if let Some(e) = r.event {
            d.hex(14, "EVENT TRE", e.transfer as u32);
            d.hex(15, "EVENT CHANNEL", e.channel as u32);
            d.hex(
                16,
                "EVENT CODE TYPE",
                (e.code as u32) << 8 | e.event_type as u32,
            );
        }
        if let Some(e) = r.error {
            d.hex(17, "DMA ERROR", e as u32 + 1);
            d.hex(14, "LAST READ", state._table_stats.last_read as u32);
            d.hex(15, "LAST VALUE", state._table_stats.last_value);
        }
    }
    d.hex(18, "DMA NOTICES", state._table_stats.notices);
    d.hex(19, "DMA IRQ ACKS", state._table_stats.acks);
    d.hex(20, "LAST BADGE", state._table_stats.badge as u32);
    d.hex(21, "LAST WRITE", state._table_stats.last_write as u32);
    if let Some(f) = state._domain.as_ref().and_then(|d| d.fault_status()) {
        d.hex(22, "GLOBAL FSR", f.0);
        d.hex(23, "CONTEXT FSR", f.1);
    }
    d.hex(
        24,
        "MEMORY PASS",
        state
            ._transfer
            .as_ref()
            .is_some_and(|o| o.report().phase == dma::Phase::Complete) as u32,
    );
    d.replace_line(25, "SIX TABLE COMMANDS");
    d.replace_line(26, "NO MODEM OR WLAN START");
    d.replace_line(27, "RETAINED UNTIL REBOOT");
    d.replace_line(28, "NO FLASH OR RETRY");
    d.replace_line(29, "RESTART WHEN DONE");
    d.flush();
}
pub(super) fn start(d: &Display, state: &mut Startup) {
    if !state
        ._transfer
        .as_ref()
        .is_some_and(|c| c.report().phase == dma::Phase::Complete)
        || !state
            ._buffers
            .as_ref()
            .is_some_and(|p| p.memory_setup_complete())
    {
        return;
    }
    let hz = frequency();
    if !(1_000_000..=1_000_000_000).contains(&hz) {
        pixel_ipa_command::quarantine(state);
        d.replace_line(1, "DMA COUNTER STOP");
        d.flush();
        return;
    }
    state._tables = Some(TableOnce::new(hz).unwrap());
    d.clear();
    d.replace_line(0, "NATIVE IPA TABLE DMA");
    d.replace_line(1, "PREPARE TABLE COMMANDS");
    d.flush();
    // Old queued input cannot prove this transaction's completion. The IRQ
    // capability is already armed by the actual command-start completion.
    let badge = receive();
    if badge != 0 {
        state._table_stats.notices = state._table_stats.notices.saturating_add(1);
        state._table_stats.badge = badge;
        pixel_ipa_command::quarantine(state);
        show(d, state);
        return;
    }
    let began = with_tables(state, |once, pool, io| {
        let setup = io.setup;
        let endpoints = io.endpoints;
        let command = io.command;
        let memory = io.memory;
        once.begin(setup, endpoints, command, memory, pool, io)
    });
    if !matches!(began, Some(Ok(()))) {
        pixel_ipa_command::quarantine(state);
        show(d, state);
        return;
    }
    loop {
        let phase = state._tables.as_ref().unwrap().report().phase;
        if matches!(phase, dma::Phase::Complete | dma::Phase::Failed) {
            break;
        }
        let badge = receive();
        let now = counter();
        let result = if badge != 0 {
            state._table_stats.notices = state._table_stats.notices.saturating_add(1);
            state._table_stats.badge = badge;
            if badge != 1 << pixel_ipa_gsi::CHANNEL {
                with_tables(state, |o, p, i| o.external_fault(p, i))
            } else {
                with_tables(state, |o, p, i| o.notified(p, i, now))
            }
        } else {
            with_tables(state, |o, p, i| o.timeout(p, i, now))
        };
        if !matches!(result, Some(Ok(()))) {
            break;
        }
        core::hint::spin_loop();
    }
    if !state
        ._tables
        .as_ref()
        .is_some_and(|o| o.report().phase == dma::Phase::Complete)
    {
        pixel_ipa_command::quarantine(state);
    }
    show(d, state);
}
pub(super) fn late_notification(state: &mut Startup) {
    if state._tables.is_none() {
        pixel_ipa_transfer::late_notification(state);
        return;
    }
    state._table_stats.notices = state._table_stats.notices.saturating_add(1);
    state._table_stats.badge = 1 << pixel_ipa_gsi::CHANNEL;
    let _ = with_tables(state, |o, p, i| o.late_fault(p, i));
    pixel_ipa_command::quarantine(state);
    if super::diagnostic_display_owned() {
        return;
    }
    if let Some(r) = state._irq_registers {
        let d = unsafe { Display::new(r.pixels) };
        show(&d, state);
    }
}
