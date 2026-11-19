//! One-shot executor for a validated Sunfish SMMUv2 install plan.
//!
//! This interprets the register/publication plan and enforces its ordering,
//! bounded TLB synchronization, and no-replay rule after an uncertain result.
//! It does not acquire or prove SMMU ownership, validate a live route snapshot,
//! allocate or own page-table frames, or provide a hardware MMIO adapter. The
//! executor also requires a non-forgeable-in-safe-code authority token. This
//! module deliberately has no production token constructor: one must be added
//! only alongside a Sunfish-specific platform gate that proves EL1 control,
//! exclusive ownership, a fresh route snapshot, and fault containment. Tests
//! use a test-only token and a mock `InstallIo`.
#![forbid(unsafe_code)]

use crate::sunfish_smmu_program::{
    AccessWidth, CB_ACTLR, CB_MAIR0, CB_MAIR1, CB_SCTLR, CB_TTBCR, CB_TTBCR2, CB_TTBR0, CB_TTBR1,
    GR0_S2CR_BASE, GR0_SMR_BASE, GR1_CBA2R_BASE, GR1_CBAR_BASE, InstallStep, RegisterBlock,
    RouteSlotPreimage, SmmuInstallPlan,
};
use crate::sunfish_smmu_snapshot::ContextBankPreimage;

pub const MAX_TLB_SYNC_POLLS: u16 = 1024;
const SCTLR_M: u32 = 1;
// FSR.FORMAT describes the translation format; it is not a fault flag. The
// working Linux WLAN context reads 0x400 with no fault interrupt or failure.
const FSR_FORMAT_MASK: u32 = 3 << 9;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    Write32,
    Write64,
    PublishPageTables,
    PublishMultiPageTables,
    PublishRootPageTables,
    PublishReceiveRootPageTables,
    BindContextFaultHandler,
    ReadForWait,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RouteRegister {
    GlobalFsr,
    Smr,
    S2cr,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextRegister {
    Cbar,
    Cba2r,
    Sctlr,
    Actlr,
    Ttbcr2,
    Ttbr0Low,
    Ttbr0High,
    Ttbr1Low,
    Ttbr1High,
    Ttbcr,
    Mair0,
    Mair1,
    Fsr,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvalidPlanStep {
    U32ValueTooWide,
    InvalidPageBytes,
    ZeroTablePage,
    InvalidMultiTableCount,
    DuplicateTablePage,
    ZeroWaitMask,
}

#[derive(Debug, Eq, PartialEq)]
pub enum InstallError<E> {
    AlreadyInstalled,
    Quarantined,
    InvalidPollBudget,
    InvalidPlan {
        step_index: usize,
        cause: InvalidPlanStep,
    },
    MissingRoutePreimage,
    MissingContextPreimage,
    RoutePreconditionIo {
        slot: usize,
        register: RouteRegister,
        source: E,
    },
    RoutePreconditionMismatch {
        slot: usize,
        register: RouteRegister,
        expected: u32,
        actual: u32,
    },
    GlobalFaultPending {
        fsr: u32,
    },
    ContextPreconditionIo {
        context_bank: usize,
        register: ContextRegister,
        source: E,
    },
    ContextPreconditionMismatch {
        context_bank: usize,
        register: ContextRegister,
        expected: u32,
        actual: u32,
    },
    ContextFaultPending {
        context_bank: usize,
        fsr: u32,
    },
    ContextTranslationAlreadyEnabled {
        context_bank: usize,
        sctlr: u32,
    },
    Io {
        step_index: usize,
        operation: Operation,
        confirmed_writes: u16,
        source: E,
    },
    WaitTimedOut {
        step_index: usize,
        block: RegisterBlock,
        index: usize,
        offset: u32,
        mask: u32,
        last_value: u32,
        polls: u16,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InstallReceipt {
    pub confirmed_writes: u16,
    pub table_publications: u16,
    pub fault_handler_bindings: u8,
    pub precondition_reads: u8,
    pub wait_reads: u16,
}

/// Proof supplied by the platform owner before the SMMU executor may perform
/// writes. There is intentionally no production constructor in this module;
/// a public Boolean, route plan, or arbitrary `InstallIo` implementation is
/// not evidence that the caller owns the hardware.
#[derive(Debug, Eq, PartialEq)]
pub struct InstallAuthority {
    _private: (),
}

#[cfg(feature = "sunfish-smmu-install-probe")]
impl InstallAuthority {
    pub(crate) const fn from_experiment_gate(
        _gate: crate::sunfish_smmu_native::ExperimentGate,
    ) -> Self {
        Self { _private: () }
    }
}

#[cfg(test)]
impl InstallAuthority {
    /// Mock-only authority for host tests. This constructor is absent from
    /// normal and AArch64 production builds.
    #[allow(dead_code)] // The host tests include this module directly by path.
    pub(crate) const fn for_test() -> Self {
        Self { _private: () }
    }
}

/// All operations required by the plan are explicit. Page-table publication
/// must make the supplied frames visible to the SMMU before returning; fault
/// handler binding must be complete before the context bank is enabled.
pub trait InstallIo {
    type Error;

    fn write32(
        &mut self,
        block: RegisterBlock,
        index: usize,
        offset: u32,
        value: u32,
    ) -> Result<(), Self::Error>;
    fn write64(
        &mut self,
        block: RegisterBlock,
        index: usize,
        offset: u32,
        value: u64,
    ) -> Result<(), Self::Error>;
    fn publish_page_tables(
        &mut self,
        physical_pages: [u64; 4],
        page_bytes: u64,
    ) -> Result<(), Self::Error>;
    fn publish_multi_page_tables(
        &mut self,
        ttbr0_root: u64,
        level2: u64,
        level3_pages: &[u64],
        ttbr1_root: u64,
        page_bytes: u64,
    ) -> Result<(), Self::Error>;
    fn bind_context_fault_handler(&mut self, context_bank: usize) -> Result<(), Self::Error>;
    fn publish_root_page_tables(
        &mut self,
        physical_pages: [u64; 7],
        page_bytes: u64,
    ) -> Result<(), Self::Error>;
    fn publish_receive_root_page_tables(
        &mut self,
        physical_pages: [u64; 8],
        page_bytes: u64,
    ) -> Result<(), Self::Error>;
    fn read32(
        &mut self,
        block: RegisterBlock,
        index: usize,
        offset: u32,
    ) -> Result<u32, Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    Ready,
    Installed,
    Quarantined,
}

/// Single-use executor. Any I/O error or exhausted wait budget may leave a
/// partially installed context; this instance then refuses every replay.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Installer {
    state: State,
}

impl Default for Installer {
    fn default() -> Self {
        Self {
            state: State::Ready,
        }
    }
}

impl Installer {
    pub const fn is_installed(&self) -> bool {
        matches!(self.state, State::Installed)
    }

    pub const fn is_quarantined(&self) -> bool {
        matches!(self.state, State::Quarantined)
    }

    /// Execute one complete plan using at most `poll_budget` reads for each
    /// explicit wait. The budget must be in `1..=MAX_TLB_SYNC_POLLS`. A failed
    /// operation is never retried, even if its error may have been returned
    /// after the hardware accepted the write or publication.
    pub fn install<I: InstallIo>(
        &mut self,
        _authority: InstallAuthority,
        io: &mut I,
        plan: &SmmuInstallPlan,
        poll_budget: u16,
    ) -> Result<InstallReceipt, InstallError<I::Error>> {
        match self.state {
            State::Installed => return Err(InstallError::AlreadyInstalled),
            State::Quarantined => return Err(InstallError::Quarantined),
            State::Ready => {}
        }
        if poll_budget == 0 || poll_budget > MAX_TLB_SYNC_POLLS {
            return Err(InstallError::InvalidPollBudget);
        }
        validate(plan)
            .map_err(|(step_index, cause)| InstallError::InvalidPlan { step_index, cause })?;

        let route_preimage = plan
            .route_preimage()
            .ok_or(InstallError::MissingRoutePreimage)?;
        let context_preimage = plan
            .context_preimage()
            .ok_or(InstallError::MissingContextPreimage)?;
        let route_reads = verify_route_preimage(io, route_preimage)?
            + if let Some(peer) = plan.peer_route_preimage() {
                verify_route_preimage(io, peer)?
            } else {
                0
            };
        let context_reads = verify_context_preimage(io, context_preimage)?;
        // Read global status last, after all other preimages. The platform
        // owner must still serialize SMMU access across this final check and
        // the plan's first operation.
        let fault_reads = verify_global_fault_preimage(io, route_preimage)?;
        let precondition_reads = route_reads + context_reads + fault_reads;

        let result = execute(io, plan, poll_budget, precondition_reads);
        self.state = if result.is_ok() {
            State::Installed
        } else {
            State::Quarantined
        };
        result
    }
}

fn verify_route_preimage<I: InstallIo>(
    io: &mut I,
    preimage: RouteSlotPreimage,
) -> Result<u8, InstallError<I::Error>> {
    for (register, offset, expected) in [
        (
            RouteRegister::Smr,
            GR0_SMR_BASE + preimage.slot() as u32 * 4,
            preimage.smr(),
        ),
        (
            RouteRegister::S2cr,
            GR0_S2CR_BASE + preimage.slot() as u32 * 4,
            preimage.s2cr(),
        ),
    ] {
        let actual = io
            .read32(RegisterBlock::Global0, 0, offset)
            .map_err(|source| InstallError::RoutePreconditionIo {
                slot: preimage.slot(),
                register,
                source,
            })?;
        if actual != expected {
            return Err(InstallError::RoutePreconditionMismatch {
                slot: preimage.slot(),
                register,
                expected,
                actual,
            });
        }
    }
    Ok(2)
}

fn verify_global_fault_preimage<I: InstallIo>(
    io: &mut I,
    preimage: RouteSlotPreimage,
) -> Result<u8, InstallError<I::Error>> {
    let actual = io
        .read32(
            RegisterBlock::Global0,
            0,
            crate::sunfish_smmu_snapshot::SMMU_GLOBAL_FSR,
        )
        .map_err(|source| InstallError::RoutePreconditionIo {
            slot: preimage.slot(),
            register: RouteRegister::GlobalFsr,
            source,
        })?;
    if actual != preimage.global_fsr() {
        return Err(InstallError::RoutePreconditionMismatch {
            slot: preimage.slot(),
            register: RouteRegister::GlobalFsr,
            expected: preimage.global_fsr(),
            actual,
        });
    }
    if actual != 0 {
        return Err(InstallError::GlobalFaultPending { fsr: actual });
    }
    Ok(1)
}

fn verify_context_preimage<I: InstallIo>(
    io: &mut I,
    preimage: ContextBankPreimage,
) -> Result<u8, InstallError<I::Error>> {
    let bank = preimage.context_bank();
    for (register, block, index, offset, expected) in [
        (
            ContextRegister::Cbar,
            RegisterBlock::Global1,
            0,
            GR1_CBAR_BASE + bank as u32 * 4,
            preimage.cbar(),
        ),
        (
            ContextRegister::Cba2r,
            RegisterBlock::Global1,
            0,
            GR1_CBA2R_BASE + bank as u32 * 4,
            preimage.cba2r(),
        ),
        (
            ContextRegister::Sctlr,
            RegisterBlock::ContextBank,
            bank,
            CB_SCTLR,
            preimage.sctlr(),
        ),
        (
            ContextRegister::Actlr,
            RegisterBlock::ContextBank,
            bank,
            CB_ACTLR,
            preimage.actlr(),
        ),
        (
            ContextRegister::Ttbcr2,
            RegisterBlock::ContextBank,
            bank,
            CB_TTBCR2,
            preimage.ttbcr2(),
        ),
        (
            ContextRegister::Ttbr0Low,
            RegisterBlock::ContextBank,
            bank,
            CB_TTBR0,
            preimage.ttbr0_low(),
        ),
        (
            ContextRegister::Ttbr0High,
            RegisterBlock::ContextBank,
            bank,
            CB_TTBR0 + 4,
            preimage.ttbr0_high(),
        ),
        (
            ContextRegister::Ttbr1Low,
            RegisterBlock::ContextBank,
            bank,
            CB_TTBR1,
            preimage.ttbr1_low(),
        ),
        (
            ContextRegister::Ttbr1High,
            RegisterBlock::ContextBank,
            bank,
            CB_TTBR1 + 4,
            preimage.ttbr1_high(),
        ),
        (
            ContextRegister::Ttbcr,
            RegisterBlock::ContextBank,
            bank,
            CB_TTBCR,
            preimage.ttbcr(),
        ),
        (
            ContextRegister::Mair0,
            RegisterBlock::ContextBank,
            bank,
            CB_MAIR0,
            preimage.mair0(),
        ),
        (
            ContextRegister::Mair1,
            RegisterBlock::ContextBank,
            bank,
            CB_MAIR1,
            preimage.mair1(),
        ),
        (
            ContextRegister::Fsr,
            RegisterBlock::ContextBank,
            bank,
            crate::sunfish_smmu_snapshot::SMMU_CB_FSR,
            preimage.fsr(),
        ),
    ] {
        let actual = io.read32(block, index, offset).map_err(|source| {
            InstallError::ContextPreconditionIo {
                context_bank: bank,
                register,
                source,
            }
        })?;
        if actual != expected {
            return Err(InstallError::ContextPreconditionMismatch {
                context_bank: bank,
                register,
                expected,
                actual,
            });
        }
    }
    if preimage.fsr() & !FSR_FORMAT_MASK != 0 {
        return Err(InstallError::ContextFaultPending {
            context_bank: bank,
            fsr: preimage.fsr(),
        });
    }
    if preimage.sctlr() & SCTLR_M != 0 {
        return Err(InstallError::ContextTranslationAlreadyEnabled {
            context_bank: bank,
            sctlr: preimage.sctlr(),
        });
    }
    Ok(13)
}

fn validate(plan: &SmmuInstallPlan) -> Result<(), (usize, InvalidPlanStep)> {
    for (step_index, step) in plan.steps().iter().enumerate() {
        match step {
            InstallStep::Write {
                width: AccessWidth::U32,
                value,
                ..
            } if *value > u64::from(u32::MAX) => {
                return Err((step_index, InvalidPlanStep::U32ValueTooWide));
            }
            InstallStep::PublishPageTables {
                physical_pages,
                page_bytes,
            } => {
                if !valid_page_bytes(*page_bytes) {
                    return Err((step_index, InvalidPlanStep::InvalidPageBytes));
                }
                if physical_pages
                    .iter()
                    .any(|page| *page == 0 || page % page_bytes != 0)
                {
                    return Err((step_index, InvalidPlanStep::ZeroTablePage));
                }
            }
            InstallStep::PublishMultiPageTables {
                ttbr0_root,
                level2,
                level3_pages,
                level3_page_count,
                ttbr1_root,
                page_bytes,
            } => {
                let count = usize::from(*level3_page_count);
                if count == 0 || count > level3_pages.len() {
                    return Err((step_index, InvalidPlanStep::InvalidMultiTableCount));
                }
                if !valid_page_bytes(*page_bytes) {
                    return Err((step_index, InvalidPlanStep::InvalidPageBytes));
                }
                if [*ttbr0_root, *level2, *ttbr1_root]
                    .iter()
                    .chain(level3_pages[..count].iter())
                    .any(|page| *page == 0 || page % page_bytes != 0)
                {
                    return Err((step_index, InvalidPlanStep::ZeroTablePage));
                }
            }
            InstallStep::PublishRootPageTables {
                physical_pages,
                page_bytes,
            } => {
                if *page_bytes != 4096 {
                    return Err((step_index, InvalidPlanStep::InvalidPageBytes));
                }
                for (i, page) in physical_pages.iter().enumerate() {
                    if *page == 0 || page % page_bytes != 0 {
                        return Err((step_index, InvalidPlanStep::ZeroTablePage));
                    }
                    if physical_pages[..i].contains(page) {
                        return Err((step_index, InvalidPlanStep::DuplicateTablePage));
                    }
                }
            }
            InstallStep::PublishReceiveRootPageTables {
                physical_pages,
                page_bytes,
            } => {
                if *page_bytes != 4096 {
                    return Err((step_index, InvalidPlanStep::InvalidPageBytes));
                }
                for (i, page) in physical_pages.iter().enumerate() {
                    if *page == 0 || page % page_bytes != 0 {
                        return Err((step_index, InvalidPlanStep::ZeroTablePage));
                    }
                    if physical_pages[..i].contains(page) {
                        return Err((step_index, InvalidPlanStep::DuplicateTablePage));
                    }
                }
            }
            InstallStep::WaitForClear { mask: 0, .. } => {
                return Err((step_index, InvalidPlanStep::ZeroWaitMask));
            }
            _ => {}
        }
    }
    Ok(())
}

fn valid_page_bytes(page_bytes: u64) -> bool {
    page_bytes != 0 && page_bytes.is_power_of_two()
}

fn execute<I: InstallIo>(
    io: &mut I,
    plan: &SmmuInstallPlan,
    poll_budget: u16,
    precondition_reads: u8,
) -> Result<InstallReceipt, InstallError<I::Error>> {
    let mut receipt = InstallReceipt {
        confirmed_writes: 0,
        table_publications: 0,
        fault_handler_bindings: 0,
        precondition_reads,
        wait_reads: 0,
    };

    for (step_index, step) in plan.steps().iter().enumerate() {
        let io_error = |operation, source, confirmed_writes| InstallError::Io {
            step_index,
            operation,
            confirmed_writes,
            source,
        };
        match *step {
            InstallStep::Write {
                block,
                index,
                offset,
                width: AccessWidth::U32,
                value,
            } => {
                let value = u32::try_from(value).map_err(|_| InstallError::InvalidPlan {
                    step_index,
                    cause: InvalidPlanStep::U32ValueTooWide,
                })?;
                io.write32(block, index, offset, value).map_err(|source| {
                    io_error(Operation::Write32, source, receipt.confirmed_writes)
                })?;
                receipt.confirmed_writes += 1;
            }
            InstallStep::Write {
                block,
                index,
                offset,
                width: AccessWidth::U64,
                value,
            } => {
                io.write64(block, index, offset, value).map_err(|source| {
                    io_error(Operation::Write64, source, receipt.confirmed_writes)
                })?;
                receipt.confirmed_writes += 1;
            }
            InstallStep::PublishPageTables {
                physical_pages,
                page_bytes,
            } => {
                io.publish_page_tables(physical_pages, page_bytes)
                    .map_err(|source| {
                        io_error(
                            Operation::PublishPageTables,
                            source,
                            receipt.confirmed_writes,
                        )
                    })?;
                receipt.table_publications += 1;
            }
            InstallStep::PublishMultiPageTables {
                ttbr0_root,
                level2,
                level3_pages,
                level3_page_count,
                ttbr1_root,
                page_bytes,
            } => {
                let count = usize::from(level3_page_count);
                io.publish_multi_page_tables(
                    ttbr0_root,
                    level2,
                    &level3_pages[..count],
                    ttbr1_root,
                    page_bytes,
                )
                .map_err(|source| {
                    io_error(
                        Operation::PublishMultiPageTables,
                        source,
                        receipt.confirmed_writes,
                    )
                })?;
                receipt.table_publications += 1;
            }
            InstallStep::PublishRootPageTables {
                physical_pages,
                page_bytes,
            } => {
                io.publish_root_page_tables(physical_pages, page_bytes)
                    .map_err(|source| {
                        io_error(
                            Operation::PublishRootPageTables,
                            source,
                            receipt.confirmed_writes,
                        )
                    })?;
                receipt.table_publications += 1;
            }
            InstallStep::PublishReceiveRootPageTables {
                physical_pages,
                page_bytes,
            } => {
                io.publish_receive_root_page_tables(physical_pages, page_bytes)
                    .map_err(|source| {
                        io_error(
                            Operation::PublishReceiveRootPageTables,
                            source,
                            receipt.confirmed_writes,
                        )
                    })?;
                receipt.table_publications += 1;
            }
            InstallStep::BindContextFaultHandler { context_bank } => {
                io.bind_context_fault_handler(context_bank)
                    .map_err(|source| {
                        io_error(
                            Operation::BindContextFaultHandler,
                            source,
                            receipt.confirmed_writes,
                        )
                    })?;
                receipt.fault_handler_bindings += 1;
            }
            InstallStep::WaitForClear {
                block,
                index,
                offset,
                mask,
            } => {
                let mut cleared = false;
                let mut last_value = 0;
                for _ in 0..poll_budget {
                    last_value = io.read32(block, index, offset).map_err(|source| {
                        io_error(Operation::ReadForWait, source, receipt.confirmed_writes)
                    })?;
                    receipt.wait_reads += 1;
                    if last_value & mask == 0 {
                        cleared = true;
                        break;
                    }
                }
                if !cleared {
                    return Err(InstallError::WaitTimedOut {
                        step_index,
                        block,
                        index,
                        offset,
                        mask,
                        last_value,
                        polls: poll_budget,
                    });
                }
            }
        }
    }

    Ok(receipt)
}
