//! Source-derived IPA v4.2 fixed-register initialization for Sunfish.
//!
//! `plan()` describes the matched LineageOS `ipa3_init_hw()` writes from a
//! caller-supplied snapshot; `execute()` runs the same ordered sequence through
//! an injected `RegisterIo`. This module has no concrete MMIO mapping, power or
//! bus-vote provider, GSI/DMA setup, service publication, or proof that
//! `ipa3_post_init()` completed. A platform caller must supply the selected
//! hardware type and an authorized, exclusive register capability; execution
//! success alone does not establish IPA readiness.
#![forbid(unsafe_code)]

pub const SUNFISH_IPA_HW_V4_2: u32 = 16;

pub const REG_VERSION: u32 = 0x034;
pub const REG_COMP_CFG: u32 = 0x03c;
pub const REG_CLKON_CFG: u32 = 0x044;
pub const REG_QSB_MAX_WRITES: u32 = 0x074;
pub const REG_QSB_MAX_READS: u32 = 0x078;
pub const REG_FILT_ROUT_HASH_EN: u32 = 0x148;
pub const REG_BCR: u32 = 0x1d0;
pub const REG_COUNTER_CFG: u32 = 0x1f0;
pub const REG_TX_CFG: u32 = 0x1fc;

const TX_CFG_DEFINED_FIELDS: u32 = 0x0001_fffc;
const TX_CFG_PA_MASK_EN: u32 = 0x0000_1000;
const COMP_CFG_DEFINED_FIELDS: u32 = 0x001f_ffff;
const COMP_CFG_GSI_INORDER_DISABLE: u32 = 0x0000_0180;
const COMP_CFG_GSI_MULTI_AXI_DISABLE: u32 = 0x0000_8000;

const CLKON_OPEN_GLOBAL_2X_AND_GLOBAL: u32 = 0x3000_0000;
const QSB_DDR_MAX_WRITES: u32 = 8;
const QSB_DDR_MAX_READS: u32 = 12;
const COUNTER_AGGR_500_USEC: u32 = 15 << 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    WrongHardwareType,
    VersionUnavailable,
}

/// Narrow register interface used by the native IPA owner. The platform
/// implementation must be backed by that owner's exact IPA MMIO capability;
/// this crate does not map or create the capability.
pub trait RegisterIo {
    type Error;

    fn read32(&mut self, offset: u32) -> Result<u32, Self::Error>;
    fn write32(&mut self, offset: u32, value: u32) -> Result<(), Self::Error>;
}

/// An I/O failure while executing the ordered Linux initialization sequence.
/// `confirmed_writes` counts writes whose calls returned success. A failed
/// write may have reached hardware, so callers must quarantine IPA and must
/// not retry this sequence on the same hardware generation.
#[derive(Debug, Eq, PartialEq)]
pub enum ExecuteError<E> {
    WrongHardwareType,
    VersionUnavailable,
    Read {
        offset: u32,
        confirmed_writes: u8,
        source: E,
    },
    Write {
        offset: u32,
        confirmed_writes: u8,
        source: E,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecuteReceipt {
    pub version: u32,
    pub confirmed_writes: u8,
    pub comp_cfg_readback: u32,
}

/// Caller-supplied state read before planning. `hardware_type` is the
/// board-selected IPA enum, and `version` is the live IPA_VERSION register.
/// The two configuration registers are needed because the Linux path reads,
/// edits fields, and writes them back.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Snapshot {
    pub hardware_type: u32,
    pub version: u32,
    pub tx_cfg: u32,
    pub comp_cfg: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegisterWrite {
    pub offset: u32,
    pub value: u32,
}

/// The exact fixed-register write sequence in `ipa3_init_hw()` for IPA v4.2.
/// `plan()` returns this description; `execute()` submits the corresponding
/// sequence through its caller-supplied register interface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InitPlan {
    writes: [RegisterWrite; 9],
}

impl InitPlan {
    pub const fn writes(&self) -> &[RegisterWrite; 9] {
        &self.writes
    }
}

/// Execute the Sunfish IPA v4.2 fixed-register sequence in the same order as
/// LineageOS `ipa3_init_hw()` / `ipa_comp_cfg()`. In particular, TX_CFG is
/// read only after BCR and clock setup, and COMP_CFG is read again between
/// its two writes so the second update uses the hardware readback. This is
/// only the pre-GSI register stage: success does not establish IPA readiness.
/// The caller must provide exclusive, authorized IPA MMIO and must quarantine
/// the hardware generation after any I/O error.
pub fn execute<I: RegisterIo>(
    io: &mut I,
    hardware_type: u32,
) -> Result<ExecuteReceipt, ExecuteError<I::Error>> {
    if hardware_type != SUNFISH_IPA_HW_V4_2 {
        return Err(ExecuteError::WrongHardwareType);
    }

    let version = read(io, REG_VERSION, 0)?;
    if version == 0 {
        return Err(ExecuteError::VersionUnavailable);
    }

    let mut confirmed_writes = 0;
    write(io, REG_BCR, 0, &mut confirmed_writes)?;
    write(
        io,
        REG_CLKON_CFG,
        CLKON_OPEN_GLOBAL_2X_AND_GLOBAL,
        &mut confirmed_writes,
    )?;

    let tx_cfg = read(io, REG_TX_CFG, confirmed_writes)?;
    let tx_cfg = (tx_cfg & TX_CFG_DEFINED_FIELDS) & !TX_CFG_PA_MASK_EN;
    write(io, REG_TX_CFG, tx_cfg, &mut confirmed_writes)?;

    write(
        io,
        REG_QSB_MAX_WRITES,
        QSB_DDR_MAX_WRITES,
        &mut confirmed_writes,
    )?;
    write(
        io,
        REG_QSB_MAX_READS,
        QSB_DDR_MAX_READS,
        &mut confirmed_writes,
    )?;
    write(
        io,
        REG_COUNTER_CFG,
        COUNTER_AGGR_500_USEC,
        &mut confirmed_writes,
    )?;

    let comp_cfg = read(io, REG_COMP_CFG, confirmed_writes)?;
    let comp_cfg = (comp_cfg & COMP_CFG_DEFINED_FIELDS) | COMP_CFG_GSI_INORDER_DISABLE;
    write(io, REG_COMP_CFG, comp_cfg, &mut confirmed_writes)?;

    let comp_cfg = read(io, REG_COMP_CFG, confirmed_writes)?;
    let comp_cfg = (comp_cfg & COMP_CFG_DEFINED_FIELDS) | COMP_CFG_GSI_MULTI_AXI_DISABLE;
    write(io, REG_COMP_CFG, comp_cfg, &mut confirmed_writes)?;
    let comp_cfg_readback = read(io, REG_COMP_CFG, confirmed_writes)?;

    write(io, REG_FILT_ROUT_HASH_EN, 0, &mut confirmed_writes)?;

    Ok(ExecuteReceipt {
        version,
        confirmed_writes,
        comp_cfg_readback,
    })
}

fn read<I: RegisterIo>(
    io: &mut I,
    offset: u32,
    confirmed_writes: u8,
) -> Result<u32, ExecuteError<I::Error>> {
    io.read32(offset).map_err(|source| ExecuteError::Read {
        offset,
        confirmed_writes,
        source,
    })
}

fn write<I: RegisterIo>(
    io: &mut I,
    offset: u32,
    value: u32,
    confirmed_writes: &mut u8,
) -> Result<(), ExecuteError<I::Error>> {
    io.write32(offset, value)
        .map_err(|source| ExecuteError::Write {
            offset,
            confirmed_writes: *confirmed_writes,
            source,
        })?;
    *confirmed_writes += 1;
    Ok(())
}

pub fn plan(snapshot: Snapshot) -> Result<InitPlan, Error> {
    if snapshot.hardware_type != SUNFISH_IPA_HW_V4_2 {
        return Err(Error::WrongHardwareType);
    }
    if snapshot.version == 0 {
        return Err(Error::VersionUnavailable);
    }

    // ipahal parses the defined v4.0 fields, clears the PA mask bit, and
    // reconstructs the register. Unmodeled/reserved bits are not preserved.
    let tx_cfg = (snapshot.tx_cfg & TX_CFG_DEFINED_FIELDS) & !TX_CFG_PA_MASK_EN;

    // IPA v4.2 parses and reconstructs every COMP_CFG field. Linux writes
    // once after disabling GSI in-order reads/writes, then reads that value,
    // disables multiple AXI masters, and writes it a second time.
    let comp_cfg_inorder =
        (snapshot.comp_cfg & COMP_CFG_DEFINED_FIELDS) | COMP_CFG_GSI_INORDER_DISABLE;
    let comp_cfg_multi_axi = comp_cfg_inorder | COMP_CFG_GSI_MULTI_AXI_DISABLE;

    Ok(InitPlan {
        writes: [
            RegisterWrite {
                offset: REG_BCR,
                value: 0,
            },
            RegisterWrite {
                offset: REG_CLKON_CFG,
                value: CLKON_OPEN_GLOBAL_2X_AND_GLOBAL,
            },
            RegisterWrite {
                offset: REG_TX_CFG,
                value: tx_cfg,
            },
            RegisterWrite {
                offset: REG_QSB_MAX_WRITES,
                value: QSB_DDR_MAX_WRITES,
            },
            RegisterWrite {
                offset: REG_QSB_MAX_READS,
                value: QSB_DDR_MAX_READS,
            },
            RegisterWrite {
                offset: REG_COUNTER_CFG,
                value: COUNTER_AGGR_500_USEC,
            },
            RegisterWrite {
                offset: REG_COMP_CFG,
                value: comp_cfg_inorder,
            },
            RegisterWrite {
                offset: REG_COMP_CFG,
                value: comp_cfg_multi_axi,
            },
            RegisterWrite {
                offset: REG_FILT_ROUT_HASH_EN,
                value: 0,
            },
        ],
    })
}
