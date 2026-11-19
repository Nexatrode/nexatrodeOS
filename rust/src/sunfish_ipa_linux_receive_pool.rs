//! Retained CPU preparation for the full IPA exception receive allocation.
//!
//! Owns a separate 2-MiB backend. The 251 two-page buffers have protected
//! 64-byte headrooms; the unused final ten pages have a separate canary.
//! Preparation and guard reports prove CPU observations only. No SMMU, DMA,
//! firmware or production-ready authority is constructed by this type.
#![forbid(unsafe_code)]

use crate::sunfish_ipa_linux_buffers::Memory;
use crate::sunfish_ipa_linux_receive::{self as receive, Plan};

pub const HEADROOM_CANARY: u32 = 0x1a2b3c4d;
pub const TAIL_CANARY: u32 = 0xe5f60718;
pub const GUARD_WORDS: usize = receive::BUFFER_COUNT * receive::HEADROOM / 4
    + (receive::DATA_RESERVED_BYTES - receive::DATA_USED_BYTES) / 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Layout,
    Consumed,
    Quarantined,
    Guard,
    Memory,
    Readback,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Report {
    pub writes_sent: usize,
    pub writes_done: usize,
    pub checked_words: usize,
    pub passes: u8,
    pub offset: Option<usize>,
    pub expected: u32,
    pub actual: u32,
    pub error: Option<Error>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GuardReport {
    pub checked_words: usize,
    pub offset: Option<usize>,
    pub error: Option<Error>,
}
pub struct DataPool<M: Memory> {
    memory: M,
    plan: Plan,
    consumed: bool,
    prepared: bool,
    quarantined: bool,
}
impl<M: Memory> DataPool<M> {
    pub fn new(memory: M) -> Result<Self, Error> {
        if memory.bytes() != receive::DATA_RESERVED_BYTES {
            return Err(Error::Layout);
        }
        Ok(Self {
            memory,
            plan: Plan::new().map_err(|_| Error::Layout)?,
            consumed: false,
            prepared: false,
            quarantined: false,
        })
    }
    pub const fn plan(&self) -> Plan {
        self.plan
    }
    /// CPU-preparation diagnostic, never a mapping or DMA receipt.
    pub const fn prepared(&self) -> bool {
        self.prepared && !self.quarantined
    }
    pub fn quarantine(&mut self) {
        self.quarantined = true;
        self.prepared = false;
    }
    /// Only the retained receive runtime may select a completed buffer.
    /// A scalar read can include unused bytes in its final word, but never
    /// headroom, another buffer or the CPU-only tail. The public packet view
    /// exposes only bytes below the completion's actual length.
    #[allow(dead_code)]
    pub(crate) fn completed_read32(&mut self, buffer: usize, offset: usize) -> Result<u32, Error> {
        if !self.prepared() {
            return Err(Error::Quarantined);
        }
        if buffer >= receive::BUFFER_COUNT || offset % 4 != 0 || offset >= receive::DMA_BYTES {
            return Err(Error::Layout);
        }
        let absolute = buffer * receive::BUFFER_BYTES + receive::HEADROOM + offset;
        let result = self.memory.read32(absolute).map_err(|_| Error::Memory);
        if result.is_err() {
            self.quarantine();
        }
        result
    }
    fn expected(offset: usize) -> u32 {
        if offset >= receive::DATA_USED_BYTES {
            TAIL_CANARY
        } else if offset % receive::BUFFER_BYTES < receive::HEADROOM {
            HEADROOM_CANARY
        } else {
            0
        }
    }
    pub fn prepare(&mut self, mut guard: impl FnMut() -> Result<(), Error>) -> Report {
        let mut r = Report {
            writes_sent: 0,
            writes_done: 0,
            checked_words: 0,
            passes: 0,
            offset: None,
            expected: 0,
            actual: 0,
            error: None,
        };
        if self.quarantined {
            r.error = Some(Error::Quarantined);
            return r;
        }
        if self.consumed {
            r.error = Some(Error::Consumed);
            return r;
        }
        self.consumed = true;
        let result = (|| {
            for pass in 0..3 {
                for chunk in 0..=receive::BUFFER_COUNT {
                    guard().map_err(|_| Error::Guard)?;
                    let start = chunk * receive::BUFFER_BYTES;
                    let end = if chunk == receive::BUFFER_COUNT {
                        receive::DATA_RESERVED_BYTES
                    } else {
                        start + receive::BUFFER_BYTES
                    };
                    for offset in (start..end).step_by(4) {
                        r.offset = Some(offset);
                        r.expected = Self::expected(offset);
                        if pass == 0 {
                            r.writes_sent += 1;
                            self.memory
                                .write32(offset, r.expected)
                                .map_err(|_| Error::Memory)?;
                            r.writes_done += 1;
                        } else {
                            r.actual = self.memory.read32(offset).map_err(|_| Error::Memory)?;
                            if r.actual != r.expected {
                                return Err(Error::Readback);
                            }
                            r.checked_words += 1;
                        }
                    }
                }
                if pass != 0 {
                    r.passes += 1;
                }
            }
            guard().map_err(|_| Error::Guard)?;
            r.offset = None;
            self.prepared = true;
            Ok(())
        })();
        r.error = result.err();
        if r.error.is_some() {
            self.quarantine();
        }
        r
    }
    /// Does not inspect device-owned packet bytes or change any memory.
    /// A concrete native caller must guard the actual retained mapping owner.
    pub fn verify_guards(&mut self, mut guard: impl FnMut() -> Result<(), Error>) -> GuardReport {
        self.verify_guards_with(|_| guard())
    }
    #[allow(dead_code)] // Standalone pool suite excludes its later controller.
    pub(crate) fn verify_guards_with(
        &mut self,
        mut guard: impl FnMut(&Self) -> Result<(), Error>,
    ) -> GuardReport {
        let mut r = GuardReport {
            checked_words: 0,
            offset: None,
            error: None,
        };
        if !self.prepared() {
            r.error = Some(if self.quarantined {
                Error::Quarantined
            } else {
                Error::Consumed
            });
            return r;
        }
        let result = (|| {
            for chunk in 0..=receive::BUFFER_COUNT {
                guard(self).map_err(|_| Error::Guard)?;
                let start = chunk * receive::BUFFER_BYTES;
                let end = if chunk == receive::BUFFER_COUNT {
                    receive::DATA_RESERVED_BYTES
                } else {
                    start + receive::HEADROOM
                };
                for offset in (start..end).step_by(4) {
                    r.offset = Some(offset);
                    if self.memory.read32(offset).map_err(|_| Error::Memory)?
                        != Self::expected(offset)
                    {
                        return Err(Error::Readback);
                    }
                    r.checked_words += 1;
                }
            }
            guard(self).map_err(|_| Error::Guard)?;
            r.offset = None;
            Ok(())
        })();
        r.error = result.err();
        if r.error.is_some() {
            self.quarantine();
        }
        r
    }
}
