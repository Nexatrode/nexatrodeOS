//! Bounded host-service actions. No native block/SCM backend is supplied here.
//! Wire fields: pinned rmtfs qmi_rmtfs.qmi and heap_mem_ext_v01.{h,c}; see
//! qcom_modem_services. Backend is a trusted, serialized ownership boundary.
#![forbid(unsafe_code)]

use crate::qcom_glink_qmi::{Reply, RequestHandle};
use crate::qcom_modem_services::{
    Direction, HEAP_BUFFER_BYTES, MAX_IOVECS, MemoryRequest, Partition, Processor, Request,
    STORAGE_BUFFER_BYTES, Service, StorageRequest,
};
use crate::qcom_qmi_router::{Address, Context};
use core::sync::atomic::{AtomicU64, Ordering};
static NEXT_HANDLERS: AtomicU64 = AtomicU64::new(1);

pub const IO_BYTES: usize = 4096;
pub const HANDLE_SLOTS: usize = 8;
pub const MAX_EXCLUSIONS: usize = 16;
const PAGE: u64 = 4096;
const RMT_INTERNAL: u16 = 1;
const MEM_INTERNAL: u16 = 3;
const MEM_INVALID_ID: u16 = 41;
const MEM_NOT_SUPPORTED: u16 = 94;
const SERVREG_INTERNAL: u16 = 1;
const IPA_INTERNAL: u16 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Fault {
    Failed,
    Uncertain,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Configuration,
    Stopped,
    Generation,
    Identity,
    Ownership,
    BackendUncertain,
    Busy,
    InvalidRequest,
    Invariant,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pool {
    Storage,
    Heap,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ownership {
    Host,
    SharedStorage,
    ModemExclusive,
    Unknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Region {
    pub base: u64,
    pub bytes: u32,
}
impl Region {
    fn end(self) -> Option<u64> {
        self.base.checked_add(u64::from(self.bytes))
    }
    fn overlaps(self, other: Self) -> bool {
        self.base < other.end().unwrap() && other.base < self.end().unwrap()
    }
    fn guarded(self) -> Option<Self> {
        Some(Self {
            base: self.base.checked_sub(PAGE)?,
            bytes: self.bytes.checked_add(2 * PAGE as u32)?,
        })
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Disk {
    pub bytes: u64,
    pub writable: bool,
    pub durable: bool,
}
/// Persistence promised to the modem by this explicitly selected runtime.
/// RAM shadow mode acknowledges writes/sync only within the retained boot;
/// it never claims that changed EFS survives restart.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Persistence {
    Durable,
    RamShadow,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Configuration {
    pub generation: u64,
    pub local_node: u32,
    pub peer_node: u32,
    pub storage_port: u32,
    pub memory_port: u32,
    pub storage: Region,
    pub heap: Region,
    /// modem_fs1, modem_fs2, modem_fsg, modem_fsc, in that order.
    pub disks: [Disk; 4],
}
#[derive(Clone, Copy)]
pub struct Profile(Configuration, Persistence);
impl Profile {
    /// Checks arithmetic, page guards, disjoint reservations and exclusions.
    /// This does not prove physical ownership or create any native mapping.
    pub fn validate(c: Configuration, excluded: &[Region]) -> Result<Self, Error> {
        Self::validate_mode(c, excluded, Persistence::Durable)
    }

    /// Select the Linux `rmtfs -r` comparison semantics explicitly. Every
    /// backing disk must be writable RAM with `durable == false`, and the
    /// retained backend must report the same mode before any request runs.
    pub fn validate_ram_shadow(c: Configuration, excluded: &[Region]) -> Result<Self, Error> {
        Self::validate_mode(c, excluded, Persistence::RamShadow)
    }

    fn validate_mode(c: Configuration, excluded: &[Region], mode: Persistence) -> Result<Self, Error> {
        let port = |p| p != 0 && p < 0xffff_fffe;
        if c.generation == 0
            || c.local_node >= 0xffff_fffe
            || c.peer_node >= 0xffff_fffe
            || c.local_node == c.peer_node
            || !port(c.storage_port)
            || !port(c.memory_port)
            || c.storage_port == c.memory_port
            || excluded.len() > MAX_EXCLUSIONS
            || c.storage.bytes == 0
            || c.storage.bytes > STORAGE_BUFFER_BYTES
            || c.heap.bytes == 0
            || c.heap.bytes > HEAP_BUFFER_BYTES
            || c.disks
                .iter()
                .any(|d| d.bytes == 0 || d.bytes % 512 != 0 || match mode {
                    Persistence::Durable => d.writable && !d.durable,
                    Persistence::RamShadow => !d.writable || d.durable,
                })
        {
            return Err(Error::Configuration);
        }
        for r in [c.storage, c.heap] {
            if r.base % PAGE != 0
                || u64::from(r.bytes) % PAGE != 0
                || r.end().is_none()
                || r.guarded().and_then(Region::end).is_none()
            {
                return Err(Error::Configuration);
            }
        }
        if c.storage.end().unwrap() > 1u64 << 32
            || c.storage
                .guarded()
                .unwrap()
                .overlaps(c.heap.guarded().unwrap())
        {
            return Err(Error::Configuration);
        }
        for r in excluded {
            if r.bytes == 0
                || r.end().is_none()
                || r.overlaps(c.storage.guarded().unwrap())
                || r.overlaps(c.heap.guarded().unwrap())
            {
                return Err(Error::Configuration);
            }
        }
        Ok(Self(c, mode))
    }
    pub fn configuration(self) -> Configuration {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Observation {
    pub generation: u64,
    pub identity: u64,
    pub ready: bool,
    pub storage: Ownership,
    pub heap: Ownership,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Grant {
    pub serial: u64,
    pub pool: Pool,
    pub region: Region,
    pub generation: u64,
    pub peer: Address,
    pub ownership: Ownership,
}

/// All methods must be bounded and serialized with native lifecycle changes.
/// Failed means THIS call had no effect; prior successful chunks may persist.
/// Uncertain means no retry/release is safe. observe/info must read actual state.
/// Observation identity covers all pool mappings and partition bindings; a
/// rebind, resize or permission change invalidates identity/readiness.
/// Grant/reclaim success confirms ownership, ordering and cache maintenance,
/// not just a submitted SCM request. disk_write/sync must implement the
/// selected persistence contract; only an explicit RAM-shadow profile may
/// acknowledge volatile writes without claiming nonvolatile durability.
/// pool_read returns one stable private copy. Receipts have no effect on Drop.
/// Backend, reservations and mappings remain retained through every fault;
/// there is no native implementation or implicit cleanup in this module.
pub trait Backend {
    type Receipt;
    fn persistence(&self) -> Persistence {
        Persistence::Durable
    }
    fn observe(&mut self) -> Result<Observation, Fault>;
    fn disk(&mut self, partition: Partition) -> Result<Disk, Fault>;
    fn zero(&mut self, pool: Pool, offset: u32, bytes: usize) -> Result<(), Fault>;
    fn grant(&mut self, grant: Grant) -> Result<Self::Receipt, Fault>;
    fn info(&mut self, receipt: &Self::Receipt) -> Result<Grant, Fault>;
    fn reclaim(&mut self, receipt: &Self::Receipt) -> Result<(), Fault>;
    fn disk_read(&mut self, partition: Partition, offset: u64, out: &mut [u8])
    -> Result<(), Fault>;
    fn disk_write(&mut self, partition: Partition, offset: u64, bytes: &[u8]) -> Result<(), Fault>;
    fn sync(&mut self, partition: Partition) -> Result<(), Fault>;
    fn pool_read(
        &mut self,
        receipt: &Self::Receipt,
        offset: u32,
        out: &mut [u8],
    ) -> Result<(), Fault>;
    fn pool_write(
        &mut self,
        receipt: &Self::Receipt,
        offset: u32,
        bytes: &[u8],
    ) -> Result<(), Fault>;
}

#[derive(Clone, Copy)]
struct Handle {
    id: u32,
    peer: Address,
    partition: Partition,
    error: bool,
}
struct Lease<R> {
    receipt: R,
    grant: Grant,
    requested: u32,
    sequence: u32,
}
#[derive(Clone, Copy)]
struct Span {
    disk: u64,
    pool: u32,
    bytes: u32,
}
const EMPTY_SPAN: Span = Span {
    disk: 0,
    pool: 0,
    bytes: 0,
};
#[derive(Clone, Copy)]
enum Action {
    Open(Partition),
    Close(usize),
    DeviceError(usize),
    Allocate {
        pool: Pool,
        handle: usize,
        at: u32,
        bytes: u32,
        sequence: u32,
        zeroed: u32,
    },
    FreeHeap,
    Transfer {
        handle: usize,
        direction: Direction,
        count: usize,
        index: usize,
        offset: u32,
        staged: bool,
        sync: bool,
    },
    Done,
}
#[derive(Clone)]
pub struct Response {
    error: Option<u16>,
    bytes: [u8; 32],
    len: usize,
}
impl Response {
    fn success() -> Self {
        Self {
            error: None,
            bytes: [0; 32],
            len: 0,
        }
    }
    fn failure(service: Service) -> Self {
        Self {
            error: Some(match service {
                Service::RemoteStorage => RMT_INTERNAL,
                Service::HeapMemory => MEM_INTERNAL,
                Service::ServiceRegistry => SERVREG_INTERNAL,
                Service::IpaServer => IPA_INTERNAL,
            }),
            ..Self::success()
        }
    }
    fn field(&mut self, kind: u8, value: &[u8]) {
        // Only fixed, locally constructed fields (at most 23 bytes total).
        let at = self.len;
        self.bytes[at] = kind;
        self.bytes[at + 1..at + 3].copy_from_slice(&(value.len() as u16).to_le_bytes());
        self.bytes[at + 3..at + 3 + value.len()].copy_from_slice(value);
        self.len += 3 + value.len();
    }
    pub fn reply(&self) -> Reply<'_> {
        match self.error {
            Some(error) => Reply::Failure { error },
            None => Reply::Success {
                optional_tlvs: &self.bytes[..self.len],
            },
        }
    }
    pub fn error(&self) -> Option<u16> {
        self.error
    }
    pub fn fields(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}
struct Job {
    handle: RequestHandle,
    context: Context,
    action: Action,
    response: Option<Response>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Progress {
    Idle,
    Progress,
    ReplyReady,
}

/// One action at a time, eight handles, one shared storage lease and one heap
/// lease. Other requests remain held in the owner's four-slot request queue.
/// Every step performs at most one action IO of at most 4096 bytes, in addition
/// to bounded observation/receipt checks. No peer address becomes a pointer.
pub struct Handlers<B: Backend> {
    id: u64,
    backend: B,
    profile: Configuration,
    persistence: Persistence,
    identity: u64,
    stopped: bool,
    error: Option<Error>,
    handles: [Option<Handle>; HANDLE_SLOTS],
    next_handle: u32,
    storage: Option<Lease<B::Receipt>>,
    heap: Option<Lease<B::Receipt>>,
    heap_owner: Option<Address>,
    last_sequence: Option<u32>,
    heap_next: u32,
    next_grant: u64,
    spans: [Span; MAX_IOVECS],
    buffer: [u8; IO_BYTES],
    job: Option<Job>,
}
fn partition_index(partition: Partition) -> usize {
    match partition {
        Partition::ModemFs1 => 0,
        Partition::ModemFs2 => 1,
        Partition::ModemFsg => 2,
        Partition::ModemFsc => 3,
    }
}
fn partitions() -> [Partition; 4] {
    [
        Partition::ModemFs1,
        Partition::ModemFs2,
        Partition::ModemFsg,
        Partition::ModemFsc,
    ]
}
fn rounded(bytes: u32) -> Option<u32> {
    bytes
        .checked_add(PAGE as u32 - 1)
        .map(|n| n & !(PAGE as u32 - 1))
}

impl<B: Backend> Handlers<B> {
    /// Always retains the backend, including failed initialization. Inspect
    /// error()/validate() before advertising. No retry or reset API is exposed.
    pub fn attach(profile: Profile, backend: B) -> Self {
        let mut this = Self {
            id: NEXT_HANDLERS
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .unwrap_or(0),
            backend,
            profile: profile.0,
            persistence: profile.1,
            identity: 0,
            stopped: false,
            error: None,
            handles: [None; HANDLE_SLOTS],
            next_handle: 1,
            storage: None,
            heap: None,
            heap_owner: None,
            last_sequence: None,
            heap_next: 0,
            next_grant: 1,
            spans: [EMPTY_SPAN; MAX_IOVECS],
            buffer: [0; IO_BYTES],
            job: None,
        };
        let result = (|| {
            if this.id == 0 {
                return Err(Error::Invariant);
            }
            if this.backend.persistence() != this.persistence {
                return Err(Error::Configuration);
            }
            let initial = this.backend.observe().map_err(|_| Error::Ownership)?;
            if initial.identity == 0 {
                return Err(Error::Identity);
            }
            this.identity = initial.identity;
            this.gate()?;
            for partition in partitions() {
                if this.backend.disk(partition).map_err(|_| Error::Ownership)?
                    != this.profile.disks[partition_index(partition)]
                {
                    return Err(Error::Configuration);
                }
            }
            this.gate()
        })();
        if let Err(error) = result {
            this.stop(error);
        }
        this
    }
    fn stop(&mut self, error: Error) -> Error {
        self.stopped = true;
        self.error = Some(error);
        error
    }
    pub fn fault(&mut self) {
        self.stop(Error::Stopped);
    }
    pub fn error(&self) -> Option<Error> {
        self.error
    }
    pub fn generation(&self) -> u64 {
        self.profile.generation
    }
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn configuration(&self) -> Configuration {
        self.profile
    }
    pub fn active_handle(&self) -> Option<RequestHandle> {
        self.job.as_ref().map(|j| j.handle)
    }

    /// Whether this handler set owns a routed service namespace. Other
    /// endpoints (for example SERVREG LOC and AP IPA) share the owner request
    /// queue and must remain available to their dedicated dispatchers.
    pub fn handles_service(&self, service: Service) -> bool {
        matches!(service, Service::RemoteStorage | Service::HeapMemory)
    }
    pub fn open_handles(&self) -> usize {
        self.handles.iter().flatten().count()
    }
    pub fn leased(&self, pool: Pool) -> bool {
        match pool {
            Pool::Storage => self.storage.is_some(),
            Pool::Heap => self.heap.is_some(),
        }
    }
    fn gate_expected(&mut self, storage: Ownership, heap: Ownership) -> Result<(), Error> {
        if self.backend.persistence() != self.persistence {
            return Err(Error::Configuration);
        }
        let state = self.backend.observe().map_err(|_| Error::Ownership)?;
        if state.generation != self.profile.generation {
            return Err(Error::Generation);
        }
        if state.identity != self.identity || !state.ready {
            return Err(Error::Identity);
        }
        if state.storage != storage || state.heap != heap {
            return Err(Error::Ownership);
        }
        Ok(())
    }
    fn gate(&mut self) -> Result<(), Error> {
        self.gate_expected(
            if self.storage.is_some() {
                Ownership::SharedStorage
            } else {
                Ownership::Host
            },
            if self.heap.is_some() {
                Ownership::ModemExclusive
            } else {
                Ownership::Host
            },
        )?;
        for lease in [&self.storage, &self.heap].into_iter().flatten() {
            if self
                .backend
                .info(&lease.receipt)
                .map_err(|_| Error::Ownership)?
                != lease.grant
            {
                return Err(Error::Ownership);
            }
        }
        Ok(())
    }
    pub fn validate(&mut self) -> Result<(), Error> {
        if self.stopped {
            return Err(Error::Stopped);
        }
        self.stopped = true;
        match self.gate() {
            Ok(()) => {
                self.stopped = false;
                Ok(())
            }
            Err(e) => Err(self.stop(e)),
        }
    }
    fn handle_index(&self, id: u32, peer: Address) -> Option<usize> {
        self.handles
            .iter()
            .position(|h| h.is_some_and(|h| h.id == id && h.peer == peer))
    }
    fn compatible(&self, context: Context, request: Request<'_>) -> bool {
        let message = match request {
            Request::Storage(StorageRequest::Open(_)) => 1,
            Request::Storage(StorageRequest::Close { .. }) => 2,
            Request::Storage(StorageRequest::ReadWrite { .. }) => 3,
            Request::Storage(StorageRequest::Allocate { .. }) => 4,
            Request::Storage(StorageRequest::DeviceError { .. }) => 5,
            Request::Memory(MemoryRequest::Allocate { .. }) => 0x22,
            Request::Memory(MemoryRequest::Free { .. }) => 0x23,
            Request::Memory(MemoryRequest::QuerySize { .. }) => 0x24,
            Request::ServiceRegistry(_) => return false,
            Request::IpaServer { .. } => return false,
        };
        let service = match request {
            Request::Storage(_) => Service::RemoteStorage,
            Request::Memory(_) => Service::HeapMemory,
            Request::ServiceRegistry(_) => return false,
            Request::IpaServer { .. } => return false,
        };
        context.generation == self.profile.generation
            && context.service == service
            && context.message_id == message
            && context.peer.node == self.profile.peer_node
            && context.peer.port != 0
            && context.peer.port < 0xffff_fffe
            && context.local.node == self.profile.local_node
            && context.local.port
                == if service == Service::RemoteStorage {
                    self.profile.storage_port
                } else {
                    self.profile.memory_port
                }
    }
    pub fn begin(
        &mut self,
        handle: RequestHandle,
        context: Context,
        request: Request<'_>,
    ) -> Result<(), Error> {
        self.validate()?;
        if self.job.is_some() {
            return Err(Error::Busy);
        }
        if !self.compatible(context, request) {
            return Err(self.stop(Error::InvalidRequest));
        }
        self.spans.fill(EMPTY_SPAN);
        let mut response = None;
        let action = self.plan(context, request, &mut response);
        self.job = Some(Job {
            handle,
            context,
            action,
            response,
        });
        Ok(())
    }
    fn plan(
        &mut self,
        context: Context,
        request: Request<'_>,
        reply: &mut Option<Response>,
    ) -> Action {
        let fail = |reply: &mut Option<Response>| {
            *reply = Some(Response::failure(context.service));
            Action::Done
        };
        let peer = context.peer;
        match request {
            Request::ServiceRegistry(_) | Request::IpaServer { .. } => fail(reply),
            Request::Storage(StorageRequest::Open(partition)) => {
                if self.handles.iter().all(Option::is_some) || self.next_handle == u32::MAX {
                    return fail(reply);
                }
                Action::Open(partition)
            }
            Request::Storage(request) => {
                let id = match request {
                    StorageRequest::Close { handle }
                    | StorageRequest::DeviceError { handle }
                    | StorageRequest::Allocate { handle, .. }
                    | StorageRequest::ReadWrite { handle, .. } => handle,
                    _ => unreachable!(),
                };
                let Some(index) = self.handle_index(id, peer) else {
                    return fail(reply);
                };
                let h = self.handles[index].unwrap();
                match request {
                    StorageRequest::Close { .. } => Action::Close(index),
                    StorageRequest::DeviceError { .. } => Action::DeviceError(index),
                    StorageRequest::Allocate { bytes, .. } => {
                        if bytes == 0 || bytes > self.profile.storage.bytes {
                            return fail(reply);
                        }
                        if let Some(lease) = &self.storage {
                            if lease.grant.peer != peer || bytes > lease.requested {
                                return fail(reply);
                            }
                            let mut result = Response::success();
                            result.field(0x10, &lease.grant.region.base.to_le_bytes());
                            *reply = Some(result);
                            Action::Done
                        } else {
                            Action::Allocate {
                                pool: Pool::Storage,
                                handle: index,
                                at: 0,
                                bytes,
                                sequence: 0,
                                zeroed: 0,
                            }
                        }
                    }
                    StorageRequest::ReadWrite {
                        direction,
                        iovecs,
                        force_sync,
                        ..
                    } => {
                        let disk = self.profile.disks[partition_index(h.partition)];
                        if (direction == Direction::Write && !disk.writable)
                            || (force_sync && !disk.durable
                                && self.persistence != Persistence::RamShadow)
                        {
                            return fail(reply);
                        }
                        let Some(lease) = &self.storage else {
                            return fail(reply);
                        };
                        if lease.grant.peer != peer {
                            return fail(reply);
                        }
                        let mut total = 0u64;
                        for (i, entry) in iovecs.iter().enumerate() {
                            let length = entry.byte_count();
                            let disk_at = entry.disk_offset();
                            let Some(pool_at) = u64::from(entry.physical_address)
                                .checked_sub(lease.grant.region.base)
                            else {
                                return fail(reply);
                            };
                            total += length;
                            if length == 0
                                || total > u64::from(STORAGE_BUFFER_BYTES)
                                || pool_at
                                    .checked_add(length)
                                    .is_none_or(|end| end > u64::from(lease.requested))
                                || disk_at
                                    .checked_add(length)
                                    .is_none_or(|end| end > disk.bytes)
                            {
                                return fail(reply);
                            }
                            for earlier in &self.spans[..i] {
                                if (pool_at < u64::from(earlier.pool) + u64::from(earlier.bytes)
                                    && u64::from(earlier.pool) < pool_at + length)
                                    || (disk_at < earlier.disk + u64::from(earlier.bytes)
                                        && earlier.disk < disk_at + length)
                                {
                                    return fail(reply);
                                }
                            }
                            self.spans[i] = Span {
                                disk: disk_at,
                                pool: pool_at as u32,
                                bytes: length as u32,
                            };
                        }
                        Action::Transfer {
                            handle: index,
                            direction,
                            count: iovecs.len(),
                            index: 0,
                            offset: 0,
                            staged: false,
                            sync: force_sync || direction == Direction::Write,
                        }
                    }
                    _ => unreachable!(),
                }
            }
            Request::Memory(request) => {
                let authorized = |client, processor: Option<Processor>| {
                    client == 1 && processor.is_none_or(|p| p == Processor::Modem)
                };
                match request {
                    MemoryRequest::QuerySize { client, processor } => {
                        let mut result = Response::success();
                        if authorized(client, processor) {
                            result.field(0x10, &self.profile.heap.bytes.to_le_bytes());
                        } else {
                            result.error = Some(MEM_INVALID_ID);
                        }
                        *reply = Some(result);
                        Action::Done
                    }
                    MemoryRequest::Allocate {
                        bytes,
                        client,
                        processor,
                        sequence,
                        contiguous,
                        alignment,
                    } => {
                        if !authorized(client, Some(processor)) {
                            let mut r = Response::failure(Service::HeapMemory);
                            r.error = Some(MEM_INVALID_ID);
                            *reply = Some(r);
                            return Action::Done;
                        }
                        if contiguous == Some(false) {
                            let mut r = Response::failure(Service::HeapMemory);
                            r.error = Some(MEM_NOT_SUPPORTED);
                            *reply = Some(r);
                            return Action::Done;
                        }
                        if bytes == 0
                            || bytes > self.profile.heap.bytes
                            || alignment.is_some_and(|a| a > 11)
                            || self.heap_owner.is_some_and(|owner| owner != peer)
                        {
                            return fail(reply);
                        }
                        if let Some(lease) = &self.heap {
                            if lease.grant.peer == peer
                                && lease.sequence == sequence
                                && lease.requested == bytes
                            {
                                *reply = Some(Self::heap_response(lease));
                                return Action::Done;
                            }
                            return fail(reply);
                        }
                        if self
                            .last_sequence
                            .is_some_and(|previous| sequence <= previous)
                        {
                            return fail(reply);
                        }
                        if rounded(bytes)
                            .and_then(|n| self.heap_next.checked_add(n))
                            .is_none_or(|end| end > self.profile.heap.bytes)
                        {
                            let mut result = Response::failure(Service::HeapMemory);
                            result.error = Some(2);
                            *reply = Some(result);
                            return Action::Done;
                        }
                        Action::Allocate {
                            pool: Pool::Heap,
                            handle: 0,
                            at: self.heap_next,
                            bytes,
                            sequence,
                            zeroed: 0,
                        }
                    }
                    MemoryRequest::Free {
                        blocks,
                        client,
                        processor,
                    } => {
                        let Some(lease) = &self.heap else {
                            return fail(reply);
                        };
                        if client.is_some_and(|c| c != 1)
                            || processor.is_some_and(|p| p != Processor::Modem)
                            || blocks.len() != 1
                            || lease.grant.peer != peer
                        {
                            return fail(reply);
                        }
                        let block = blocks.iter().next().unwrap();
                        if block.physical_address != lease.grant.region.base
                            || block.bytes != lease.requested
                        {
                            return fail(reply);
                        }
                        Action::FreeHeap
                    }
                }
            }
        }
    }
    fn heap_response(lease: &Lease<B::Receipt>) -> Response {
        let mut response = Response::success();
        response.field(0x10, &lease.sequence.to_le_bytes());
        let mut block = [0u8; 13];
        block[0] = 1;
        block[1..9].copy_from_slice(&lease.grant.region.base.to_le_bytes());
        block[9..].copy_from_slice(&lease.requested.to_le_bytes());
        response.field(0x11, &block);
        response
    }
    pub fn response(&self) -> Option<&Response> {
        self.job.as_ref().and_then(|j| j.response.as_ref())
    }
    /// Called only after the owner successfully copied the cached reply into
    /// its TX queue. Backpressure must leave the job and all effects retained.
    pub fn reply_queued(&mut self, handle: &RequestHandle) -> Result<(), Error> {
        if self.stopped {
            return Err(Error::Stopped);
        }
        if !self
            .job
            .as_ref()
            .is_some_and(|j| j.handle == *handle && j.response.is_some())
        {
            return Err(self.stop(Error::Invariant));
        }
        self.job = None;
        Ok(())
    }
    pub fn step(&mut self) -> Result<Progress, Error> {
        self.validate()?;
        let Some(job) = self.job.as_ref() else {
            return Ok(Progress::Idle);
        };
        if job.response.is_some() {
            return Ok(Progress::ReplyReady);
        }
        let action = job.action;
        let context = job.context;
        self.stopped = true; // Unwinding action IO never permits later retry.
        let result = self.perform(action, context);
        match result {
            Ok(Some(response)) => {
                self.job.as_mut().unwrap().response = Some(response);
                self.stopped = false;
                Ok(Progress::ReplyReady)
            }
            Ok(None) => {
                self.stopped = false;
                Ok(Progress::Progress)
            }
            Err(error) => Err(self.stop(error)),
        }
    }
    fn failure(
        &mut self,
        context: Context,
        fault: Fault,
        handle: Option<usize>,
    ) -> Result<Option<Response>, Error> {
        if fault == Fault::Uncertain {
            return Err(Error::BackendUncertain);
        }
        self.gate()?;
        if let Some(index) = handle {
            self.handles[index].as_mut().unwrap().error = true;
        }
        Ok(Some(Response::failure(context.service)))
    }
    fn perform(&mut self, action: Action, context: Context) -> Result<Option<Response>, Error> {
        match action {
            Action::Open(partition) => {
                let index = self
                    .handles
                    .iter()
                    .position(Option::is_none)
                    .ok_or(Error::Invariant)?;
                let id = self.next_handle;
                self.next_handle = self.next_handle.checked_add(1).ok_or(Error::Invariant)?;
                self.handles[index] = Some(Handle {
                    id,
                    peer: context.peer,
                    partition,
                    error: false,
                });
                let mut response = Response::success();
                response.field(0x10, &id.to_le_bytes());
                Ok(Some(response))
            }
            Action::DeviceError(index) => {
                let mut r = Response::success();
                r.field(0x10, &[u8::from(self.handles[index].unwrap().error)]);
                Ok(Some(r))
            }
            Action::Close(index) => {
                let peer = self.handles[index].unwrap().peer;
                if self
                    .storage
                    .as_ref()
                    .is_some_and(|lease| lease.grant.peer == peer)
                    && self
                        .handles
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| *i != index)
                        .all(|(_, h)| h.is_none_or(|h| h.peer != peer))
                {
                    if let Err(fault) = self
                        .backend
                        .reclaim(&self.storage.as_ref().ok_or(Error::Invariant)?.receipt)
                    {
                        return self.failure(context, fault, Some(index));
                    }
                    self.gate_expected(
                        Ownership::Host,
                        if self.heap.is_some() {
                            Ownership::ModemExclusive
                        } else {
                            Ownership::Host
                        },
                    )?;
                    self.storage = None;
                    self.gate()?;
                }
                self.handles[index] = None;
                Ok(Some(Response::success()))
            }
            Action::Allocate {
                pool,
                handle,
                at,
                bytes,
                sequence,
                zeroed,
            } => {
                let amount = rounded(bytes).ok_or(Error::Invariant)?;
                if zeroed < amount {
                    let count = (amount - zeroed).min(IO_BYTES as u32) as usize;
                    if let Err(fault) = self.backend.zero(pool, at + zeroed, count) {
                        return self.failure(context, fault, None);
                    }
                    self.gate()?;
                    self.job.as_mut().unwrap().action = Action::Allocate {
                        pool,
                        handle,
                        at,
                        bytes,
                        sequence,
                        zeroed: zeroed + count as u32,
                    };
                    return Ok(None);
                }
                let region = Region {
                    base: u64::from(at)
                        + if pool == Pool::Storage {
                            self.profile.storage.base
                        } else {
                            self.profile.heap.base
                        },
                    bytes: amount,
                };
                let grant = Grant {
                    serial: self.next_grant,
                    pool,
                    region,
                    generation: self.profile.generation,
                    peer: context.peer,
                    ownership: if pool == Pool::Storage {
                        Ownership::SharedStorage
                    } else {
                        Ownership::ModemExclusive
                    },
                };
                self.next_grant = self.next_grant.checked_add(1).ok_or(Error::Invariant)?;
                let receipt = match self.backend.grant(grant) {
                    Ok(r) => r,
                    Err(fault) => return self.failure(context, fault, None),
                };
                let lease = Lease {
                    receipt,
                    grant,
                    requested: bytes,
                    sequence,
                };
                match pool {
                    Pool::Storage => self.storage = Some(lease),
                    Pool::Heap => self.heap = Some(lease),
                }
                self.gate()?; // Receipt is retained even if the backend lies or fails.
                match pool {
                    Pool::Storage => {
                        let mut response = Response::success();
                        response.field(0x10, &region.base.to_le_bytes());
                        Ok(Some(response))
                    }
                    Pool::Heap => {
                        self.heap_next = at + amount;
                        self.heap_owner = Some(context.peer);
                        self.last_sequence = Some(sequence);
                        Ok(Some(Self::heap_response(self.heap.as_ref().unwrap())))
                    }
                }
            }
            Action::FreeHeap => {
                if let Err(fault) = self
                    .backend
                    .reclaim(&self.heap.as_ref().ok_or(Error::Invariant)?.receipt)
                {
                    return self.failure(context, fault, None);
                }
                self.gate_expected(
                    if self.storage.is_some() {
                        Ownership::SharedStorage
                    } else {
                        Ownership::Host
                    },
                    Ownership::Host,
                )?;
                self.heap = None;
                self.gate()?;
                Ok(Some(Response::success()))
            }
            Action::Transfer {
                handle,
                direction,
                count,
                index,
                offset,
                staged,
                sync,
            } => {
                let partition = self.handles[handle].ok_or(Error::Invariant)?.partition;
                if index == count {
                    if sync {
                        if let Err(fault) = self.backend.sync(partition) {
                            return self.failure(context, fault, Some(handle));
                        }
                        self.gate()?;
                    }
                    return Ok(Some(Response::success()));
                }
                let span = self.spans[index];
                let n = (span.bytes - offset).min(IO_BYTES as u32) as usize;
                let lease = &self.storage.as_ref().ok_or(Error::Invariant)?.receipt;
                let result = match (direction, staged) {
                    (Direction::Read, false) => self.backend.disk_read(
                        partition,
                        span.disk + u64::from(offset),
                        &mut self.buffer[..n],
                    ),
                    (Direction::Read, true) => {
                        self.backend
                            .pool_write(lease, span.pool + offset, &self.buffer[..n])
                    }
                    (Direction::Write, false) => {
                        self.backend
                            .pool_read(lease, span.pool + offset, &mut self.buffer[..n])
                    }
                    (Direction::Write, true) => self.backend.disk_write(
                        partition,
                        span.disk + u64::from(offset),
                        &self.buffer[..n],
                    ),
                };
                if let Err(fault) = result {
                    return self.failure(context, fault, Some(handle));
                }
                self.gate()?;
                let (index, offset) = if staged && offset + n as u32 == span.bytes {
                    (index + 1, 0)
                } else {
                    (index, offset + if staged { n as u32 } else { 0 })
                };
                self.job.as_mut().unwrap().action = Action::Transfer {
                    handle,
                    direction,
                    count,
                    index,
                    offset,
                    staged: !staged,
                    sync,
                };
                Ok(None)
            }
            Action::Done => Err(Error::Invariant),
        }
    }
}
