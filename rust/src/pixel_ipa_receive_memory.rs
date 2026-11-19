//! Concrete receive RAM handoff/preparation after actual memory/table DMA.
//! No RX channel START, receive TRE, receive doorbell or packet access.
use super::*;
use personal_os_rust_server::{
    sunfish_ipa_linux_command as cmd, sunfish_ipa_linux_endpoint as ep,
    sunfish_ipa_linux_gsi as gsi, sunfish_ipa_linux_receive as rx,
    sunfish_ipa_linux_receive_pool::{self as data, DataPool},
    sunfish_ipa_linux_table_transfer::TableOnce,
    sunfish_ipa_linux_transfer as dma, sunfish_ipa_linux_uc as uc,
};

struct Guard<'a> {
    irq: IrqIo<'a>,
    setup: &'a gsi::SetupOnce,
    endpoints: &'a ep::ConfigureOnce,
    command: &'a cmd::StartOnce,
    memory: &'a dma::TransferOnce,
    tables: &'a TableOnce,
}
impl Guard<'_> {
    fn check(&mut self) -> Result<(), data::Error> {
        if self.setup.report().phase != gsi::SetupPhase::Complete
            || self.setup.report().error.is_some()
            || self.setup.report().mask_sent
            || self.endpoints.report().phase != ep::Phase::Complete
            || self.endpoints.report().error.is_some()
            || self.endpoints.report().readback != ep::COMPLETE
            || self.command.report().phase != cmd::Phase::Complete
            || self.command.report().error.is_some()
            || !self.command.report().ack_done
            || self.command.report().mask_sent
            || self.memory.report().phase != dma::Phase::Complete
            || self.memory.report().error.is_some()
            || !self.memory.report().ack_done
            || self.memory.report().containment_sent
            || self.memory.report().target_words != 1619
            || self.memory.report().canary_checks != 50
            || self.memory.report().final_samples != 2
            || self.tables.report().phase != dma::Phase::Complete
            || self.tables.report().error.is_some()
            || !self.tables.report().ack_done
            || self.tables.report().containment_sent
            || self.tables.report().target_words != 120
            || self.irq.pool.is_none_or(|p| !p.table_setup_complete())
        {
            return Err(data::Error::Guard);
        }
        uc::Io::guard(&mut self.irq).map_err(|_| data::Error::Guard)?;
        for spec in ep::PROFILE {
            if word(self.irq.registers.ipa, spec.offset()) & spec.register.mask()
                != spec.value & spec.register.mask()
            {
                return Err(data::Error::Guard);
            }
        }
        for (r, expected) in [
            (cmd::Read::Status, 1),
            (cmd::Read::TypeMask, 0x4c),
            (cmd::Read::ChannelMask, 0),
            (cmd::Read::EventMask, 0),
            (cmd::Read::GlobalEnable, 1),
            (cmd::Read::GeneralEnable, 14),
            (cmd::Read::IeobMask, 1),
            (cmd::Read::TypeStatus, 0),
            (cmd::Read::ChannelStatus, 0),
            (cmd::Read::EventStatus, 0),
            (cmd::Read::GlobalStatus, 0),
            (cmd::Read::GeneralStatus, 0),
            (cmd::Read::IeobStatus, 0),
        ] {
            if word(self.irq.registers.gsi, r.offset()) != expected {
                return Err(data::Error::Guard);
            }
        }
        for e in ep::ENDPOINTS {
            let expected = if e.channel() == 1 { 2 } else { 1 };
            if (word(self.irq.registers.gsi, cmd::Read::Channel(e).offset()) >> 20) & 15 != expected
                || (word(self.irq.registers.gsi, cmd::Read::Event(e).offset()) >> 20) & 15 != 1
            {
                return Err(data::Error::Guard);
            }
        }
        Ok(())
    }
}
pub(super) fn show(d: &Display, state: &Startup) {
    d.clear();
    d.replace_line(0, "NATIVE IPA RX MEMORY");
    let passed = state._receive_data.as_ref().is_some_and(|p| p.prepared())
        && state._receive_prepare.is_some_and(|r| r.error.is_none())
        && state._receive_guards.is_some_and(|r| r.error.is_none());
    d.replace_line(
        1,
        if passed {
            "RX MEMORY PREP PASS"
        } else {
            "RX MEMORY PREP STOP"
        },
    );
    d.hex(2, "SMMU TABLES", 8);
    d.hex(3, "DMA DATA PAGES", rx::DATA_USED_PAGES as u32);
    d.hex(4, "CPU RAM PAGES", (rx::DATA_RESERVED_BYTES / 4096) as u32);
    if let Some(r) = state._receive_prepare {
        d.hex(5, "PREP SENT", r.writes_sent as u32);
        d.hex(6, "PREP DONE", r.writes_done as u32);
        d.hex(7, "WORDS CHECKED", r.checked_words as u32);
        d.hex(8, "READ PASSES", r.passes as u32);
        d.hex(17, "PREP ERROR", r.error.map_or(0, |e| e as u32 + 1));
        if let Some(offset) = r.offset {
            d.hex(18, "PREP OFFSET", offset as u32);
        }
    }
    if let Some(r) = state._receive_guards {
        d.hex(9, "GUARD WORDS", r.checked_words as u32);
        d.hex(19, "GUARD ERROR", r.error.map_or(0, |e| e as u32 + 1));
    }
    d.hex(10, "CPU BASE VA", 0x76001000);
    d.hex(11, "RX IOVA", rx::DATA_IOVA as u32);
    d.hex(
        12,
        "DATA PA LOW",
        state._receive_physical.unwrap_or(0) as u32,
    );
    d.hex(13, "TAIL CPU ONLY", 10);
    d.hex(
        14,
        "TABLE DMA PASS",
        state
            ._tables
            .as_ref()
            .is_some_and(|t| t.report().phase == dma::Phase::Complete) as u32,
    );
    if let Some((g, c)) = state._domain.as_ref().and_then(|v| v.fault_status()) {
        d.hex(15, "GLOBAL FSR", g);
        d.hex(16, "CONTEXT FSR", c);
    }
    d.replace_line(22, "NO RX CHANNEL START");
    d.replace_line(23, "NO RECEIVE DOORBELL");
    d.replace_line(24, "NO RX PACKET ACCESS");
    d.replace_line(26, "NO MODEM OR WLAN START");
    d.replace_line(27, "RETAINED UNTIL REBOOT");
    d.replace_line(28, "NO FLASH OR RETRY");
    d.replace_line(29, "RESTART WHEN DONE");
    d.flush();
}
pub(super) fn prepare(d: &Display, state: &mut Startup) {
    if state._receive_data.is_some()
        || state._receive_prepare.is_some()
        || state
            ._tables
            .as_ref()
            .is_none_or(|t| t.report().phase != dma::Phase::Complete)
    {
        return;
    }
    d.clear();
    d.replace_line(0, "NATIVE IPA RX MEMORY");
    d.replace_line(1, "HANDOFF RECEIVE RAM");
    d.flush();
    let memory = match state
        ._domain
        .as_mut()
        .and_then(|p| p.take_receive_memory().ok())
    {
        Some(memory) => memory,
        None => {
            pixel_ipa_command::quarantine(state);
            show(d, state);
            return;
        }
    };
    state._receive_physical = Some(memory.physical_base());
    let pool = match DataPool::new(memory) {
        Ok(pool) => pool,
        Err(_) => {
            pixel_ipa_command::quarantine(state);
            show(d, state);
            return;
        }
    };
    // Retain before the first potentially uncertain CPU store. The backend
    // is the one transferred by the installed eight-table domain owner.
    state._receive_data = Some(pool);
    let result = (|| {
        let mut guard = Guard {
            irq: IrqIo {
                registers: state._irq_registers?,
                expected: state._config_report.as_ref()?.after?,
                domain: pixel_smmu_install::Access::Probe(state._domain.as_ref()?),
                pool: state._buffers.as_ref(),
                firmware: state._firmware.as_ref(),
            },
            setup: state._gsi.as_ref()?,
            endpoints: state._endpoint.as_ref()?,
            command: state._command.as_ref()?,
            memory: state._transfer.as_ref()?,
            tables: state._tables.as_ref()?,
        };
        let data = state._receive_data.as_mut()?;
        d.replace_line(1, "PREPARE FULL RX RAM");
        d.flush();
        let prepared = data.prepare(|| guard.check());
        state._receive_prepare = Some(prepared);
        if prepared.error.is_some() {
            return Some(false);
        }
        let guards = data.verify_guards(|| guard.check());
        state._receive_guards = Some(guards);
        Some(guards.error.is_none() && guard.check().is_ok())
    })();
    if result != Some(true) {
        pixel_ipa_command::quarantine(state);
    }
    show(d, state);
}
