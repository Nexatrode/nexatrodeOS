//! Pure request schemas for Sunfish's host storage and heap-memory endpoints.
//!
//! References: linux-msm/rmtfs b30a3eb38f9af283f18dbd3c7755653efc52c094
//! qmi_rmtfs.{qmi,c}; Lineage kernel 28f9290ae067d3e7857126eb6312720dbe9c7c80
//! drivers/soc/qcom/memshare/heap_mem_ext_v01.{h,c}.
//!
//! Decoding grants no storage or memory authority. Handles, client IDs and
//! addresses must later be checked against backend state and generation leases.
//! Size ceilings below are the initial Sunfish profile, not universal QMI limits.
#![forbid(unsafe_code)]

use crate::qcom_qmi::{Error, Kind, Message};

pub const STORAGE_BUFFER_BYTES: u32 = 2 * 1024 * 1024;
pub const HEAP_BUFFER_BYTES: u32 = 5 * 1024 * 1024;
pub const MAX_IOVECS: usize = 255;
pub const MAX_HEAP_BLOCKS: usize = 64;
const SECTOR_BYTES: u64 = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Service {
    RemoteStorage,
    HeapMemory,
    ServiceRegistry,
    IpaServer,
}

impl Service {
    pub const fn descriptor(self) -> (u32, u8, u32) {
        match self {
            Self::RemoteStorage => (0x0e, 1, 0),
            Self::HeapMemory => (0x34, 1, 1),
            Self::ServiceRegistry => (0x40, 1, 1),
            Self::IpaServer => (
                crate::qcom_ipa_service::SERVICE_ID,
                crate::qcom_ipa_service::SERVICE_VERSION as u8,
                crate::qcom_ipa_service::AP_INSTANCE,
            ),
        }
    }

    pub(crate) const fn index(self) -> usize {
        match self {
            Self::RemoteStorage => 0,
            Self::HeapMemory => 1,
            Self::ServiceRegistry => 2,
            Self::IpaServer => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Partition {
    ModemFs1,
    ModemFs2,
    ModemFsg,
    ModemFsc,
}

impl Partition {
    fn decode(path: &[u8]) -> Result<Self, Error> {
        match path {
            b"/boot/modem_fs1" => Ok(Self::ModemFs1),
            b"/boot/modem_fs2" => Ok(Self::ModemFs2),
            b"/boot/modem_fsg" => Ok(Self::ModemFsg),
            b"/boot/modem_fsc" => Ok(Self::ModemFsc),
            _ => Err(Error::InvalidValue),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    Read,
    Write,
}

fn boolean(value: u8) -> Result<bool, Error> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(Error::InvalidValue),
    }
}

fn bounded_size(value: u32, maximum: u32) -> Result<u32, Error> {
    if value == 0 || value > maximum {
        Err(Error::InvalidValue)
    } else {
        Ok(value)
    }
}

fn le32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn le64(bytes: &[u8]) -> u64 {
    u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Iovec {
    pub sector: u32,
    /// Historical wire name is phys_offset; it is an untrusted physical address.
    pub physical_address: u32,
    pub sectors: u32,
}

impl Iovec {
    pub fn disk_offset(self) -> u64 {
        u64::from(self.sector) * SECTOR_BYTES
    }

    pub fn byte_count(self) -> u64 {
        u64::from(self.sectors) * SECTOR_BYTES
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Iovecs<'a> {
    entries: &'a [u8],
}

impl<'a> Iovecs<'a> {
    fn decode(value: &'a [u8]) -> Result<Self, Error> {
        let (&count, entries) = value.split_first().ok_or(Error::InvalidValue)?;
        if usize::from(count) > MAX_IOVECS || entries.len() != usize::from(count) * 12 {
            return Err(Error::InvalidValue);
        }
        let list = Self { entries };
        let mut total = 0u64;
        for entry in list.iter() {
            let bytes = entry.byte_count();
            total = total.checked_add(bytes).ok_or(Error::RangeOverflow)?;
            if total > u64::from(STORAGE_BUFFER_BYTES) {
                return Err(Error::InvalidValue);
            }
            // Do not permit the 32-bit physical-address interval to wrap.
            if u64::from(entry.physical_address) + bytes > (1u64 << 32) {
                return Err(Error::RangeOverflow);
            }
        }
        Ok(list)
    }

    pub fn len(self) -> usize {
        self.entries.len() / 12
    }

    pub fn is_empty(self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(self) -> impl ExactSizeIterator<Item = Iovec> + 'a {
        self.entries.chunks_exact(12).map(|entry| Iovec {
            sector: le32(&entry[..4]),
            physical_address: le32(&entry[4..8]),
            sectors: le32(&entry[8..]),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageRequest<'a> {
    Open(Partition),
    Close {
        handle: u32,
    },
    ReadWrite {
        handle: u32,
        direction: Direction,
        iovecs: Iovecs<'a>,
        force_sync: bool,
    },
    Allocate {
        handle: u32,
        bytes: u32,
    },
    DeviceError {
        handle: u32,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Processor {
    Modem,
    Adsp,
    Wcnss,
}

impl Processor {
    fn decode(value: u32) -> Result<Self, Error> {
        match value {
            0 => Ok(Self::Modem),
            1 => Ok(Self::Adsp),
            2 => Ok(Self::Wcnss),
            _ => Err(Error::InvalidValue),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeapBlock {
    pub physical_address: u64,
    pub bytes: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeapBlocks<'a> {
    entries: &'a [u8],
}

impl<'a> HeapBlocks<'a> {
    fn decode(value: &'a [u8]) -> Result<Self, Error> {
        let (&count, entries) = value.split_first().ok_or(Error::InvalidValue)?;
        if usize::from(count) > MAX_HEAP_BLOCKS || entries.len() != usize::from(count) * 12 {
            return Err(Error::InvalidValue);
        }
        let list = Self { entries };
        let mut total = 0u64;
        for block in list.iter() {
            bounded_size(block.bytes, HEAP_BUFFER_BYTES)?;
            block
                .physical_address
                .checked_add(u64::from(block.bytes))
                .ok_or(Error::RangeOverflow)?;
            total += u64::from(block.bytes);
            if total > u64::from(HEAP_BUFFER_BYTES) {
                return Err(Error::InvalidValue);
            }
        }
        Ok(list)
    }

    pub fn len(self) -> usize {
        self.entries.len() / 12
    }

    pub fn is_empty(self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(self) -> impl ExactSizeIterator<Item = HeapBlock> + 'a {
        self.entries.chunks_exact(12).map(|entry| HeapBlock {
            physical_address: le64(&entry[..8]),
            bytes: le32(&entry[8..]),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryRequest<'a> {
    QuerySize {
        client: u32,
        processor: Option<Processor>,
    },
    Allocate {
        bytes: u32,
        client: u32,
        processor: Processor,
        sequence: u32,
        contiguous: Option<bool>,
        /// Wire enum 0..11 represents alignments from 2 bytes through 4 KiB.
        alignment: Option<u8>,
    },
    Free {
        blocks: HeapBlocks<'a>,
        client: Option<u32>,
        processor: Option<Processor>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Request<'a> {
    Storage(StorageRequest<'a>),
    Memory(MemoryRequest<'a>),
    ServiceRegistry(crate::qcom_servreg::Query<'a>),
    IpaServer { message_id: u16 },
}

/// Empty arrays and absent optional processor/client fields retain their wire
/// meaning. A later backend must validate handles, client policy, ownership,
/// ranges, overlapping blocks and actual leases before carrying out any action.
pub fn decode<'a>(service: Service, message: &Message<'a>) -> Result<Request<'a>, Error> {
    if message.kind != Kind::Request {
        return Err(Error::UnsupportedKind);
    }
    match service {
        Service::RemoteStorage => decode_storage(message).map(Request::Storage),
        Service::HeapMemory => decode_memory(message).map(Request::Memory),
        Service::ServiceRegistry => {
            crate::qcom_servreg::decode(message).map(Request::ServiceRegistry)
        }
        Service::IpaServer => {
            crate::qcom_ipa_service::validate_request(message)?;
            Ok(Request::IpaServer {
                message_id: message.message_id,
            })
        }
    }
}

fn decode_storage<'a>(message: &Message<'a>) -> Result<StorageRequest<'a>, Error> {
    Ok(match message.message_id {
        1 => {
            message.check_fields(&[1])?;
            StorageRequest::Open(Partition::decode(message.required(1)?)?)
        }
        2 | 5 => {
            message.check_fields(&[1])?;
            let handle = message.u32(1)?;
            if message.message_id == 2 {
                StorageRequest::Close { handle }
            } else {
                StorageRequest::DeviceError { handle }
            }
        }
        3 => {
            message.check_fields(&[1, 2, 3, 4])?;
            StorageRequest::ReadWrite {
                handle: message.u32(1)?,
                direction: if boolean(message.u8(2)?)? {
                    Direction::Write
                } else {
                    Direction::Read
                },
                iovecs: Iovecs::decode(message.required(3)?)?,
                force_sync: boolean(message.u8(4)?)?,
            }
        }
        4 => {
            message.check_fields(&[1, 2])?;
            StorageRequest::Allocate {
                handle: message.u32(1)?,
                bytes: bounded_size(message.u32(2)?, STORAGE_BUFFER_BYTES)?,
            }
        }
        _ => return Err(Error::UnsupportedMessage),
    })
}

fn decode_memory<'a>(message: &Message<'a>) -> Result<MemoryRequest<'a>, Error> {
    Ok(match message.message_id {
        0x22 => {
            message.check_fields(&[1, 2, 3, 4, 0x10, 0x11])?;
            let alignment = message.optional_u32(0x11)?;
            if alignment.is_some_and(|value| value > 11) {
                return Err(Error::InvalidValue);
            }
            MemoryRequest::Allocate {
                bytes: bounded_size(message.u32(1)?, HEAP_BUFFER_BYTES)?,
                client: message.u32(2)?,
                processor: Processor::decode(message.u32(3)?)?,
                sequence: message.u32(4)?,
                contiguous: message.optional_u8(0x10)?.map(boolean).transpose()?,
                alignment: alignment.map(|value| value as u8),
            }
        }
        0x23 => {
            message.check_fields(&[1, 0x10, 0x11])?;
            MemoryRequest::Free {
                blocks: HeapBlocks::decode(message.required(1)?)?,
                client: message.optional_u32(0x10)?,
                processor: message
                    .optional_u32(0x11)?
                    .map(Processor::decode)
                    .transpose()?,
            }
        }
        0x24 => {
            message.check_fields(&[1, 0x10])?;
            MemoryRequest::QuerySize {
                client: message.u32(1)?,
                processor: message
                    .optional_u32(0x10)?
                    .map(Processor::decode)
                    .transpose()?,
            }
        }
        _ => return Err(Error::UnsupportedMessage),
    })
}
