#![allow(dead_code)]

#[path = "../rust/src/qcom_mdt.rs"]
mod qcom_mdt;
#[path = "../rust/src/qcom_pas_init.rs"]
mod qcom_pas_init;
#[path = "../rust/src/qcom_pas_metadata.rs"]
mod qcom_pas_metadata;
#[path = "../rust/src/sunfish_ipa_fws.rs"]
mod sunfish_ipa_fws;
#[path = "../rust/src/sunfish_ipa_pas.rs"]
mod sunfish_ipa_pas;

use qcom_mdt::{MdtImage, MdtSegment, MDT_MAX_PROGRAM_HEADERS};
use qcom_pas_init::{Request, Transport as PasScmTransport};
use sunfish_ipa_fws::{SegmentData, SegmentFile, StagingMemory};
use sunfish_ipa_pas::{
    CallKind, Error, IpaPasTransport, MetadataRange, PasBandwidthProvider, Phase,
    ResourceEnvelopeTransport, StartOnce, TransportError,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Operation {
    Query(u32),
    Init(u64),
    MemSetup(u64, u64),
    Auth,
}

#[derive(Default)]
struct FakeTransport {
    calls: Vec<Operation>,
    kinds: Vec<CallKind>,
    fail_at: Option<usize>,
    unsupported_at: Option<usize>,
    service_error_at: Option<usize>,
}

impl IpaPasTransport for FakeTransport {
    fn invoke(&mut self, kind: CallKind, request: &Request) -> Result<[u64; 8], TransportError> {
        let words = request.words();
        let index = self.calls.len();
        if self.fail_at == Some(index) {
            return Err(TransportError::SecureCall(0xdead));
        }
        let operation = match words[0] {
            0x42000601 => Operation::Query(words[2] as u32),
            0x42000201 => Operation::Init(words[3]),
            0x42000202 => Operation::MemSetup(words[3], words[4]),
            0x42000205 => Operation::Auth,
            _ => panic!("unexpected PAS envelope: {words:?}"),
        };
        self.calls.push(operation);
        self.kinds.push(kind);
        if self.unsupported_at == Some(index) {
            return Ok([0, 0, 0, 0, 0, 0, 0, 0]);
        }
        if self.service_error_at == Some(index) {
            return Ok([0, 9, 0, 0, 0, 0, 0, 0]);
        }
        let service = if matches!(operation, Operation::Query(_)) {
            1
        } else {
            0
        };
        Ok([0, service, 0, 0, 0, 0, 0, 0])
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AdapterEvent {
    Acquire,
    Secure([u64; 8]),
    Release(u8),
}

#[derive(Clone)]
struct FakeScm {
    events: std::rc::Rc<std::cell::RefCell<Vec<AdapterEvent>>>,
    error: Option<u64>,
}

impl PasScmTransport for FakeScm {
    fn invoke(&mut self, request: &Request) -> Result<[u64; 8], u64> {
        self.events
            .borrow_mut()
            .push(AdapterEvent::Secure(*request.words()));
        match self.error {
            Some(error) => Err(error),
            None if request.words()[0] == 0x4200_0601 => Ok([0, 1, 0, 0, 0, 0, 0, 0]),
            None => Ok([0; 8]),
        }
    }
}

#[derive(Clone)]
struct FakeBandwidth {
    events: std::rc::Rc<std::cell::RefCell<Vec<AdapterEvent>>>,
    acquire_error: Option<u64>,
    release_error: Option<u64>,
}

impl PasBandwidthProvider for FakeBandwidth {
    type Lease = u8;

    fn acquire_shared_pas_vote(&mut self) -> Result<Self::Lease, u64> {
        self.events.borrow_mut().push(AdapterEvent::Acquire);
        self.acquire_error.map_or(Ok(7), Err)
    }

    fn release_shared_pas_vote(&mut self, lease: Self::Lease) -> Result<(), u64> {
        self.events.borrow_mut().push(AdapterEvent::Release(lease));
        self.release_error.map_or(Ok(()), Err)
    }
}

fn fake_envelope(
    scm_error: Option<u64>,
    acquire_error: Option<u64>,
    release_error: Option<u64>,
) -> (
    FakeScm,
    FakeBandwidth,
    std::rc::Rc<std::cell::RefCell<Vec<AdapterEvent>>>,
) {
    let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    (
        FakeScm {
            events: events.clone(),
            error: scm_error,
        },
        FakeBandwidth {
            events: events.clone(),
            acquire_error,
            release_error,
        },
        events,
    )
}

#[test]
fn pas_envelope_votes_only_around_init_and_auth_and_checks_request_class() {
    let (mut scm, mut bandwidth, events) = fake_envelope(None, None, None);
    let mut io = ResourceEnvelopeTransport::new(&mut scm, &mut bandwidth);

    io.invoke(CallKind::Availability, &Request::ipa_init_query())
        .unwrap();
    assert_eq!(
        *events.borrow(),
        [AdapterEvent::Secure(*Request::ipa_init_query().words())]
    );

    events.borrow_mut().clear();
    let mem_setup = Request::ipa_mem_setup(0x98f8_0000, 0x10000);
    io.invoke(CallKind::MemSetup, &mem_setup).unwrap();
    assert_eq!(*events.borrow(), [AdapterEvent::Secure(*mem_setup.words())]);

    events.borrow_mut().clear();
    let init = Request::ipa_init_image(0x9000_0000);
    io.invoke(CallKind::InitImage, &init).unwrap();
    assert_eq!(
        *events.borrow(),
        [
            AdapterEvent::Acquire,
            AdapterEvent::Secure(*init.words()),
            AdapterEvent::Release(7),
        ]
    );

    events.borrow_mut().clear();
    let auth = Request::ipa_auth_reset();
    io.invoke(CallKind::AuthAndReset, &auth).unwrap();
    assert_eq!(
        *events.borrow(),
        [
            AdapterEvent::Acquire,
            AdapterEvent::Secure(*auth.words()),
            AdapterEvent::Release(7),
        ]
    );

    events.borrow_mut().clear();
    assert_eq!(
        io.invoke(CallKind::InitImage, &Request::ipa_init_query()),
        Err(TransportError::RequestKindMismatch)
    );
    assert!(events.borrow().is_empty());
}

#[test]
fn pas_envelope_never_calls_scm_without_vote_and_releases_after_smc_failure() {
    let (mut scm, mut bandwidth, events) = fake_envelope(None, Some(0x11), None);
    let mut io = ResourceEnvelopeTransport::new(&mut scm, &mut bandwidth);
    let init = Request::ipa_init_image(0x9000_0000);
    assert_eq!(
        io.invoke(CallKind::InitImage, &init),
        Err(TransportError::BandwidthAcquire(0x11))
    );
    assert_eq!(*events.borrow(), [AdapterEvent::Acquire]);

    let (mut scm, mut bandwidth, events) = fake_envelope(Some(0x22), None, None);
    let mut io = ResourceEnvelopeTransport::new(&mut scm, &mut bandwidth);
    assert_eq!(
        io.invoke(CallKind::AuthAndReset, &Request::ipa_auth_reset()),
        Err(TransportError::SecureCall(0x22))
    );
    assert_eq!(
        *events.borrow(),
        [
            AdapterEvent::Acquire,
            AdapterEvent::Secure(*Request::ipa_auth_reset().words()),
            AdapterEvent::Release(7),
        ]
    );
}

#[test]
fn pas_envelope_surfaces_release_failures_and_preserves_both_errors() {
    let (mut scm, mut bandwidth, events) = fake_envelope(None, None, Some(0x33));
    let mut io = ResourceEnvelopeTransport::new(&mut scm, &mut bandwidth);
    let init = Request::ipa_init_image(0x9000_0000);
    assert_eq!(
        io.invoke(CallKind::InitImage, &init),
        Err(TransportError::BandwidthRelease(0x33))
    );
    assert_eq!(
        *events.borrow(),
        [
            AdapterEvent::Acquire,
            AdapterEvent::Secure(*init.words()),
            AdapterEvent::Release(7),
        ]
    );

    let (mut scm, mut bandwidth, _) = fake_envelope(Some(0x44), None, Some(0x55));
    let mut io = ResourceEnvelopeTransport::new(&mut scm, &mut bandwidth);
    assert_eq!(
        io.invoke(CallKind::AuthAndReset, &Request::ipa_auth_reset()),
        Err(TransportError::SecureAndBandwidthRelease {
            secure: 0x44,
            bandwidth: 0x55,
        })
    );
}

#[test]
fn start_session_latches_bandwidth_errors_before_memory_setup_or_auth() {
    let mut start = StartOnce::new(plan(true), metadata()).unwrap();
    let (mut scm, mut bandwidth, events) = fake_envelope(None, None, Some(0x66));
    let mut io = ResourceEnvelopeTransport::new(&mut scm, &mut bandwidth);
    start.check(&mut io, |_| {}).unwrap();
    assert_eq!(
        start.initialize(&mut io, metadata(), |_| {}),
        Err(Error::Bandwidth)
    );
    assert_eq!(start.report().phase, Phase::Failed);
    assert_eq!(start.report().issued_mask, 0x0f);
    assert_eq!(start.report().returned_mask, 0x07);
    assert_eq!(start.report().kernel_error, None);
    assert_eq!(
        start.report().transport_error,
        Some(TransportError::BandwidthRelease(0x66))
    );
    assert_eq!(
        *events.borrow(),
        [
            AdapterEvent::Secure(*Request::ipa_init_query().words()),
            AdapterEvent::Secure(*Request::ipa_mem_setup_query().words()),
            AdapterEvent::Secure(*Request::ipa_auth_query().words()),
            AdapterEvent::Acquire,
            AdapterEvent::Secure(*Request::ipa_init_image(metadata().physical_address).words()),
            AdapterEvent::Release(7),
        ]
    );
}

#[derive(Default)]
struct FakeMemory {
    writes: Vec<(u32, Vec<u8>)>,
    zeroes: Vec<(u32, u32)>,
    syncs: usize,
}

impl StagingMemory for FakeMemory {
    fn write(&mut self, offset: u32, bytes: &[u8]) {
        self.writes.push((offset, bytes.to_vec()));
    }

    fn zero(&mut self, offset: u32, bytes: u32) {
        self.zeroes.push((offset, bytes));
    }

    fn sync(&mut self) {
        self.syncs += 1;
    }
}

fn plan(relocatable: bool) -> sunfish_ipa_fws::LoadPlan {
    let mut segments = [None; MDT_MAX_PROGRAM_HEADERS];
    let address = if relocatable {
        0x1000
    } else {
        sunfish_ipa_fws::REGION_BASE as u32
    };
    segments[0] = Some(MdtSegment {
        index: 0,
        linked_address: address,
        file_size: 4,
        memory_size: 0x1000,
        flags: if relocatable { 1 << 27 } else { 0 },
    });
    let image = MdtImage {
        segments,
        program_header_count: 1,
        loadable_segment_count: 1,
        linked_min_address: address,
        linked_max_address: address + 0x1000,
        relocatable,
    };
    sunfish_ipa_fws::plan(&image, &[SegmentFile { index: 0, bytes: 4 }]).unwrap()
}

fn metadata() -> MetadataRange {
    MetadataRange {
        physical_address: 0x9000_0000,
        bytes: 0x1000,
    }
}

fn segment() -> [SegmentData<'static>; 1] {
    [SegmentData {
        index: 0,
        bytes: &[0xaa, 0xbb, 0xcc, 0xdd],
    }]
}

#[test]
fn relocatable_ipa_start_matches_linux_pil_pas_order_and_envelopes() {
    let mut start = StartOnce::new(plan(true), metadata()).unwrap();
    let mut secure = FakeTransport::default();
    start.check(&mut secure, |_| {}).unwrap();
    start.initialize(&mut secure, metadata(), |_| {}).unwrap();
    start.setup_memory(&mut secure, |_| {}).unwrap();
    let mut memory = FakeMemory::default();
    let staged = start.stage(&segment(), &mut memory).unwrap();
    start.authenticate(&mut secure, |_| {}).unwrap();

    assert_eq!(staged.segments_staged, 1);
    assert_eq!(staged.file_bytes_staged, 4);
    assert_eq!(staged.bss_bytes_zeroed, 0xffc);
    assert_eq!(memory.writes, [(0, vec![0xaa, 0xbb, 0xcc, 0xdd])]);
    assert_eq!(memory.zeroes, [(4, 0xffc)]);
    assert_eq!(memory.syncs, 1);
    assert_eq!(
        secure.calls,
        [
            Operation::Query(0x02000201),
            Operation::Query(0x02000202),
            Operation::Query(0x02000205),
            Operation::Init(metadata().physical_address),
            Operation::MemSetup(sunfish_ipa_fws::REGION_BASE, 0x1000),
            Operation::Auth,
        ]
    );
    assert_eq!(
        secure.kinds,
        [
            CallKind::Availability,
            CallKind::Availability,
            CallKind::Availability,
            CallKind::InitImage,
            CallKind::MemSetup,
            CallKind::AuthAndReset,
        ]
    );
    let report = start.report();
    assert!(report.passed());
    assert_eq!(report.issued_mask, 0x3f);
    assert_eq!(report.returned_mask, 0x3f);
    assert_eq!(report.replies[3].unwrap(), [0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(
        report.last_request.unwrap().words(),
        &[0x42000205, 1, 0x0f, 0, 0, 0, 0, 0]
    );
}

#[test]
fn fixed_ipa_mdt_skips_mem_setup_query_and_call() {
    let mut start = StartOnce::new(plan(false), metadata()).unwrap();
    let mut secure = FakeTransport::default();
    start.check(&mut secure, |_| {}).unwrap();
    start.initialize(&mut secure, metadata(), |_| {}).unwrap();
    let mut memory = FakeMemory::default();
    start.stage(&segment(), &mut memory).unwrap();
    start.authenticate(&mut secure, |_| {}).unwrap();

    assert_eq!(
        secure.calls,
        [
            Operation::Query(0x02000201),
            Operation::Query(0x02000205),
            Operation::Init(metadata().physical_address),
            Operation::Auth,
        ]
    );
    assert_eq!(
        secure.kinds,
        [
            CallKind::Availability,
            CallKind::Availability,
            CallKind::InitImage,
            CallKind::AuthAndReset,
        ]
    );
    assert!(start.report().passed());
}

#[test]
fn unavailable_or_uncertain_init_is_terminal_and_never_replayed() {
    let mut start = StartOnce::new(plan(true), metadata()).unwrap();
    let mut secure = FakeTransport {
        unsupported_at: Some(0),
        ..FakeTransport::default()
    };
    assert_eq!(
        start.check(&mut secure, |_| {}),
        Err(Error::InitUnavailable)
    );
    assert_eq!(start.report().phase, Phase::Failed);
    assert_eq!(secure.calls.len(), 1);
    assert_eq!(start.check(&mut secure, |_| {}), Err(Error::WrongStage));
    assert_eq!(secure.calls.len(), 1);

    let mut start = StartOnce::new(plan(true), metadata()).unwrap();
    let mut secure = FakeTransport {
        fail_at: Some(3),
        ..FakeTransport::default()
    };
    start.check(&mut secure, |_| {}).unwrap();
    assert_eq!(
        start.initialize(&mut secure, metadata(), |_| {}),
        Err(Error::Kernel)
    );
    assert_eq!(start.report().issued_mask, 0x0f);
    assert_eq!(start.report().returned_mask, 0x07);
    assert_eq!(secure.calls.len(), 3);
}

#[test]
fn metadata_and_staging_failures_stop_before_auth_reset() {
    assert_eq!(
        StartOnce::new(
            plan(true),
            MetadataRange {
                physical_address: 0x9000_0001,
                bytes: 0x1000,
            },
        )
        .err(),
        Some(Error::InvalidMetadata)
    );

    let mut start = StartOnce::new(plan(true), metadata()).unwrap();
    let mut secure = FakeTransport::default();
    start.check(&mut secure, |_| {}).unwrap();
    start.initialize(&mut secure, metadata(), |_| {}).unwrap();
    start.setup_memory(&mut secure, |_| {}).unwrap();
    let mut memory = FakeMemory::default();
    assert_eq!(start.stage(&[], &mut memory), Err(Error::Stage));
    assert_eq!(start.report().phase, Phase::Failed);
    assert_eq!(secure.calls.len(), 5);
    assert_eq!(
        start.authenticate(&mut secure, |_| {}),
        Err(Error::WrongStage)
    );
    assert_eq!(secure.calls.len(), 5);
}

#[test]
fn changed_metadata_or_nonzero_scm_service_result_stops_the_session() {
    let mut start = StartOnce::new(plan(true), metadata()).unwrap();
    let mut secure = FakeTransport::default();
    start.check(&mut secure, |_| {}).unwrap();
    assert_eq!(
        start.initialize(
            &mut secure,
            MetadataRange {
                physical_address: metadata().physical_address + 0x1000,
                ..metadata()
            },
            |_| {},
        ),
        Err(Error::MetadataChanged)
    );
    assert_eq!(secure.calls.len(), 3);

    let mut start = StartOnce::new(plan(true), metadata()).unwrap();
    let mut secure = FakeTransport {
        service_error_at: Some(3),
        ..FakeTransport::default()
    };
    start.check(&mut secure, |_| {}).unwrap();
    assert_eq!(
        start.initialize(&mut secure, metadata(), |_| {}),
        Err(Error::Service)
    );
    assert_eq!(secure.calls.len(), 4);
}

#[test]
fn mem_setup_and_auth_failures_are_terminal_and_never_replayed() {
    let mut start = StartOnce::new(plan(true), metadata()).unwrap();
    let mut secure = FakeTransport {
        service_error_at: Some(4),
        ..FakeTransport::default()
    };
    start.check(&mut secure, |_| {}).unwrap();
    start.initialize(&mut secure, metadata(), |_| {}).unwrap();
    assert_eq!(start.setup_memory(&mut secure, |_| {}), Err(Error::Service));
    assert_eq!(start.report().phase, Phase::Failed);
    assert_eq!(start.report().issued_mask, 0x1f);
    assert_eq!(start.report().returned_mask, 0x1f);
    assert_eq!(secure.calls.len(), 5);
    let mut memory = FakeMemory::default();
    assert_eq!(start.stage(&segment(), &mut memory), Err(Error::WrongStage));
    assert!(memory.writes.is_empty());
    assert_eq!(
        start.setup_memory(&mut secure, |_| {}),
        Err(Error::WrongStage)
    );
    assert_eq!(secure.calls.len(), 5);

    let mut start = StartOnce::new(plan(true), metadata()).unwrap();
    let mut secure = FakeTransport {
        service_error_at: Some(5),
        ..FakeTransport::default()
    };
    start.check(&mut secure, |_| {}).unwrap();
    start.initialize(&mut secure, metadata(), |_| {}).unwrap();
    start.setup_memory(&mut secure, |_| {}).unwrap();
    let mut memory = FakeMemory::default();
    start.stage(&segment(), &mut memory).unwrap();
    assert_eq!(start.authenticate(&mut secure, |_| {}), Err(Error::Service));
    assert_eq!(start.report().phase, Phase::Failed);
    assert_eq!(start.report().issued_mask, 0x3f);
    assert_eq!(start.report().returned_mask, 0x3f);
    assert_eq!(secure.calls.len(), 6);
    assert_eq!(
        start.authenticate(&mut secure, |_| {}),
        Err(Error::WrongStage)
    );
    assert_eq!(secure.calls.len(), 6);
}

#[test]
fn metadata_end_overflow_is_rejected_before_any_scm_call() {
    assert_eq!(
        StartOnce::new(
            plan(true),
            MetadataRange {
                physical_address: 0xffff_ffff_ffff_f000,
                bytes: 0x1000,
            },
        )
        .err(),
        Some(Error::InvalidMetadata)
    );
}
