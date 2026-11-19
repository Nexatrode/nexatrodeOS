//! Pre-PAS IPA v4.2 interrupt arming from the pinned working Linux profile.
//!
//! Mirrors the IRQ_EN writes in ipa_interrupt_config()/ipa_uc_config(), after
//! a real IRQ binding. This experiment never starts firmware, sends a UC
//! command or clears interrupt evidence. Any notification masks the source,
//! retains the delivered IRQ masked and stops. It is not a production UC ISR,
//! firmware-ready capability or proof of physical interrupt delivery.
#![forbid(unsafe_code)]

pub const IRQ: u32 = 343; // GIC SPI 311 + 32
pub const CHANNEL: u8 = 2;
pub const ENABLE_OFFSET: usize = 0x300c;
pub const STATUS_OFFSET: usize = 0x3008;
pub const EVENT_MASK: u32 = 1 << 2;
pub const RESPONSE_MASK: u32 = 1 << 3;
pub const ENABLED: u32 = EVENT_MASK | RESPONSE_MASK;
// Measured cold status 0x10080: UC_RX_CMD_Q_NOT_FULL (7) and BAM_GSI_IDLE
// (16). Both remain disabled and are unused by the pinned Linux driver.
// Preserve their raw status; do not clear them or allow any other pending bit.
pub const ALLOWED_DISABLED_STATUS: u32 = (1 << 7) | (1 << 16);
pub const COMPLETE: u8 = 7;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Read {
    Enable,
    Status,
}
impl Read {
    pub const fn offset(self) -> usize {
        match self {
            Self::Enable => ENABLE_OFFSET,
            Self::Status => STATUS_OFFSET,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Shared {
    Event,
    Response,
}
impl Shared {
    pub const fn offset(self) -> usize {
        match self {
            Self::Event => 20,
            Self::Response => 12,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Step {
    Disable,
    Event,
    Response,
}
impl Step {
    pub const fn value(self) -> u32 {
        match self {
            Self::Disable => 0,
            Self::Event => EVENT_MASK,
            Self::Response => ENABLED,
        }
    }
}
pub struct Write {
    step: Step,
}
impl Write {
    pub const fn step(&self) -> Step {
        self.step
    }
    pub const fn value(&self) -> u32 {
        self.step.value()
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    AlreadyStarted,
    Io,
    Domain,
    Config,
    Resources,
    Buffers,
    Binding,
    Changed,
    Pending,
    Readback,
    Notification,
    AlreadyStopped,
}
impl Error {
    pub const fn code(self) -> u32 {
        self as u32 + 1
    }
}
pub trait Io {
    fn guard(&mut self, expected_enable: u32) -> Result<(), Error>;
    fn read(&mut self, register: Read) -> Result<u32, Error>;
    fn write(&mut self, request: &Write) -> Result<(), Error>;
    /// Actual Microkit IRQ capability ACK, once, before any nonzero enable.
    fn bind(&mut self) -> Result<(), Error>;
    /// Read only one defined byte; never widen across reserved UC bytes.
    fn shared(&mut self, field: Shared) -> Result<u8, Error>;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Snapshot {
    pub enabled: u32,
    pub status: u32,
}
fn capture(io: &mut impl Io, expected_enable: u32) -> Result<Snapshot, Error> {
    io.guard(expected_enable)?;
    Ok(Snapshot {
        enabled: io.read(Read::Enable)?,
        status: io.read(Read::Status)?,
    })
}
fn quiet(sample: Snapshot, expected: u32) -> Result<(), Error> {
    if sample.enabled != expected {
        return Err(Error::Readback);
    }
    if sample.status & !ALLOWED_DISABLED_STATUS != 0 {
        return Err(Error::Pending);
    }
    Ok(())
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Report {
    pub submitted: u8,
    pub completed: u8,
    pub bound: bool,
    pub failed: Option<Step>,
    pub last: Option<Snapshot>,
    pub error: Option<Error>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Notice {
    pub mask_submitted: bool,
    pub last: Option<Snapshot>,
    pub event: Option<u8>,
    pub response: Option<u8>,
    pub error: Error,
}
pub struct ArmOnce {
    started: bool,
    notified: bool,
    armed: bool,
}
impl ArmOnce {
    pub const fn new() -> Self {
        Self {
            started: false,
            notified: false,
            armed: false,
        }
    }
    pub fn run(&mut self, io: &mut impl Io, mut progress: impl FnMut(Step)) -> Report {
        let mut r = Report {
            submitted: 0,
            completed: 0,
            bound: false,
            failed: None,
            last: None,
            error: None,
        };
        if self.started {
            r.error = Some(Error::AlreadyStarted);
            return r;
        }
        self.started = true;
        let result = (|| {
            let first = capture(io, 0)?;
            r.last = Some(first);
            quiet(first, 0)?;
            let second = capture(io, 0)?;
            r.last = Some(second);
            if first != second {
                return Err(Error::Changed);
            }
            let mut expected = 0;
            for (index, step) in [Step::Disable, Step::Event, Step::Response]
                .into_iter()
                .enumerate()
            {
                r.failed = Some(step);
                progress(step);
                let before = capture(io, expected)?;
                r.last = Some(before);
                quiet(before, expected)?;
                if step == Step::Event {
                    io.bind()?;
                    r.bound = true;
                    let bound = capture(io, expected)?;
                    r.last = Some(bound);
                    quiet(bound, expected)?;
                }
                r.submitted |= 1 << index;
                io.write(&Write { step })?;
                expected = step.value();
                let after = capture(io, expected)?;
                r.last = Some(after);
                quiet(after, expected)?;
                r.completed |= 1 << index;
            }
            for _ in 0..2 {
                let final_sample = capture(io, ENABLED)?;
                r.last = Some(final_sample);
                quiet(final_sample, ENABLED)?;
            }
            r.failed = None;
            Ok(())
        })();
        r.error = result.err();
        self.armed = r.error.is_none();
        r
    }
    // Transfer the successful arming state once. A copied Report cannot
    // construct the runtime handler, and the old containment path is retired.
    pub(crate) fn transfer_to_runtime(&mut self) -> bool {
        if !self.armed || self.notified {
            return false;
        }
        self.armed = false;
        self.notified = true;
        true
    }
    /// Containment is distinct from retry/rollback. Mask first even if a guard
    /// has failed, retain pending status, never clear or ACK the delivered IRQ.
    /// One notification only; later calls cannot perform additional I/O.
    pub fn notification(&mut self, io: &mut impl Io) -> Notice {
        let mut n = Notice {
            mask_submitted: false,
            last: None,
            event: None,
            response: None,
            error: Error::Notification,
        };
        if self.notified {
            n.error = Error::AlreadyStopped;
            return n;
        }
        self.notified = true;
        self.started = true;
        self.armed = false;
        let result = (|| {
            n.mask_submitted = true;
            io.write(&Write {
                step: Step::Disable,
            })?;
            let sample = Snapshot {
                enabled: io.read(Read::Enable)?,
                status: io.read(Read::Status)?,
            };
            n.last = Some(sample);
            if sample.enabled != 0 {
                return Err(Error::Readback);
            }
            if sample.status & EVENT_MASK != 0 {
                n.event = Some(io.shared(Shared::Event)?);
            }
            if sample.status & RESPONSE_MASK != 0 {
                n.response = Some(io.shared(Shared::Response)?);
            }
            Ok(())
        })();
        if let Err(error) = result {
            n.error = error;
        }
        n
    }
}
impl Default for ArmOnce {
    fn default() -> Self {
        Self::new()
    }
}
