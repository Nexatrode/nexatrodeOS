#![no_std]
#![no_main]

//! Read-only snapshot of the GCC UFS power, reset, clock-root and gate state.
//! The probe deliberately avoids UFSHCI/PHY MMIO and performs no writes.

#[path = "freestanding.rs"]
mod freestanding;
#[allow(dead_code)]
mod pixel_probe_display;
#[path = "qcom_ufs_gcc.rs"]
mod qcom_ufs_gcc;

use pixel_probe_display::Display;
use qcom_ufs_gcc::{Gate, NotReady, Root, UfsGccSnapshot};
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, memory_region_symbol,
    protection_domain,
};

fn read_register(base: *const u32, offset: u32) -> u32 {
    unsafe { core::arch::asm!("dsb sy", options(nostack, preserves_flags)) };
    let value = unsafe { base.add((offset / 4) as usize).read_volatile() };
    unsafe { core::arch::asm!("dsb sy", options(nostack, preserves_flags)) };
    u32::from_le(value)
}

fn checkpoint_read(display: &Display, base: *const u32, offset: u32, label: &str) -> u32 {
    display.replace_line(1, "READING");
    display.hex(2, "OFFSET", offset);
    display.replace_line(3, label);
    display.flush();

    let value = read_register(base, offset);

    display.replace_line(1, "READ OK");
    display.hex(2, label, value);
    display.flush();
    value
}

fn mapping_checkpoint(
    display: &Display,
    stage: &str,
    label: &str,
    address: usize,
    expected: usize,
    completed: &str,
) -> bool {
    display.replace_line(1, stage);
    display.hex(2, label, address as u32);
    let valid = address == expected;
    display.replace_line(3, if valid { completed } else { "ADDR INVALID" });
    display.flush();
    valid
}

fn readiness_label(result: Result<(), NotReady>) -> &'static str {
    match result {
        Ok(()) => "GCC SUBSET READY",
        Err(NotReady::DomainOff) => "UFS DOMAIN OFF",
        Err(NotReady::PhyResetAsserted) => "PHY RESET ASSERTED",
        Err(NotReady::RootUpdatePending(Root::Axi)) => "AXI RCG UPDATE",
        Err(NotReady::RootUpdatePending(Root::Unipro)) => "UNIPRO RCG UPDATE",
        Err(NotReady::RootUpdatePending(Root::Ice)) => "ICE RCG UPDATE",
        Err(NotReady::RootUpdatePending(Root::PhyAux)) => "AUX RCG UPDATE",
        Err(NotReady::RootOff(Root::Axi)) => "AXI ROOT OFF",
        Err(NotReady::RootOff(Root::Unipro)) => "UNIPRO ROOT OFF",
        Err(NotReady::RootOff(Root::Ice)) => "ICE ROOT OFF",
        Err(NotReady::RootOff(Root::PhyAux)) => "AUX ROOT OFF",
        Err(NotReady::RootConfigurationMismatch(Root::Axi)) => "AXI RCG CONFIG",
        Err(NotReady::RootConfigurationMismatch(Root::Unipro)) => "UNIPRO RCG CONFIG",
        Err(NotReady::RootConfigurationMismatch(Root::Ice)) => "ICE RCG CONFIG",
        Err(NotReady::RootConfigurationMismatch(Root::PhyAux)) => "AUX RCG CONFIG",
        Err(NotReady::ClockGateOff(Gate::Ahb)) => "AHB GATE OFF",
        Err(NotReady::ClockGateOff(Gate::Axi)) => "AXI GATE OFF",
        Err(NotReady::ClockGateOff(Gate::Unipro)) => "UNIPRO GATE OFF",
        Err(NotReady::ClockGateOff(Gate::Ice)) => "ICE GATE OFF",
        Err(NotReady::ClockGateOff(Gate::PhyAux)) => "AUX GATE OFF",
        Err(NotReady::ClockGateOff(Gate::TxSymbol)) => "TX SYMBOL GATE OFF",
        Err(NotReady::ClockGateOff(Gate::RxSymbol)) => "RX SYMBOL GATE OFF",
        Err(NotReady::ClockGateOff(Gate::AggregateAxi)) => "AGGREGATE AXI OFF",
        Err(NotReady::ClockGateOff(Gate::MemReference)) => "MEM REF GATE OFF",
        Err(NotReady::ClockHalted(Gate::Ahb)) => "AHB CLOCK HALTED",
        Err(NotReady::ClockHalted(Gate::Axi)) => "AXI CLOCK HALTED",
        Err(NotReady::ClockHalted(Gate::Unipro)) => "UNIPRO CLOCK HALTED",
        Err(NotReady::ClockHalted(Gate::Ice)) => "ICE CLOCK HALTED",
        Err(NotReady::ClockHalted(Gate::PhyAux)) => "AUX CLOCK HALTED",
        Err(NotReady::ClockHalted(Gate::TxSymbol)) => "TX SYMBOL HALTED",
        Err(NotReady::ClockHalted(Gate::RxSymbol)) => "RX SYMBOL HALTED",
        Err(NotReady::ClockHalted(Gate::AggregateAxi)) => "AGGREGATE AXI HALTED",
        Err(NotReady::ClockHalted(Gate::MemReference)) => "MEM REF HALTED",
    }
}

#[protection_domain]
fn init() -> UfsGccSnapshotProbe {
    let display = unsafe { Display::new(memory_region_symbol!(splash_fb: *mut u32).as_ptr()) };
    display.clear();
    display.line(0, "UFS GCC READ ONLY");
    display.line(1, "SNAPSHOT START");
    display.flush();

    display.replace_line(1, "MAP LOCAL");
    display.flush();
    let local_vaddr = *sel4_microkit::var!(gcc_ufs_local: usize = 0);
    if !mapping_checkpoint(
        &display,
        "MAP LOCAL",
        "LOCAL VADDR",
        local_vaddr,
        0x6400_0000,
        "LOCAL MAPPED",
    ) {
        display.line(4, "NO MMIO READ");
        display.line(29, "RESTART WHEN DONE");
        display.flush();
        return UfsGccSnapshotProbe;
    }
    let local = local_vaddr as *const u32;

    display.replace_line(1, "MAP AGGREGATE");
    display.flush();
    let aggregate_vaddr = *sel4_microkit::var!(gcc_ufs_aggregate: usize = 0);
    if !mapping_checkpoint(
        &display,
        "MAP AGGREGATE",
        "AGG VADDR",
        aggregate_vaddr,
        0x6400_1000,
        "AGG MAPPED",
    ) {
        display.line(4, "NO MMIO READ");
        display.line(29, "RESTART WHEN DONE");
        display.flush();
        return UfsGccSnapshotProbe;
    }
    let aggregate = aggregate_vaddr as *const u32;

    display.replace_line(1, "MAP MEM REF");
    display.flush();
    let mem_ref_vaddr = *sel4_microkit::var!(gcc_ufs_mem_ref: usize = 0);
    if !mapping_checkpoint(
        &display,
        "MAP MEM REF",
        "MEM REF VADDR",
        mem_ref_vaddr,
        0x6400_2000,
        "MEM REF MAPPED",
    ) {
        display.line(4, "NO MMIO READ");
        display.line(29, "RESTART WHEN DONE");
        display.flush();
        return UfsGccSnapshotProbe;
    }
    let mem_ref = mem_ref_vaddr as *const u32;

    let gdsc = checkpoint_read(&display, local, 0x004, "GDSCR");
    let phy_bcr = checkpoint_read(&display, local, 0x000, "PHY BCR");
    let axi_cmd = checkpoint_read(&display, local, 0x020, "AXI CMD");
    let axi_cfg = checkpoint_read(&display, local, 0x024, "AXI CFG");
    let unipro_cmd = checkpoint_read(&display, local, 0x060, "UNIPRO CMD");
    let unipro_cfg = checkpoint_read(&display, local, 0x064, "UNIPRO CFG");
    let ice_cmd = checkpoint_read(&display, local, 0x048, "ICE CMD");
    let ice_cfg = checkpoint_read(&display, local, 0x04c, "ICE CFG");
    let aux_cmd = checkpoint_read(&display, local, 0x098, "AUX CMD");
    let aux_cfg = checkpoint_read(&display, local, 0x09c, "AUX CFG");
    let ahb_cbcr = checkpoint_read(&display, local, 0x014, "AHB CBCR");
    let tx_cbcr = checkpoint_read(&display, local, 0x018, "TX SYM CBCR");
    let rx_cbcr = checkpoint_read(&display, local, 0x01c, "RX SYM CBCR");
    let axi_cbcr = checkpoint_read(&display, local, 0x038, "AXI CBCR");
    let unipro_cbcr = checkpoint_read(&display, local, 0x08c, "UNIPRO CBCR");
    let ice_cbcr = checkpoint_read(&display, local, 0x090, "ICE CBCR");
    let aux_cbcr = checkpoint_read(&display, local, 0x094, "AUX CBCR");
    let aggregate_cbcr = checkpoint_read(&display, aggregate, 0x024, "AGGR AXI CBCR");
    let mem_ref_cbcr = checkpoint_read(&display, mem_ref, 0x000, "MEM REF CBCR");

    let gcc_state = UfsGccSnapshot {
        gdsc,
        phy_bcr,
        axi_command: axi_cmd,
        axi_config: axi_cfg,
        unipro_command: unipro_cmd,
        unipro_config: unipro_cfg,
        ice_command: ice_cmd,
        ice_config: ice_cfg,
        phy_aux_command: aux_cmd,
        phy_aux_config: aux_cfg,
        ahb_cbcr,
        axi_cbcr,
        unipro_cbcr,
        ice_cbcr,
        phy_aux_cbcr: aux_cbcr,
        tx_symbol_cbcr: tx_cbcr,
        rx_symbol_cbcr: rx_cbcr,
        aggregate_axi_cbcr: aggregate_cbcr,
        mem_reference_cbcr: mem_ref_cbcr,
    };
    let readiness = readiness_label(gcc_state.check_gcc_pre_cap_state());

    display.clear();
    display.line(0, "UFS GCC SNAPSHOT");
    display.hex(1, "GDSCR", gdsc);
    display.hex(2, "PHY BCR", phy_bcr);
    display.hex(3, "AXI CMD", axi_cmd);
    display.hex(4, "AXI CFG", axi_cfg);
    display.hex(5, "UNIPRO CMD", unipro_cmd);
    display.hex(6, "UNIPRO CFG", unipro_cfg);
    display.hex(7, "ICE CMD", ice_cmd);
    display.hex(8, "ICE CFG", ice_cfg);
    display.hex(9, "AUX CMD", aux_cmd);
    display.hex(10, "AUX CFG", aux_cfg);
    display.hex(11, "AHB CBCR", ahb_cbcr);
    display.hex(12, "AXI CBCR", axi_cbcr);
    display.hex(13, "UNIPRO CBCR", unipro_cbcr);
    display.hex(14, "ICE CBCR", ice_cbcr);
    display.hex(15, "AUX CBCR", aux_cbcr);
    display.hex(16, "TX SYM CBCR", tx_cbcr);
    display.hex(17, "RX SYM CBCR", rx_cbcr);
    display.hex(18, "AGGR AXI CBCR", aggregate_cbcr);
    display.hex(19, "MEM REF CBCR", mem_ref_cbcr);
    display.line(20, "GCC PRE CAP CHECK");
    display.line(21, readiness);
    display.line(22, "GCC READS ONLY");
    display.line(23, "NO HCI MMIO");
    display.line(24, "NO UFS COMMAND");
    display.line(25, "NO DMA OR SMMU");
    display.line(26, "NO POWER OR RESET WRITE");
    display.line(29, "RESTART WHEN DONE");
    display.flush();

    debug_println!(
        "UFS-GCC-RO|DONE|gdsc={:08X}|bcr={:08X}|axi={:08X}:{:08X}|unipro={:08X}:{:08X}|ice={:08X}:{:08X}|aux={:08X}:{:08X}|cbcr={:08X},{:08X},{:08X},{:08X},{:08X},{:08X},{:08X},{:08X},{:08X}",
        gdsc,
        phy_bcr,
        axi_cmd,
        axi_cfg,
        unipro_cmd,
        unipro_cfg,
        ice_cmd,
        ice_cfg,
        aux_cmd,
        aux_cfg,
        ahb_cbcr,
        axi_cbcr,
        unipro_cbcr,
        ice_cbcr,
        aux_cbcr,
        tx_cbcr,
        rx_cbcr,
        aggregate_cbcr,
        mem_ref_cbcr
    );
    UfsGccSnapshotProbe
}

struct UfsGccSnapshotProbe;

impl Handler for UfsGccSnapshotProbe {
    type Error = Infallible;

    fn notified(&mut self, _: ChannelSet) -> Result<(), Self::Error> {
        Ok(())
    }

    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}
