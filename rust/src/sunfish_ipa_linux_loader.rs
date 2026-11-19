//! Retained one-attempt loader for the exact private working IPA firmware.
//!
//! Owns metadata and target-memory views through failure/success. It composes
//! the packed-image planner, compact metadata and PAS sequence. No Drop-time
//! free, retry, rollback or production-ready permit is provided. Physical
//! mapping/SCM-resource authority remains a native backend precondition.
#![forbid(unsafe_code)]

use crate::{
    qcom_mdt::MDT_MAX_PROGRAM_HEADERS,
    qcom_pas_init::Transport,
    sunfish_ipa_fws::{self as fws, SegmentData, StagingMemory},
    sunfish_ipa_linux_pas::{LinuxTransport, Resources},
    sunfish_ipa_pas::{self as pas, MetadataRange, StartOnce},
    sunfish_linux_firmware::Ipa,
    sunfish_lpd::Sha256,
};

pub const METADATA_BYTES: usize = 8192;
pub const FIRMWARE_BYTES: usize = 29120;
pub const FIRMWARE_SHA256: [u8; 32] = [
    0x7d, 0x61, 0xb6, 0x42, 0x88, 0x24, 0x87, 0x19, 0xd1, 0x4a, 0x4b, 0x31, 0x6e, 0x9c, 0xb8, 0x39,
    0x17, 0xc1, 0x1a, 0x2c, 0x97, 0x53, 0xdf, 0xf5, 0x47, 0x5a, 0x7a, 0x2d, 0xb9, 0x53, 0x94, 0xdd,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Identity,
    Image,
    Geometry,
    Consumed,
    DirtyMetadata,
    Metadata,
    Io,
    Guard,
    Pas(pas::Error),
    Stage,
    Readback,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    New,
    Metadata,
    Prepared,
    Checking,
    Initializing,
    MemorySetup,
    Staging,
    Verifying,
    Authenticating,
    AuthReturned,
    Failed,
}

/// Own actual coherent/noncached, contiguous allocator metadata pages and the
/// exact reserved target mapping. Never access metadata after INIT submission,
/// nor the target after AUTH submission: secure firmware may protect them.
/// Methods must report uncertain writes as errors without replaying them.
pub trait Memory {
    fn metadata_physical(&self) -> u64;
    fn metadata_read(&mut self, offset: usize) -> Result<u8, Error>;
    fn metadata_write(&mut self, offset: usize, value: u8) -> Result<(), Error>;
    fn target_read(&mut self, offset: usize) -> Result<u8, Error>;
    fn target_write(&mut self, offset: usize, value: u8) -> Result<(), Error>;
    fn sync(&mut self) -> Result<(), Error>;
}

pub struct Image<'a> {
    ipa: Ipa<'a>,
}
impl<'a> Image<'a> {
    /// Exact private reference identity; firmware signature authentication is
    /// still exclusively PAS's job. Do not substitute another board's image.
    pub fn private_reference(bytes: &'a [u8]) -> Result<Self, Error> {
        Self::identity(bytes, FIRMWARE_BYTES, FIRMWARE_SHA256)
    }
    fn identity(bytes: &'a [u8], length: usize, digest: [u8; 32]) -> Result<Self, Error> {
        if bytes.len() != length {
            return Err(Error::Identity);
        }
        let mut hash = Sha256::new();
        hash.update(bytes);
        if hash.finish() != digest {
            return Err(Error::Identity);
        }
        let ipa = Ipa::parse(bytes).map_err(|_| Error::Image)?;
        let (header, hash) = ipa.packed().metadata_parts();
        if header
            .len()
            .checked_add(hash.len())
            .is_none_or(|n| n == 0 || n > METADATA_BYTES)
        {
            return Err(Error::Metadata);
        }
        Ok(Self { ipa })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Report {
    pub phase: Phase,
    pub metadata_length: usize,
    pub metadata_writes: usize,
    pub target_writes: usize,
    pub target_verified: usize,
    pub metadata_submitted: bool,
    pub auth_submitted: bool,
    pub error: Option<Error>,
}
pub struct Loader<'a, M> {
    image: Image<'a>,
    memory: core::mem::ManuallyDrop<M>,
    session: StartOnce,
    report: Report,
}
impl<'a, M: Memory> Loader<'a, M> {
    /// Consumes and retains memory before the first possible write. A checked
    /// address is not proof of allocator ownership; the native backend must
    /// establish that authority. No resource can be extracted after creation.
    pub fn new(image: Image<'a>, memory: M) -> Result<Self, Error> {
        let physical = memory.metadata_physical();
        let end = physical
            .checked_add(METADATA_BYTES as u64)
            .ok_or(Error::Geometry)?;
        if physical < 0x80000000
            || physical & 4095 != 0
            || end > 1 << 40
            || (physical < fws::REGION_BASE + fws::REGION_BYTES && fws::REGION_BASE < end)
        {
            return Err(Error::Geometry);
        }
        let (a, b) = image.ipa.packed().metadata_parts();
        let length = a.len() + b.len();
        let session = StartOnce::new(
            *image.ipa.plan(),
            MetadataRange {
                physical_address: physical,
                bytes: length as u32,
            },
        )
        .map_err(Error::Pas)?;
        Ok(Self {
            image,
            memory: core::mem::ManuallyDrop::new(memory),
            session,
            report: Report {
                phase: Phase::New,
                metadata_length: length,
                metadata_writes: 0,
                target_writes: 0,
                target_verified: 0,
                metadata_submitted: false,
                auth_submitted: false,
                error: None,
            },
        })
    }
    pub const fn report(&self) -> Report {
        self.report
    }
    pub fn pas_report(&self) -> &pas::Report {
        self.session.report()
    }
    fn fail(&mut self, error: Error) -> Error {
        self.report.error.get_or_insert(error);
        self.report.phase = Phase::Failed;
        error
    }
    pub fn prepare(&mut self) -> Result<(), Error> {
        if self.report.phase != Phase::New {
            return Err(Error::Consumed);
        }
        self.report.phase = Phase::Metadata;
        let result = (|| {
            for i in 0..METADATA_BYTES {
                if self.memory.metadata_read(i)? != 0 {
                    return Err(Error::DirtyMetadata);
                }
            }
            let (header, hash) = self.image.ipa.packed().metadata_parts();
            for (i, &value) in header.iter().chain(hash).enumerate() {
                self.report.metadata_writes += 1;
                self.memory.metadata_write(i, value)?;
            }
            self.memory.sync()?;
            for i in 0..METADATA_BYTES {
                let expected = if i < header.len() {
                    header[i]
                } else if i < header.len() + hash.len() {
                    hash[i - header.len()]
                } else {
                    0
                };
                if self.memory.metadata_read(i)? != expected {
                    return Err(Error::Readback);
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            return Err(self.fail(error));
        }
        self.report.phase = Phase::Prepared;
        Ok(())
    }
    /// Native guard checks current power/domain/configuration before every
    /// phase. The supplied transport additionally enforces the SCM envelope
    /// for *every* mutating call. Progress runs before each secure operation.
    pub fn start<S: Transport, R: Resources>(
        &mut self,
        io: &mut LinuxTransport<'_, S, R>,
        mut guard: impl FnMut(Phase) -> Result<(), Error>,
        mut progress: impl FnMut(&pas::Report),
    ) -> Result<(), Error> {
        if self.report.phase != Phase::Prepared {
            return Err(Error::Consumed);
        }
        let result = (|| {
            self.report.phase = Phase::Checking;
            guard(self.report.phase)?;
            self.session.check(io, &mut progress).map_err(Error::Pas)?;
            self.report.phase = Phase::Initializing;
            guard(self.report.phase)?;
            // Final verification before INIT. Secure firmware can alter/protect
            // this allocation once submitted, even if the call fails.
            let (header, hash) = self.image.ipa.packed().metadata_parts();
            self.memory.sync()?;
            for i in 0..METADATA_BYTES {
                let expected = if i < header.len() {
                    header[i]
                } else if i < header.len() + hash.len() {
                    hash[i - header.len()]
                } else {
                    0
                };
                if self.memory.metadata_read(i)? != expected {
                    return Err(Error::Readback);
                }
            }
            let metadata = MetadataRange {
                physical_address: self.memory.metadata_physical(),
                bytes: self.report.metadata_length as u32,
            };
            self.report.metadata_submitted = true;
            self.session
                .initialize(io, metadata, &mut progress)
                .map_err(Error::Pas)?;
            if self.image.ipa.plan().mem_setup.is_some() {
                self.report.phase = Phase::MemorySetup;
                guard(self.report.phase)?;
                self.session
                    .setup_memory(io, &mut progress)
                    .map_err(Error::Pas)?;
            }
            self.report.phase = Phase::Staging;
            guard(self.report.phase)?;
            let mut files =
                core::array::from_fn::<_, MDT_MAX_PROGRAM_HEADERS, _>(|_| SegmentData {
                    index: 0,
                    bytes: &[],
                });
            let mut count = 0;
            for s in self
                .image
                .ipa
                .packed()
                .segments()
                .filter(|s| !s.bytes.is_empty())
            {
                files[count] = SegmentData {
                    index: s.descriptor.index,
                    bytes: s.bytes,
                };
                count += 1;
            }
            let mut staging = Stage {
                memory: &mut *self.memory,
                error: None,
                submitted: &mut self.report.target_writes,
            };
            let result = self.session.stage(&files[..count], &mut staging);
            if let Some(error) = staging.error {
                return Err(error);
            }
            result.map_err(Error::Pas)?;
            self.report.phase = Phase::Verifying;
            guard(self.report.phase)?;
            for place in self.image.ipa.plan().placements().iter().flatten() {
                let source = files[..count]
                    .iter()
                    .find(|s| s.index == place.index)
                    .map(|s| s.bytes)
                    .unwrap_or(&[]);
                for i in 0..place.memory_bytes as usize {
                    let expected = source.get(i).copied().unwrap_or(0);
                    if self.memory.target_read(place.offset as usize + i)? != expected {
                        return Err(Error::Readback);
                    }
                    self.report.target_verified += 1;
                }
            }
            self.memory.sync()?;
            self.report.phase = Phase::Authenticating;
            guard(self.report.phase)?;
            self.report.auth_submitted = true;
            self.session
                .authenticate(io, &mut progress)
                .map_err(Error::Pas)?;
            self.report.phase = Phase::AuthReturned;
            Ok(())
        })();
        if let Err(error) = result {
            return Err(self.fail(error));
        }
        Ok(())
    }
}
// Existing planner's staging interface is infallible. Latch the first native
// failure and suppress all subsequent access; the owning Loader checks it
// before verification or AUTH. A claimed StageReport cannot bypass this check.
struct Stage<'a, M> {
    memory: &'a mut M,
    error: Option<Error>,
    submitted: &'a mut usize,
}
impl<M: Memory> Stage<'_, M> {
    fn byte(&mut self, offset: usize, value: u8) {
        if self.error.is_some() {
            return;
        }
        *self.submitted += 1;
        self.error = self.memory.target_write(offset, value).err();
    }
}
impl<M: Memory> StagingMemory for Stage<'_, M> {
    fn write(&mut self, offset: u32, bytes: &[u8]) {
        for (i, &v) in bytes.iter().enumerate() {
            self.byte(offset as usize + i, v);
            if self.error.is_some() {
                break;
            }
        }
    }
    fn zero(&mut self, offset: u32, bytes: u32) {
        for i in 0..bytes as usize {
            self.byte(offset as usize + i, 0);
            if self.error.is_some() {
                break;
            }
        }
    }
    fn sync(&mut self) {
        if self.error.is_none() {
            self.error = self.memory.sync().err();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qcom_pas_init::{Request, Transport};
    use crate::sunfish_ipa_linux_pas::{LinuxTransport, Resources};
    use std::{cell::RefCell, rc::Rc, vec, vec::Vec};
    #[derive(Default)]
    struct State {
        metadata: Vec<u8>,
        target: Vec<u8>,
        init: bool,
        setup: bool,
        auth: bool,
        metadata_writes: usize,
        target_writes: usize,
        reads: usize,
        calls: Vec<u64>,
        events: Vec<&'static str>,
        fail_metadata: Option<usize>,
        fail_target: Option<usize>,
        fail_read: Option<usize>,
        fail_call: Option<usize>,
        service_call: Option<usize>,
        acquire_error: bool,
        release_error: bool,
        drops: usize,
        active: bool,
    }
    struct Ram(Rc<RefCell<State>>);
    impl Drop for Ram {
        fn drop(&mut self) {
            self.0.borrow_mut().drops += 1;
        }
    }
    impl Memory for Ram {
        fn metadata_physical(&self) -> u64 {
            0xa0000000
        }
        fn metadata_read(&mut self, i: usize) -> Result<u8, Error> {
            let mut s = self.0.borrow_mut();
            assert!(!s.init, "metadata touched after INIT");
            let n = s.reads;
            s.reads += 1;
            if s.fail_read == Some(n) {
                return Err(Error::Io);
            }
            Ok(s.metadata[i])
        }
        fn metadata_write(&mut self, i: usize, v: u8) -> Result<(), Error> {
            let mut s = self.0.borrow_mut();
            assert!(!s.init);
            let n = s.metadata_writes;
            s.metadata_writes += 1;
            s.metadata[i] = v;
            if s.fail_metadata == Some(n) {
                Err(Error::Io)
            } else {
                Ok(())
            }
        }
        fn target_read(&mut self, i: usize) -> Result<u8, Error> {
            let mut s = self.0.borrow_mut();
            assert!(s.setup && !s.auth);
            let n = s.reads;
            s.reads += 1;
            if s.fail_read == Some(n) {
                Err(Error::Io)
            } else {
                Ok(s.target[i])
            }
        }
        fn target_write(&mut self, i: usize, v: u8) -> Result<(), Error> {
            let mut s = self.0.borrow_mut();
            assert!(s.setup && !s.auth);
            let n = s.target_writes;
            s.target_writes += 1;
            s.target[i] = v;
            if s.fail_target == Some(n) {
                Err(Error::Io)
            } else {
                Ok(())
            }
        }
        fn sync(&mut self) -> Result<(), Error> {
            Ok(())
        }
    }
    struct Secure(Rc<RefCell<State>>);
    impl Transport for Secure {
        fn invoke(&mut self, r: &Request) -> Result<[u64; 8], u64> {
            let mut s = self.0.borrow_mut();
            let n = s.calls.len();
            s.calls.push(r.words()[0]);
            s.events.push("smc");
            match r.words()[0] {
                0x42000601 => assert!(!s.active),
                0x42000201 => {
                    assert!(s.active);
                    s.init = true;
                }
                0x42000202 => {
                    assert!(s.active && s.init);
                    s.setup = true;
                }
                0x42000205 => {
                    assert!(s.active && s.setup);
                    s.auth = true;
                }
                _ => panic!("unapproved secure call"),
            }
            if s.fail_call == Some(n) {
                return Err(99);
            }
            Ok([
                0,
                if s.service_call == Some(n) {
                    9
                } else {
                    u64::from(r.words()[0] == 0x42000601)
                },
                0,
                0,
                0,
                0,
                0,
                0,
            ])
        }
    }
    struct Votes(Rc<RefCell<State>>);
    impl Resources for Votes {
        type Lease = ();
        fn acquire(&mut self) -> Result<(), u64> {
            let mut s = self.0.borrow_mut();
            s.events.push("acquire");
            assert!(!s.active);
            if s.acquire_error {
                return Err(11);
            }
            s.active = true;
            Ok(())
        }
        fn release(&mut self, _: ()) -> Result<(), u64> {
            let mut s = self.0.borrow_mut();
            s.events.push("release");
            assert!(s.active);
            s.active = false;
            if s.release_error { Err(12) } else { Ok(()) }
        }
    }
    fn fixture() -> Vec<u8> {
        let mut b = vec![0; 0x204];
        b[..7].copy_from_slice(b"\x7fELF\x01\x01\x01");
        b[18..20].copy_from_slice(&164u16.to_le_bytes());
        b[20..24].copy_from_slice(&1u32.to_le_bytes());
        b[28..32].copy_from_slice(&52u32.to_le_bytes());
        b[40..42].copy_from_slice(&52u16.to_le_bytes());
        b[42..44].copy_from_slice(&32u16.to_le_bytes());
        b[44..46].copy_from_slice(&4u16.to_le_bytes());
        for (i, t, off, pa, file, mem, flags) in [
            (0, 0, 0, 0, 180, 0, 7 << 24),
            (1, 0, 0x100, 0x5000, 8, 8, 2 << 24),
            (2, 1, 0x200, 0x1000, 4, 16, 1 << 27),
            (3, 1, u32::MAX, 0x3000, 0, 32, 1 << 27),
        ] {
            for (d, v) in [
                (0, t),
                (4, off),
                (12, pa),
                (16, file),
                (20, mem),
                (24, flags),
            ] {
                let at = 52 + i * 32 + d;
                b[at..at + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
        b[0x100..0x108].copy_from_slice(b"hashdata");
        b[0x200..].copy_from_slice(b"data");
        b
    }
    fn image(bytes: &[u8]) -> Image<'_> {
        let mut h = Sha256::new();
        h.update(bytes);
        Image::identity(bytes, bytes.len(), h.finish()).unwrap()
    }
    fn state() -> Rc<RefCell<State>> {
        Rc::new(RefCell::new(State {
            metadata: vec![0; METADATA_BYTES],
            target: vec![0xa5; 0x10000],
            ..Default::default()
        }))
    }
    fn start(loader: &mut Loader<'_, Ram>, s: &Rc<RefCell<State>>) -> Result<(), Error> {
        let mut secure = Secure(s.clone());
        let mut votes = Votes(s.clone());
        loader.start(
            &mut LinuxTransport::new(&mut secure, &mut votes),
            |_| Ok(()),
            |_| {},
        )
    }
    fn no_retry(loader: &mut Loader<'_, Ram>, s: &Rc<RefCell<State>>) {
        let before = (
            s.borrow().reads,
            s.borrow().metadata_writes,
            s.borrow().target_writes,
            s.borrow().calls.len(),
        );
        assert_eq!(loader.prepare(), Err(Error::Consumed));
        assert_eq!(start(loader, s), Err(Error::Consumed));
        assert_eq!(
            before,
            (
                s.borrow().reads,
                s.borrow().metadata_writes,
                s.borrow().target_writes,
                s.borrow().calls.len()
            )
        );
    }
    #[test]
    fn full_packed_sequence_holds_resources_for_mem_setup_and_never_reuses_protected_views() {
        let b = fixture();
        let s = state();
        let mut loader = Loader::new(image(&b), Ram(s.clone())).unwrap();
        loader.prepare().unwrap();
        start(&mut loader, &s).unwrap();
        let r = loader.report();
        assert_eq!(
            (
                r.phase,
                r.metadata_length,
                r.metadata_writes,
                r.target_writes,
                r.target_verified
            ),
            (Phase::AuthReturned, 188, 188, 48, 48)
        );
        assert!(r.metadata_submitted && r.auth_submitted && loader.pas_report().passed());
        assert_eq!(
            s.borrow().events,
            vec![
                "smc", "smc", "smc", "acquire", "smc", "release", "acquire", "smc", "release",
                "acquire", "smc", "release"
            ]
        );
        assert_eq!(&s.borrow().target[..4], b"data");
        for (i, &v) in s.borrow().target.iter().enumerate() {
            if (4..16).contains(&i) || (0x2000..0x2020).contains(&i) {
                assert_eq!(v, 0);
            } else if i >= 4 {
                assert_eq!(v, 0xa5, "gap modified {i:x}");
            }
        }
        no_retry(&mut loader, &s);
        drop(loader);
        assert_eq!(
            s.borrow().drops,
            0,
            "uncertain/secure-owned memory must not be freed"
        );
    }
    #[test]
    fn public_image_constructor_rejects_foreign_bytes_and_wrong_length() {
        assert_eq!(
            Image::private_reference(&fixture()).err(),
            Some(Error::Identity)
        );
        assert_eq!(
            Image::private_reference(&vec![0; FIRMWARE_BYTES]).err(),
            Some(Error::Identity)
        );
    }
    #[test]
    fn dirty_metadata_and_each_uncertain_metadata_store_are_terminal_without_secure_calls() {
        let b = fixture();
        for index in 0..188 {
            let s = state();
            s.borrow_mut().fail_metadata = Some(index);
            let mut l = Loader::new(image(&b), Ram(s.clone())).unwrap();
            assert_eq!(l.prepare(), Err(Error::Io));
            assert_eq!(s.borrow().metadata_writes, index + 1);
            assert!(s.borrow().calls.is_empty());
            no_retry(&mut l, &s);
        }
        let s = state();
        s.borrow_mut().metadata[8191] = 1;
        let mut l = Loader::new(image(&b), Ram(s.clone())).unwrap();
        assert_eq!(l.prepare(), Err(Error::DirtyMetadata));
        assert_eq!(l.report().metadata_writes, 0);
        no_retry(&mut l, &s);
    }
    #[test]
    fn changed_payload_or_padding_before_init_blocks_submission() {
        let b = fixture();
        for offset in [0, 187, 188, 8191] {
            let s = state();
            let mut l = Loader::new(image(&b), Ram(s.clone())).unwrap();
            l.prepare().unwrap();
            s.borrow_mut().metadata[offset] ^= 1;
            assert_eq!(start(&mut l, &s), Err(Error::Readback));
            assert_eq!(s.borrow().calls.len(), 3);
            assert!(!l.report().metadata_submitted);
            no_retry(&mut l, &s);
        }
    }
    #[test]
    fn each_uncertain_target_store_stops_before_verification_or_authentication() {
        let b = fixture();
        for index in 0..48 {
            let s = state();
            s.borrow_mut().fail_target = Some(index);
            let mut l = Loader::new(image(&b), Ram(s.clone())).unwrap();
            l.prepare().unwrap();
            assert_eq!(start(&mut l, &s), Err(Error::Io));
            assert_eq!(s.borrow().target_writes, index + 1);
            assert_eq!(s.borrow().calls.len(), 5);
            assert!(!l.report().auth_submitted);
            no_retry(&mut l, &s);
        }
    }
    #[test]
    fn uncertain_secure_calls_and_service_errors_never_advance_to_later_operations() {
        let b = fixture();
        for index in 0..6 {
            for service in [false, true] {
                let s = state();
                if service {
                    s.borrow_mut().service_call = Some(index);
                } else {
                    s.borrow_mut().fail_call = Some(index);
                }
                let mut l = Loader::new(image(&b), Ram(s.clone())).unwrap();
                l.prepare().unwrap();
                assert!(start(&mut l, &s).is_err());
                assert_eq!(s.borrow().calls.len(), index + 1);
                no_retry(&mut l, &s);
                drop(l);
                assert_eq!(s.borrow().drops, 0);
            }
        }
    }
    #[test]
    fn guards_and_resource_failures_prevent_later_stages() {
        let b = fixture();
        for phase in [
            Phase::Checking,
            Phase::Initializing,
            Phase::MemorySetup,
            Phase::Staging,
            Phase::Verifying,
            Phase::Authenticating,
        ] {
            let s = state();
            let mut l = Loader::new(image(&b), Ram(s.clone())).unwrap();
            l.prepare().unwrap();
            let mut secure = Secure(s.clone());
            let mut votes = Votes(s.clone());
            assert_eq!(
                l.start(
                    &mut LinuxTransport::new(&mut secure, &mut votes),
                    |p| if p == phase {
                        Err(Error::Guard)
                    } else {
                        Ok(())
                    },
                    |_| {}
                ),
                Err(Error::Guard)
            );
            assert!(!l.report().auth_submitted);
            no_retry(&mut l, &s);
        }
        for acquire in [false, true] {
            let s = state();
            s.borrow_mut().acquire_error = acquire;
            s.borrow_mut().release_error = !acquire;
            let mut l = Loader::new(image(&b), Ram(s.clone())).unwrap();
            l.prepare().unwrap();
            assert!(start(&mut l, &s).is_err());
            assert_eq!(s.borrow().calls.len(), if acquire { 3 } else { 4 });
            no_retry(&mut l, &s);
        }
    }
    #[test]
    fn metadata_or_target_read_failure_cannot_be_treated_as_verification_success() {
        let b = fixture();
        for index in [0, 8191, 8192, 16383, 16384, 24575, 24576, 24623] {
            let s = state();
            s.borrow_mut().fail_read = Some(index);
            let mut l = Loader::new(image(&b), Ram(s.clone())).unwrap();
            let result = l.prepare().and_then(|_| start(&mut l, &s));
            assert_eq!(result, Err(Error::Io), "read {index}");
            assert!(!l.report().auth_submitted);
            no_retry(&mut l, &s);
        }
    }
}
