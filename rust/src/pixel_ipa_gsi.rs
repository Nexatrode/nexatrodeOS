//! One native allocation/context setup, with no channel START or TRE publish.
use super::*;
use personal_os_rust_server::{sunfish_ipa_linux_gsi as gsi, sunfish_ipa_linux_uc as uc};

pub(super) const CHANNEL: usize = 3;
const GSI_BADGE: u64 = 1 << CHANNEL;
#[derive(Default)]
pub(super) struct Stats {
    notices: u32,
    acks: u32,
    binds: u32,
    reads: u32,
    writes: u32,
    last_read: usize,
    last_value: u32,
    last_write: usize,
    badge: u64,
}
fn counter() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!("isb","mrs {v}, cntpct_el0",v=out(reg)value,options(nostack,preserves_flags));
    }
    value
}
fn frequency() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!("mrs {v}, cntfrq_el0",v=out(reg)value,options(nostack,preserves_flags));
    }
    value
}
fn code(e: gsi::Error) -> u32 {
    use gsi::Error::*;
    match e {
        NotPrepared => 1,
        HardwareLimits => 2,
        Layout => 3,
        Consumed => 4,
        Guard => 5,
        Io => 6,
        State => 7,
        Pending => 8,
        UnexpectedIrq => 9,
        Timeout => 10,
        Readback => 11,
        Acknowledge => 12,
        Binding => 13,
        CommandResult(result) => 0x100 + u32::from(result),
    }
}
struct Native<'a> {
    retained: IrqIo<'a>,
    plan: gsi::Plan,
    stats: &'a mut Stats,
}
impl Native<'_> {
    fn readable(&self, offset: usize) -> bool {
        if [
            gsi::GSI_STATUS,
            gsi::HW_PARAM_2,
            gsi::TYPE_STATUS,
            gsi::TYPE_MASK,
            gsi::CHANNEL_STATUS,
            gsi::EVENT_STATUS,
            gsi::CHANNEL_MASK,
            gsi::EVENT_MASK,
            gsi::GLOBAL_STATUS,
            gsi::GLOBAL_ENABLE,
            gsi::GENERAL_STATUS,
            gsi::GENERIC_SCRATCH,
            gsi::ERROR_LOG,
        ]
        .contains(&offset)
            || gsi::IRQ_CONFIG
                .iter()
                .chain(&gsi::IRQ_ENABLE)
                .any(|w| w.offset == offset)
        {
            return true;
        }
        (0..4).any(|i| {
            self.plan.event_words(i).unwrap()[..10]
                .iter()
                .chain(self.plan.channel_words(i, 0).unwrap().iter())
                .any(|w| w.offset == offset)
        })
    }
    fn writable(&self, word: gsi::Word) -> bool {
        let value = word.value;
        match word.offset {
            gsi::TYPE_MASK => matches!(value, 0 | 4 | 0x44 | 0x45 | 0x46),
            gsi::CHANNEL_MASK | gsi::EVENT_MASK => {
                value == 0 || (value.is_power_of_two() && value <= 8)
            }
            gsi::CHANNEL_CLEAR | gsi::EVENT_CLEAR => {
                value == u32::MAX || (value.is_power_of_two() && value <= 8)
            }
            gsi::CHANNEL_COMMAND | gsi::EVENT_COMMAND => value < 4,
            gsi::GENERIC_COMMAND => [0x402, 0x422, 0x442, 0x462].contains(&value),
            gsi::GLOBAL_ENABLE => matches!(value, 0 | 1 | 3),
            gsi::GLOBAL_CLEAR => value == 2,
            gsi::GENERIC_SCRATCH => value & (7 << 5) == 0,
            _ => {
                gsi::IRQ_CONFIG
                    .iter()
                    .chain(&gsi::IRQ_ENABLE)
                    .any(|w| w.offset == word.offset && w.value == value)
                    || (word.offset == gsi::ERROR_LOG && value == 0)
                    || (0..4).any(|i| {
                        self.plan
                            .event_words(i)
                            .unwrap()
                            .iter()
                            .chain(self.plan.channel_words(i, 0).unwrap().iter())
                            .any(|w| {
                                w.offset == word.offset
                                    && if word.offset == 0x1c06c + i * 0x80 {
                                        value >> 16 == 32
                                    } else {
                                        w.value == value
                                    }
                            })
                    })
            }
        }
    }
}
impl gsi::Io for Native<'_> {
    fn ticks(&mut self) -> u64 {
        counter()
    }
    fn guard(&mut self) -> Result<(), gsi::Error> {
        uc::Io::guard(&mut self.retained).map_err(|_| gsi::Error::Guard)
    }
    fn read(&mut self, offset: usize) -> Result<u32, gsi::Error> {
        if !self.readable(offset) {
            return Err(gsi::Error::Io);
        }
        self.stats.reads = self.stats.reads.saturating_add(1);
        self.stats.last_read = offset;
        let value = word(self.retained.registers.gsi, offset);
        self.stats.last_value = value;
        Ok(value)
    }
    fn write(&mut self, request: &gsi::Request) -> Result<(), gsi::Error> {
        let w = request.word();
        if !self.writable(w) {
            return Err(gsi::Error::Io);
        }
        self.stats.writes = self.stats.writes.saturating_add(1);
        self.stats.last_write = w.offset;
        // SAFETY: actual opt-in CapDL audit grants exactly GSI pages c/1c/1d/
        // 1e/1f, uncached RW/NX. Closed requests and this whitelist restrict
        // them to pinned setup words, owned ring IOVAs and ALLOCATE opcodes.
        // All foreign GSI pages remain RO; memory/firmware/domain stay held.
        unsafe {
            core::arch::asm!("dsb sy", options(nostack, preserves_flags));
            self.retained
                .registers
                .gsi
                .cast_mut()
                .add(w.offset / 4)
                .write_volatile(w.value.to_le());
            core::arch::asm!("dsb sy", options(nostack, preserves_flags));
        }
        Ok(())
    }
    fn acknowledge(&mut self) -> Result<(), gsi::Error> {
        Channel::new(CHANNEL)
            .irq_ack()
            .map_err(|_| gsi::Error::Acknowledge)?;
        self.stats.acks = self.stats.acks.saturating_add(1);
        Ok(())
    }
}
impl gsi::SetupIo for Native<'_> {
    fn bind(&mut self) -> Result<(), gsi::Error> {
        // CapDL binds level-high IRQ 464 to this owner's channel 3. The
        // initial ACK arms it only after all nine GSI masks are disabled.
        Channel::new(CHANNEL)
            .irq_ack()
            .map_err(|_| gsi::Error::Binding)?;
        self.stats.binds = self.stats.binds.saturating_add(1);
        Ok(())
    }
}
fn with_native<T>(
    state: &mut Startup,
    f: impl FnOnce(&mut gsi::SetupOnce, &mut Native<'_>) -> T,
) -> Option<T> {
    let registers = state._irq_registers?;
    let expected = state._config_report.as_ref()?.after?;
    let domain = state._domain.as_ref()?;
    let pool = state._buffers.as_ref()?;
    let firmware = state._firmware.as_ref()?;
    let plan = state._gsi.as_ref()?.plan();
    let retained = IrqIo {
        registers,
        expected,
        domain: pixel_smmu_install::Access::Probe(domain),
        pool: Some(pool),
        firmware: Some(firmware),
    };
    let mut io = Native {
        retained,
        plan,
        stats: &mut state._gsi_stats,
    };
    Some(f(state._gsi.as_mut()?, &mut io))
}
fn show(d: &Display, state: &Startup) {
    let Some(once) = state._gsi.as_ref() else {
        return;
    };
    let r = once.report();
    let s = &state._gsi_stats;
    d.clear();
    d.replace_line(0, "NATIVE IPA GSI V2");
    d.replace_line(
        1,
        if r.phase == gsi::SetupPhase::Complete {
            "GSI ALLOCATION PASS"
        } else {
            "GSI SETUP STOP"
        },
    );
    d.hex(2, "GSI PHASE", r.phase as u32);
    d.hex(3, "CHANNEL INDEX", u32::from(r.index));
    d.hex(4, "EVENT ALLOC", u32::from(r.events_allocated));
    d.hex(5, "EVENT PROGRAM", u32::from(r.events_programmed));
    d.hex(6, "CHAN ALLOC", u32::from(r.channels_allocated));
    d.hex(7, "CHAN PROGRAM", u32::from(r.channels_programmed));
    d.hex(8, "MODEM ACCEPT", u32::from(r.modem_accepted));
    d.hex(9, "MODEM FRESH", u32::from(r.modem_fresh));
    d.hex(10, "MODEM EXISTING", u32::from(r.modem_existing));
    d.hex(11, "GSI NOTICES", s.notices);
    d.hex(12, "GSI IRQ ACKS", s.acks);
    d.hex(13, "INITIAL ARM", s.binds);
    d.hex(14, "GSI WRITES", s.writes);
    d.hex(15, "LAST WRITE", s.last_write as u32);
    d.hex(16, "LAST READ", s.last_read as u32);
    d.hex(17, "LAST BADGE", s.badge as u32);
    if let Some(e) = r.error {
        d.hex(18, "GSI ERROR", code(e));
    }
    if let Some(raw) = r.error_log_after {
        d.hex(19, "ERROR LOG POST", raw);
    }
    if let Some(e) = r.containment_error {
        d.hex(19, "MASK ERROR", code(e));
    }
    if let Some(p) = state._domain.as_ref().and_then(|d| d.fault_status()) {
        d.hex(20, "GLOBAL FSR", p.0);
        d.hex(21, "CONTEXT FSR", p.1);
    }
    if let Some(p) = state._firmware.as_ref() {
        d.hex(22, "PAS RETURNED", p.pas_report().returned_mask);
    }
    d.hex(23, "LAST VALUE", s.last_value);
    d.replace_line(24, "NO CHANNEL START OR TRE");
    d.replace_line(25, "NO MODEM OR WLAN START");
    d.replace_line(26, "RETAINED UNTIL REBOOT");
    d.replace_line(27, "NO FLASH OR RETRY");
    if let Some(raw) = r.error_log_before {
        d.hex(28, "ERROR LOG PRE", raw);
    }
    d.replace_line(29, "RESTART WHEN DONE");
    d.flush();
}
pub(super) fn start(d: &Display, state: &mut Startup) {
    let valid = state._firmware.as_ref().is_some_and(|f| {
        f.report().phase == personal_os_rust_server::sunfish_ipa_linux_loader::Phase::AuthReturned
            && f.pas_report().passed()
    });
    if !valid {
        return;
    }
    d.clear();
    d.replace_line(0, "NATIVE IPA GSI SETUP");
    d.replace_line(1, "CHECK GSI PREREQUISITES");
    d.flush();
    let hz = frequency();
    if !(1_000_000..=1_000_000_000).contains(&hz) {
        d.replace_line(1, "GSI COUNTER STOP");
        d.flush();
        return;
    }
    let plan = state._buffers.as_ref().and_then(|p| {
        state
            ._irq_registers
            .and_then(|r| gsi::Plan::from_prepared(p, word(r.gsi, gsi::HW_PARAM_2)).ok())
    });
    let Some(plan) = plan else {
        d.replace_line(1, "GSI BUFFER STOP");
        d.flush();
        return;
    };
    state._gsi = Some(gsi::SetupOnce::new(plan, gsi::command_timeout_ticks(hz).unwrap()).unwrap());
    d.replace_line(1, "ALLOCATE GSI RESOURCES");
    d.flush();
    let started = with_native(state, |once, io| once.start(io));
    if !matches!(started, Some(Ok(()))) {
        show(d, state);
        return;
    }
    loop {
        let phase = state._gsi.as_ref().unwrap().report().phase;
        if matches!(phase, gsi::SetupPhase::Complete | gsi::SetupPhase::Failed) {
            break;
        }
        // Same input/reply capability slots used by Microkit's recv(), with
        // nonblocking receive so a missing physical IRQ has a bounded wait.
        // No client IPC endpoints are granted by the actual CapDL audit.
        let (_, badge) =
            sel4::Cap::<sel4::cap_type::Endpoint>::from_bits(1)
                .nb_recv(sel4::Cap::<sel4::cap_type::Reply>::from_bits(4));
        let now = counter();
        if badge != 0 {
            state._gsi_stats.notices = state._gsi_stats.notices.saturating_add(1);
            state._gsi_stats.badge = badge;
            if badge != GSI_BADGE {
                let _ = with_native(state, |once, io| once.external_fault(io));
                dispatch_uc(state, true);
                if let Some(pool) = state._buffers.as_mut() {
                    pool.quarantine();
                }
                if let Some(domain) = state._domain.as_mut() {
                    domain.stop_on_fault();
                }
                break;
            }
            if !matches!(
                with_native(state, |once, io| once.notified(io, now)),
                Some(Ok(()))
            ) {
                break;
            }
        } else if !matches!(
            with_native(state, |once, io| once.timeout(io, now)),
            Some(Ok(()))
        ) {
            break;
        }
        core::hint::spin_loop();
    }
    show(d, state);
}
pub(super) fn late_notification(state: &mut Startup) {
    state._gsi_stats.notices = state._gsi_stats.notices.saturating_add(1);
    state._gsi_stats.badge = GSI_BADGE;
    let _ = with_native(state, |once, io| once.late_notification(io));
    if super::diagnostic_display_owned() {
        return;
    }
    if let Some(registers) = state._irq_registers {
        let d = unsafe { Display::new(registers.pixels) };
        show(&d, state);
    }
}
