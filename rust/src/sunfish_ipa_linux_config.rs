//! Primitive IPA v4.2 configuration from Linux 2267e248204e7e051e0f7355298949a6440de579.
//!
//! Implements exactly ipa_hardware_config(), in source order, after the native
//! active power floors. This is only the first part of ipa_config(): no local
//! memory/canaries, interrupt enable, endpoint/resource/table configuration,
//! SMMU, PAS, GSI commands or DMA. It never creates an IPA Ready capability.
//! Every error consumes the attempt; partial writes remain until cold reboot.
#![forbid(unsafe_code)]

pub const COUNT: usize = 9;
pub const COMPLETE: u16 = (1 << COUNT) - 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub enum Register {
    Bcr,
    Tx,
    Clkon,
    Comp,
    QsbWrites,
    QsbReads,
    Counter,
    Hash,
    Idle,
}
impl Register {
    pub const fn offset(self) -> usize {
        PROFILE[self as usize].offset
    }
    pub const fn label(self) -> &'static str {
        match self {
            Self::Bcr => "BCR",
            Self::Tx => "TX CFG",
            Self::Clkon => "CLKON",
            Self::Comp => "COMP CFG",
            Self::QsbWrites => "QSB WRITES",
            Self::QsbReads => "QSB READS",
            Self::Counter => "COUNTER",
            Self::Hash => "HASH ENABLE",
            Self::Idle => "IDLE CFG",
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Spec {
    pub register: Register,
    pub offset: usize,
    pub clear: u32,
    pub set: u32,
}
// Direct writes deliberately replace the whole word, as in Linux; only TX and
// COMP are read-modify-write. Do not silently preserve fields Linux replaces.
pub const PROFILE: [Spec; COUNT] = [
    Spec {
        register: Register::Bcr,
        offset: 0x1d0,
        clear: 0xffffffff,
        set: 0x00000000,
    },
    Spec {
        register: Register::Tx,
        offset: 0x1fc,
        clear: 0x00001000,
        set: 0x00000000,
    },
    Spec {
        register: Register::Clkon,
        offset: 0x044,
        clear: 0xffffffff,
        set: 0x30000000,
    },
    Spec {
        register: Register::Comp,
        offset: 0x03c,
        clear: 0x00000000,
        set: 0x00008180,
    },
    Spec {
        register: Register::QsbWrites,
        offset: 0x074,
        clear: 0xffffffff,
        set: 0x00000008,
    },
    Spec {
        register: Register::QsbReads,
        offset: 0x078,
        clear: 0xffffffff,
        set: 0x0000000c,
    },
    Spec {
        register: Register::Counter,
        offset: 0x1f0,
        clear: 0xffffffff,
        set: 0x000000f0,
    },
    Spec {
        register: Register::Hash,
        offset: 0x148,
        clear: 0xffffffff,
        set: 0x00000000,
    },
    Spec {
        register: Register::Idle,
        offset: 0x240,
        clear: 0xffffffff,
        set: 0x00000100,
    },
];
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Read {
    Config(Register),
    Flavor,
    SharedMemory,
    Aggregation,
    IrqEnable,
    GsiStatus,
    GsiParam2,
}
impl Read {
    pub const fn is_gsi(self) -> bool {
        matches!(self, Self::GsiStatus | Self::GsiParam2)
    }
    pub const fn offset(self) -> usize {
        match self {
            Self::Config(r) => r.offset(),
            Self::Flavor => 0x210,
            Self::SharedMemory => 0x54,
            Self::Aggregation => 0xb4,
            Self::IrqEnable => 0x300c,
            Self::GsiStatus => 0x1f000,
            Self::GsiParam2 => 0x1f040,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    AlreadyStarted,
    PowerNotComplete,
    Io,
    BaselineChanged,
    UnexpectedHardware,
    ActiveHardware,
    Changed,
    Readback,
}
/// Only this coordinator can construct a write request. The native adapter
/// receives a closed register enum; callers cannot supply arbitrary offsets.
pub struct Write {
    register: Register,
    value: u32,
}
impl Write {
    pub const fn register(&self) -> Register {
        self.register
    }
    pub const fn value(&self) -> u32 {
        self.value
    }
}
pub trait Io {
    fn read(&mut self, register: Read) -> Result<u32, Error>;
    fn write(&mut self, request: &Write) -> Result<(), Error>;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Snapshot {
    pub config: [u32; COUNT],
    pub guards: [u32; 6],
}
const GUARDS: [Read; 6] = [
    Read::Flavor,
    Read::SharedMemory,
    Read::Aggregation,
    Read::IrqEnable,
    Read::GsiStatus,
    Read::GsiParam2,
];
fn capture(io: &mut impl Io) -> Result<Snapshot, Error> {
    let mut snapshot = Snapshot {
        config: [0; COUNT],
        guards: [0; 6],
    };
    for (i, s) in PROFILE.into_iter().enumerate() {
        snapshot.config[i] = io.read(Read::Config(s.register))?;
    }
    for (i, r) in GUARDS.into_iter().enumerate() {
        snapshot.guards[i] = io.read(r)?;
    }
    Ok(snapshot)
}
fn validate(s: Snapshot) -> Result<(), Error> {
    // Bind this experiment to the measured cold Sunfish configuration. MAX_PIPES
    // is not decoded: its low-nibble definition in the pinned table does not
    // describe the measured 17-pipe total; producer/consumer fields are unchanged.
    if s.guards[0] != 0x08090811 || s.guards[1] != 0x00000415 || s.guards[5] != 0xe4046b59 {
        return Err(Error::UnexpectedHardware);
    }
    if s.guards[2] != 0 || s.guards[3] != 0 || s.guards[4] != 0 {
        return Err(Error::ActiveHardware);
    }
    Ok(())
}
/// Read-only comparison after another startup stage. Both complete samples
/// must still equal the successful configuration report, including idle GSI,
/// disabled IPA interrupts and inactive aggregation. This grants no authority
/// and never replays a configuration write on mismatch or I/O failure.
pub fn verify_retained(io: &mut impl Io, expected: Snapshot) -> Result<(), Error> {
    verify_retained_uc_irq(io, expected, 0)
}
/// Read-only comparison during the bounded UC IRQ arm stage. The original
/// primitive receipt must still describe disabled interrupts; only 0, UC_0
/// (bit 2), or UC_0|UC_1 (bits 2/3) may replace that single guard word. This
/// diagnostic comparison grants no interrupt or hardware-start authority.
pub fn verify_retained_uc_irq(
    io: &mut impl Io,
    mut expected: Snapshot,
    enabled: u32,
) -> Result<(), Error> {
    validate(expected)?;
    if !matches!(enabled, 0 | 4 | 12) {
        return Err(Error::ActiveHardware);
    }
    expected.guards[3] = enabled;
    for _ in 0..2 {
        if capture(io)? != expected {
            return Err(Error::Changed);
        }
    }
    Ok(())
}
/// Diagnostic comparison after a real retained PAS-auth session. Linux's
/// gsi_setup() requires GSI_STATUS.ENABLED (bit 0); no GSI command has been
/// issued at this stage. Permit only that source-defined status transition,
/// preserving every other primitive/guard word and the UC interrupt enables.
/// The native caller must independently require its retained Loader's actual
/// AuthReturned session; this read-only check grants no firmware/DMA permit.
pub fn verify_retained_after_auth(io: &mut impl Io, mut cold: Snapshot) -> Result<(), Error> {
    validate(cold)?;
    cold.guards[3] = 12;
    cold.guards[4] = 1;
    for _ in 0..2 {
        if capture(io)? != cold {
            return Err(Error::Changed);
        }
    }
    Ok(())
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Report {
    pub submitted: u16,
    pub completed: u16,
    pub failed: Option<Register>,
    pub before: Option<Snapshot>,
    pub after: Option<Snapshot>,
    pub expected: [u32; COUNT],
    pub error: Option<Error>,
}
pub struct ConfigureOnce {
    started: bool,
}
impl ConfigureOnce {
    pub const fn new() -> Self {
        Self { started: false }
    }
    /// `power_complete` is a checked report condition from the concrete native
    /// power stage, not a production ownership/PM capability or a Ready token.
    pub fn run(
        &mut self,
        io: &mut impl Io,
        power_complete: bool,
        mut progress: impl FnMut(Register, u32),
    ) -> Report {
        let mut report = Report {
            submitted: 0,
            completed: 0,
            failed: None,
            before: None,
            after: None,
            expected: [0; COUNT],
            error: None,
        };
        if self.started {
            report.error = Some(Error::AlreadyStarted);
            return report;
        }
        self.started = true;
        let result = (|| {
            if !power_complete {
                return Err(Error::PowerNotComplete);
            }
            let mut expected = capture(io)?;
            report.before = Some(expected);
            report.after = Some(expected);
            let second = capture(io)?;
            report.after = Some(second);
            if expected != second {
                return Err(Error::BaselineChanged);
            }
            validate(expected)?;
            report.expected =
                core::array::from_fn(|i| (expected.config[i] & !PROFILE[i].clear) | PROFILE[i].set);
            for (i, spec) in PROFILE.into_iter().enumerate() {
                report.failed = Some(spec.register);
                progress(spec.register, report.expected[i]);
                let before = capture(io)?;
                report.after = Some(before);
                if before != expected {
                    return Err(Error::Changed);
                }
                let request = Write {
                    register: spec.register,
                    value: report.expected[i],
                };
                report.submitted |= 1 << i; // Even an uncertain write is never retried.
                io.write(&request)?;
                expected.config[i] = request.value;
                let after = capture(io)?;
                report.after = Some(after);
                if after != expected {
                    return Err(Error::Readback);
                }
                report.completed |= 1 << i;
            }
            let final_sample = capture(io)?;
            report.after = Some(final_sample);
            if final_sample != expected {
                return Err(Error::Readback);
            }
            report.failed = None;
            Ok(())
        })();
        report.error = result.err();
        report
    }
}
impl Default for ConfigureOnce {
    fn default() -> Self {
        Self::new()
    }
}
