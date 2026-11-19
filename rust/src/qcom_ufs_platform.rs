//! Offline Sunfish UFS resource-map and bring-up contract.
//!
//! This module does not access hardware. It validates the audited resource
//! description and models the order a future native owner must enforce before
//! `qcom_ufs_host::RegisterIo::platform_ready()` may return true. No native
//! implementation of `PlatformIo` exists in this repository.
#![deny(unsafe_op_in_unsafe_fn)]

pub const HOST_BASE: u64 = 0x01d8_4000;
pub const HOST_BYTES: u64 = 0x3000;
pub const PHY_BASE: u64 = 0x01d8_7000;
pub const PHY_BYTES: u64 = 0x1000;
pub const PHY_RUNTIME_BYTES: u64 = 0x0ddc;
pub const ICE_BASE: u64 = 0x01d9_0000;
pub const ICE_BYTES: u64 = 0x8000;
pub const UFS_IRQ_SPI: u16 = 265;
pub const SMMU_STREAM_ID: u16 = 0x300;
/// Maximum DMA window accepted by the capability-gated native UFS adapters.
pub const MAX_DMA_APERTURE_BYTES: u64 = 16 * 1024 * 1024;
/// Candidate profile virtual address for the contiguous UFS backing pool.
pub const UFS_DMA_POOL_VIRTUAL_BASE: u64 = 0x6500_0000;
/// Candidate profile pool size. This describes ordinary backing RAM, not an
/// active UFS IOVA aperture.
pub const UFS_DMA_POOL_BYTES: u64 = MAX_DMA_APERTURE_BYTES;
/// Candidate profile virtual address for SMMUv2 LPAE table pages, separate
/// from the UFS pages exposed through the device's DMA aperture.
pub const UFS_SMMU_TABLE_POOL_VIRTUAL_BASE: u64 = 0x6600_0000;
/// An aligned 16 MiB aperture uses one root, one L2, eight L3, and one TTBR1
/// table page: 11 pages total, kept outside the DMA-visible backing pool.
pub const UFS_SMMU_TABLE_PAGE_COUNT: usize = 11;
pub const UFS_SMMU_TABLE_POOL_BYTES: u64 = UFS_SMMU_TABLE_PAGE_COUNT as u64 * DMA_PAGE_BYTES;
pub const UFS_SMMU_L3_TABLE_COUNT: usize = 8;
pub const DEVICE_RESET_GPIO: u16 = 119;
pub const DMA_PAGE_BYTES: u64 = 4096;
pub const PHY_RESET_DELAY_MIN_US: u32 = 1_000;
pub const PHY_RESET_DELAY_MAX_US: u32 = 1_100;
pub const PHY_RAIL_SETTLE_MIN_US: u32 = 1_000;
pub const PHY_RAIL_SETTLE_MAX_US: u32 = 1_100;

pub const POWER_DOMAIN_UFS_PHY_GDSC: u8 = 1;
pub const RESET_GCC_UFS_PHY_BCR: u8 = 3;
pub const SUPPLY_VCC: u8 = 1 << 0;
pub const SUPPLY_VCCQ2: u8 = 1 << 1;
pub const SUPPLY_VDDA_PHY: u8 = 1 << 2;
pub const SUPPLY_VDDA_PLL: u8 = 1 << 3;
/// Separate Qualcomm PHY regulator-client vote for the device reference-clock
/// pad. Sunfish aliases it to the same physical PMIC rail as `vdda-pll`, but
/// Linux obtains and enables it as a distinct consumer during PHY power-on.
pub const SUPPLY_VDDP_REF_CLK: u8 = 1 << 4;
pub const REGULATORS_ALL: u8 =
    SUPPLY_VCC | SUPPLY_VCCQ2 | SUPPLY_VDDA_PHY | SUPPLY_VDDA_PLL | SUPPLY_VDDP_REF_CLK;
pub const CLOCK_CORE: u16 = 1 << 0;
pub const CLOCK_BUS_AGGR: u16 = 1 << 1;
pub const CLOCK_IFACE: u16 = 1 << 2;
pub const CLOCK_CORE_UNIPRO: u16 = 1 << 3;
pub const CLOCK_CORE_ICE: u16 = 1 << 4;
pub const CLOCK_REF: u16 = 1 << 5;
pub const CLOCK_TX_LANE0_SYNC: u16 = 1 << 6;
pub const CLOCK_RX_LANE0_SYNC: u16 = 1 << 7;
pub const CORE_CLOCKS_ALL: u16 =
    CLOCK_CORE | CLOCK_BUS_AGGR | CLOCK_IFACE | CLOCK_CORE_UNIPRO | CLOCK_CORE_ICE | CLOCK_REF;
pub const LANE_CLOCKS_ALL: u16 = CLOCK_TX_LANE0_SYNC | CLOCK_RX_LANE0_SYNC;
pub const INTERCONNECT_AGGRE1: u8 = 1 << 0;
pub const INTERCONNECT_GEM: u8 = 1 << 1;
pub const INTERCONNECTS_ALL: u8 = INTERCONNECT_AGGRE1 | INTERCONNECT_GEM;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockProvider {
    Gcc,
    Rpmh,
}

/// Minimum shared GCC CX corner requested by a source clock rate. Values are
/// ordered as the SDM Magpie GCC driver's VDD level table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum VddCorner {
    None,
    Min,
    Lower,
    Low,
    LowL1,
    Nominal,
    High,
}

impl VddCorner {
    pub const fn max(self, other: Self) -> Self {
        if self as u8 >= other as u8 {
            self
        } else {
            other
        }
    }

    /// RPMh ARC regulator request value from `qcom,rpmh-regulator.h`.
    /// `None` means this clock adds no CX vote; the remaining levels match
    /// `vdd-level-sdmmagpie.h`'s VDD-to-RPMh table.
    pub const fn rpmh_level(self) -> Option<u16> {
        match self {
            Self::None => None,
            Self::Min => Some(49),
            Self::Lower => Some(65),
            Self::Low => Some(129),
            Self::LowL1 => Some(193),
            Self::Nominal => Some(257),
            Self::High => Some(385),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClockSpec {
    pub name: &'static str,
    pub provider: ClockProvider,
    pub id: u16,
    /// Zero in the captured `freq-table-hz` means this clock has no UFS-host
    /// min/max rate request; nonzero values are hertz.
    pub min_rate_hz: u32,
    pub max_rate_hz: u32,
    /// Lower bound requested from the GCC provider's shared `vdd_cx` rail.
    pub required_cx_corner: VddCorner,
}

impl ClockSpec {
    pub const fn new(
        name: &'static str,
        provider: ClockProvider,
        id: u16,
        min_rate_hz: u32,
        max_rate_hz: u32,
        required_cx_corner: VddCorner,
    ) -> Self {
        Self {
            name,
            provider,
            id,
            min_rate_hz,
            max_rate_hz,
            required_cx_corner,
        }
    }
}

/// Clock specifiers and min/max rate pairs from the captured Sunfish
/// `ufshc@1d84000` node, in `clock-names` order. Provider IDs are GCC
/// (`clock_gcc`) and RPMh; zero rate pairs mean no UFS rate request.
pub const UFS_HOST_CLOCKS: [ClockSpec; 8] = [
    ClockSpec::new(
        "core_clk",
        ClockProvider::Gcc,
        0x7e,
        50_000_000,
        200_000_000,
        VddCorner::Nominal,
    ),
    ClockSpec::new(
        "bus_aggr_clk",
        ClockProvider::Gcc,
        0x09,
        0,
        0,
        VddCorner::None,
    ),
    ClockSpec::new("iface_clk", ClockProvider::Gcc, 0x7d, 0, 0, VddCorner::None),
    ClockSpec::new(
        "core_clk_unipro",
        ClockProvider::Gcc,
        0x89,
        37_500_000,
        150_000_000,
        VddCorner::Nominal,
    ),
    ClockSpec::new(
        "core_clk_ice",
        ClockProvider::Gcc,
        0x81,
        75_000_000,
        300_000_000,
        VddCorner::Nominal,
    ),
    ClockSpec::new("ref_clk", ClockProvider::Rpmh, 0x00, 0, 0, VddCorner::None),
    ClockSpec::new(
        "tx_lane0_sync_clk",
        ClockProvider::Gcc,
        0x88,
        0,
        0,
        VddCorner::None,
    ),
    ClockSpec::new(
        "rx_lane0_sync_clk",
        ClockProvider::Gcc,
        0x87,
        0,
        0,
        VddCorner::None,
    ),
];

/// PHY clock specifiers from the captured Sunfish `ufsphy@1d87000` node, in
/// `clock-names` order.
pub const UFS_PHY_CLOCKS: [ClockSpec; 3] = [
    ClockSpec::new(
        "ref_clk_src",
        ClockProvider::Rpmh,
        0x00,
        0,
        0,
        VddCorner::None,
    ),
    ClockSpec::new("ref_clk", ClockProvider::Gcc, 0x7c, 0, 0, VddCorner::None),
    ClockSpec::new(
        "ref_aux_clk",
        ClockProvider::Gcc,
        0x84,
        0,
        0,
        VddCorner::Lower,
    ),
];

/// Minimum shared CX corner required to honor the UFS host/PHY clock profile.
pub const UFS_PRE_CAP_CX_FLOOR: VddCorner = {
    let mut floor = VddCorner::None;
    let mut index = 0;
    while index < UFS_HOST_CLOCKS.len() {
        floor = floor.max(UFS_HOST_CLOCKS[index].required_cx_corner);
        index += 1;
    }
    index = 0;
    while index < UFS_PHY_CLOCKS.len() {
        floor = floor.max(UFS_PHY_CLOCKS[index].required_cx_corner);
        index += 1;
    }
    floor
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidState,
    InvalidMapping,
    InvalidDmaAperture,
    InvalidDmaBacking,
    InvalidSmmuTables,
    ClaimFailed,
    ClaimUncertain,
    LeaseLost,
    StepFailed(Step),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    Uninitialized,
    Rejected,
    Preparing,
    ControllerPowered,
    ReadyForHci,
    Quarantined,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClaimFailure {
    /// The authority confirms it acquired no resources.
    Denied,
    /// Acquisition may have partially succeeded; the adapter must retain and
    /// quarantine its partial claims before returning this result.
    Uncertain,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Window {
    pub base: u64,
    pub bytes: u64,
    pub device_uncached_rw: bool,
    pub exclusive: bool,
}

impl Window {
    pub const fn new(base: u64, bytes: u64) -> Self {
        Self {
            base,
            bytes,
            device_uncached_rw: true,
            exclusive: true,
        }
    }

    fn valid(self) -> bool {
        self.base != 0
            && self.bytes != 0
            && self.base.is_multiple_of(DMA_PAGE_BYTES)
            && self.bytes.is_multiple_of(DMA_PAGE_BYTES)
            && self.base.checked_add(self.bytes).is_some()
            && self.device_uncached_rw
            && self.exclusive
    }

    fn overlaps(self, other: Self) -> bool {
        let Some(end) = self.base.checked_add(self.bytes) else {
            return true;
        };
        let Some(other_end) = other.base.checked_add(other.bytes) else {
            return true;
        };
        self.base < other_end && other.base < end
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InterruptMap {
    pub gic_spi: u16,
    pub level_high: bool,
    pub exclusive: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DmaAperture {
    pub base: u64,
    pub bytes: u64,
    pub exclusive: bool,
}

/// Bounded CPU/physical extent injected by the candidate Microkit profile.
///
/// This is only backing RAM geometry. It does not establish an IOVA, SMMU
/// translation, live exclusivity, cache maintenance, or device ownership.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DmaBackingPool {
    virtual_base: u64,
    physical_base: u64,
    bytes: u64,
}

impl DmaBackingPool {
    /// Validate the candidate profile's setvar values before deriving any
    /// per-page backing addresses. The physical base is selected by Microkit
    /// at build time and therefore is not pinned to one constant here.
    pub fn from_profile(virtual_base: u64, physical_base: u64, bytes: u64) -> Result<Self, Error> {
        if virtual_base != UFS_DMA_POOL_VIRTUAL_BASE
            || physical_base == 0
            || !physical_base.is_multiple_of(DMA_PAGE_BYTES)
            || bytes != UFS_DMA_POOL_BYTES
            || !bytes.is_multiple_of(DMA_PAGE_BYTES)
            || physical_base
                .checked_add(bytes)
                .is_none_or(|end| end > (1u64 << 40))
            || virtual_base.checked_add(bytes).is_none()
        {
            return Err(Error::InvalidDmaBacking);
        }
        Ok(Self {
            virtual_base,
            physical_base,
            bytes,
        })
    }

    pub const fn virtual_base(self) -> u64 {
        self.virtual_base
    }

    pub const fn physical_base(self) -> u64 {
        self.physical_base
    }

    pub const fn bytes(self) -> u64 {
        self.bytes
    }

    pub const fn page_count(self) -> usize {
        (self.bytes / DMA_PAGE_BYTES) as usize
    }

    /// Physical backing address for one page in the candidate region.
    pub fn page_physical(self, index: usize) -> Result<u64, Error> {
        if index >= self.page_count() {
            return Err(Error::InvalidDmaBacking);
        }
        self.physical_base
            .checked_add(index as u64 * DMA_PAGE_BYTES)
            .ok_or(Error::InvalidDmaBacking)
    }
}

/// Candidate, CPU-mapped LPAE table storage. Its pages must stay outside the
/// translated DMA aperture; the build audit verifies the contiguous run and
/// the exact owner mapping before this runtime token is constructed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UfsSmmuTablePool {
    virtual_base: u64,
    physical_base: u64,
    bytes: u64,
    level3_pages: [u64; UFS_SMMU_L3_TABLE_COUNT],
}

impl UfsSmmuTablePool {
    pub fn from_profile(virtual_base: u64, physical_base: u64, bytes: u64) -> Result<Self, Error> {
        if virtual_base != UFS_SMMU_TABLE_POOL_VIRTUAL_BASE
            || physical_base == 0
            || !physical_base.is_multiple_of(DMA_PAGE_BYTES)
            || bytes != UFS_SMMU_TABLE_POOL_BYTES
            || !bytes.is_multiple_of(DMA_PAGE_BYTES)
            || physical_base
                .checked_add(bytes)
                .is_none_or(|end| end > (1u64 << 40))
            || virtual_base.checked_add(bytes).is_none()
        {
            return Err(Error::InvalidSmmuTables);
        }
        let mut level3_pages = [0; UFS_SMMU_L3_TABLE_COUNT];
        for (index, page) in level3_pages.iter_mut().enumerate() {
            *page = physical_base
                .checked_add((index as u64 + 2) * DMA_PAGE_BYTES)
                .ok_or(Error::InvalidSmmuTables)?;
        }
        Ok(Self {
            virtual_base,
            physical_base,
            bytes,
            level3_pages,
        })
    }

    pub const fn virtual_base(self) -> u64 {
        self.virtual_base
    }

    pub const fn physical_base(self) -> u64 {
        self.physical_base
    }

    pub const fn bytes(self) -> u64 {
        self.bytes
    }

    pub fn page_physical(self, index: usize) -> Result<u64, Error> {
        if index >= UFS_SMMU_TABLE_PAGE_COUNT {
            return Err(Error::InvalidSmmuTables);
        }
        self.physical_base
            .checked_add(index as u64 * DMA_PAGE_BYTES)
            .ok_or(Error::InvalidSmmuTables)
    }

    pub fn ttbr0_root(self) -> u64 {
        self.physical_base
    }

    pub fn level2(self) -> u64 {
        self.physical_base + DMA_PAGE_BYTES
    }

    pub const fn level3_pages(&self) -> &[u64; UFS_SMMU_L3_TABLE_COUNT] {
        &self.level3_pages
    }

    pub fn ttbr1_root(self) -> u64 {
        self.physical_base + (UFS_SMMU_TABLE_PAGE_COUNT as u64 - 1) * DMA_PAGE_BYTES
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DmaMap {
    pub stream_id: u16,
    pub aperture: DmaAperture,
    pub translated: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResetProvider {
    Gcc,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SunfishMap {
    pub host: Window,
    pub phy: Window,
    pub ice: Window,
    pub irq: InterruptMap,
    pub dma: DmaMap,
    pub reset_controller: ResetProvider,
    pub reset_id: u8,
    pub device_reset_gpio: u16,
    pub device_reset_active_low: bool,
    pub power_domain: u8,
    pub regulator_mask: u8,
    pub clock_mask: u16,
    pub interconnect_mask: u8,
}

impl SunfishMap {
    /// Offline-only layout fixture. The DMA aperture is synthetic; its address
    /// is not a discovered or authorized device mapping and must never be used
    /// by a native platform authority.
    pub const fn offline_fixture_layout() -> Self {
        Self {
            host: Window::new(HOST_BASE, HOST_BYTES),
            phy: Window::new(PHY_BASE, PHY_BYTES),
            ice: Window::new(ICE_BASE, ICE_BYTES),
            irq: InterruptMap {
                gic_spi: UFS_IRQ_SPI,
                level_high: true,
                exclusive: true,
            },
            dma: DmaMap {
                stream_id: SMMU_STREAM_ID,
                aperture: DmaAperture {
                    base: 0x8000_0000,
                    bytes: 0x0100_0000,
                    exclusive: true,
                },
                translated: true,
            },
            reset_controller: ResetProvider::Gcc,
            reset_id: RESET_GCC_UFS_PHY_BCR,
            device_reset_gpio: DEVICE_RESET_GPIO,
            device_reset_active_low: true,
            power_domain: POWER_DOMAIN_UFS_PHY_GDSC,
            regulator_mask: REGULATORS_ALL,
            clock_mask: CORE_CLOCKS_ALL | LANE_CLOCKS_ALL,
            interconnect_mask: INTERCONNECTS_ALL,
        }
    }

    /// Validate static host/PHY and board-control facts needed for the Linux
    /// pre-CAP power phase. IRQ binding and DMA/SMMU translation are deferred
    /// until after the read-only CAP/VER snapshot.
    pub fn validate_controller(self) -> Result<(), Error> {
        if self.host.base != HOST_BASE
            || self.host.bytes != HOST_BYTES
            || self.phy.base != PHY_BASE
            || self.phy.bytes != PHY_BYTES
            || self.ice.base != ICE_BASE
            || self.ice.bytes != ICE_BYTES
            || !self.host.valid()
            || !self.phy.valid()
            || !self.ice.valid()
            || self.host.overlaps(self.phy)
            || self.host.overlaps(self.ice)
            || self.phy.overlaps(self.ice)
            || self.reset_controller != ResetProvider::Gcc
            || self.reset_id != RESET_GCC_UFS_PHY_BCR
            || self.device_reset_gpio != DEVICE_RESET_GPIO
            || !self.device_reset_active_low
            || self.power_domain != POWER_DOMAIN_UFS_PHY_GDSC
            || self.regulator_mask != REGULATORS_ALL
            || self.clock_mask != CORE_CLOCKS_ALL | LANE_CLOCKS_ALL
            || self.interconnect_mask != INTERCONNECTS_ALL
        {
            return Err(Error::InvalidMapping);
        }
        Ok(())
    }

    /// Validate the complete host, interrupt, and translated-DMA map for HCE
    /// and command traffic. This does not prove fresh boot, live ownership,
    /// working rails, or an installed SMMU translation.
    pub fn validate(self) -> Result<(), Error> {
        self.validate_controller()?;
        self.validate_hce_layout()?;
        if !self.dma.translated {
            return Err(Error::InvalidMapping);
        }
        Ok(())
    }

    fn validate_hce_layout(self) -> Result<(), Error> {
        if self.irq.gic_spi != UFS_IRQ_SPI || !self.irq.level_high || !self.irq.exclusive {
            return Err(Error::InvalidMapping);
        }
        self.validate_dma_aperture_layout()
    }

    fn validate_dma_aperture_layout(self) -> Result<(), Error> {
        if self.dma.stream_id != SMMU_STREAM_ID
            || self.dma.aperture.base == 0
            || self.dma.aperture.bytes == 0
            || !self.dma.aperture.base.is_multiple_of(DMA_PAGE_BYTES)
            || !self.dma.aperture.bytes.is_multiple_of(DMA_PAGE_BYTES)
            || self
                .dma
                .aperture
                .base
                .checked_add(self.dma.aperture.bytes)
                .is_none()
            || !self.dma.aperture.exclusive
        {
            return Err(Error::InvalidMapping);
        }
        Ok(())
    }

    /// Check descriptor bounds against a planned exclusive aperture without
    /// claiming that its SMMU translation has been installed. This is suitable
    /// before the post-CAP `ReserveDmaAndBindIrq` step.
    pub fn validate_dma_range_layout(self, address: u64, bytes: u64) -> Result<(), Error> {
        self.validate_controller()?;
        self.validate_hce_layout()?;
        if bytes == 0 {
            return Err(Error::InvalidDmaAperture);
        }
        let end = address
            .checked_add(bytes)
            .ok_or(Error::InvalidDmaAperture)?;
        let aperture_end = self
            .dma
            .aperture
            .base
            .checked_add(self.dma.aperture.bytes)
            .ok_or(Error::InvalidDmaAperture)?;
        if address < self.dma.aperture.base || end > aperture_end {
            return Err(Error::InvalidDmaAperture);
        }
        Ok(())
    }

    /// Check that a DMA descriptor or payload range stays inside the claimed
    /// translated aperture. Device-visible IOVAs must be validated separately
    /// by the real SMMU owner before it supplies this map.
    pub fn validate_dma_range(self, address: u64, bytes: u64) -> Result<(), Error> {
        self.validate()?;
        self.validate_dma_range_layout(address, bytes)
    }
}

/// Ordered platform actions, deliberately coarser than individual register
/// writes. A native adapter must follow the pinned Qualcomm driver and return
/// success only after each stage's completion/readback condition is verified.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Step {
    EnableHbaPower,
    AcquireHostClocksAndSetMaxRates,
    EnableHostClocks,
    EnableUfsDeviceRails,
    InitializePhy,
    AcquireLaneClockHandles,
    ApplyUfsBusVote,
    RunQualcommPostClockSetup,
    ReserveDmaAndBindIrq,
    AssertPhySoftReset,
    WaitResetAssertPropagation,
    CalibratePhy,
    DeassertPhySoftReset,
    WaitRailsSettle,
    StartSerdes,
    WaitPcsReady,
    SelectUniproMode,
    EnableLaneClocks,
    InitializeIce,
}

impl Step {
    /// Timing range taken from the pinned Sunfish Qualcomm host sequence.
    pub const fn delay_range_us(self) -> Option<(u32, u32)> {
        match self {
            Self::WaitResetAssertPropagation => {
                Some((PHY_RESET_DELAY_MIN_US, PHY_RESET_DELAY_MAX_US))
            }
            Self::WaitRailsSettle => Some((PHY_RAIL_SETTLE_MIN_US, PHY_RAIL_SETTLE_MAX_US)),
            _ => None,
        }
    }
}

/// Linux `ufshcd_hba_init()` and `ufs_qcom_init()` actions completed before
/// `ufshcd_init()` reads CAP and HCI version. The generic clock list includes
/// both lane-sync clocks; nonzero maximum clock rates are set before the full
/// list is enabled. Those rates require the GCC provider's shared CX rail at
/// least at the nominal corner. Qualcomm's lane-clock callback runs again later
/// during HCE.
pub const PRE_CAPABILITY_STEPS: [Step; 8] = [
    Step::EnableHbaPower,
    Step::AcquireHostClocksAndSetMaxRates,
    Step::EnableHostClocks,
    Step::EnableUfsDeviceRails,
    Step::InitializePhy,
    Step::AcquireLaneClockHandles,
    Step::ApplyUfsBusVote,
    Step::RunQualcommPostClockSetup,
];

/// Post-CAP host preparation followed by Linux
/// `ufs_qcom_hce_enable_notify(PRE_CHANGE)`. DMA and IRQ are prepared first;
/// CAP/VER have already been read before PHY reset and link startup.
pub const PRE_HCE_ENABLE_STEPS: [Step; 11] = [
    Step::ReserveDmaAndBindIrq,
    Step::AssertPhySoftReset,
    Step::WaitResetAssertPropagation,
    Step::CalibratePhy,
    Step::DeassertPhySoftReset,
    Step::WaitRailsSettle,
    Step::StartSerdes,
    Step::WaitPcsReady,
    Step::SelectUniproMode,
    Step::EnableLaneClocks,
    Step::InitializeIce,
];

/// Full plan for callers that intentionally prepare the link in one operation.
/// A source-faithful caller can instead use the two phase-specific arrays and
/// take the read-only CAP/VER snapshot between them.
pub const STARTUP_STEPS: [Step; 19] = [
    Step::EnableHbaPower,
    Step::AcquireHostClocksAndSetMaxRates,
    Step::EnableHostClocks,
    Step::EnableUfsDeviceRails,
    Step::InitializePhy,
    Step::AcquireLaneClockHandles,
    Step::ApplyUfsBusVote,
    Step::RunQualcommPostClockSetup,
    Step::ReserveDmaAndBindIrq,
    Step::AssertPhySoftReset,
    Step::WaitResetAssertPropagation,
    Step::CalibratePhy,
    Step::DeassertPhySoftReset,
    Step::WaitRailsSettle,
    Step::StartSerdes,
    Step::WaitPcsReady,
    Step::SelectUniproMode,
    Step::EnableLaneClocks,
    Step::InitializeIce,
];

/// Opaque epoch value returned only by the platform adapter's fresh-boot,
/// exclusive-claim operation. Its meaning must be tied to real capabilities,
/// not this software model or a manifest boolean.
/// Native platform authority boundary. Implementations must bind every claim
/// and operation to live, exclusive capabilities for the exact map and fresh-
/// boot epoch. They must not treat `offline_fixture_layout()` or static
/// manifest values as proof of DMA/SMMU, IRQ, power, clock, reset, or controller
/// ownership. Any uncertain partial claim must remain retained and quarantined.
pub trait PlatformIo {
    type Lease;

    /// Must atomically verify fresh-boot authority and exclusive ownership of
    /// UFS host/PHY/ICE and board-control resources. The pre-CAP phase need not
    /// bind the IRQ or configure DMA/SMMU; `ReserveDmaAndBindIrq` must establish
    /// those before HCE. `Denied` guarantees no resource was acquired. For
    /// `Uncertain`, the adapter must retain and quarantine every partial claim.
    fn claim_exclusive_fresh_boot(&mut self, map: &SunfishMap)
    -> Result<Self::Lease, ClaimFailure>;
    /// Revalidate ownership and all mappings against this exact lease epoch.
    fn lease_live(&mut self, lease: &Self::Lease) -> bool;
    /// Apply one ordered stage and verify its required completion/readback.
    /// `AcquireHostClocksAndSetMaxRates` must hold at least
    /// `UFS_PRE_CAP_CX_FLOOR` through the shared GCC `vdd_cx` resource. Delay
    /// steps must honor `Step::delay_range_us`; the reset and analog-rail
    /// settling windows are both 1,000–1,100 microseconds in the pinned driver.
    fn perform(&mut self, lease: &Self::Lease, step: Step) -> Result<(), ()>;
}

pub struct Platform<L> {
    state: State,
    lease: Option<L>,
    map: Option<SunfishMap>,
    completed_steps: u8,
}

impl<L> Platform<L> {
    pub const fn new() -> Self {
        Self {
            state: State::Uninitialized,
            lease: None,
            map: None,
            completed_steps: 0,
        }
    }

    pub const fn state(&self) -> State {
        self.state
    }

    pub const fn completed_steps(&self) -> u8 {
        self.completed_steps
    }
}

impl<L> Platform<L> {
    /// Bring the controller to the Linux-equivalent pre-CAP power stage.
    /// CAP/VER may be read after this succeeds; this stage does not enable the
    /// PHY reset/SerDes sequence or issue a UFS command.
    pub fn prepare_controller<I: PlatformIo<Lease = L>>(
        &mut self,
        io: &mut I,
        map: SunfishMap,
    ) -> Result<(), Error> {
        if self.state != State::Uninitialized {
            return Err(Error::InvalidState);
        }
        if map.validate_controller().is_err() {
            self.state = State::Rejected;
            return Err(Error::InvalidMapping);
        }
        let lease = match io.claim_exclusive_fresh_boot(&map) {
            Ok(lease) => lease,
            Err(ClaimFailure::Denied) => {
                self.state = State::Rejected;
                return Err(Error::ClaimFailed);
            }
            Err(ClaimFailure::Uncertain) => {
                self.state = State::Quarantined;
                return Err(Error::ClaimUncertain);
            }
        };
        self.lease = Some(lease);
        self.map = Some(map);
        self.state = State::Preparing;
        self.perform_steps(io, &PRE_CAPABILITY_STEPS)?;
        self.state = State::ControllerPowered;
        Ok(())
    }

    /// Complete post-CAP/VER preparation. Descriptor bounds are checked against
    /// the planned aperture before `ReserveDmaAndBindIrq`; that step must then
    /// establish and verify the live translation. Failure retains the lease and
    /// quarantines the powered controller.
    pub fn prepare_hce_enable<I: PlatformIo<Lease = L>>(
        &mut self,
        io: &mut I,
        descriptor_dma: (u64, u64),
    ) -> Result<(), Error> {
        if self.state != State::ControllerPowered {
            return Err(Error::InvalidState);
        }
        let Some(map) = self.map else {
            self.state = State::Quarantined;
            return Err(Error::LeaseLost);
        };
        if let Err(error) = map.validate_dma_range_layout(descriptor_dma.0, descriptor_dma.1) {
            self.state = State::Quarantined;
            return Err(error);
        }
        self.perform_steps(io, &PRE_HCE_ENABLE_STEPS)?;
        self.state = State::ReadyForHci;
        Ok(())
    }

    /// Prepare the entire controller and link in one call. Source-faithful
    /// bring-up may call `prepare_controller`, read CAP/VER, then call
    /// `prepare_hce_enable` so host-memory sizing follows the reported CAP.
    pub fn prepare<I: PlatformIo<Lease = L>>(
        &mut self,
        io: &mut I,
        map: SunfishMap,
        descriptor_dma: (u64, u64),
    ) -> Result<(), Error> {
        if self.state != State::Uninitialized {
            return Err(Error::InvalidState);
        }
        if map.validate_controller().is_err() {
            self.state = State::Rejected;
            return Err(Error::InvalidMapping);
        }
        if let Err(error) = map.validate_dma_range_layout(descriptor_dma.0, descriptor_dma.1) {
            self.state = State::Rejected;
            return Err(error);
        }
        self.prepare_controller(io, map)?;
        self.prepare_hce_enable(io, descriptor_dma)
    }

    fn perform_steps<I: PlatformIo<Lease = L>>(
        &mut self,
        io: &mut I,
        steps: &[Step],
    ) -> Result<(), Error> {
        for &step in steps {
            let Some(lease) = self.lease.as_ref() else {
                self.state = State::Quarantined;
                return Err(Error::LeaseLost);
            };
            if !io.lease_live(lease) {
                self.state = State::Quarantined;
                return Err(Error::LeaseLost);
            }
            if io.perform(lease, step).is_err() {
                self.state = State::Quarantined;
                return Err(Error::StepFailed(step));
            }
            self.completed_steps += 1;
            if !io.lease_live(lease) {
                self.state = State::Quarantined;
                return Err(Error::LeaseLost);
            }
        }
        Ok(())
    }

    /// Live gate for the read-only CAP/VER/HCS snapshot between the two source
    /// phases. It does not imply DMA/SMMU readiness or permission to start HCE.
    pub fn controller_ready<I: PlatformIo<Lease = L>>(&mut self, io: &mut I) -> bool {
        if !matches!(self.state, State::ControllerPowered | State::ReadyForHci) {
            return false;
        }
        let Some(lease) = self.lease.as_ref() else {
            self.state = State::Quarantined;
            return false;
        };
        if !io.lease_live(lease) {
            self.state = State::Quarantined;
            return false;
        }
        true
    }

    /// Live gate intended for a future RegisterIo/DmaIo adapter. Readiness
    /// disappears immediately when the authoritative lease is lost.
    pub fn ready<I: PlatformIo<Lease = L>>(&mut self, io: &mut I) -> bool {
        if self.state != State::ReadyForHci {
            return false;
        }
        let Some(lease) = self.lease.as_ref() else {
            self.state = State::Quarantined;
            return false;
        };
        if !io.lease_live(lease) {
            self.state = State::Quarantined;
            return false;
        }
        true
    }
}

impl<L> Default for Platform<L> {
    fn default() -> Self {
        Self::new()
    }
}
