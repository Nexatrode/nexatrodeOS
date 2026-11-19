//! Typed second transaction after this boot's actual memory-setup completion.
//! Uses fresh ring/payload slots in the same retained pool. No Ready token.
#![forbid(unsafe_code)]
use crate::{
    sunfish_ipa_linux_buffers::{Memory, Pool},
    sunfish_ipa_linux_command::StartOnce,
    sunfish_ipa_linux_endpoint::ConfigureOnce,
    sunfish_ipa_linux_gsi::SetupOnce,
    sunfish_ipa_linux_tables::Plan,
    sunfish_ipa_linux_transfer::{Error, Io, Report, TransferOnce},
};
pub struct TableOnce {
    inner: TransferOnce,
}
impl TableOnce {
    pub fn new(timeout_ticks: u64) -> Result<Self, Error> {
        Ok(Self {
            inner: TransferOnce::new_tables(timeout_ticks)?,
        })
    }
    pub const fn report(&self) -> Report {
        self.inner.report()
    }
    pub fn plan(&self) -> &Plan {
        self.inner.table_plan()
    }
    pub fn begin<M: Memory>(
        &mut self,
        setup: &SetupOnce,
        endpoints: &ConfigureOnce,
        command: &StartOnce,
        memory: &TransferOnce,
        pool: &mut Pool<M>,
        io: &mut impl Io<M>,
    ) -> Result<(), Error> {
        self.inner
            .begin_tables(setup, endpoints, command, memory, pool, io)
    }
    pub fn notified<M: Memory>(
        &mut self,
        pool: &mut Pool<M>,
        io: &mut impl Io<M>,
        now: u64,
    ) -> Result<(), Error> {
        self.inner.notified(pool, io, now)
    }
    pub fn timeout<M: Memory>(
        &mut self,
        pool: &mut Pool<M>,
        io: &mut impl Io<M>,
        now: u64,
    ) -> Result<(), Error> {
        self.inner.timeout(pool, io, now)
    }
    pub fn external_fault<M: Memory>(
        &mut self,
        pool: &mut Pool<M>,
        io: &mut impl Io<M>,
    ) -> Result<(), Error> {
        self.inner.external_fault(pool, io)
    }
    pub fn late_fault<M: Memory>(
        &mut self,
        pool: &mut Pool<M>,
        io: &mut impl Io<M>,
    ) -> Result<(), Error> {
        self.inner.late_fault(pool, io)
    }
}
