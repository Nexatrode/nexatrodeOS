//! Selected Linux RMTFS requests over retained, pre-shared native resources.
//! No heap, PAS, persistent disk or physical-address dereference is available.
#![forbid(unsafe_code)]
use crate::qcom_glink_qmi::{Reply, RequestHandle};
use crate::qcom_modem_handlers::{Disk, Fault};
use crate::qcom_modem_storage::RAM_IO_BYTES as IO_BYTES;
use crate::qcom_modem_services::{
    Direction, MAX_IOVECS, Partition, Request, Service, StorageRequest,
};
use crate::qcom_modem_storage::{self as storage};
use crate::qcom_qmi_router::{Address, Context};
use crate::sunfish_linux_rmtfs::{BUFFER_BYTES, BUFFER_PA, Io, Resources};
use core::sync::atomic::{AtomicU64, Ordering};

static NEXT_SERVER: AtomicU64 = AtomicU64::new(1);
fn next_id() -> u64 {
    let mut value = NEXT_SERVER.load(Ordering::Relaxed);
    loop {
        let Some(next) = value.checked_add(1) else {
            return 0;
        };
        match NEXT_SERVER.compare_exchange_weak(value, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return value,
            Err(actual) => value = actual,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Configuration {
    pub generation: u64,
    pub local: Address,
    pub peer_node: u32,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Configuration,
    Stopped,
    Resource,
    Binding,
    Request,
    Busy,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Progress {
    Idle,
    Progress,
    ReplyReady,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Report {
    pub open_handles: usize,
    pub requests: u64,
    pub replies_queued: u64,
    pub chunks: u64,
    pub active_request: bool,
    pub active_message: Option<u16>,
    pub active_write: Option<bool>,
    pub active_sectors_total: Option<u64>,
    pub active_sectors_remaining: Option<u64>,
    pub failed: bool,
}
#[derive(Clone, Copy)]
struct File {
    id: u32,
    partition: Partition,
    error: bool,
}
#[derive(Clone, Copy)]
struct Span {
    sector: u32,
    physical: u64,
    sectors: u32,
}
const EMPTY: Span = Span {
    sector: 0,
    physical: 0,
    sectors: 0,
};
#[derive(Clone, Copy)]
enum Action {
    Open(Partition),
    Close(usize),
    Allocate(u32),
    DeviceError(usize),
    Transfer {
        file: usize,
        count: usize,
        index: usize,
        offset: u32,
        write: bool,
        sync: bool,
    },
    Done,
}
struct Job {
    handle: RequestHandle,
    message_id: u16,
    action: Action,
    response: Option<Response>,
}
pub(crate) struct Response {
    error: bool,
    field: [u8; 11],
    len: usize,
}
impl Response {
    fn success() -> Self {
        Self {
            error: false,
            field: [0; 11],
            len: 0,
        }
    }
    fn failure() -> Self {
        Self {
            error: true,
            ..Self::success()
        }
    }
    fn value(bytes: &[u8]) -> Self {
        let mut s = Self::success();
        s.field[0] = 0x10;
        s.field[1..3].copy_from_slice(&(bytes.len() as u16).to_le_bytes());
        s.field[3..3 + bytes.len()].copy_from_slice(bytes);
        s.len = 3 + bytes.len();
        s
    }
    pub(crate) fn reply(&self) -> Reply<'_> {
        if self.error {
            Reply::Failure { error: 1 }
        } else {
            Reply::Success {
                optional_tlvs: &self.field[..self.len],
            }
        }
    }
    pub(crate) fn is_success(&self) -> bool { !self.error }
}
/// One bounded request with a cached reply across transport backpressure.
/// Handles follow the authenticated peer node, not a transient QRTR port.
/// Close neither revokes the shared reservation nor discards boot-local EFS.
pub struct Server<'a, I: Io> {
    resources: Resources<'a, I>,
    config: Configuration,
    disks: [Disk; 4],
    id: u64,
    binding: Option<u64>,
    files: [Option<File>; 4],
    next_handle: u32,
    spans: [Span; MAX_IOVECS],
    job: Option<Job>,
    requests: u64,
    replies: u64,
    chunks: u64,
    failed: bool,
    busy: bool,
}
impl<'a, I: Io> Server<'a, I> {
    /// Retain all resources, including failed attachment. Only pre-shared,
    /// verified RAM-backed resources may be advertised; no data/SCM IO here.
    pub fn attach(config: Configuration, resources: Resources<'a, I>) -> Self {
        let storage = resources.configuration();
        let mut s = Self {
            resources,
            config,
            disks: storage.bindings.map(|b| b.disk),
            id: next_id(),
            binding: None,
            files: [None; 4],
            next_handle: 0,
            spans: [EMPTY; MAX_IOVECS],
            job: None,
            requests: 0,
            replies: 0,
            chunks: 0,
            failed: true,
            busy: true,
        };
        if s.id != 0
            && config.generation != 0
            && config.generation == storage.generation
            && config.local.node == 1
            && config.peer_node == 0
            && config.local.port != 0
            && config.local.port < 0xffff_fffe
            && s.resources.shared().is_ok()
        {
            s.failed = false;
            s.busy = false;
        }
        s
    }
    pub fn configuration(&self) -> Configuration {
        self.config
    }
    pub fn report(&self) -> Report {
        let transfer = self.job.as_ref().and_then(|job| match job.action {
            Action::Transfer { count, index, offset, write, .. } => {
                let total = self.spans[..count].iter().map(|span| u64::from(span.sectors)).sum();
                let remaining = self.spans[index..count].iter().map(|span| u64::from(span.sectors)).sum::<u64>()
                    .saturating_sub(u64::from(offset));
                Some((write, total, remaining))
            }
            _ => None,
        });
        Report {
            open_handles: self.files.iter().flatten().count(),
            requests: self.requests,
            replies_queued: self.replies,
            chunks: self.chunks,
            active_request: self.job.is_some(),
            active_message: self.job.as_ref().map(|job| job.message_id),
            active_write: transfer.map(|value| value.0),
            active_sectors_total: transfer.map(|value| value.1),
            active_sectors_remaining: transfer.map(|value| value.2),
            failed: self.failed || self.busy || self.resources.status().2,
        }
    }
    pub fn resource_status(&self) -> (crate::sunfish_linux_rmtfs::Status, storage::Status, bool) {
        self.resources.status()
    }
    pub(crate) fn id(&self) -> u64 {
        self.id
    }
    pub(crate) fn bound(&self) -> Option<u64> {
        self.binding
    }
    pub(crate) fn bind(&mut self, identity: u64) -> Result<(), Error> {
        self.validate()?;
        if identity == 0 || self.binding.is_some() {
            return Err(self.stop(Error::Binding));
        }
        self.binding = Some(identity);
        Ok(())
    }
    pub fn fault(&mut self) {
        self.failed = true;
    }
    fn stop(&mut self, e: Error) -> Error {
        self.failed = true;
        e
    }
    pub(crate) fn validate(&mut self) -> Result<(), Error> {
        if self.failed || self.busy {
            return Err(self.stop(Error::Stopped));
        }
        self.busy = true;
        self.resources
            .shared()
            .map_err(|_| self.stop(Error::Resource))?;
        self.busy = false;
        Ok(())
    }
    #[cfg(feature = "sunfish-linux-mpss-auth")]
    pub(crate) fn pre_auth_shared(&mut self, generation: u64) -> Result<(), Error>
    where
        I: crate::sunfish_mpss_linux_boot::AuthIo,
    {
        self.validate()?;
        if self.job.is_some() {
            return Err(self.stop(Error::Stopped));
        }
        self.busy = true;
        self.resources
            .pre_auth_shared(generation)
            .map_err(|_| self.stop(Error::Resource))?;
        self.busy = false;
        Ok(())
    }
    #[cfg(feature = "sunfish-linux-mpss-auth")]
    pub(crate) fn submit_mpss_auth(
        &mut self,
        generation: u64,
        request: &crate::sunfish_mpss_linux_loader::Request,
    ) -> Result<(), Error>
    where
        I: crate::sunfish_mpss_linux_boot::AuthIo,
    {
        self.validate()?;
        self.busy = true;
        self.resources
            .submit_mpss_auth(generation, request)
            .map_err(|_| self.stop(Error::Resource))?;
        self.busy = false;
        self.validate()
    }
    #[cfg(feature = "sunfish-linux-mpss-auth")]
    pub(crate) fn runtime_shared(&mut self) -> Result<(), Error>
    where
        I: crate::sunfish_mpss_linux_boot::AuthIo,
    {
        self.validate()?;
        self.busy = true;
        self.resources
            .runtime_shared()
            .map_err(|_| self.stop(Error::Resource))?;
        self.busy = false;
        Ok(())
    }
    pub(crate) fn active_handle(&self) -> Option<RequestHandle> {
        self.job.as_ref().map(|j| j.handle)
    }
    fn file(&self, id: u32) -> Option<usize> {
        self.files
            .iter()
            .position(|f| f.is_some_and(|f| f.id == id))
    }
    pub(crate) fn begin(
        &mut self,
        handle: RequestHandle,
        context: Context,
        request: Request<'_>,
    ) -> Result<(), Error> {
        self.validate()?;
        if self.job.is_some() {
            return Err(self.stop(Error::Busy));
        }
        let Request::Storage(request) = request else {
            return Err(self.stop(Error::Request));
        };
        let message = match request {
            StorageRequest::Open(_) => 1,
            StorageRequest::Close { .. } => 2,
            StorageRequest::ReadWrite { .. } => 3,
            StorageRequest::Allocate { .. } => 4,
            StorageRequest::DeviceError { .. } => 5,
        };
        if context.generation != self.config.generation
            || context.service != Service::RemoteStorage
            || context.local != self.config.local
            || context.peer.node != self.config.peer_node
            || context.peer.port == 0
            || context.peer.port >= 0xffff_fffe
            || context.message_id != message
        {
            return Err(self.stop(Error::Request));
        }
        let mut response = None;
        let action = self.plan(request, &mut response);
        self.job = Some(Job {
            handle,
            message_id: message,
            action,
            response,
        });
        self.requests = self
            .requests
            .checked_add(1)
            .ok_or_else(|| self.stop(Error::Stopped))?;
        Ok(())
    }
    fn plan(&mut self, request: StorageRequest<'_>, reply: &mut Option<Response>) -> Action {
        let fail = |r: &mut Option<Response>| {
            *r = Some(Response::failure());
            Action::Done
        };
        let id = match request {
            StorageRequest::Open(p) => return Action::Open(p),
            StorageRequest::Close { handle }
            | StorageRequest::Allocate { handle, .. }
            | StorageRequest::DeviceError { handle }
            | StorageRequest::ReadWrite { handle, .. } => handle,
        };
        let Some(file) = self.file(id) else {
            return fail(reply);
        };
        match request {
            StorageRequest::Open(_) => unreachable!(),
            StorageRequest::Close { .. } => Action::Close(file),
            StorageRequest::DeviceError { .. } => Action::DeviceError(file),
            StorageRequest::Allocate { bytes, .. } => {
                if bytes == 0 || bytes > BUFFER_BYTES {
                    fail(reply)
                } else {
                    Action::Allocate(bytes)
                }
            }
            StorageRequest::ReadWrite {
                direction,
                iovecs,
                force_sync,
                ..
            } => {
                let partition = self.files[file].unwrap().partition;
                let disk_bytes = self.disks[storage::index(partition)].bytes;
                self.spans.fill(EMPTY);
                // Every vector is checked before any chunk takes effect.
                for (i, v) in iovecs.iter().enumerate() {
                    let n = v.byte_count();
                    let pa = u64::from(v.physical_address);
                    if n == 0
                        || pa % 512 != 0
                        || pa < BUFFER_PA
                        || pa
                            .checked_add(n)
                            .is_none_or(|end| end > BUFFER_PA + u64::from(BUFFER_BYTES))
                        || v.disk_offset()
                            .checked_add(n)
                            .is_none_or(|end| end > disk_bytes)
                    {
                        return fail(reply);
                    }
                    self.spans[i] = Span {
                        sector: v.sector,
                        physical: pa,
                        sectors: v.sectors,
                    };
                }
                Action::Transfer {
                    file,
                    count: iovecs.len(),
                    index: 0,
                    offset: 0,
                    write: direction == Direction::Write,
                    sync: force_sync || direction == Direction::Write,
                }
            }
        }
    }
    pub(crate) fn step(&mut self) -> Result<Progress, Error> {
        self.validate()?;
        let Some(job) = self.job.as_ref() else {
            return Ok(Progress::Idle);
        };
        if job.response.is_some() {
            return Ok(Progress::ReplyReady);
        }
        let action = job.action;
        self.busy = true;
        let response = match action {
            Action::Open(p) => {
                if let Some(f) = self.files.iter().flatten().find(|f| f.partition == p) {
                    Some(Response::value(&f.id.to_le_bytes()))
                } else if let Some(at) = self.files.iter().position(Option::is_none) {
                    if self.next_handle == u32::MAX {
                        Some(Response::failure())
                    } else {
                        self.resources
                            .disk(p)
                            .map_err(|_| self.stop(Error::Resource))?;
                        let id = self.next_handle;
                        self.next_handle += 1;
                        self.files[at] = Some(File {
                            id,
                            partition: p,
                            error: false,
                        });
                        Some(Response::value(&id.to_le_bytes()))
                    }
                } else {
                    Some(Response::failure())
                }
            }
            Action::Close(i) => {
                self.files[i] = None;
                Some(Response::success())
            }
            Action::DeviceError(i) => {
                Some(Response::value(&[u8::from(self.files[i].unwrap().error)]))
            }
            Action::Allocate(bytes) => {
                let pa = self
                    .resources
                    .allocation(bytes)
                    .map_err(|_| self.stop(Error::Resource))?;
                Some(Response::value(&pa.to_le_bytes()))
            }
            Action::Transfer {
                file,
                count,
                index,
                offset,
                write,
                sync,
            } => {
                let p = self.files[file].unwrap().partition;
                if index == count {
                    if sync {
                        self.resources
                            .sync(p)
                            .map_err(|_| self.stop(Error::Resource))?;
                    }
                    Some(Response::success())
                } else {
                    let span = self.spans[index];
                    let n = (span.sectors - offset).min(IO_BYTES as u32 / 512);
                    match self.resources.transfer(
                        p,
                        span.sector + offset,
                        span.physical + u64::from(offset) * 512,
                        n,
                        write,
                    ) {
                        Err(Fault::Uncertain) => return Err(self.stop(Error::Resource)),
                        Err(Fault::Failed) => {
                            self.files[file].as_mut().unwrap().error = true;
                            self.job.as_mut().unwrap().response = Some(Response::failure());
                        }
                        Ok(()) => {
                            self.chunks = self
                                .chunks
                                .checked_add(1)
                                .ok_or_else(|| self.stop(Error::Stopped))?;
                        }
                    }
                    let (index, offset) = if offset + n == span.sectors {
                        (index + 1, 0)
                    } else {
                        (index, offset + n)
                    };
                    self.job.as_mut().unwrap().action = Action::Transfer {
                        file,
                        count,
                        index,
                        offset,
                        write,
                        sync,
                    };
                    None
                }
            }
            Action::Done => return Err(self.stop(Error::Request)),
        };
        if let Some(response) = response {
            self.job.as_mut().unwrap().response = Some(response);
        }
        self.busy = false;
        self.validate()?;
        if self.job.as_ref().unwrap().response.is_some() {
            Ok(Progress::ReplyReady)
        } else {
            Ok(Progress::Progress)
        }
    }
    pub(crate) fn response(&self) -> Option<&Response> {
        self.job.as_ref()?.response.as_ref()
    }
    /// Commit only after actual queue success; transport retains both ACKs.
    pub(crate) fn reply_queued(&mut self, handle: &RequestHandle) -> Result<(), Error> {
        self.validate()?;
        if self
            .job
            .as_ref()
            .is_none_or(|j| j.handle != *handle || j.response.is_none())
        {
            return Err(self.stop(Error::Request));
        }
        self.replies = self
            .replies
            .checked_add(1)
            .ok_or_else(|| self.stop(Error::Stopped))?;
        self.job = None;
        Ok(())
    }
}
