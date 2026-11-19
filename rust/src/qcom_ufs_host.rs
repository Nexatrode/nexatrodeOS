//! Bounded UFSHCI control and interrupt core for the Sunfish storage port.
//!
//! This starts the generic host-controller layer after a platform adapter has
//! already established exclusive ownership, power, clocks, reset, PHY/ICE,
//! and DMA/SMMU mappings. No native platform adapter currently implements
//! `RegisterIo`; tests use a fake register bank only.
#![forbid(unsafe_code)]

use crate::qcom_ufs_protocol::{self as protocol, HciVersion};

pub const REG_CAPABILITIES: u32 = 0x00;
pub const REG_VERSION: u32 = 0x08;
pub const REG_INTERRUPT_STATUS: u32 = 0x20;
pub const REG_INTERRUPT_ENABLE: u32 = 0x24;
pub const REG_CONTROLLER_STATUS: u32 = 0x30;
pub const REG_CONTROLLER_ENABLE: u32 = 0x34;
pub const REG_UTRL_BASE_LO: u32 = 0x50;
pub const REG_UTRL_BASE_HI: u32 = 0x54;
pub const REG_UTRL_DOORBELL: u32 = 0x58;
pub const REG_UTRL_RUN_STOP: u32 = 0x60;
pub const REG_UTMRL_BASE_LO: u32 = 0x70;
pub const REG_UTMRL_BASE_HI: u32 = 0x74;
pub const REG_UTMRL_RUN_STOP: u32 = 0x80;
pub const REG_UIC_COMMAND: u32 = 0x90;
pub const REG_UIC_ARGUMENT_1: u32 = 0x94;
pub const REG_UIC_ARGUMENT_2: u32 = 0x98;
pub const REG_UIC_ARGUMENT_3: u32 = 0x9c;

const HCE_ENABLE: u32 = 1;
const HCS_DEVICE_PRESENT: u32 = 1;
const HCS_UTRL_READY: u32 = 1 << 1;
const HCS_UTMRL_READY: u32 = 1 << 2;
const HCS_UIC_READY: u32 = 1 << 3;
const HCS_ALL_READY: u32 = HCS_UTRL_READY | HCS_UTMRL_READY | HCS_UIC_READY;
const CAP_TRANSFER_SLOTS: u32 = 0x0000_001f;
const CAP_READY_TO_TRANSFER_REQUESTS: u32 = 0x0000_ff00;
const CAP_TASK_MANAGEMENT_SLOTS: u32 = 0x0007_0000;
const CAP_AUTO_HIBERN8: u32 = 0x0080_0000;
const CAP_64_BIT: u32 = 0x0100_0000;
const CAP_OUT_OF_ORDER: u32 = 0x0200_0000;
const CAP_UIC_DME_TEST_MODE: u32 = 0x0400_0000;
const CAP_CRYPTO: u32 = 0x1000_0000;
const INT_TRANSFER_COMPLETE: u32 = 1;
const INT_UIC_ERROR: u32 = 1 << 2;
const INT_LINK_LOST: u32 = 1 << 7;
const INT_LINK_STARTUP: u32 = 1 << 8;
const INT_UIC_COMPLETE: u32 = 1 << 10;
const INT_DEVICE_FATAL: u32 = 1 << 11;
const INT_CONTROLLER_FATAL: u32 = 1 << 16;
const INT_SYSTEM_BUS_FATAL: u32 = 1 << 17;
const INT_CRYPTO_FATAL: u32 = 1 << 18;
const INT_FATAL: u32 =
    INT_DEVICE_FATAL | INT_CONTROLLER_FATAL | INT_SYSTEM_BUS_FATAL | INT_CRYPTO_FATAL;
const INT_ERRORS: u32 = INT_UIC_ERROR | INT_LINK_LOST | INT_FATAL;
const INT_ENABLED: u32 = INT_TRANSFER_COMPLETE | INT_LINK_STARTUP | INT_UIC_COMPLETE | INT_ERRORS;
const UIC_DME_LINK_STARTUP: u32 = 0x16;
const UIC_RESULT_MASK: u32 = 0xff;
const DMA_LIST_ALIGN: u64 = 1024;
const MIN_LIST_BYTES: u64 = 1024;
const MAX_POLL_ATTEMPTS: u32 = 64;
// Sunfish UCD layout: 512-byte request, 512-byte response, then one 16-byte PRDT.
const UCD_BYTES: usize = 1040;
const UCD_RESPONSE_OFFSET: u64 = 512;
const UCD_PRDT_OFFSET: usize = 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    PlatformNotReady,
    InvalidState,
    PollLimit,
    Io,
    Timeout,
    HciVersion,
    ControllerActive,
    DeviceMissing,
    UicNotReady,
    UicFailure,
    HostError,
    InvalidDma,
    AddressWidth,
    ListsNotReady,
    Busy,
    Packet(protocol::Error),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    Disabled,
    Enabled,
    LinkUp,
    Operational,
    Failed,
}

/// Narrow register interface. `controller_power_ready` authorizes only the
/// pre-CAP identity snapshot. `platform_ready` requires all audited resources
/// and DMA mappings to be live and exclusive for HCE, link, and command I/O.
pub trait RegisterIo {
    /// Controller power/read-only access is ready after the pre-CAP source
    /// phase; it does not imply a DMA aperture or an HCE-ready link.
    fn controller_power_ready(&mut self) -> bool {
        self.platform_ready()
    }
    fn platform_ready(&mut self) -> bool;
    fn read32(&mut self, offset: u32) -> Result<u32, Error>;
    fn write32(&mut self, offset: u32, value: u32) -> Result<(), Error>;
    fn dma_write_barrier(&mut self) -> Result<(), Error>;
    fn dma_read_barrier(&mut self) -> Result<(), Error>;
    /// Wait at least this long, allowing a native IRQ/wait implementation to sleep.
    fn delay_us(&mut self, microseconds: u32);
}

/// DMA-copy interface for a future native mapping adapter. Every address must
/// already belong to the exclusive UFS DMA domain and remain mapped for the call;
/// each access must revalidate the current DMA/lifecycle generation.
pub trait DmaIo {
    fn ready(&mut self) -> bool;
    fn write(&mut self, address: u64, bytes: &[u8]) -> Result<(), Error>;
    fn read(&mut self, address: u64, bytes: &mut [u8]) -> Result<(), Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Interrupt {
    pub raw: u32,
    pub transfer_complete: bool,
    pub link_startup: bool,
    pub uic_complete: bool,
    pub errors: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LunInfo {
    pub lun: u8,
    pub inquiry: protocol::StandardInquiry,
    pub capacity: protocol::Capacity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HciSnapshot {
    pub version: HciVersion,
    pub capabilities: ControllerCapabilities,
    pub controller_status: u32,
}

/// Decoded UFSHCI CAP register. Slot counts use the hardware's zero-based
/// fields converted to usable counts, so list allocation can be checked before
/// the host is enabled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControllerCapabilities {
    pub raw: u32,
    pub transfer_request_slots: u8,
    pub ready_to_transfer_requests: u16,
    pub task_management_slots: u8,
    pub auto_hibern8: bool,
    pub addressing_64_bit: bool,
    pub out_of_order_data_delivery: bool,
    pub uic_dme_test_mode: bool,
    pub crypto: bool,
}

impl ControllerCapabilities {
    pub const fn from_register(raw: u32) -> Self {
        Self {
            raw,
            transfer_request_slots: ((raw & CAP_TRANSFER_SLOTS) + 1) as u8,
            ready_to_transfer_requests: (((raw & CAP_READY_TO_TRANSFER_REQUESTS) >> 8) + 1) as u16,
            task_management_slots: (((raw & CAP_TASK_MANAGEMENT_SLOTS) >> 16) + 1) as u8,
            auto_hibern8: raw & CAP_AUTO_HIBERN8 != 0,
            addressing_64_bit: raw & CAP_64_BIT != 0,
            out_of_order_data_delivery: raw & CAP_OUT_OF_ORDER != 0,
            uic_dme_test_mode: raw & CAP_UIC_DME_TEST_MODE != 0,
            crypto: raw & CAP_CRYPTO != 0,
        }
    }
}

pub struct Host {
    state: State,
    capabilities: Option<ControllerCapabilities>,
    hci_version: Option<HciVersion>,
    transfer_list: Option<u64>,
    task_list: Option<u64>,
}

impl Host {
    pub const fn new() -> Self {
        Self {
            state: State::Disabled,
            capabilities: None,
            hci_version: None,
            transfer_list: None,
            task_list: None,
        }
    }

    pub const fn state(&self) -> State {
        self.state
    }

    pub const fn capabilities(&self) -> Option<ControllerCapabilities> {
        self.capabilities
    }

    /// Read the source-ordered CAP, VERSION, and HCS registers without writing
    /// any controller register. Only the pre-CAP power gate is required; no
    /// DMA/SMMU readiness or HCE/link startup is inferred from this snapshot.
    pub fn read_only_snapshot<I: RegisterIo>(io: &mut I) -> Result<HciSnapshot, Error> {
        if !io.controller_power_ready() {
            return Err(Error::PlatformNotReady);
        }
        let capabilities = ControllerCapabilities::from_register(io.read32(REG_CAPABILITIES)?);
        let version = decode_hci_version(io.read32(REG_VERSION)?)?;
        let controller_status = io.read32(REG_CONTROLLER_STATUS)?;
        Ok(HciSnapshot {
            version,
            capabilities,
            controller_status,
        })
    }

    /// Enable HCE with a strict poll limit. Qualcomm platform initialization is
    /// a required external precondition and is checked before touching registers.
    pub fn enable<I: RegisterIo>(&mut self, io: &mut I, poll_limit: u32) -> Result<(), Error> {
        if self.state != State::Disabled {
            return Err(Error::InvalidState);
        }
        validate_poll_limit(poll_limit)?;
        let snapshot = Self::read_only_snapshot(io)?;
        if !io.platform_ready() {
            return Err(Error::PlatformNotReady);
        }
        if self.checked(io.read32(REG_CONTROLLER_ENABLE))? & HCE_ENABLE != 0 {
            return self.fail(Error::ControllerActive);
        }
        self.hci_version = Some(snapshot.version);
        self.capabilities = Some(snapshot.capabilities);
        self.ensure_platform(io)?;
        self.checked(io.write32(REG_CONTROLLER_ENABLE, HCE_ENABLE))?;
        // The matching host driver waits before reading HCE because the bit
        // may transiently retain its pre-initialization value.
        io.delay_us(1_000);
        for _ in 0..poll_limit {
            if self.checked(io.read32(REG_CONTROLLER_ENABLE))? & HCE_ENABLE != 0 {
                self.state = State::Enabled;
                return Ok(());
            }
            io.delay_us(5_000);
        }
        self.state = State::Failed;
        Err(Error::Timeout)
    }

    /// Issue the Sunfish generic DME_LINK_STARTUP UIC command and require both
    /// a successful UIC result and HCS device-present indication.
    pub fn link_startup<I: RegisterIo>(
        &mut self,
        io: &mut I,
        poll_limit: u32,
    ) -> Result<(), Error> {
        if self.state != State::Enabled {
            return Err(Error::InvalidState);
        }
        validate_poll_limit(poll_limit)?;
        if !io.platform_ready() {
            self.state = State::Failed;
            return Err(Error::PlatformNotReady);
        }
        let host_status = self.checked(io.read32(REG_CONTROLLER_STATUS))?;
        if host_status & HCS_UIC_READY == 0 {
            return self.fail(Error::UicNotReady);
        }
        let pending = self.checked(io.read32(REG_INTERRUPT_STATUS))?;
        if pending & INT_ERRORS != 0 {
            return self.fail(Error::HostError);
        }
        if pending != 0 {
            self.checked(io.write32(REG_INTERRUPT_STATUS, pending))?;
        }
        self.checked(io.write32(REG_UIC_ARGUMENT_1, 0))?;
        self.checked(io.write32(REG_UIC_ARGUMENT_2, 0))?;
        self.checked(io.write32(REG_UIC_ARGUMENT_3, 0))?;
        self.checked(io.dma_write_barrier())?;
        self.ensure_platform(io)?;
        self.checked(io.write32(REG_UIC_COMMAND, UIC_DME_LINK_STARTUP))?;
        for _ in 0..poll_limit {
            let status = self.checked(io.read32(REG_INTERRUPT_STATUS))?;
            if status & INT_ERRORS != 0 {
                self.checked(io.write32(REG_INTERRUPT_STATUS, status))?;
                return self.fail(Error::HostError);
            }
            if status & INT_UIC_COMPLETE != 0 {
                self.checked(io.write32(REG_INTERRUPT_STATUS, INT_UIC_COMPLETE))?;
                let result = self.checked(io.read32(REG_UIC_ARGUMENT_2))? & UIC_RESULT_MASK;
                if result != 0 {
                    return self.fail(Error::UicFailure);
                }
                let link_status = self.checked(io.read32(REG_CONTROLLER_STATUS))?;
                if link_status & HCS_DEVICE_PRESENT == 0 {
                    return self.fail(Error::DeviceMissing);
                }
                self.state = State::LinkUp;
                return Ok(());
            }
            io.delay_us(1_000);
        }
        self.state = State::Failed;
        Err(Error::Timeout)
    }

    /// Program one or more controller-owned UTRD/UTMRD lists. The list memory
    /// itself must already be DMA-mapped, page-stable, and exclusively owned.
    pub fn make_operational<I: RegisterIo>(
        &mut self,
        io: &mut I,
        transfer_list: u64,
        task_list: u64,
    ) -> Result<(), Error> {
        if self.state != State::LinkUp {
            return Err(Error::InvalidState);
        }
        if !io.platform_ready() {
            self.state = State::Failed;
            return Err(Error::PlatformNotReady);
        }
        let pending = self.checked(io.read32(REG_INTERRUPT_STATUS))?;
        if pending & INT_ERRORS != 0 {
            return self.fail(Error::HostError);
        }
        if pending != 0 {
            self.checked(io.write32(REG_INTERRUPT_STATUS, pending))?;
        }
        let capabilities = self.capabilities.ok_or(Error::InvalidState)?;
        validate_list(transfer_list, task_list, capabilities.addressing_64_bit)?;
        self.checked(io.write32(REG_INTERRUPT_ENABLE, INT_ENABLED))?;
        self.checked(io.write32(REG_UTRL_BASE_LO, transfer_list as u32))?;
        self.checked(io.write32(REG_UTRL_BASE_HI, (transfer_list >> 32) as u32))?;
        self.checked(io.write32(REG_UTMRL_BASE_LO, task_list as u32))?;
        self.checked(io.write32(REG_UTMRL_BASE_HI, (task_list >> 32) as u32))?;
        self.checked(io.dma_write_barrier())?;
        let status = self.checked(io.read32(REG_CONTROLLER_STATUS))?;
        if status & HCS_DEVICE_PRESENT == 0 {
            return self.fail(Error::DeviceMissing);
        }
        if status & HCS_ALL_READY != HCS_ALL_READY {
            return self.fail(Error::ListsNotReady);
        }
        self.ensure_platform(io)?;
        self.checked(io.write32(REG_UTMRL_RUN_STOP, 1))?;
        self.checked(io.write32(REG_UTRL_RUN_STOP, 1))?;
        self.transfer_list = Some(transfer_list);
        self.task_list = Some(task_list);
        self.state = State::Operational;
        Ok(())
    }

    /// Submit exactly one UTP transfer through slot zero, wait for its IRQ, and
    /// validate the completed UTRD/response. Payload bytes live in the caller's
    /// already mapped `data_address`; this method only stages command metadata.
    pub fn execute_command<I: RegisterIo, D: DmaIo>(
        &mut self,
        io: &mut I,
        dma: &mut D,
        command: protocol::ScsiCommand,
        lun: u8,
        task_tag: u8,
        command_descriptor: u64,
        data_address: Option<u64>,
        poll_limit: u32,
    ) -> Result<(), Error> {
        if self.state != State::Operational {
            return Err(Error::InvalidState);
        }
        validate_poll_limit(poll_limit)?;
        self.ensure_live(io, dma)?;
        let transfer_list = self.transfer_list.ok_or(Error::InvalidState)?;
        let task_list = self.task_list.ok_or(Error::InvalidState)?;
        if overlaps(
            command_descriptor,
            UCD_BYTES as u64,
            transfer_list,
            MIN_LIST_BYTES,
        ) || overlaps(
            command_descriptor,
            UCD_BYTES as u64,
            task_list,
            MIN_LIST_BYTES,
        ) {
            return Err(Error::InvalidDma);
        }
        if let Some(data) = data_address {
            let bytes = command.transfer_bytes() as u64;
            if bytes != 0
                && (overlaps(data, bytes, transfer_list, MIN_LIST_BYTES)
                    || overlaps(data, bytes, task_list, MIN_LIST_BYTES))
            {
                return Err(Error::InvalidDma);
            }
        }
        if self.checked(io.read32(REG_CONTROLLER_STATUS))? & HCS_DEVICE_PRESENT == 0 {
            return self.fail(Error::DeviceMissing);
        }
        if self.checked(io.read32(REG_UTRL_DOORBELL))? & 1 != 0 {
            return self.fail(Error::Busy);
        }
        let pending = self.checked(io.read32(REG_INTERRUPT_STATUS))?;
        if pending & INT_ERRORS != 0 {
            return self.fail(Error::HostError);
        }
        if pending != 0 {
            self.checked(io.write32(REG_INTERRUPT_STATUS, pending))?;
        }
        let hci_version = self.hci_version.ok_or(Error::InvalidState)?;
        let capabilities = self.capabilities.ok_or(Error::InvalidState)?;
        let packet = protocol::encode_transfer(
            command,
            lun,
            task_tag,
            hci_version,
            capabilities.addressing_64_bit,
            command_descriptor,
            data_address,
        )
        .map_err(Error::Packet)?;

        let mut ucd = [0; UCD_BYTES];
        ucd[..protocol::UPIU_BYTES].copy_from_slice(&packet.command_upiu);
        ucd[UCD_PRDT_OFFSET..UCD_PRDT_OFFSET + protocol::PRDT_BYTES].copy_from_slice(&packet.prdt);
        self.ensure_live(io, dma)?;
        self.checked(dma.write(command_descriptor, &ucd))?;
        self.ensure_live(io, dma)?;
        self.checked(dma.write(transfer_list, &packet.utrd))?;
        self.checked(io.dma_write_barrier())?;
        self.ensure_live(io, dma)?;
        self.checked(io.write32(REG_UTRL_DOORBELL, 1))?;

        let mut completed = false;
        for _ in 0..poll_limit {
            self.ensure_live(io, dma)?;
            let irq = self.handle_interrupt(io)?;
            if irq.errors != 0 {
                return self.fail(Error::HostError);
            }
            if irq.transfer_complete {
                completed = true;
                break;
            }
            io.delay_us(1_000);
        }
        if !completed {
            return self.fail(Error::Timeout);
        }
        if self.checked(io.read32(REG_UTRL_DOORBELL))? & 1 != 0 {
            return self.fail(Error::Busy);
        }
        self.ensure_live(io, dma)?;
        self.checked(io.dma_read_barrier())?;
        let mut utrd = [0; protocol::UTRD_BYTES];
        let mut response = [0; protocol::UPIU_BYTES];
        self.checked(dma.read(transfer_list, &mut utrd))?;
        self.ensure_live(io, dma)?;
        self.checked(dma.read(command_descriptor + UCD_RESPONSE_OFFSET, &mut response))?;
        protocol::validate_completion(&utrd, &response, lun, task_tag)
            .map_err(Error::Packet)
            .or_else(|error| self.fail(error))
    }

    /// Run one bounded, read-only discovery sequence against a caller-selected
    /// LUN. The caller supplies a DMA buffer of at least STANDARD_INQUIRY_BYTES
    /// that is disjoint from the command descriptor and UFSHCI lists. No LUN
    /// scan, retry, READ CAPACITY(16), partition-table read, or media write is
    /// performed. Any command or response failure stops the host.
    pub fn discover_lun<I: RegisterIo, D: DmaIo>(
        &mut self,
        io: &mut I,
        dma: &mut D,
        lun: u8,
        first_task_tag: u8,
        command_descriptor: u64,
        data_buffer: u64,
        poll_limit: u32,
    ) -> Result<LunInfo, Error> {
        let inquiry_tag = first_task_tag
            .checked_add(1)
            .ok_or(Error::Packet(protocol::Error::InvalidTag))?;
        let capacity_tag = first_task_tag
            .checked_add(2)
            .ok_or(Error::Packet(protocol::Error::InvalidTag))?;
        if capacity_tag >= 32 {
            return Err(Error::Packet(protocol::Error::InvalidTag));
        }

        self.execute_command(
            io,
            dma,
            protocol::ScsiCommand::test_unit_ready(),
            lun,
            first_task_tag,
            command_descriptor,
            None,
            poll_limit,
        )?;
        self.execute_command(
            io,
            dma,
            protocol::ScsiCommand::inquiry(),
            lun,
            inquiry_tag,
            command_descriptor,
            Some(data_buffer),
            poll_limit,
        )?;
        self.ensure_live(io, dma)?;
        let mut inquiry_bytes = [0; protocol::STANDARD_INQUIRY_BYTES];
        self.checked(dma.read(data_buffer, &mut inquiry_bytes))?;
        self.ensure_live(io, dma)?;
        let inquiry = match protocol::parse_standard_inquiry(&inquiry_bytes) {
            Ok(value) => value,
            Err(error) => return self.fail(Error::Packet(error)),
        };

        self.execute_command(
            io,
            dma,
            protocol::ScsiCommand::read_capacity10(),
            lun,
            capacity_tag,
            command_descriptor,
            Some(data_buffer),
            poll_limit,
        )?;
        self.ensure_live(io, dma)?;
        let mut capacity_bytes = [0; protocol::READ_CAPACITY10_BYTES];
        self.checked(dma.read(data_buffer, &mut capacity_bytes))?;
        self.ensure_live(io, dma)?;
        let capacity = match protocol::parse_read_capacity10(&capacity_bytes) {
            Ok(value) => value,
            Err(error) => return self.fail(Error::Packet(error)),
        };

        Ok(LunInfo {
            lun,
            inquiry,
            capacity,
        })
    }

    /// Read and acknowledge one bounded interrupt snapshot. Fatal/link errors
    /// quarantine the host; callers must stop submitting descriptors.
    pub fn handle_interrupt<I: RegisterIo>(&mut self, io: &mut I) -> Result<Interrupt, Error> {
        if self.state != State::Operational {
            return Err(Error::InvalidState);
        }
        if !io.platform_ready() {
            self.state = State::Failed;
            return Err(Error::PlatformNotReady);
        }
        let raw = self.checked(io.read32(REG_INTERRUPT_STATUS))?;
        if raw == 0 {
            return Ok(Interrupt {
                raw,
                transfer_complete: false,
                link_startup: false,
                uic_complete: false,
                errors: 0,
            });
        }
        self.checked(io.write32(REG_INTERRUPT_STATUS, raw))?;
        let errors = raw & INT_ERRORS;
        if errors != 0 {
            self.state = State::Failed;
        }
        Ok(Interrupt {
            raw,
            transfer_complete: raw & INT_TRANSFER_COMPLETE != 0,
            link_startup: raw & INT_LINK_STARTUP != 0,
            uic_complete: raw & INT_UIC_COMPLETE != 0,
            errors,
        })
    }

    fn fail<T>(&mut self, error: Error) -> Result<T, Error> {
        self.state = State::Failed;
        Err(error)
    }

    fn checked<T>(&mut self, result: Result<T, Error>) -> Result<T, Error> {
        match result {
            Ok(value) => Ok(value),
            Err(error) => self.fail(error),
        }
    }

    fn ensure_platform<I: RegisterIo>(&mut self, io: &mut I) -> Result<(), Error> {
        if io.platform_ready() {
            Ok(())
        } else {
            self.fail(Error::PlatformNotReady)
        }
    }

    fn ensure_live<I: RegisterIo, D: DmaIo>(
        &mut self,
        io: &mut I,
        dma: &mut D,
    ) -> Result<(), Error> {
        if io.platform_ready() && dma.ready() {
            Ok(())
        } else {
            self.fail(Error::PlatformNotReady)
        }
    }
}

impl Default for Host {
    fn default() -> Self {
        Self::new()
    }
}

fn validate_list(transfer: u64, task: u64, supports_64_bit: bool) -> Result<(), Error> {
    for address in [transfer, task] {
        if address == 0 || address & (DMA_LIST_ALIGN - 1) != 0 {
            return Err(Error::InvalidDma);
        }
        let end = address
            .checked_add(MIN_LIST_BYTES - 1)
            .ok_or(Error::InvalidDma)?;
        if !supports_64_bit && end > u64::from(u32::MAX) {
            return Err(Error::AddressWidth);
        }
    }
    let transfer_end = transfer
        .checked_add(MIN_LIST_BYTES)
        .ok_or(Error::InvalidDma)?;
    let task_end = task.checked_add(MIN_LIST_BYTES).ok_or(Error::InvalidDma)?;
    if transfer < task_end && task < transfer_end {
        return Err(Error::InvalidDma);
    }
    Ok(())
}

fn decode_hci_version(value: u32) -> Result<HciVersion, Error> {
    match value {
        0x0000_0200 => Ok(HciVersion::V2_0),
        0x0000_0210 => Ok(HciVersion::V2_1),
        _ => Err(Error::HciVersion),
    }
}

fn overlaps(a: u64, a_bytes: u64, b: u64, b_bytes: u64) -> bool {
    let Some(a_end) = a.checked_add(a_bytes) else {
        return true;
    };
    let Some(b_end) = b.checked_add(b_bytes) else {
        return true;
    };
    a < b_end && b < a_end
}

fn validate_poll_limit(poll_limit: u32) -> Result<(), Error> {
    if poll_limit == 0 {
        return Err(Error::InvalidState);
    }
    if poll_limit > MAX_POLL_ATTEMPTS {
        return Err(Error::PollLimit);
    }
    Ok(())
}
