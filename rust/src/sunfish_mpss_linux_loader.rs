//! Selected Linux PAS4 loader for the exact private cold-reference modem MBN.
//! Metadata stays allocated and is never inspected after INIT submission;
//! target memory is never accessed after AUTH submission. No legacy consumed-
//! metadata fingerprint, retry/repair, shutdown or modem-ready permit is used.
#![forbid(unsafe_code)]
use crate::{qcom_mdt_packed::Packed, sunfish_lpd::Sha256};
use core::mem::ManuallyDrop;

pub const BASE: u64 = 0x8b000000;
pub const BYTES: usize = 0x09800000;
pub const METADATA_BYTES: usize = 0x3000;
pub const CHUNK: usize = 4096;
/// Opt-in pre-auth target work budget. Metadata remains 4 KiB per step.
/// All scalar accesses, per-access barriers, full readback and terminal failure
/// behavior remain. Lifecycle/domain checks and startup servicing occur less
/// often: up to 64 KiB, rather than 4 KiB, can be touched between those checks.
#[cfg(feature = "sunfish-mpss-target-chunk-64k")]
pub const TARGET_CHUNK: usize = 65536;
#[cfg(not(feature = "sunfish-mpss-target-chunk-64k"))]
pub const TARGET_CHUNK: usize = CHUNK;
pub const FIRMWARE_BYTES: usize = 68_460_672;
pub const FIRMWARE_SHA256: [u8; 32] = [
    0x84, 0x89, 0xe6, 0x82, 0x8d, 0xc1, 0x2e, 0xd9, 0x8e, 0x26, 0xde, 0xc1, 0x5a, 0xdb, 0x71, 0x02,
    0xa3, 0x1a, 0xaf, 0xaf, 0x73, 0x14, 0xe2, 0x49, 0x85, 0x4c, 0x73, 0x7d, 0xdb, 0xa4, 0xcd, 0xc0,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Identity,
    Image,
    Geometry,
    Stage,
    Consumed,
    Memory,
    Guard,
    Acquire,
    Transport,
    Release,
    Monitor,
    Service,
    Unavailable,
    Readback,
}
pub struct Image<'a> {
    packed: Packed<'a>,
    bytes: usize,
}
impl<'a> Image<'a> {
    pub fn private_reference(bytes: &'a [u8]) -> Result<Self, Error> {
        Self::identity(bytes, FIRMWARE_BYTES, FIRMWARE_SHA256, BYTES)
    }
    fn identity(
        data: &'a [u8],
        length: usize,
        digest: [u8; 32],
        bytes: usize,
    ) -> Result<Self, Error> {
        if data.len() != length {
            return Err(Error::Identity);
        }
        let mut h = Sha256::new();
        h.update(data);
        if h.finish() != digest {
            return Err(Error::Identity);
        }
        let packed = Packed::parse(data).map_err(|_| Error::Image)?;
        let image = packed.image();
        let (header, hash) = packed.metadata_parts();
        if image.program_header_count != 31
            || image.loadable_segment_count != 29
            || !image.relocatable
            || u64::from(image.linked_min_address) != BASE
            || image.memory_extent() as usize != bytes
            || header.len() + hash.len() > METADATA_BYTES
            || header.is_empty()
            || hash.is_empty()
        {
            return Err(Error::Geometry);
        }
        Ok(Self { packed, bytes })
    }
    pub fn metadata_length(&self) -> usize {
        let (h, b) = self.packed.metadata_parts();
        h.len() + b.len()
    }
    fn metadata_byte(&self, at: usize) -> u8 {
        let (h, b) = self.packed.metadata_parts();
        if at < h.len() {
            h[at]
        } else {
            b.get(at - h.len()).copied().unwrap_or(0)
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Availability,
    InitImage,
    MemSetup,
    AuthAndReset,
}
/// Closed Sunfish PAS4 envelope. The only varying address is the validated
/// private metadata allocation; firmware identity fixes the target extent.
pub struct Request {
    words: [u64; 8],
    kind: Kind,
}
impl Request {
    pub fn words(&self) -> &[u64; 8] {
        &self.words
    }
    pub fn kind(&self) -> Kind {
        self.kind
    }
    fn query(command: u32) -> Self {
        Self {
            words: [
                0x42000601,
                1,
                0x02000200 | u64::from(command),
                0,
                0,
                0,
                0,
                0,
            ],
            kind: Kind::Availability,
        }
    }
    fn init(pa: u64) -> Self {
        Self {
            words: [0x42000201, 0x82, 4, pa, 0, 0, 0, 0],
            kind: Kind::InitImage,
        }
    }
    fn mem(bytes: usize) -> Self {
        Self {
            words: [0x42000202, 3, 4, BASE, bytes as u64, 0, 0, 0],
            kind: Kind::MemSetup,
        }
    }
    fn auth() -> Self {
        Self {
            words: [0x42000205, 1, 4, 0, 0, 0, 0, 0],
            kind: Kind::AuthAndReset,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Guard {
    pub generation: u64,
    pub metadata_submitted: bool,
    pub auth_submitted: bool,
}
/// Trusted source-matched platform boundary. A mutating operation always
/// acquires/releases SCM clocks + crypto-to-EBI resources. Cold native providers
/// may retain declared nondecreasing floors; they must not invent restoration.
/// invoke performs the source-matched bounded monitor policy, not a retry of
/// this logical request. Uncertain effects must not be reported as success.
pub trait Backend {
    type Lease;
    fn guard(&mut self, state: Guard) -> Result<(), Error>;
    fn acquire(&mut self) -> Result<Self::Lease, Error>;
    fn invoke(&mut self, request: &Request) -> Result<[u64; 8], Error>;
    fn release(&mut self, lease: Self::Lease) -> Result<(), Error>;
}
/// Exclusive coherent/uncached metadata and fixed target mappings. Offsets
/// stay within those two reservations. Every access is bounded, fallible and
/// serialized; no implicit retry, ownership transition or Drop-time unmap.
pub trait Memory {
    fn metadata_physical(&self) -> u64;
    fn metadata_write(&mut self, at: usize, value: u8) -> Result<(), Error>;
    fn metadata_read(&mut self, at: usize) -> Result<u8, Error>;
    fn target_write8(&mut self, at: usize, value: u8) -> Result<(), Error>;
    fn target_read8(&mut self, at: usize) -> Result<u8, Error>;
    fn target_write32(&mut self, at: usize, value: u32) -> Result<(), Error>;
    fn target_read32(&mut self, at: usize) -> Result<u32, Error>;
    fn sync(&mut self) -> Result<(), Error>;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Metadata,
    VerifyMetadata,
    Queries,
    InitImage,
    MemSetup,
    Staging,
    VerifyTarget,
    AuthAndReset,
    AuthReturned,
    Failed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Report {
    pub phase: Phase,
    pub metadata_length: usize,
    pub metadata_writes: usize,
    pub metadata_verified: usize,
    pub target_writes: usize,
    pub target_verified: usize,
    pub metadata_submitted: bool,
    pub auth_submitted: bool,
    /// Bits 0..2: availability checks; 3: INIT; 4: MEM_SETUP; 5: AUTH.
    pub attempted_mask: u32,
    pub returned_mask: u32,
    pub last_request: Option<Kind>,
    pub last_reply: Option<[u64; 8]>,
    pub error: Option<Error>,
}
/// Authentication result only, not an SMP2P-ready, GLINK-write, WLAN/DMA permit.
/// A later platform handoff must bind the actual generation/mappings/owners.
pub struct AuthReturned {
    generation: u64,
}
impl AuthReturned {
    pub fn generation(&self) -> u64 {
        self.generation
    }
}
pub struct Loader<'a, M: Memory> {
    image: Image<'a>,
    memory: ManuallyDrop<M>,
    generation: u64,
    metadata_pa: u64,
    report: Report,
    at: usize,
    segment: usize,
    query: usize,
    busy: bool,
    receipt_taken: bool,
}
impl<'a, M: Memory> Loader<'a, M> {
    /// Invalid geometry returns both owners intact. Retain the returned owners
    /// until the platform can prove quiescence; metadata/target ownership is
    /// an independent native prerequisite, not supplied by parsing this image.
    pub fn new(
        generation: u64,
        image: Image<'a>,
        memory: M,
    ) -> Result<Self, (Image<'a>, M, Error)> {
        let pa = memory.metadata_physical();
        if generation == 0
            || pa % 4096 != 0
            || pa < 0x80000000
            || pa
                .checked_add(METADATA_BYTES as u64)
                .is_none_or(|e| e > 1 << 40)
            || (pa < BASE + BYTES as u64 && BASE < pa + METADATA_BYTES as u64)
        {
            return Err((image, memory, Error::Geometry));
        }
        let length = image.metadata_length();
        Ok(Self {
            image,
            memory: ManuallyDrop::new(memory),
            generation,
            metadata_pa: pa,
            report: Report {
                phase: Phase::Metadata,
                metadata_length: length,
                metadata_writes: 0,
                metadata_verified: 0,
                target_writes: 0,
                target_verified: 0,
                metadata_submitted: false,
                auth_submitted: false,
                attempted_mask: 0,
                returned_mask: 0,
                last_request: None,
                last_reply: None,
                error: None,
            },
            at: 0,
            segment: 0,
            query: 0,
            busy: false,
            receipt_taken: false,
        })
    }
    pub fn report(&self) -> Report {
        let mut r = self.report;
        if self.busy && r.error.is_none() {
            r.phase = Phase::Failed;
            r.error = Some(Error::Consumed);
        }
        r
    }
    fn fail(&mut self, e: Error) -> Error {
        self.report.phase = Phase::Failed;
        self.report.error.get_or_insert(e);
        e
    }
    fn guard(&self) -> Guard {
        Guard {
            generation: self.generation,
            metadata_submitted: self.report.metadata_submitted,
            auth_submitted: self.report.auth_submitted,
        }
    }
    fn call(&mut self, b: &mut impl Backend, r: Request) -> Result<(), Error> {
        let bit = match r.kind {
            Kind::Availability => self.query,
            Kind::InitImage => 3,
            Kind::MemSetup => 4,
            Kind::AuthAndReset => 5,
        };
        self.report.attempted_mask |= 1 << bit;
        self.report.last_request = Some(r.kind);
        self.report.last_reply = None;
        let result = if r.kind == Kind::Availability {
            b.invoke(&r).map_err(|_| Error::Transport)
        } else {
            let lease = b.acquire().map_err(|_| Error::Acquire)?;
            let result = b
                .guard(self.guard())
                .map_err(|_| Error::Guard)
                .and_then(|_| b.invoke(&r).map_err(|_| Error::Transport));
            if let Ok(reply) = result {
                self.report.last_reply = Some(reply);
                self.report.returned_mask |= 1 << bit;
            }
            let release = b.release(lease);
            if release.is_err() {
                return Err(Error::Release);
            }
            result
        };
        let reply = result?;
        self.report.last_reply = Some(reply);
        self.report.returned_mask |= 1 << bit;
        if reply[0] != 0 {
            return Err(Error::Monitor);
        }
        if reply[1] != u64::from(r.kind == Kind::Availability) {
            return Err(if r.kind == Kind::Availability {
                Error::Unavailable
            } else {
                Error::Service
            });
        }
        Ok(())
    }
    /// One <=4-KiB memory chunk or one logical secure transaction. No target
    /// effect occurs before successful MEM_SETUP. No metadata/target access
    /// occurs after the corresponding INIT/AUTH submission, even on errors.
    pub fn step(&mut self, b: &mut impl Backend) -> Result<Phase, Error> {
        if self.busy
            || self.report.error.is_some()
            || matches!(self.report.phase, Phase::AuthReturned | Phase::Failed)
        {
            return Err(self.fail(Error::Consumed));
        }
        self.busy = true;
        let result = (|| {
            b.guard(self.guard()).map_err(|_| Error::Guard)?;
            match self.report.phase {
                Phase::Metadata | Phase::VerifyMetadata => {
                    let end = (self.at + CHUNK).min(METADATA_BYTES);
                    for i in self.at..end {
                        let expected = self.image.metadata_byte(i);
                        if self.report.phase == Phase::Metadata {
                            self.report.metadata_writes += 1;
                            self.memory.metadata_write(i, expected)?;
                        } else {
                            if self.memory.metadata_read(i)? != expected {
                                return Err(Error::Readback);
                            }
                            self.report.metadata_verified += 1;
                        }
                    }
                    self.memory.sync()?;
                    self.at = end;
                    if end == METADATA_BYTES {
                        self.at = 0;
                        self.report.phase = if self.report.phase == Phase::Metadata {
                            Phase::VerifyMetadata
                        } else {
                            Phase::Queries
                        };
                    }
                }
                Phase::Queries => {
                    self.call(b, Request::query([1, 2, 5][self.query]))?;
                    self.query += 1;
                    if self.query == 3 {
                        self.report.phase = Phase::InitImage;
                    }
                }
                Phase::InitImage => {
                    self.report.metadata_submitted = true;
                    self.call(b, Request::init(self.metadata_pa))?;
                    self.report.phase = Phase::MemSetup;
                }
                Phase::MemSetup => {
                    self.call(b, Request::mem(self.image.bytes))?;
                    self.report.phase = Phase::Staging;
                }
                Phase::Staging | Phase::VerifyTarget => {
                    let s = self
                        .image
                        .packed
                        .segments()
                        .nth(self.segment)
                        .ok_or(Error::Stage)?;
                    let base = s.descriptor.linked_address as usize - BASE as usize;
                    let end = (self.at + TARGET_CHUNK).min(s.descriptor.memory_size as usize);
                    let write = self.report.phase == Phase::Staging;
                    let mut at = self.at;
                    while at < end {
                        let offset = base + at;
                        let n = if offset % 4 == 0 && end - at >= 4 {
                            4
                        } else {
                            1
                        };
                        let mut value = [0; 4];
                        for (i, v) in value[..n].iter_mut().enumerate() {
                            *v = s.bytes.get(at + i).copied().unwrap_or(0);
                        }
                        if write {
                            self.report.target_writes += n;
                            if n == 4 {
                                self.memory
                                    .target_write32(offset, u32::from_le_bytes(value))?;
                            } else {
                                self.memory.target_write8(offset, value[0])?;
                            }
                        } else {
                            let actual = if n == 4 {
                                self.memory.target_read32(offset)?.to_le_bytes()
                            } else {
                                [self.memory.target_read8(offset)?, 0, 0, 0]
                            };
                            if actual[..n] != value[..n] {
                                return Err(Error::Readback);
                            }
                            self.report.target_verified += n;
                        }
                        at += n;
                    }
                    self.memory.sync()?;
                    self.at = end;
                    if end == s.descriptor.memory_size as usize {
                        self.segment += 1;
                        self.at = 0;
                        if self.segment == 29 {
                            self.segment = 0;
                            self.report.phase = if write {
                                Phase::VerifyTarget
                            } else {
                                Phase::AuthAndReset
                            };
                        }
                    }
                }
                Phase::AuthAndReset => {
                    self.report.auth_submitted = true;
                    self.call(b, Request::auth())?;
                    self.report.phase = Phase::AuthReturned;
                }
                Phase::AuthReturned | Phase::Failed => return Err(Error::Stage),
            }
            b.guard(self.guard()).map_err(|_| Error::Guard)
        })();
        match result {
            Ok(()) => {
                self.busy = false;
                Ok(self.report.phase)
            }
            Err(e) => Err(self.fail(e)),
        }
    }
    pub fn take_auth_returned(&mut self) -> Result<AuthReturned, Error> {
        if self.busy
            || self.receipt_taken
            || self.report.error.is_some()
            || self.report.phase != Phase::AuthReturned
        {
            return Err(self.fail(Error::Consumed));
        }
        self.receipt_taken = true;
        Ok(AuthReturned {
            generation: self.generation,
        })
    }
}

#[cfg(test)]
#[path = "../../tests/sunfish_mpss_linux_loader_cases.rs"]
pub(crate) mod tests;
