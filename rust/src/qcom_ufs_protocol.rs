//! Bounded UFSHCI transfer encoding for the Sunfish storage-provider port.
//!
//! The descriptor layout was checked against Google's Sunfish 4.14 kernel
//! branch at 6ef255005ceaae118d35fba8585de7ba1d4d5efb. This module builds
//! SCSI UPIUs, UTRDs, and one-entry PRDTs; it does not initialize the UFS
//! controller/PHY, own DMA/IOMMU mappings, or access a physical device.
#![forbid(unsafe_code)]

pub const SECTOR_BYTES: u64 = 512;
pub const MIN_LOGICAL_BLOCK_BYTES: u32 = 512;
pub const MAX_LOGICAL_BLOCK_BYTES: u32 = 4096;
pub const MAX_DATA_BYTES: usize = 4096;
pub const MAX_PRDT_BYTES: usize = 256 * 1024;
pub const UPIU_BYTES: usize = 32;
pub const UTRD_BYTES: usize = 32;
pub const PRDT_BYTES: usize = 16;
pub const STANDARD_INQUIRY_BYTES: usize = 36;
pub const READ_CAPACITY10_BYTES: usize = 8;

const TRANSACTION_COMMAND: u8 = 0x01;
const TRANSACTION_RESPONSE: u8 = 0x21;
const UPIU_FLAG_WRITE: u8 = 0x20;
const UPIU_FLAG_READ: u8 = 0x40;
const UPIU_COMMAND_SET_SCSI: u8 = 0;
const UTP_HOST_TO_DEVICE: u32 = 0x0200_0000;
const UTP_DEVICE_TO_HOST: u32 = 0x0400_0000;
const UTP_COMMAND_TYPE_OFFSET: u32 = 28;
const OCS_INVALID: u32 = 0x0f;
const OCS_SUCCESS: u32 = 0;
const RESPONSE_OFFSET_DWORDS: u16 = 128;
const RESPONSE_LENGTH_DWORDS: u16 = 128;
const PRDT_OFFSET_DWORDS: u16 = 256;
const CDB_READ_10: u8 = 0x28;
const CDB_WRITE_10: u8 = 0x2a;
const CDB_SYNCHRONIZE_CACHE_10: u8 = 0x35;
const CDB_TEST_UNIT_READY: u8 = 0x00;
const CDB_INQUIRY: u8 = 0x12;
const CDB_READ_CAPACITY_10: u8 = 0x25;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidLun,
    InvalidTag,
    ZeroBlocks,
    Range,
    TooLarge,
    MissingDataAddress,
    UnexpectedDataAddress,
    MisalignedAddress,
    AliasedBuffers,
    AddressOverflow,
    AddressWidth,
    Ocs,
    Response,
    ScsiStatus,
    Residual,
    DiscoveryResponse,
    UnsupportedPeripheral,
    CapacityNeeds16,
    UnsupportedBlockSize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HciVersion {
    V1_0,
    V1_1,
    V2_0,
    V2_1,
    V3_0,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Direction {
    None,
    Read,
    Write,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScsiCommand {
    cdb: [u8; 16],
    direction: Direction,
    transfer_bytes: usize,
}

impl ScsiCommand {
    pub fn test_unit_ready() -> Self {
        let mut cdb = [0; 16];
        cdb[0] = CDB_TEST_UNIT_READY;
        Self {
            cdb,
            direction: Direction::None,
            transfer_bytes: 0,
        }
    }

    pub fn inquiry() -> Self {
        let mut cdb = [0; 16];
        cdb[0] = CDB_INQUIRY;
        cdb[4] = STANDARD_INQUIRY_BYTES as u8;
        Self {
            cdb,
            direction: Direction::Read,
            transfer_bytes: STANDARD_INQUIRY_BYTES,
        }
    }

    pub fn read_capacity10() -> Self {
        let mut cdb = [0; 16];
        cdb[0] = CDB_READ_CAPACITY_10;
        Self {
            cdb,
            direction: Direction::Read,
            transfer_bytes: READ_CAPACITY10_BYTES,
        }
    }

    pub fn read10(first_lba: u64, blocks: u16) -> Result<Self, Error> {
        Self::read10_with_block_size(first_lba, blocks, SECTOR_BYTES as u32)
    }

    pub fn read10_with_block_size(
        first_lba: u64,
        blocks: u16,
        block_bytes: u32,
    ) -> Result<Self, Error> {
        Self::rw10(CDB_READ_10, Direction::Read, first_lba, blocks, block_bytes)
    }

    pub fn write10(first_lba: u64, blocks: u16) -> Result<Self, Error> {
        Self::write10_with_block_size(first_lba, blocks, SECTOR_BYTES as u32)
    }

    pub fn write10_with_block_size(
        first_lba: u64,
        blocks: u16,
        block_bytes: u32,
    ) -> Result<Self, Error> {
        Self::rw10(
            CDB_WRITE_10,
            Direction::Write,
            first_lba,
            blocks,
            block_bytes,
        )
    }

    fn rw10(
        opcode: u8,
        direction: Direction,
        first_lba: u64,
        blocks: u16,
        block_bytes: u32,
    ) -> Result<Self, Error> {
        if blocks == 0 {
            return Err(Error::ZeroBlocks);
        }
        if !supported_logical_block_bytes(block_bytes) {
            return Err(Error::UnsupportedBlockSize);
        }
        let last_lba = first_lba
            .checked_add(u64::from(blocks) - 1)
            .ok_or(Error::Range)?;
        let lba = u32::try_from(first_lba).map_err(|_| Error::Range)?;
        if last_lba > u64::from(u32::MAX) {
            return Err(Error::Range);
        }
        let transfer_bytes = usize::from(blocks)
            .checked_mul(block_bytes as usize)
            .ok_or(Error::TooLarge)?;
        if transfer_bytes > MAX_DATA_BYTES || transfer_bytes > MAX_PRDT_BYTES {
            return Err(Error::TooLarge);
        }
        let mut cdb = [0; 16];
        cdb[0] = opcode;
        cdb[2..6].copy_from_slice(&lba.to_be_bytes());
        cdb[7..9].copy_from_slice(&blocks.to_be_bytes());
        Ok(Self {
            cdb,
            direction,
            transfer_bytes,
        })
    }

    pub fn synchronize_cache10() -> Self {
        let mut cdb = [0; 16];
        cdb[0] = CDB_SYNCHRONIZE_CACHE_10;
        Self {
            cdb,
            direction: Direction::None,
            transfer_bytes: 0,
        }
    }

    pub fn cdb(&self) -> &[u8; 16] {
        &self.cdb
    }

    pub fn transfer_bytes(&self) -> usize {
        self.transfer_bytes
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StandardInquiry {
    pub removable: bool,
    pub vendor_id: [u8; 8],
    pub product_id: [u8; 16],
    pub product_revision: [u8; 4],
}

/// Parse only the 36-byte standard INQUIRY prefix for a direct-access LU.
/// Vendor strings remain raw bytes and must not be treated as authority.
pub fn parse_standard_inquiry(bytes: &[u8]) -> Result<StandardInquiry, Error> {
    if bytes.len() != STANDARD_INQUIRY_BYTES || bytes[4] < 31 {
        return Err(Error::DiscoveryResponse);
    }
    let peripheral_qualifier = bytes[0] >> 5;
    let peripheral_type = bytes[0] & 0x1f;
    if peripheral_qualifier != 0 || peripheral_type != 0 {
        return Err(Error::UnsupportedPeripheral);
    }
    Ok(StandardInquiry {
        removable: bytes[1] & 0x80 != 0,
        vendor_id: bytes[8..16]
            .try_into()
            .map_err(|_| Error::DiscoveryResponse)?,
        product_id: bytes[16..32]
            .try_into()
            .map_err(|_| Error::DiscoveryResponse)?,
        product_revision: bytes[32..36]
            .try_into()
            .map_err(|_| Error::DiscoveryResponse)?,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Capacity {
    /// Capacity expressed in the storage API's 512-byte sector units.
    pub sectors: u64,
    /// Native SCSI logical block size, currently 512 or 4096 bytes.
    pub block_bytes: u32,
}

/// Parse READ CAPACITY(10), retaining native block geometry while exposing
/// capacity in the provider's 512-byte address units. The `0xffffffff` sentinel
/// requires READ CAPACITY(16), which this bounded packet layer does not issue.
pub fn parse_read_capacity10(bytes: &[u8]) -> Result<Capacity, Error> {
    if bytes.len() != READ_CAPACITY10_BYTES {
        return Err(Error::DiscoveryResponse);
    }
    let last_lba = u32::from_be_bytes(
        bytes[0..4]
            .try_into()
            .map_err(|_| Error::DiscoveryResponse)?,
    );
    if last_lba == u32::MAX {
        return Err(Error::CapacityNeeds16);
    }
    let block_bytes = u32::from_be_bytes(
        bytes[4..8]
            .try_into()
            .map_err(|_| Error::DiscoveryResponse)?,
    );
    if !supported_logical_block_bytes(block_bytes) {
        return Err(Error::UnsupportedBlockSize);
    }
    let native_blocks = u64::from(last_lba) + 1;
    let capacity_bytes = native_blocks
        .checked_mul(u64::from(block_bytes))
        .ok_or(Error::DiscoveryResponse)?;
    Ok(Capacity {
        sectors: capacity_bytes / SECTOR_BYTES,
        block_bytes,
    })
}

pub const fn supported_logical_block_bytes(block_bytes: u32) -> bool {
    block_bytes == MIN_LOGICAL_BLOCK_BYTES || block_bytes == MAX_LOGICAL_BLOCK_BYTES
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransferPacket {
    /// UFSHCI transfer request descriptor, encoded little-endian.
    pub utrd: [u8; UTRD_BYTES],
    /// SCSI command UPIU, encoded with big-endian multi-byte fields.
    pub command_upiu: [u8; UPIU_BYTES],
    /// One UFSHCI physical region descriptor; zeroed for cache sync.
    pub prdt: [u8; PRDT_BYTES],
    pub transfer_bytes: usize,
}

fn check_dma_range(address: u64, bytes: usize, supports_64_bit: bool) -> Result<(), Error> {
    if bytes == 0 {
        return Err(Error::TooLarge);
    }
    let end = address
        .checked_add(u64::try_from(bytes).map_err(|_| Error::AddressOverflow)?)
        .ok_or(Error::AddressOverflow)?;
    if !supports_64_bit && end - 1 > u64::from(u32::MAX) {
        return Err(Error::AddressWidth);
    }
    Ok(())
}

fn set_prdt_entry(
    prdt: &mut [u8; PRDT_BYTES],
    address: u64,
    bytes: usize,
    supports_64_bit: bool,
    command_descriptor_address: u64,
) -> Result<(), Error> {
    if bytes > MAX_DATA_BYTES || bytes > MAX_PRDT_BYTES {
        return Err(Error::TooLarge);
    }
    if address & 0x3 != 0 || bytes & 0x3 != 0 {
        return Err(Error::MisalignedAddress);
    }
    check_dma_range(address, bytes, supports_64_bit)?;
    let data_end = address
        .checked_add(u64::try_from(bytes).map_err(|_| Error::AddressOverflow)?)
        .ok_or(Error::AddressOverflow)?;
    let command_end = command_descriptor_address
        .checked_add(1040)
        .ok_or(Error::AddressOverflow)?;
    if address < command_end && command_descriptor_address < data_end {
        return Err(Error::AliasedBuffers);
    }
    prdt[0..8].copy_from_slice(&address.to_le_bytes());
    prdt[12..16].copy_from_slice(&((bytes as u32) - 1).to_le_bytes());
    Ok(())
}

/// Encode one bounded SCSI command. Data commands use exactly one PRDT entry.
/// `command_descriptor_address` and `data_address` must already be DMA-mapped
/// by the future trusted platform provider; these are addresses, not CPU pointers.
pub fn encode_transfer(
    command: ScsiCommand,
    lun: u8,
    task_tag: u8,
    hci: HciVersion,
    supports_64_bit: bool,
    command_descriptor_address: u64,
    data_address: Option<u64>,
) -> Result<TransferPacket, Error> {
    if lun >= 8 {
        return Err(Error::InvalidLun);
    }
    if task_tag >= 32 {
        return Err(Error::InvalidTag);
    }
    if command_descriptor_address & 0x7f != 0 {
        return Err(Error::MisalignedAddress);
    }
    check_dma_range(command_descriptor_address, 1040, supports_64_bit)?;

    let mut prdt = [0; PRDT_BYTES];
    let mut command_upiu = [0; UPIU_BYTES];
    let mut utrd = [0; UTRD_BYTES];

    let direction_bits = match (command.direction, data_address) {
        (Direction::None, None) => {
            if command.transfer_bytes != 0 {
                return Err(Error::UnexpectedDataAddress);
            }
            0
        }
        (Direction::None, Some(_)) => return Err(Error::UnexpectedDataAddress),
        (Direction::Read | Direction::Write, None) => return Err(Error::MissingDataAddress),
        (Direction::Read, Some(address)) => {
            set_prdt_entry(
                &mut prdt,
                address,
                command.transfer_bytes,
                supports_64_bit,
                command_descriptor_address,
            )?;
            UTP_DEVICE_TO_HOST
        }
        (Direction::Write, Some(address)) => {
            set_prdt_entry(
                &mut prdt,
                address,
                command.transfer_bytes,
                supports_64_bit,
                command_descriptor_address,
            )?;
            UTP_HOST_TO_DEVICE
        }
    };

    let upiu_flags = match command.direction {
        Direction::None => 0,
        Direction::Read => UPIU_FLAG_READ,
        Direction::Write => UPIU_FLAG_WRITE,
    };
    command_upiu[0] = TRANSACTION_COMMAND;
    command_upiu[1] = upiu_flags;
    command_upiu[2] = lun;
    command_upiu[3] = task_tag;
    command_upiu[4] = UPIU_COMMAND_SET_SCSI;
    command_upiu[12..16].copy_from_slice(&(command.transfer_bytes as u32).to_be_bytes());
    command_upiu[16..32].copy_from_slice(&command.cdb);

    let command_type = if hci == HciVersion::V2_0 { 1 } else { 0 };
    let dword0 = direction_bits | (command_type << UTP_COMMAND_TYPE_OFFSET);
    utrd[0..4].copy_from_slice(&dword0.to_le_bytes());
    utrd[8..12].copy_from_slice(&OCS_INVALID.to_le_bytes());
    utrd[16..20].copy_from_slice(&(command_descriptor_address as u32).to_le_bytes());
    utrd[20..24].copy_from_slice(&((command_descriptor_address >> 32) as u32).to_le_bytes());
    utrd[24..26].copy_from_slice(&RESPONSE_LENGTH_DWORDS.to_le_bytes());
    utrd[26..28].copy_from_slice(&RESPONSE_OFFSET_DWORDS.to_le_bytes());
    let prdt_entries = u16::from(command.transfer_bytes != 0);
    utrd[28..30].copy_from_slice(&prdt_entries.to_le_bytes());
    utrd[30..32].copy_from_slice(&PRDT_OFFSET_DWORDS.to_le_bytes());

    Ok(TransferPacket {
        utrd,
        command_upiu,
        prdt,
        transfer_bytes: command.transfer_bytes,
    })
}

/// Accept only a fully completed descriptor and matching successful SCSI UPIU.
/// The caller must quarantine uncertain IO instead of retrying it.
pub fn validate_completion(
    utrd: &[u8; UTRD_BYTES],
    response_upiu: &[u8; UPIU_BYTES],
    lun: u8,
    task_tag: u8,
) -> Result<(), Error> {
    let ocs = u32::from_le_bytes([utrd[8], utrd[9], utrd[10], utrd[11]]) & 0x0f;
    if ocs != OCS_SUCCESS {
        return Err(Error::Ocs);
    }
    if response_upiu[0] != TRANSACTION_RESPONSE
        || response_upiu[2] != lun
        || response_upiu[3] != task_tag
        || response_upiu[6] != 0
    {
        return Err(Error::Response);
    }
    if response_upiu[7] != 0 {
        return Err(Error::ScsiStatus);
    }
    let residual = u32::from_be_bytes([
        response_upiu[12],
        response_upiu[13],
        response_upiu[14],
        response_upiu[15],
    ]);
    if residual != 0 {
        return Err(Error::Residual);
    }
    Ok(())
}
