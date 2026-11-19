//! IPA v4.2 endpoint census and resource limits from Linux 2267e248204e7e051e0f7355298949a6440de579.
//!
//! This is a pre-firmware subset of ipa_config(), with interrupts kept disabled.
//! ipa_endpoint_config() only validates availability; ipa_table_config() does
//! no writes on v4.2 because hashing is unsupported. The seven resource writes
//! do not program endpoints, enable interrupts or submit GSI/DMA commands.
//! Every failure consumes the attempt; partial configuration stays until reboot.
//! Neither the topology nor a successful report grants production readiness.
#![forbid(unsafe_code)]

pub const COUNT: usize = 7;
pub const COMPLETE: u8 = (1 << COUNT) - 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Endpoint {
    pub id: u8,
    pub toward_ipa: bool,
}
pub const ENDPOINTS: [Endpoint; 8] = [
    Endpoint {
        id: 6,
        toward_ipa: true,
    },
    Endpoint {
        id: 8,
        toward_ipa: false,
    },
    Endpoint {
        id: 1,
        toward_ipa: true,
    },
    Endpoint {
        id: 9,
        toward_ipa: false,
    },
    Endpoint {
        id: 5,
        toward_ipa: true,
    },
    Endpoint {
        id: 11,
        toward_ipa: false,
    },
    Endpoint {
        id: 4,
        toward_ipa: true,
    },
    Endpoint {
        id: 10,
        toward_ipa: false,
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Topology {
    pub tx_count: u8,
    pub rx_count: u8,
    pub rx_base: u8,
    pub available: u32,
    pub defined: u32,
}
/// Match the pinned driver's producer/consumer decoding, not the unrelated
/// low-nibble MAX_PIPES field. No hardware or ownership is inferred here.
pub fn topology(flavor: u32) -> Result<Topology, Error> {
    let tx_count = ((flavor >> 8) & 0x1f) as u8;
    let rx_count = ((flavor >> 16) & 0x1f) as u8;
    let rx_base = ((flavor >> 24) & 0x0f) as u8;
    let limit = u32::from(rx_base) + u32::from(rx_count);
    if limit > 32 || u32::from(tx_count) > limit {
        return Err(Error::Topology);
    }
    let low_mask = |count: u8| ((1u64 << count) - 1) as u32;
    let available = low_mask(tx_count) | (low_mask(rx_count) << rx_base);
    let mut defined = 0;
    for ep in ENDPOINTS {
        if u32::from(ep.id) >= limit
            || available & (1 << ep.id) == 0
            || (ep.toward_ipa && ep.id >= tx_count)
            || (!ep.toward_ipa && ep.id < rx_base)
        {
            return Err(Error::Topology);
        }
        defined |= 1 << ep.id;
    }
    Ok(Topology {
        tx_count,
        rx_count,
        rx_base,
        available,
        defined,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub enum Register {
    SourceContexts,
    SourceLists,
    SourceBuffers,
    SourceDmars,
    SourceAcks,
    DestinationData,
    DestinationDmars,
}
impl Register {
    pub const fn offset(self) -> usize {
        PROFILE[self as usize].offset
    }
    pub const fn label(self) -> &'static str {
        match self {
            Self::SourceContexts => "SRC CONTEXTS",
            Self::SourceLists => "SRC LISTS",
            Self::SourceBuffers => "SRC BUFFERS",
            Self::SourceDmars => "SRC DMARS",
            Self::SourceAcks => "SRC ACKS",
            Self::DestinationData => "DST DATA",
            Self::DestinationDmars => "DST DMARS",
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Spec {
    pub register: Register,
    pub offset: usize,
    pub value: u32,
}
// Exactly one source and one destination group. Group Y and reserved fields
// are zero in Linux's full-word writes; no 23/45/67 group register is touched.
pub const PROFILE: [Spec; COUNT] = [
    Spec {
        register: Register::SourceContexts,
        offset: 0x400,
        value: 0x3f03,
    },
    Spec {
        register: Register::SourceLists,
        offset: 0x420,
        value: 0x0303,
    },
    Spec {
        register: Register::SourceBuffers,
        offset: 0x440,
        value: 0x0a0a,
    },
    Spec {
        register: Register::SourceDmars,
        offset: 0x460,
        value: 0x0101,
    },
    Spec {
        register: Register::SourceAcks,
        offset: 0x480,
        value: 0x0505,
    },
    Spec {
        register: Register::DestinationData,
        offset: 0x500,
        value: 0x0303,
    },
    Spec {
        register: Register::DestinationDmars,
        offset: 0x520,
        value: 0x3f01,
    },
];
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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    AlreadyStarted,
    Topology,
    Domain,
    Config,
    Buffers,
    Io,
    BaselineChanged,
    Changed,
    Readback,
}
impl Error {
    pub const fn code(self) -> u32 {
        match self {
            Self::AlreadyStarted => 1,
            Self::Topology => 2,
            Self::Domain => 3,
            Self::Config => 4,
            Self::Buffers => 5,
            Self::Io => 6,
            Self::BaselineChanged => 7,
            Self::Changed => 8,
            Self::Readback => 9,
        }
    }
}
pub trait Io {
    /// Concrete adapter checks the retained SMMU/fault state, prepared pool
    /// and two unchanged primitive IPA/GSI snapshots before every sample.
    fn guard(&mut self) -> Result<(), Error>;
    fn read(&mut self, register: Register) -> Result<u32, Error>;
    fn write(&mut self, request: &Write) -> Result<(), Error>;
}
fn capture(io: &mut impl Io) -> Result<[u32; COUNT], Error> {
    io.guard()?;
    let mut values = [0; COUNT];
    for spec in PROFILE {
        values[spec.register as usize] = io.read(spec.register)?;
    }
    Ok(values)
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Report {
    pub submitted: u8,
    pub completed: u8,
    pub failed: Option<Register>,
    pub topology: Option<Topology>,
    pub before: Option<[u32; COUNT]>,
    pub after: Option<[u32; COUNT]>,
    pub error: Option<Error>,
}
pub struct ConfigureOnce {
    started: bool,
}
impl ConfigureOnce {
    pub const fn new() -> Self {
        Self { started: false }
    }
    pub fn run(
        &mut self,
        io: &mut impl Io,
        flavor: u32,
        mut progress: impl FnMut(Register, u32),
    ) -> Report {
        let mut report = Report {
            submitted: 0,
            completed: 0,
            failed: None,
            topology: None,
            before: None,
            after: None,
            error: None,
        };
        if self.started {
            report.error = Some(Error::AlreadyStarted);
            return report;
        }
        self.started = true;
        let result = (|| {
            report.topology = Some(topology(flavor)?);
            let mut expected = capture(io)?;
            report.before = Some(expected);
            report.after = Some(expected);
            let second = capture(io)?;
            report.after = Some(second);
            if second != expected {
                return Err(Error::BaselineChanged);
            }
            for (i, spec) in PROFILE.into_iter().enumerate() {
                report.failed = Some(spec.register);
                progress(spec.register, spec.value);
                let before = capture(io)?;
                report.after = Some(before);
                if before != expected {
                    return Err(Error::Changed);
                }
                report.submitted |= 1 << i;
                io.write(&Write {
                    register: spec.register,
                    value: spec.value,
                })?;
                expected[i] = spec.value;
                let after = capture(io)?;
                report.after = Some(after);
                if after != expected {
                    return Err(Error::Readback);
                }
                report.completed |= 1 << i;
            }
            for _ in 0..2 {
                let final_sample = capture(io)?;
                report.after = Some(final_sample);
                if final_sample != expected {
                    return Err(Error::Readback);
                }
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
