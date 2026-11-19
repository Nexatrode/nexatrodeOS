//! Bounded Sunfish Apps-DRV2 resource reads, using the matched Lineage RSC ABI.
//!
//! This is a controller transaction, not a read-only MMIO snapshot: it writes
//! active TCS0 and its interrupt bit. Public read constructors always clear
//! the ePCB WRITE bit. Crate-private fixed CX/MSS TURBO and XO-on commands
//! share the engine with the separate power-test coordinator. A distinct BCM
//! facade permits READs of six observed fixed bus resources. No arbitrary address/data,
//! sleep/wake access, bank fallback, retry, cancellation, or DMA is supplied.
//! The caller must independently establish exclusive standalone AP ownership;
//! an idle status register cannot establish that ownership.

pub const OBSERVED_CONFIG: u32 = 0x8000_8104;
pub const CX_ADDRESS: u32 = 0x0003_0000;
pub const READ_MESSAGE: u32 = 8 | (1 << 8); // Response requested, WRITE clear.
pub const CX_TURBO_SELECTOR: u32 = 6;
pub const MSS_TURBO_SELECTOR: u32 = 7;
pub const XO_ON_VALUE: u32 = 3;
/// Reassert only the native MC0 read fields, adding the BCM commit bit.
pub const MC0_OBSERVED_READ: u32 = 0x2991_7f75;
pub const MC0_REASSERT_WORD: u32 = MC0_OBSERVED_READ | (1 << 30);
/// Fixed PAS-route floor experiment: retain observed X, increase only SN Y.
pub const SN_PAS_FLOOR_WORD: u32 = 0x613c_5d02;
/// The reference CE0 arithmetic exceeds both 14-bit fields; saturation is
/// explicit and is not proof of sufficient bandwidth for a secure operation.
pub const CE0_PAS_FLOOR_WORD: u32 = 0x6fff_ffff;
const MODE: u32 = 1 << 16;
const TRIGGER: u32 = 1 << 24;
const ISSUED_COMPLETE: u32 = (1 << 8) | (1 << 16);

#[derive(Clone, Copy)]
pub(crate) enum Command {
    Read(ReadResource),
    ReadBcm(BcmResource),
    ReadBcmTarget(BcmTarget),
    CxTurbo,
    MssTurbo,
    XoOn,
    Mc0Reassert,
    Sn11PasFloor,
    Sn0PasFloor,
    Ce0PasFloor,
    /// Only the source-matched Linux IPA startup coordinator constructs this
    /// crate-private command, after validating CMD_DB and both baseline reads.
    #[allow(dead_code)] // Some standalone protocol harnesses omit the coordinator.
    IpaLinuxFloor {
        target: BcmTarget,
        data: u32,
    },
    /// Source-derived mainline SCM crypto-to-EBI cold hold, never an unvote.
    #[allow(dead_code)]
    ScmLinuxFloor {
        resource: BcmResource,
        data: u32,
    },
    /// Selected MPSS cold hold; coordinator validates ARC auxiliary levels.
    #[allow(dead_code)]
    MpssLinuxFloor {
        resource: ReadResource,
        data: u32,
    },
}

impl Command {
    fn address(self) -> u32 {
        match self {
            Self::Read(resource) => resource.address(),
            Self::ReadBcm(resource) => resource.address(),
            Self::ReadBcmTarget(target) => target.address(),
            Self::CxTurbo => CX_ADDRESS,
            Self::MssTurbo => ReadResource::Mss.address(),
            Self::XoOn => ReadResource::Xo.address(),
            Self::Mc0Reassert => BcmResource::Mc0.address(),
            Self::Sn11PasFloor => BcmResource::Sn11.address(),
            Self::Sn0PasFloor => BcmResource::Sn0.address(),
            Self::Ce0PasFloor => BcmResource::Ce0.address(),
            Self::IpaLinuxFloor { target, .. } => target.address(),
            Self::ScmLinuxFloor { resource, .. } => resource.address(),
            Self::MpssLinuxFloor { resource, .. } => resource.address(),
        }
    }
    fn message(self) -> u32 {
        READ_MESSAGE
            | match self {
                Self::Read(_) | Self::ReadBcm(_) | Self::ReadBcmTarget(_) => 0,
                _ => 1 << 16,
            }
    }
    fn data(self) -> u32 {
        match self {
            Self::Read(_) | Self::ReadBcm(_) | Self::ReadBcmTarget(_) => 0,
            Self::CxTurbo => CX_TURBO_SELECTOR,
            Self::MssTurbo => MSS_TURBO_SELECTOR,
            Self::XoOn => XO_ON_VALUE,
            Self::Mc0Reassert => MC0_REASSERT_WORD,
            Self::Sn11PasFloor | Self::Sn0PasFloor => SN_PAS_FLOOR_WORD,
            Self::Ce0PasFloor => CE0_PAS_FLOOR_WORD,
            Self::IpaLinuxFloor { data, .. } => data,
            Self::ScmLinuxFloor { data, .. } => data,
            Self::MpssLinuxFloor { data, .. } => data,
        }
    }
    fn wait(self) -> u32 {
        // Match the reference bus command's complete/commit flag. The bit
        // is cleared by checked cleanup; all old transactions still use 0.
        match self {
            Self::Mc0Reassert
            | Self::Sn11PasFloor
            | Self::Sn0PasFloor
            | Self::Ce0PasFloor
            | Self::IpaLinuxFloor { .. } => 1,
            Self::ScmLinuxFloor { .. } => 1,
            _ => 0,
        }
    }
}

/// Only the four command-DB addresses confirmed by the resource-read photo.
/// No caller-supplied raw address or resource WRITE is supported.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadResource {
    Cx,
    Mss,
    Mx,
    Xo,
}

impl ReadResource {
    pub const ALL: [Self; 4] = [Self::Cx, Self::Mss, Self::Mx, Self::Xo];

    pub const fn address(self) -> u32 {
        match self {
            Self::Cx => CX_ADDRESS,
            Self::Mss => 0x0003_0060,
            Self::Mx => 0x0003_0010,
            Self::Xo => 0x0003_0090,
        }
    }

    pub const fn index(self) -> usize {
        match self {
            Self::Cx => 0,
            Self::Mss => 1,
            Self::Mx => 2,
            Self::Xo => 3,
        }
    }
}

/// Only the six BCM addresses observed in photo D2520B77. This does not
/// expose a bus WRITE or interpret a response as a previous local vote.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BcmResource {
    Mc0,
    Acv,
    Ce0,
    Sn11,
    Sn0,
    Sh0,
}

impl BcmResource {
    pub const ALL: [Self; 6] = [
        Self::Mc0,
        Self::Acv,
        Self::Ce0,
        Self::Sn11,
        Self::Sn0,
        Self::Sh0,
    ];
    pub const fn address(self) -> u32 {
        match self {
            Self::Mc0 => 0x50000,
            Self::Acv => 0x5000c,
            Self::Ce0 => 0x500a0,
            Self::Sn11 => 0x5006c,
            Self::Sn0 => 0x50078,
            Self::Sh0 => 0x5003c,
        }
    }
    pub const fn index(self) -> usize {
        match self {
            Self::Mc0 => 0,
            Self::Acv => 1,
            Self::Ce0 => 2,
            Self::Sn11 => 3,
            Self::Sn0 => 4,
            Self::Sh0 => 5,
        }
    }
}

/// BCM resources present on the four source-validated IPA routes. These names
/// select CMD_DB entries; they are not a vote or an ownership claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BcmTargetName {
    Sn11,
    Sn0,
    Sh0,
    Mc0,
    Acv,
    Sn1,
    Sh5,
    Sh2,
    Sn15,
    Cn0,
    Ip0,
}

impl BcmTargetName {
    pub const IPA_PATHS: [Self; 11] = [
        Self::Sn11,
        Self::Sn0,
        Self::Sh0,
        Self::Mc0,
        Self::Acv,
        Self::Sn1,
        Self::Sh5,
        Self::Sh2,
        Self::Sn15,
        Self::Cn0,
        Self::Ip0,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sn11 => "SN11",
            Self::Sn0 => "SN0",
            Self::Sh0 => "SH0",
            Self::Mc0 => "MC0",
            Self::Acv => "ACV",
            Self::Sn1 => "SN1",
            Self::Sh5 => "SH5",
            Self::Sh2 => "SH2",
            Self::Sn15 => "SN15",
            Self::Cn0 => "CN0",
            Self::Ip0 => "IP0",
        }
    }
}

/// Validated address from one of the selected live CMD_DB BCM records.
/// The constructor only permits aligned addresses on RPMh slave 5.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BcmTarget {
    name: BcmTargetName,
    address: u32,
}

impl BcmTarget {
    pub fn new(name: BcmTargetName, address: u32) -> Result<Self, Error> {
        if address & 3 != 0 || (address >> 16) & 7 != 5 {
            return Err(Error::InvalidTarget);
        }
        Ok(Self { name, address })
    }

    pub const fn name(self) -> BcmTargetName {
        self.name
    }

    pub const fn address(self) -> u32 {
        self.address
    }
}

/// Offsets within the first 4 KiB of Apps RSC at physical 0x18220000.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Register {
    Config,
    IrqEnable,
    IrqStatus,
    IrqClear,
    Wait,
    Control,
    Idle,
    Enable,
    Message,
    Address,
    Data,
    CommandStatus,
    Response,
    OtherIdle,
    OtherEnable,
    OtherWait,
    OtherControl,
}

impl Register {
    pub const fn offset(self) -> usize {
        match self {
            Self::Config => 0x00c,
            Self::IrqEnable => 0xd00,
            Self::IrqStatus => 0xd04,
            Self::IrqClear => 0xd08,
            Self::Wait => 0xd10,
            Self::Control => 0xd14,
            Self::Idle => 0xd18,
            Self::Enable => 0xd1c,
            Self::Message => 0xd30,
            Self::Address => 0xd34,
            Self::Data => 0xd38,
            Self::CommandStatus => 0xd3c,
            Self::Response => 0xd40,
            Self::OtherWait => 0xfb0,
            Self::OtherControl => 0xfb4,
            Self::OtherIdle => 0xfb8,
            Self::OtherEnable => 0xfbc,
        }
    }
}

/// Each access must be volatile and ordered against preceding device accesses.
pub trait RegisterIo {
    fn read(&mut self, register: Register) -> u32;
    fn write(&mut self, register: Register, value: u32);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Geometry,
    Handoff,
    Readback,
    PollLimit,
    Completion,
    Cleanup,
    WrongStage,
    InvalidTarget,
}

impl Error {
    pub const fn code(self) -> u32 {
        match self {
            Self::Geometry => 1,
            Self::Handoff => 2,
            Self::Readback => 3,
            Self::PollLimit => 4,
            Self::Completion => 5,
            Self::Cleanup => 6,
            Self::WrongStage => 7,
            Self::InvalidTarget => 8,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stage {
    New,
    InFlight,
    Completed,
    Failed,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Snapshot {
    pub config: u32,
    pub irq_enable: u32,
    pub irq_status: u32,
    pub idle: u32,
    pub enable: u32,
    pub wait: u32,
    pub control: u32,
    pub command_status: u32,
    pub response: u32,
    pub other_idle: u32,
    pub other_enable: u32,
    pub other_wait: u32,
    pub other_control: u32,
}

impl Snapshot {
    pub fn read(io: &mut impl RegisterIo) -> Self {
        Self {
            config: io.read(Register::Config),
            irq_enable: io.read(Register::IrqEnable),
            irq_status: io.read(Register::IrqStatus),
            idle: io.read(Register::Idle),
            enable: io.read(Register::Enable),
            wait: io.read(Register::Wait),
            control: io.read(Register::Control),
            command_status: io.read(Register::CommandStatus),
            response: io.read(Register::Response),
            other_idle: io.read(Register::OtherIdle),
            other_enable: io.read(Register::OtherEnable),
            other_wait: io.read(Register::OtherWait),
            other_control: io.read(Register::OtherControl),
        }
    }

    fn eligible(self) -> bool {
        self.irq_status == 0
            && self.idle == 1
            && self.other_idle == 1
            && self.enable == 0
            && self.other_enable == 0
            && self.wait == 0
            && self.other_wait == 0
            && self.control & !(MODE | TRIGGER) == 0
            && self.other_control & !(MODE | TRIGGER) == 0
    }
}

pub(crate) struct ActiveTransaction {
    command: Command,
    stage: Stage,
    polls_left: u32,
    initial: Snapshot,
    last: Snapshot,
    response: Option<u32>,
}

/// Preserve the original CX probe entry point. `new()` still selects CX.
pub type CxRead = ResourceRead;

/// The public read facade cannot construct a resource WRITE.
pub struct ResourceRead(ActiveTransaction);

impl ResourceRead {
    pub fn new(polls: u32) -> Self {
        Self::for_resource(ReadResource::Cx, polls)
    }

    pub fn for_resource(resource: ReadResource, polls: u32) -> Self {
        Self(ActiveTransaction::new(Command::Read(resource), polls))
    }
    pub const fn stage(&self) -> Stage {
        self.0.stage()
    }
    pub const fn initial(&self) -> Snapshot {
        self.0.initial()
    }
    pub const fn last(&self) -> Snapshot {
        self.0.last()
    }
    pub const fn response(&self) -> Option<u32> {
        self.0.response()
    }
    pub fn start(&mut self, io: &mut impl RegisterIo) -> Result<(), Error> {
        self.0.start(io)
    }
    pub fn poll(&mut self, io: &mut impl RegisterIo) -> Result<bool, Error> {
        self.0.poll(io)
    }
}

/// Bus READ facade: cannot construct an ePCB resource WRITE.
pub struct BcmRead(ActiveTransaction);
impl BcmRead {
    pub fn new(resource: BcmResource, polls: u32) -> Self {
        Self(ActiveTransaction::new(Command::ReadBcm(resource), polls))
    }
    pub fn for_target(target: BcmTarget, polls: u32) -> Self {
        Self(ActiveTransaction::new(
            Command::ReadBcmTarget(target),
            polls,
        ))
    }
    pub const fn response(&self) -> Option<u32> {
        self.0.response()
    }
    pub const fn last(&self) -> Snapshot {
        self.0.last()
    }
    pub fn start(&mut self, io: &mut impl RegisterIo) -> Result<(), Error> {
        self.0.start(io)
    }
    pub fn poll(&mut self, io: &mut impl RegisterIo) -> Result<bool, Error> {
        self.0.poll(io)
    }
}

impl ActiveTransaction {
    pub(crate) fn new(command: Command, polls: u32) -> Self {
        Self {
            command,
            stage: Stage::New,
            polls_left: polls,
            initial: Snapshot::default(),
            last: Snapshot::default(),
            response: None,
        }
    }

    pub const fn stage(&self) -> Stage {
        self.stage
    }
    pub const fn initial(&self) -> Snapshot {
        self.initial
    }
    pub const fn last(&self) -> Snapshot {
        self.last
    }
    pub const fn response(&self) -> Option<u32> {
        self.response
    }

    fn sync(io: &mut impl RegisterIo, register: Register, value: u32) -> Result<(), Error> {
        io.write(register, value);
        if io.read(register) != value {
            return Err(Error::Readback);
        }
        Ok(())
    }

    /// Issue once after two equal, eligible snapshots. Standalone AP ownership
    /// and DT-selected DRV2/active-TCS0 authority are caller preconditions.
    pub fn start(&mut self, io: &mut impl RegisterIo) -> Result<(), Error> {
        let result = self.start_inner(io);
        if result.is_err() {
            self.stage = Stage::Failed;
        }
        result
    }

    fn start_inner(&mut self, io: &mut impl RegisterIo) -> Result<(), Error> {
        if self.stage != Stage::New {
            return Err(Error::WrongStage);
        }
        self.initial = Snapshot::read(io);
        self.last = self.initial;
        if self.polls_left == 0 {
            return Err(Error::PollLimit);
        }
        if self.initial.config != OBSERVED_CONFIG {
            return Err(Error::Geometry);
        }
        self.last = Snapshot::read(io);
        if self.initial != self.last || !self.last.eligible() {
            return Err(Error::Handoff);
        }

        // One fixed command, command slot 0 only. Do not clear stale IRQs to
        // manufacture an eligible handoff or overwrite any enabled command.
        Self::sync(io, Register::Message, self.command.message())?;
        Self::sync(io, Register::Address, self.command.address())?;
        Self::sync(io, Register::Data, self.command.data())?;
        Self::sync(io, Register::Wait, self.command.wait())?;
        Self::sync(io, Register::Enable, 1)?;
        Self::sync(io, Register::IrqEnable, self.initial.irq_enable | 1)?;
        let control = self.initial.control & !TRIGGER;
        Self::sync(io, Register::Control, control)?;
        Self::sync(io, Register::Control, control & !MODE)?;
        Self::sync(io, Register::Control, MODE)?;
        // Never readback/retry this trigger. Subsequent errors are terminal;
        // leave in-flight state intact for physical reset and diagnosis.
        io.write(Register::Control, MODE | TRIGGER);
        self.stage = Stage::InFlight;
        Ok(())
    }

    /// Poll at most the configured count. A completed response is usable only
    /// after issued+complete, idle, unchanged command, and verified cleanup.
    pub fn poll(&mut self, io: &mut impl RegisterIo) -> Result<bool, Error> {
        let result = self.poll_inner(io);
        if result.is_err() {
            self.stage = Stage::Failed;
        }
        result
    }

    fn poll_inner(&mut self, io: &mut impl RegisterIo) -> Result<bool, Error> {
        if self.stage != Stage::InFlight {
            return Err(Error::WrongStage);
        }
        if self.polls_left == 0 {
            return Err(Error::PollLimit);
        }
        self.polls_left -= 1;
        let irq = io.read(Register::IrqStatus);
        self.last.irq_status = irq;
        if irq & 1 == 0 {
            return Ok(false);
        }
        self.last = Snapshot::read(io);
        if self.last.config != self.initial.config
            || self.last.idle != 1
            || self.last.irq_status & 1 == 0
            || self.last.enable != 1
            || self.last.wait != self.command.wait()
            || self.last.control & !(MODE | TRIGGER) != 0
            || self.last.control & MODE == 0
            || self.last.command_status & ISSUED_COMPLETE != ISSUED_COMPLETE
            || io.read(Register::Message) != self.command.message()
            || io.read(Register::Address) != self.command.address()
            || io.read(Register::Data) != self.command.data()
            || self.last.irq_enable != self.initial.irq_enable | 1
            || self.last.other_idle != self.initial.other_idle
            || self.last.other_enable != self.initial.other_enable
            || self.last.other_wait != self.initial.other_wait
            || self.last.other_control != self.initial.other_control
        {
            return Err(Error::Completion);
        }
        let response = self.last.response;
        // Never restore an inherited high trigger bit: that would retrigger.
        Self::sync(io, Register::Control, self.last.control & !TRIGGER)?;
        Self::sync(io, Register::Control, 0)?;
        Self::sync(io, Register::Enable, 0)?;
        Self::sync(io, Register::Wait, 0)?;
        io.write(Register::IrqClear, 1); // Only the completed active-TCS0 bit.
        Self::sync(io, Register::IrqEnable, self.initial.irq_enable)?;
        self.last = Snapshot::read(io);
        if self.last.irq_status & 1 != 0
            || self.last.idle != 1
            || self.last.enable != 0
            || self.last.wait != 0
            || self.last.control != 0
            || self.last.irq_enable != self.initial.irq_enable
            || self.last.other_idle != self.initial.other_idle
            || self.last.other_enable != self.initial.other_enable
            || self.last.other_wait != self.initial.other_wait
            || self.last.other_control != self.initial.other_control
        {
            return Err(Error::Cleanup);
        }
        self.response = match self.command {
            Command::Read(_) | Command::ReadBcm(_) | Command::ReadBcmTarget(_) => Some(response),
            _ => None,
        };
        self.stage = Stage::Completed;
        Ok(true)
    }
}

/// Two ordered passes over CX/MSS/MX/XO, reusing TCS0 only after cleanup.
/// Responses are opaque and may differ between passes; neither pass identifies
/// the previous AP vote or provides an automatic power rollback value.
#[derive(Clone, Copy, Debug)]
pub struct ReadSeries {
    pub responses: [[Option<u32>; 4]; 2],
    pub completed: u8,
    pub error: Option<Error>,
    pub failed_resource: Option<ReadResource>,
    pub failed_pass: Option<u8>,
    pub before: Snapshot,
    pub after: Snapshot,
}

impl ReadSeries {
    pub fn passed(&self) -> bool {
        self.error.is_none()
            && self.completed == 8
            && self.responses.iter().flatten().all(Option::is_some)
    }
}

/// Finite reads only. Stop at the first fault, retaining verified earlier
/// responses without issuing any later command. `wait` is called between
/// unsuccessful polls; it must return and may not access this controller.
pub fn read_series(
    io: &mut impl RegisterIo,
    polls_per_read: u32,
    mut wait: impl FnMut(),
) -> ReadSeries {
    let mut report = ReadSeries {
        responses: [[None; 4]; 2],
        completed: 0,
        error: None,
        failed_resource: None,
        failed_pass: None,
        before: Snapshot::default(),
        after: Snapshot::default(),
    };
    for pass in 0..2 {
        for resource in ReadResource::ALL {
            let mut read = ResourceRead::for_resource(resource, polls_per_read);
            let result = read.start(io).and_then(|()| {
                loop {
                    if read.poll(io)? {
                        return Ok(());
                    }
                    wait();
                }
            });
            if report.completed == 0 {
                report.before = read.initial();
            }
            if let Err(error) = result {
                report.error = Some(error);
                report.failed_resource = Some(resource);
                report.failed_pass = Some(pass as u8);
                report.after = Snapshot::read(io); // Diagnostic reads only.
                return report;
            }
            report.responses[pass][resource.index()] = read.response();
            report.completed += 1;
            report.after = read.last();
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        words: [u32; 1024],
        writes: std::vec::Vec<(Register, u32)>,
        complete: bool,
        corrupt: Option<Register>,
        trigger_count: usize,
        stop_after: Option<usize>,
        unrelated_irq: bool,
        vary_response: bool,
    }
    impl Fake {
        fn ready() -> Self {
            let mut f = Self {
                words: [0; 1024],
                writes: std::vec::Vec::new(),
                complete: false,
                corrupt: None,
                trigger_count: 0,
                stop_after: None,
                unrelated_irq: true,
                vary_response: false,
            };
            f.set(Register::Config, OBSERVED_CONFIG);
            f.set(Register::Idle, 1);
            f.set(Register::OtherIdle, 1);
            f.set(Register::IrqEnable, 0xa6); // Inherited other bits preserved.
            f.set(Register::Control, MODE | TRIGGER); // A completed old AMC.
            f.set(Register::Response, 0xdeadbeef); // Stale, must not be returned.
            f
        }
        fn set(&mut self, register: Register, value: u32) {
            self.words[register.offset() / 4] = value;
        }
        fn finish(&mut self) {
            self.set(
                Register::IrqStatus,
                1 | if self.unrelated_irq { 4 } else { 0 },
            );
            self.set(Register::CommandStatus, ISSUED_COMPLETE);
            let response = if self.vary_response {
                ((self.words[Register::Address.offset() / 4] & 0xffff) << 8)
                    | self.trigger_count as u32
            } else {
                9
            };
            self.set(Register::Response, response);
            self.set(Register::Idle, 1);
        }
    }
    impl RegisterIo for Fake {
        fn read(&mut self, register: Register) -> u32 {
            self.words[register.offset() / 4]
        }
        fn write(&mut self, register: Register, value: u32) {
            self.writes.push((register, value));
            if self.corrupt == Some(register) {
                return;
            }
            if register == Register::IrqClear {
                self.words[Register::IrqStatus.offset() / 4] &= !value;
            } else {
                self.set(register, value);
            }
            if register == Register::Control && value == MODE | TRIGGER {
                self.trigger_count += 1;
                self.set(Register::Idle, 0);
                if self.complete
                    && self
                        .stop_after
                        .is_none_or(|count| self.trigger_count <= count)
                {
                    self.finish();
                }
            }
        }
    }

    #[test]
    fn one_read_completes_cleans_only_its_bank_and_preserves_other_irq_bits() {
        let mut f = Fake::ready();
        f.complete = true;
        let before = f.words;
        let mut read = CxRead::new(2);
        read.start(&mut f).unwrap();
        assert!(read.poll(&mut f).unwrap());
        assert_eq!(read.stage(), Stage::Completed);
        assert_eq!(read.response(), Some(9));
        assert_eq!(f.read(Register::IrqStatus), 4);
        assert_eq!(f.read(Register::IrqEnable), 0xa6);
        assert_eq!(f.read(Register::Control), 0);
        assert!(f.writes.iter().all(|(r, v)| match r {
            Register::Message => *v == 0x108 && *v & (1 << 16) == 0,
            Register::Address => *v == CX_ADDRESS,
            Register::Data | Register::Wait => *v == 0,
            Register::Enable | Register::IrqClear => *v <= 1,
            Register::IrqEnable => (*v & !1) == (before[Register::IrqEnable.offset() / 4] & !1),
            Register::Control => *v & !(MODE | TRIGGER) == 0,
            _ => false,
        }));
        assert_eq!(
            f.writes
                .iter()
                .filter(|(r, v)| *r == Register::Control && *v & TRIGGER != 0)
                .count(),
            1
        );
        assert_eq!(read.initial().other_control, read.last().other_control);
        assert_eq!(read.poll(&mut f), Err(Error::WrongStage));
        let writes = f.writes.len();
        assert_eq!(read.start(&mut f), Err(Error::WrongStage));
        assert_eq!(f.writes.len(), writes);
    }

    #[test]
    fn refuses_busy_enabled_stale_irq_unknown_control_and_geometry_without_writes() {
        for (r, v) in [
            (Register::Config, OBSERVED_CONFIG ^ 0x1000),
            (Register::Idle, 0),
            (Register::OtherIdle, 0),
            (Register::Enable, 1),
            (Register::OtherEnable, 2),
            (Register::Wait, 1),
            (Register::OtherWait, 1),
            (Register::Control, 2),
            (Register::OtherControl, 2),
            (Register::IrqStatus, 1),
        ] {
            let mut f = Fake::ready();
            f.set(r, v);
            assert!(CxRead::new(2).start(&mut f).is_err(), "{r:?}");
            assert!(f.writes.is_empty(), "{r:?}");
        }
        let mut f = Fake::ready();
        assert_eq!(CxRead::new(0).start(&mut f), Err(Error::PollLimit));
        assert!(f.writes.is_empty());
    }

    #[test]
    fn timeout_is_terminal_without_cleanup_retry_or_stale_response() {
        let mut f = Fake::ready();
        let mut read = CxRead::new(2);
        read.start(&mut f).unwrap();
        let writes = f.writes.len();
        assert_eq!(read.poll(&mut f), Ok(false));
        assert_eq!(read.poll(&mut f), Ok(false));
        assert_eq!(read.poll(&mut f), Err(Error::PollLimit));
        assert_eq!(read.response(), None);
        assert_eq!(read.stage(), Stage::Failed);
        f.finish();
        assert_eq!(read.poll(&mut f), Err(Error::WrongStage));
        assert_eq!(f.writes.len(), writes);
    }

    #[test]
    fn failed_register_readback_never_triggers_or_retries() {
        let mut f = Fake::ready();
        f.corrupt = Some(Register::Message);
        let mut read = CxRead::new(1);
        assert_eq!(read.start(&mut f), Err(Error::Readback));
        assert_eq!(f.writes, [(Register::Message, READ_MESSAGE)]);
        assert_eq!(read.start(&mut f), Err(Error::WrongStage));
        assert_eq!(f.writes.len(), 1);
    }

    #[test]
    fn invalid_completion_never_returns_data_or_cleans_an_uncertain_bank() {
        for (r, v) in [
            (Register::CommandStatus, 1 << 8),
            (Register::Idle, 0),
            (Register::Message, 0x10108),
            (Register::Address, CX_ADDRESS + 4),
            (Register::Data, 1),
            (Register::OtherEnable, 1),
            (Register::IrqEnable, 0),
        ] {
            let mut f = Fake::ready();
            let mut read = CxRead::new(1);
            read.start(&mut f).unwrap();
            f.finish();
            f.set(r, v);
            let writes = f.writes.len();
            assert_eq!(read.poll(&mut f), Err(Error::Completion), "{r:?}");
            assert_eq!(f.writes.len(), writes);
            assert_eq!(read.response(), None);
        }
    }

    #[test]
    fn series_reuses_clean_bank_for_exactly_eight_allowlisted_reads() {
        let mut f = Fake::ready();
        f.complete = true;
        f.unrelated_irq = false;
        f.vary_response = true;
        let report = read_series(&mut f, 2, || {});
        assert!(report.passed());
        assert_eq!(
            report.responses,
            [
                [Some(1), Some(0x6002), Some(0x1003), Some(0x9004)],
                [Some(5), Some(0x6006), Some(0x1007), Some(0x9008)],
            ]
        );
        assert_eq!(f.trigger_count, 8);
        let addresses: std::vec::Vec<u32> = f
            .writes
            .iter()
            .filter_map(|(r, v)| (*r == Register::Address).then_some(*v))
            .collect();
        assert_eq!(
            addresses,
            [
                CX_ADDRESS, 0x30060, 0x30010, 0x30090, CX_ADDRESS, 0x30060, 0x30010, 0x30090
            ]
        );
        assert_eq!(
            f.writes
                .iter()
                .filter(|(r, _)| *r == Register::IrqClear)
                .count(),
            8
        );
        assert_eq!(report.before.irq_enable, report.after.irq_enable);
        assert_eq!(report.after.control, 0);
        assert!(
            f.writes
                .iter()
                .filter(|(r, _)| *r == Register::Message)
                .all(|(_, v)| *v == READ_MESSAGE && *v & (1 << 16) == 0)
        );
    }

    #[test]
    fn series_stops_at_first_timeout_without_later_resources_or_partial_success() {
        for completed in 0..8 {
            let mut f = Fake::ready();
            f.complete = true;
            f.unrelated_irq = false;
            f.stop_after = Some(completed);
            let report = read_series(&mut f, 2, || {});
            assert!(!report.passed());
            assert_eq!(report.error, Some(Error::PollLimit));
            assert_eq!(usize::from(report.completed), completed);
            assert_eq!(report.failed_pass, Some((completed / 4) as u8));
            assert_eq!(
                report.failed_resource,
                Some(ReadResource::ALL[completed % 4])
            );
            assert_eq!(
                report
                    .responses
                    .iter()
                    .flatten()
                    .filter(|v| v.is_some())
                    .count(),
                completed
            );
            assert_eq!(f.trigger_count, completed + 1);
            assert_eq!(
                f.writes
                    .iter()
                    .filter(|(r, _)| *r == Register::IrqClear)
                    .count(),
                completed
            );
        }
    }

    #[test]
    fn series_never_clears_an_unrelated_pending_irq_to_start_another_read() {
        let mut f = Fake::ready();
        f.complete = true; // Completes with unrelated IRQ4 too.
        let report = read_series(&mut f, 2, || {});
        assert_eq!(report.completed, 1);
        assert_eq!(report.failed_resource, Some(ReadResource::Mss));
        assert_eq!(report.error, Some(Error::Handoff));
        assert_eq!(f.trigger_count, 1);
        assert_eq!(f.read(Register::IrqStatus), 4);
        assert_eq!(
            f.writes
                .iter()
                .filter(|(r, _)| *r == Register::IrqClear)
                .count(),
            1
        );
    }
}
