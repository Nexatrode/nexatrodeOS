//! Storage-only selected-Linux service inside the existing serialized owner.
#![forbid(unsafe_code)]
use super::*;
use crate::sunfish_linux_rmtfs as resource;
use crate::sunfish_linux_rmtfs_server::{self as server, Server};

impl<I: Io> Owner<I> {
    /// Make bounded forward progress without repeating the whole QRTR router
    /// between every bounded storage chunk. Each step still performs its own
    /// identity, lifecycle and storage validation. Yield immediately on a
    /// reply, backpressure, idleness or error so other services get a turn.
    pub fn dispatch_linux_rmtfs_burst<P: resource::Io>(
        &mut self,
        s: &mut Server<'_, P>,
        irqs: &mut impl Interrupts,
    ) -> Result<ServiceProgress, Error> {
        const STEPS: usize = 2;
        for _ in 0..STEPS {
            match self.dispatch_linux_rmtfs_step(s, irqs)? {
                ServiceProgress::Progress => {},
                other => return Ok(other),
            }
        }
        Ok(ServiceProgress::Progress)
    }
    fn bind_linux_rmtfs<P: resource::Io>(
        &mut self,
        s: &mut Server<'_, P>,
        activate: bool,
    ) -> Result<(), Error> {
        self.adapter.revalidate(self.generation)?;
        let c = s.configuration();
        let config = self.adapter.configuration();
        let endpoint = self.adapter.endpoint(Service::RemoteStorage);
        if self.handlers_id.is_some()
            || c.generation != self.generation
            || c.local.node != config.local_node
            || c.peer_node != config.peer_node
            || endpoint.is_none_or(|e| e.address != c.local)
            || if activate {
                self.linux_rmtfs_id.is_some() || s.bound().is_some()
            } else {
                self.linux_rmtfs_id != Some(s.id()) || s.bound() != Some(self.identity())
            }
        {
            return Err(Error::LinuxRmtfs(server::Error::Binding));
        }
        if activate {
            s.bind(self.identity()).map_err(Error::LinuxRmtfs)?;
            self.linux_rmtfs_id = Some(s.id());
        }
        s.validate().map_err(Error::LinuxRmtfs)
    }
    /// One activation with real pre-shared resources; no heap dependency.
    /// Retain the server and owner after any failure or uncertain operation.
    pub fn activate_linux_rmtfs<P: resource::Io>(
        &mut self,
        s: &mut Server<'_, P>,
        irqs: &mut impl Interrupts,
    ) -> Result<(), Error> {
        let result = (|| {
            self.check()?;
            self.stopped = true;
            self.bind_linux_rmtfs(s, true)?;
            self.stopped = false;
            self.set_service_available(Service::RemoteStorage, true, irqs)
        })();
        if let Err(e) = result {
            s.fault();
            return Err(self.stop(e));
        }
        Ok(())
    }
    /// One bounded chunk or cached reply. Backpressure preserves both request
    /// and response, so disk/memory effects are never rerun just to enqueue.
    pub fn dispatch_linux_rmtfs_step<P: resource::Io>(
        &mut self,
        s: &mut Server<'_, P>,
        irqs: &mut impl Interrupts,
    ) -> Result<ServiceProgress, Error> {
        let result = (|| {
            self.check()?;
            self.stopped = true;
            self.bind_linux_rmtfs(s, false)?;
            if s.active_handle().is_none() {
                let mut selected = None;
                for handle in self.requests.iter().flatten().copied() {
                    if self
                        .adapter
                        .request_qmi(self.generation, &handle)?
                        .0
                        .service
                        == Service::RemoteStorage
                    {
                        selected = Some(handle);
                        break;
                    }
                }
                let Some(handle) = selected else {
                    return Ok(server::Progress::Idle);
                };
                let (context, request) = self.adapter.request(self.generation, &handle)?;
                s.begin(handle, context, request)
                    .map_err(Error::LinuxRmtfs)?;
            }
            if !self.requests.iter().any(|h| *h == s.active_handle()) {
                return Err(Error::LinuxRmtfs(server::Error::Request));
            }
            s.step().map_err(Error::LinuxRmtfs)
        })();
        let progress = match result {
            Ok(p) => {
                self.stopped = false;
                p
            }
            Err(e) => {
                s.fault();
                return Err(self.stop(e));
            }
        };
        let result = match progress {
            server::Progress::Idle => Ok(ServiceProgress::Idle),
            server::Progress::Progress => self.schedule(irqs).map(|_| ServiceProgress::Progress),
            server::Progress::ReplyReady => {
                let handle = s.active_handle().unwrap();
                let report = s.report();
                let track = self.rmtfs_large_reply.is_none()
                    && report.active_message == Some(3)
                    && report.active_write == Some(false)
                    && report.active_sectors_total == Some(4094);
                let success = s.response().unwrap().is_success();
                match self.queue_reply_inner(&handle, s.response().unwrap().reply(), irqs) {
                    Ok(()) => {
                        if track {
                            self.rmtfs_large_reply = Some(handle);
                            self.rmtfs_large_reply_flags |= 1 | (u32::from(success) << 4);
                            self.mark_rmtfs_event(0);
                        }
                        s.reply_queued(&handle)
                            .map(|_| ServiceProgress::ReplyQueued)
                            .map_err(Error::LinuxRmtfs)
                    },
                    Err(Error::Transport(fifo::Error::Session(
                        crate::qcom_glink_qmi::Error::FlowBusy
                        | crate::qcom_glink_qmi::Error::NoCapacity,
                    ))) => Ok(ServiceProgress::WaitingReply),
                    Err(e) => Err(e),
                }
            }
        };
        match result {
            Ok(p) => Ok(p),
            Err(e) => {
                s.fault();
                Err(self.stop(e))
            }
        }
    }
}
