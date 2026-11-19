//! Bounded canary portion of the working Linux v4.2 ipa_mem_config().
//!
//! Only the 25 source-defined canary words in the 8-KiB ipa-shared aperture
//! are writable through this coordinator. This does not implement the DMA
//! zero-buffer allocator, IMEM/SMEM mappings or ipa_mem_setup() commands and
//! cannot create an IPA/DMA Ready capability. Partial writes remain retained
//! until cold reboot; errors consume the attempt without retry or rollback.
#![forbid(unsafe_code)]

use crate::sunfish_ipa_linux_config as config;

pub const SHARED_PHYSICAL: u64 = 0x01e47000;
pub const SHARED_BYTES: usize = 0x2000;
pub const CANARY_VALUE: u32 = 0xdeadbeef;
pub const WORD_COUNT: usize = 25;
pub const COMPLETE: u32 = (1 << WORD_COUNT) - 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Region {
    pub name: &'static str,
    pub offset: usize,
    pub size: usize,
    pub canary_count: usize,
}

pub const REGIONS: [Region; 17] = [
    Region {
        name: "UC_SHARED",
        offset: 0x0000,
        size: 0x0080,
        canary_count: 0,
    },
    Region {
        name: "UC_INFO",
        offset: 0x0080,
        size: 0x0200,
        canary_count: 0,
    },
    Region {
        name: "V4_FILTER_HASHED",
        offset: 0x0288,
        size: 0x0000,
        canary_count: 2,
    },
    Region {
        name: "V4_FILTER",
        offset: 0x0290,
        size: 0x0078,
        canary_count: 2,
    },
    Region {
        name: "V6_FILTER_HASHED",
        offset: 0x0310,
        size: 0x0000,
        canary_count: 2,
    },
    Region {
        name: "V6_FILTER",
        offset: 0x0318,
        size: 0x0078,
        canary_count: 2,
    },
    Region {
        name: "V4_ROUTE_HASHED",
        offset: 0x0398,
        size: 0x0000,
        canary_count: 2,
    },
    Region {
        name: "V4_ROUTE",
        offset: 0x03a0,
        size: 0x0078,
        canary_count: 2,
    },
    Region {
        name: "V6_ROUTE_HASHED",
        offset: 0x0420,
        size: 0x0000,
        canary_count: 2,
    },
    Region {
        name: "V6_ROUTE",
        offset: 0x0428,
        size: 0x0078,
        canary_count: 2,
    },
    Region {
        name: "MODEM_HEADER",
        offset: 0x04a8,
        size: 0x0140,
        canary_count: 2,
    },
    Region {
        name: "MODEM_PROC_CTX",
        offset: 0x05f0,
        size: 0x0200,
        canary_count: 2,
    },
    Region {
        name: "AP_PROC_CTX",
        offset: 0x07f0,
        size: 0x0200,
        canary_count: 0,
    },
    Region {
        name: "PDN_CONFIG",
        offset: 0x09f8,
        size: 0x0050,
        canary_count: 2,
    },
    Region {
        name: "STATS_QUOTA_MODEM",
        offset: 0x0a50,
        size: 0x0060,
        canary_count: 2,
    },
    Region {
        name: "STATS_TETHERING",
        offset: 0x0ab0,
        size: 0x0140,
        canary_count: 0,
    },
    Region {
        name: "MODEM",
        offset: 0x0bf0,
        size: 0x140c,
        canary_count: 0,
    },
];
// Linux's final pseudo-region is kept separately so its canary remains in
// range even though the region itself has zero size at the aperture end.
pub const END_MARKER: Region = Region {
    name: "END_MARKER",
    offset: 0x2000,
    size: 0,
    canary_count: 1,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Geometry {
    pub offset: usize,
    pub reported_bytes: usize,
    pub usable_bytes: usize,
}
pub fn geometry(shared_mem_size: u32) -> Result<Geometry, Error> {
    // Bind writes to the measured cold Sunfish profile. Linux reports 0x20a8
    // bytes, larger than the DT's mapped 0x2000; never enlarge that aperture.
    if shared_mem_size != 0x415 {
        return Err(Error::Geometry);
    }
    let offset = ((shared_mem_size >> 16) as usize) * 8;
    let reported_bytes = ((shared_mem_size & 0xffff) as usize) * 8;
    let usable_bytes = reported_bytes.min(SHARED_BYTES.checked_sub(offset).ok_or(Error::Geometry)?);
    for region in REGIONS.into_iter().chain([END_MARKER]) {
        if region.offset % 8 != 0
            || region.offset < region.canary_count * 4
            || region
                .offset
                .checked_add(region.size)
                .ok_or(Error::Geometry)?
                > usable_bytes
        {
            return Err(Error::Geometry);
        }
    }
    Ok(Geometry {
        offset,
        reported_bytes,
        usable_bytes,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CanaryWord {
    index: usize,
    offset: usize,
}
impl CanaryWord {
    pub const fn index(self) -> usize {
        self.index
    }
    pub const fn offset(self) -> usize {
        self.offset
    }
    pub const fn value(self) -> u32 {
        CANARY_VALUE
    }
}
pub fn words() -> [CanaryWord; WORD_COUNT] {
    let mut words = [CanaryWord {
        index: 0,
        offset: 0,
    }; WORD_COUNT];
    let mut index = 0;
    for region in REGIONS.into_iter().chain([END_MARKER]) {
        // Linux starts at the region base and pre-decrements once per word.
        for word in 1..=region.canary_count {
            words[index] = CanaryWord {
                index,
                offset: region.offset - word * 4,
            };
            index += 1;
        }
    }
    assert_eq!(index, WORD_COUNT);
    words
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    AlreadyStarted,
    Domain,
    Geometry,
    Config(config::Error),
    Io,
    Readback,
}
impl Error {
    pub const fn code(self) -> u32 {
        match self {
            Self::AlreadyStarted => 1,
            Self::Domain => 2,
            Self::Geometry => 3,
            Self::Config(_) => 4,
            Self::Io => 5,
            Self::Readback => 6,
        }
    }
}
pub trait Io: config::Io {
    /// Verify the retained real domain and its current global/context fault
    /// status. This experiment does not accept a production Ready substitute.
    fn domain_quiet(&mut self) -> Result<(), Error>;
    fn read_canary(&mut self, word: CanaryWord) -> Result<u32, Error>;
    fn write_canary(&mut self, word: CanaryWord) -> Result<(), Error>;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Report {
    pub submitted: u32,
    pub completed: u32,
    pub failed: Option<CanaryWord>,
    pub last_read: u32,
    pub verified_words: usize,
    pub geometry: Option<Geometry>,
    pub error: Option<Error>,
}
pub struct CanariesOnce {
    started: bool,
}
impl CanariesOnce {
    pub const fn new() -> Self {
        Self { started: false }
    }
    pub fn run(
        &mut self,
        io: &mut impl Io,
        expected: config::Snapshot,
        mut progress: impl FnMut(CanaryWord),
    ) -> Report {
        let mut report = Report {
            submitted: 0,
            completed: 0,
            failed: None,
            last_read: 0,
            verified_words: 0,
            geometry: None,
            error: None,
        };
        if self.started {
            report.error = Some(Error::AlreadyStarted);
            return report;
        }
        self.started = true;
        let result = (|| {
            let layout = geometry(expected.guards[1])?;
            report.geometry = Some(layout);
            for word in words() {
                report.failed = Some(word);
                io.domain_quiet()?;
                config::verify_retained(io, expected).map_err(Error::Config)?;
                progress(word);
                report.submitted |= 1 << word.index;
                io.write_canary(word)?;
                report.last_read = io.read_canary(word)?;
                if report.last_read != CANARY_VALUE {
                    return Err(Error::Readback);
                }
                report.completed |= 1 << word.index;
            }
            // Two full canary samples detect a changed earlier word as well
            // as a non-sticking final write. No region payload is cleared.
            for _ in 0..2 {
                for word in words() {
                    report.failed = Some(word);
                    report.last_read = io.read_canary(word)?;
                    if report.last_read != CANARY_VALUE {
                        return Err(Error::Readback);
                    }
                }
                io.domain_quiet()?;
                config::verify_retained(io, expected).map_err(Error::Config)?;
            }
            report.verified_words = WORD_COUNT;
            report.failed = None;
            Ok(())
        })();
        report.error = result.err();
        report
    }
}
impl Default for CanariesOnce {
    fn default() -> Self {
        Self::new()
    }
}
