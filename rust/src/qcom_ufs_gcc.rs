//! Read-only Sunfish GCC UFS-state decoder for the pre-CAP stage.
//!
//! Register offsets and bit meanings follow the pinned Sunfish Lineage source
//! (`28f9290ae067d3e7857126eb6312720dbe9c7c80`). This is a pure decoder: it
//! neither maps GCC nor writes clocks, power domains, resets, or RPMh votes.

pub const GCC_BASE: u64 = 0x0010_0000;
pub const UFS_GDSCR_OFFSET: u32 = 0x77004;
pub const UFS_PHY_BCR_OFFSET: u32 = 0x77000;
pub const GDSC_POWER_ON: u32 = 1 << 31;
pub const GDSC_HW_CONTROL: u32 = 1 << 1;
pub const GDSC_SW_COLLAPSE: u32 = 1;
pub const RESET_ASSERTED: u32 = 1;
pub const RCG_CMD_UPDATE: u32 = 1;
pub const RCG_ROOT_OFF: u32 = 1 << 31;
pub const RCG_CFG_CHECK_MASK: u32 = 0x0000_071f;
const RCG_CFG_DIVIDER_MASK: u32 = 0x1f;
pub const CBCR_ENABLE: u32 = 1;
pub const CBCR_CLOCK_OFF: u32 = 1 << 31;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Root {
    Axi,
    Unipro,
    Ice,
    PhyAux,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RootSpec {
    pub root: Root,
    pub command_offset: u32,
    pub config_offset: u32,
    pub source_select: u8,
    /// RCG2 half-integer divider encoding (`2 * divisor - 1`).
    pub hid_divider: u8,
    pub target_rate_hz: u32,
}

impl RootSpec {
    pub const fn expected_config(self) -> u32 {
        ((self.source_select as u32) << 8) | self.hid_divider as u32
    }

    /// Match the selected parent and effective divider for this source.
    ///
    /// The pinned `clk_rcg2_calc_rate()` treats HID 0 as the parent rate and
    /// HID 1 as `parent * 2 / 2`, so both encodings produce the same rate for
    /// a divide-by-one source such as the 19.2 MHz UFS PHY auxiliary clock.
    pub const fn configuration_matches(self, config: u32) -> bool {
        let observed = config & RCG_CFG_CHECK_MASK;
        let expected = self.expected_config() & RCG_CFG_CHECK_MASK;
        if self.hid_divider == 1 && observed & RCG_CFG_DIVIDER_MASK == 0 {
            observed & !RCG_CFG_DIVIDER_MASK == expected & !RCG_CFG_DIVIDER_MASK
        } else {
            observed == expected
        }
    }
}

pub const UFS_RATE_ROOTS: [RootSpec; 4] = [
    RootSpec {
        root: Root::Axi,
        command_offset: 0x77020,
        config_offset: 0x77024,
        source_select: 1,
        hid_divider: 5,
        target_rate_hz: 200_000_000,
    },
    RootSpec {
        root: Root::Unipro,
        command_offset: 0x77060,
        config_offset: 0x77064,
        source_select: 1,
        hid_divider: 7,
        target_rate_hz: 150_000_000,
    },
    RootSpec {
        root: Root::Ice,
        command_offset: 0x77048,
        config_offset: 0x7704c,
        source_select: 1,
        hid_divider: 3,
        target_rate_hz: 300_000_000,
    },
    RootSpec {
        root: Root::PhyAux,
        command_offset: 0x77098,
        config_offset: 0x7709c,
        source_select: 0,
        hid_divider: 1,
        target_rate_hz: 19_200_000,
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Gate {
    Ahb,
    Axi,
    Unipro,
    Ice,
    PhyAux,
    TxSymbol,
    RxSymbol,
    AggregateAxi,
    MemReference,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UfsGccSnapshot {
    pub gdsc: u32,
    pub phy_bcr: u32,
    pub axi_command: u32,
    pub axi_config: u32,
    pub unipro_command: u32,
    pub unipro_config: u32,
    pub ice_command: u32,
    pub ice_config: u32,
    pub phy_aux_command: u32,
    pub phy_aux_config: u32,
    pub ahb_cbcr: u32,
    pub axi_cbcr: u32,
    pub unipro_cbcr: u32,
    pub ice_cbcr: u32,
    pub phy_aux_cbcr: u32,
    pub tx_symbol_cbcr: u32,
    pub rx_symbol_cbcr: u32,
    pub aggregate_axi_cbcr: u32,
    pub mem_reference_cbcr: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotReady {
    DomainOff,
    PhyResetAsserted,
    RootUpdatePending(Root),
    RootOff(Root),
    RootConfigurationMismatch(Root),
    ClockGateOff(Gate),
    ClockHalted(Gate),
}

fn root_is_ready(root: Root, command: u32, config: u32, spec: RootSpec) -> Result<(), NotReady> {
    if command & RCG_CMD_UPDATE != 0 {
        return Err(NotReady::RootUpdatePending(root));
    }
    // Linux's clk_rcg2_is_enabled() checks the hardware ROOT_OFF status bit;
    // ROOT_EN is a request bit and is not itself the active-state indicator.
    if command & RCG_ROOT_OFF != 0 {
        return Err(NotReady::RootOff(root));
    }
    if !spec.configuration_matches(config) {
        return Err(NotReady::RootConfigurationMismatch(root));
    }
    Ok(())
}

fn gate_is_ready(gate: Gate, value: u32, halt_status_is_valid: bool) -> Result<(), NotReady> {
    if value & CBCR_ENABLE == 0 {
        return Err(NotReady::ClockGateOff(gate));
    }
    if halt_status_is_valid && value & CBCR_CLOCK_OFF != 0 {
        return Err(NotReady::ClockHalted(gate));
    }
    Ok(())
}

impl UfsGccSnapshot {
    /// Verify only the GCC-observable subset of UFS pre-CAP readiness.
    /// RPMh reference clocks, PMIC rails, HBA regulator, PHY power state,
    /// interconnect votes, and exclusive ownership need separate live proofs.
    pub fn check_gcc_pre_cap_state(self) -> Result<(), NotReady> {
        if self.gdsc & GDSC_POWER_ON == 0 {
            return Err(NotReady::DomainOff);
        }
        if self.phy_bcr & RESET_ASSERTED != 0 {
            return Err(NotReady::PhyResetAsserted);
        }
        root_is_ready(
            Root::Axi,
            self.axi_command,
            self.axi_config,
            UFS_RATE_ROOTS[0],
        )?;
        root_is_ready(
            Root::Unipro,
            self.unipro_command,
            self.unipro_config,
            UFS_RATE_ROOTS[1],
        )?;
        root_is_ready(
            Root::Ice,
            self.ice_command,
            self.ice_config,
            UFS_RATE_ROOTS[2],
        )?;
        root_is_ready(
            Root::PhyAux,
            self.phy_aux_command,
            self.phy_aux_config,
            UFS_RATE_ROOTS[3],
        )?;

        for (gate, value, halt_status_is_valid) in [
            (Gate::Ahb, self.ahb_cbcr, true),
            (Gate::Axi, self.axi_cbcr, true),
            (Gate::Unipro, self.unipro_cbcr, true),
            (Gate::Ice, self.ice_cbcr, true),
            (Gate::PhyAux, self.phy_aux_cbcr, true),
            (Gate::TxSymbol, self.tx_symbol_cbcr, false),
            (Gate::RxSymbol, self.rx_symbol_cbcr, false),
            (Gate::AggregateAxi, self.aggregate_axi_cbcr, true),
            (Gate::MemReference, self.mem_reference_cbcr, true),
        ] {
            gate_is_ready(gate, value, halt_status_is_valid)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready_snapshot() -> UfsGccSnapshot {
        UfsGccSnapshot {
            gdsc: GDSC_POWER_ON,
            phy_bcr: 0,
            axi_command: 0,
            axi_config: UFS_RATE_ROOTS[0].expected_config(),
            unipro_command: 0,
            unipro_config: UFS_RATE_ROOTS[1].expected_config(),
            ice_command: 0,
            ice_config: UFS_RATE_ROOTS[2].expected_config(),
            phy_aux_command: 0,
            phy_aux_config: 0,
            ahb_cbcr: CBCR_ENABLE,
            axi_cbcr: CBCR_ENABLE,
            unipro_cbcr: CBCR_ENABLE,
            ice_cbcr: CBCR_ENABLE,
            phy_aux_cbcr: CBCR_ENABLE,
            tx_symbol_cbcr: CBCR_ENABLE | CBCR_CLOCK_OFF,
            rx_symbol_cbcr: CBCR_ENABLE | CBCR_CLOCK_OFF,
            aggregate_axi_cbcr: CBCR_ENABLE,
            mem_reference_cbcr: CBCR_ENABLE,
        }
    }

    #[test]
    fn accepts_exact_linux_pre_cap_gcc_profile() {
        assert_eq!(ready_snapshot().check_gcc_pre_cap_state(), Ok(()));
    }

    #[test]
    fn root_off_status_controls_active_state_not_the_enable_request_bit() {
        let mut snapshot = ready_snapshot();
        snapshot.axi_command = 0;
        assert_eq!(snapshot.check_gcc_pre_cap_state(), Ok(()));

        snapshot.axi_command = 1 << 1;
        assert_eq!(snapshot.check_gcc_pre_cap_state(), Ok(()));
    }

    #[test]
    fn divide_by_one_accepts_zero_or_one_hid_encoding() {
        let auxiliary = UFS_RATE_ROOTS[3];
        assert_eq!(auxiliary.expected_config(), 1);
        assert!(auxiliary.configuration_matches(0));
        assert!(auxiliary.configuration_matches(1));
        assert!(!auxiliary.configuration_matches(0x100));
    }

    #[test]
    fn classifies_observed_sunfish_gcc_snapshot_as_gcc_ready() {
        let observed = UfsGccSnapshot {
            gdsc: 0xf822_2000,
            phy_bcr: 0,
            axi_command: 0,
            axi_config: 0x0010_0105,
            unipro_command: 0,
            unipro_config: 0x0010_0107,
            ice_command: 0,
            ice_config: 0x0000_0103,
            phy_aux_command: 0,
            phy_aux_config: 0,
            ahb_cbcr: 0x0000_0001,
            axi_cbcr: 0x0000_0221,
            unipro_cbcr: 0x0000_4221,
            ice_cbcr: 0x0000_4221,
            phy_aux_cbcr: 0x0000_0001,
            tx_symbol_cbcr: 0x0000_0001,
            rx_symbol_cbcr: 0x0000_0001,
            aggregate_axi_cbcr: 0x0000_0001,
            mem_reference_cbcr: 0x0000_0003,
        };

        assert_eq!(observed.check_gcc_pre_cap_state(), Ok(()));
    }

    #[test]
    fn rejects_power_off_or_asserted_phy_reset() {
        let mut snapshot = ready_snapshot();
        snapshot.gdsc = 0;
        assert_eq!(snapshot.check_gcc_pre_cap_state(), Err(NotReady::DomainOff));

        let mut snapshot = ready_snapshot();
        snapshot.phy_bcr = RESET_ASSERTED;
        assert_eq!(
            snapshot.check_gcc_pre_cap_state(),
            Err(NotReady::PhyResetAsserted)
        );
    }

    #[test]
    fn rejects_unfinished_or_mismatched_rate_root() {
        let mut snapshot = ready_snapshot();
        snapshot.axi_command = RCG_CMD_UPDATE;
        assert_eq!(
            snapshot.check_gcc_pre_cap_state(),
            Err(NotReady::RootUpdatePending(Root::Axi))
        );

        let mut snapshot = ready_snapshot();
        snapshot.unipro_config ^= 1;
        assert_eq!(
            snapshot.check_gcc_pre_cap_state(),
            Err(NotReady::RootConfigurationMismatch(Root::Unipro))
        );

        let mut snapshot = ready_snapshot();
        snapshot.phy_aux_config ^= 1 << 8;
        assert_eq!(
            snapshot.check_gcc_pre_cap_state(),
            Err(NotReady::RootConfigurationMismatch(Root::PhyAux))
        );
    }

    #[test]
    fn rejects_hardware_off_rate_root() {
        let mut snapshot = ready_snapshot();
        snapshot.unipro_command |= RCG_ROOT_OFF;
        assert_eq!(
            snapshot.check_gcc_pre_cap_state(),
            Err(NotReady::RootOff(Root::Unipro))
        );
    }

    #[test]
    fn ignores_unrelated_rcg_configuration_bits() {
        let mut snapshot = ready_snapshot();
        snapshot.axi_config |= 1 << 12;
        assert_eq!(snapshot.check_gcc_pre_cap_state(), Ok(()));
    }

    #[test]
    fn rejects_missing_required_gate_but_skips_unavailable_symbol_halt_status() {
        let mut snapshot = ready_snapshot();
        snapshot.ice_cbcr = 0;
        assert_eq!(
            snapshot.check_gcc_pre_cap_state(),
            Err(NotReady::ClockGateOff(Gate::Ice))
        );

        let mut snapshot = ready_snapshot();
        snapshot.tx_symbol_cbcr = CBCR_ENABLE | CBCR_CLOCK_OFF;
        assert_eq!(snapshot.check_gcc_pre_cap_state(), Ok(()));
    }

    #[test]
    fn rejects_halted_gates_where_the_controller_exposes_halt_status() {
        let mut snapshot = ready_snapshot();
        snapshot.axi_cbcr = CBCR_ENABLE | CBCR_CLOCK_OFF;
        assert_eq!(
            snapshot.check_gcc_pre_cap_state(),
            Err(NotReady::ClockHalted(Gate::Axi))
        );
    }
}
