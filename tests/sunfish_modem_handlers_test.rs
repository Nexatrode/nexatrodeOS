//! Host backing images and simulated ownership. No phone or native SCM access.
#[path = "modem_handlers_fixture.rs"]
pub mod fixture;
#[allow(dead_code)]
#[path = "../rust/src/qcom_wlan_power.rs"]
pub mod qcom_wlan_power;
#[path = "../rust/src/qcom_glink_qmi.rs"]
pub mod qcom_glink_qmi;
#[path = "../rust/src/qcom_ipa.rs"]
pub mod qcom_ipa;
#[path = "../rust/src/qcom_ipa_client.rs"]
pub mod qcom_ipa_client;
#[path = "../rust/src/qcom_ipa_service.rs"]
#[allow(dead_code)]
pub mod qcom_ipa_service;
#[path = "../rust/src/qcom_modem_handlers.rs"]
pub mod qcom_modem_handlers;
#[path = "../rust/src/qcom_modem_services.rs"]
pub mod qcom_modem_services;
#[path = "../rust/src/qcom_msa.rs"]
pub mod qcom_msa;
#[path = "../rust/src/qcom_qmi.rs"]
pub mod qcom_qmi;
#[path = "../rust/src/qcom_qmi_router.rs"]
pub mod qcom_qmi_router;
#[path = "../rust/src/qcom_qrtr_control.rs"]
pub mod qcom_qrtr_control;
#[path = "../rust/src/qcom_servreg.rs"]
pub mod qcom_servreg;
#[path = "../rust/src/qcom_servreg_notif.rs"]
pub mod qcom_servreg_notif;
#[path = "../rust/src/qcom_servreg_notif_client.rs"]
pub mod qcom_servreg_notif_client;
#[path = "../rust/src/qcom_tftp.rs"]
pub mod qcom_tftp;
#[path = "../rust/src/qcom_wlfw_bdf.rs"]
pub mod qcom_wlfw_bdf;
#[path = "../rust/src/qcom_wlfw_client.rs"]
pub mod qcom_wlfw_client;
#[path = "../rust/src/sunfish_ipa_profile.rs"]
pub mod sunfish_ipa_profile;
#[path = "../rust/src/wlfw_protocol.rs"]
pub mod wlfw_protocol;
use fixture::*;
use qcom_glink_qmi::{Config, Event, Published, RequestHandle, Session};
use qcom_modem_handlers::*;
use qcom_modem_services::{HEAP_BUFFER_BYTES, STORAGE_BUFFER_BYTES, Service};

fn field(kind: u8, bytes: &[u8]) -> Vec<u8> {
    let mut b = vec![kind];
    b.extend((bytes.len() as u16).to_le_bytes());
    b.extend(bytes);
    b
}
fn u32f(kind: u8, value: u32) -> Vec<u8> {
    field(kind, &value.to_le_bytes())
}
fn new_worker() -> (FakeBackend, Handlers<FakeBackend>) {
    let c = config(8192, 8192, 16384);
    let b = backend(c);
    let h = Handlers::attach(Profile::validate(c, &[]).unwrap(), b.clone());
    assert_eq!(h.error(), None);
    b.clear();
    (b, h)
}

#[test]
fn handler_dispatch_claims_only_storage_and_heap_services() {
    let (_backend, handlers) = new_worker();
    assert!(handlers.handles_service(Service::RemoteStorage));
    assert!(handlers.handles_service(Service::HeapMemory));
    assert!(!handlers.handles_service(Service::ServiceRegistry));
    assert!(!handlers.handles_service(Service::IpaServer));
}

struct Pending {
    session: Session,
    handle: RequestHandle,
}
fn begin(
    w: &mut Handlers<FakeBackend>,
    service: Service,
    id: u16,
    fields: &[Vec<u8>],
    peer: u32,
) -> Pending {
    let mut s = Session::new(
        Config {
            local_node: 1,
            peer_node: 0,
            local_channel: 1,
            remote_channel: 2,
        },
        1,
    )
    .unwrap();
    s.register(1, Service::RemoteStorage, 0x4000).unwrap();
    s.register(1, Service::HeapMemory, 0x4001).unwrap();
    let intent = s.offer_receive(1).unwrap();
    s.publish_next(1, |_| Ok(())).unwrap();
    let payload: Vec<u8> = fields.iter().flatten().copied().collect();
    let mut qmi = vec![0, 0x34, 0x12];
    qmi.extend(id.to_le_bytes());
    qmi.extend((payload.len() as u16).to_le_bytes());
    qmi.extend(payload);
    let mut packet: Vec<u8> = [
        1,
        1,
        0,
        peer,
        0,
        qmi.len() as u32,
        1,
        if service == Service::RemoteStorage {
            0x4000
        } else {
            0x4001
        },
    ]
    .iter()
    .flat_map(|w| w.to_le_bytes())
    .collect();
    packet.extend(qmi);
    packet.resize((packet.len() + 3) & !3, 0x5a);
    let mut record: Vec<u8> = [0x20009u32, intent, packet.len() as u32, 0]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    record.extend(packet);
    record.resize((record.len() + 7) & !7, 0xa5);
    let Event::Request(handle) = s.ingest(1, &record).unwrap() else {
        panic!("not a request")
    };
    let (context, request) = s.request(&handle).unwrap();
    w.begin(handle, context, request).unwrap();
    Pending { session: s, handle }
}
fn drive(w: &mut Handlers<FakeBackend>, b: &FakeBackend) -> Result<(), Error> {
    for _ in 0..4096 {
        let before = b.action_count();
        let result = w.step()?;
        assert!(b.action_count() - before <= 1, "one IO action per step");
        if result == Progress::ReplyReady {
            return Ok(());
        }
    }
    panic!("bounded action failed to finish")
}
fn finish(w: &mut Handlers<FakeBackend>, b: &FakeBackend, mut p: Pending) -> Response {
    drive(w, b).unwrap();
    let response = w.response().unwrap().clone();
    p.session.queue_reply(&p.handle, response.reply()).unwrap();
    w.reply_queued(&p.handle).unwrap();
    // Check the actual response encoder with an independent peer intent.
    let mut intents: Vec<u8> = [0x20005u32, 1, 4096, 0]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    p.session.ingest(1, &intents).unwrap();
    intents.clear();
    for _ in 0..4 {
        let mut out = Vec::new();
        let event = p
            .session
            .publish_next(1, |r| {
                out.extend(r);
                Ok(())
            })
            .unwrap();
        if matches!(event, Some(Published::Data { .. })) {
            assert_eq!(&out[49..51], &0x1234u16.to_le_bytes());
            let body = u32::from_le_bytes(out[36..40].try_into().unwrap()) as usize;
            let message = qcom_qmi::Message::decode(&out[48..48 + body]).unwrap();
            let result = message.required(2).unwrap();
            assert_eq!(
                u16::from_le_bytes(result[0..2].try_into().unwrap()),
                u16::from(response.error().is_some())
            );
            assert_eq!(
                u16::from_le_bytes(result[2..4].try_into().unwrap()),
                response.error().unwrap_or(0)
            );
            return response;
        }
    }
    panic!("no response datagram")
}
fn request(
    w: &mut Handlers<FakeBackend>,
    b: &FakeBackend,
    s: Service,
    id: u16,
    f: &[Vec<u8>],
) -> Response {
    let p = begin(w, s, id, f, 9);
    finish(w, b, p)
}
fn open(w: &mut Handlers<FakeBackend>, b: &FakeBackend, path: &[u8]) -> u32 {
    let r = request(w, b, Service::RemoteStorage, 1, &[field(1, path)]);
    assert_eq!(r.error(), None);
    u32::from_le_bytes(r.fields()[3..7].try_into().unwrap())
}
fn alloc_storage(w: &mut Handlers<FakeBackend>, b: &FakeBackend, h: u32, bytes: u32) -> Response {
    request(
        w,
        b,
        Service::RemoteStorage,
        4,
        &[u32f(1, h), u32f(2, bytes)],
    )
}
fn mem_alloc(w: &mut Handlers<FakeBackend>, b: &FakeBackend, bytes: u32, seq: u32) -> Response {
    request(
        w,
        b,
        Service::HeapMemory,
        0x22,
        &[u32f(1, bytes), u32f(2, 1), u32f(3, 0), u32f(4, seq)],
    )
}
fn blocks(entries: &[(u64, u32)]) -> Vec<u8> {
    let mut v = vec![entries.len() as u8];
    for (at, n) in entries {
        v.extend(at.to_le_bytes());
        v.extend(n.to_le_bytes());
    }
    field(1, &v)
}
fn free(w: &mut Handlers<FakeBackend>, b: &FakeBackend, entries: &[(u64, u32)]) -> Response {
    request(w, b, Service::HeapMemory, 0x23, &[blocks(entries)])
}
fn rw_fields(handle: u32, write: bool, spans: &[(u32, u32, u32)], force: bool) -> Vec<Vec<u8>> {
    let mut v = vec![spans.len() as u8];
    for (sector, address, n) in spans {
        v.extend(sector.to_le_bytes());
        v.extend(address.to_le_bytes());
        v.extend(n.to_le_bytes());
    }
    vec![
        u32f(1, handle),
        field(2, &[u8::from(write)]),
        field(3, &v),
        field(4, &[u8::from(force)]),
    ]
}

#[test]
fn configuration_checks_guards_overlap_exclusions_and_durability() {
    let good = config(8192, 8192, 16384);
    assert!(Profile::validate(good, &[]).is_ok());
    for case in 0..9 {
        let mut c = good;
        match case {
            0 => c.storage.base += 1,
            1 => c.heap.base = c.storage.base + u64::from(c.storage.bytes),
            2 => c.storage.bytes = STORAGE_BUFFER_BYTES + 4096,
            3 => c.heap.bytes = HEAP_BUFFER_BYTES + 4096,
            4 => c.heap.base = u64::MAX - 4095,
            5 => c.disks[0].durable = false,
            6 => c.storage_port = c.memory_port,
            7 => c.generation = 0,
            _ => c.peer_node = c.local_node,
        }
        assert!(Profile::validate(c, &[]).is_err(), "{case}");
    }
    assert!(
        Profile::validate(
            good,
            &[Region {
                base: good.storage.base - 4096,
                bytes: 4096
            }]
        )
        .is_err()
    );
    assert!(
        Profile::validate(
            good,
            &[Region {
                base: u64::MAX,
                bytes: 1
            }]
        )
        .is_err()
    );
}

#[test]
fn ram_shadow_requires_explicit_matching_mode_and_accepts_sync_within_that_boot() {
    let mut c = config(8192, 8192, 16384);
    c.disks.iter_mut().for_each(|disk| { disk.writable = true; disk.durable = false; });
    assert!(Profile::validate(c, &[]).is_err());
    let b = backend(c);
    let mismatched = Handlers::attach(Profile::validate_ram_shadow(c, &[]).unwrap(), b.clone());
    assert_eq!(mismatched.error(), Some(Error::Configuration));
    b.0.borrow_mut().persistence = Persistence::RamShadow;
    let mut w = Handlers::attach(Profile::validate_ram_shadow(c, &[]).unwrap(), b.clone());
    assert_eq!(w.error(), None);
    let handle = open(&mut w, &b, b"/boot/modem_fs1");
    assert_eq!(alloc_storage(&mut w, &b, handle, 8192).error(), None);
    let base = c.storage.base as u32;
    b.0.borrow_mut().storage[..512].fill(0x77);
    assert_eq!(request(&mut w, &b, Service::RemoteStorage, 3,
        &rw_fields(handle, true, &[(0, base, 1)], true)).error(), None);
    assert_eq!(&b.0.borrow().disks[0][..512], &[0x77; 512]);
    assert_eq!(b.count(Tag::Sync), 1);
    // A later mode change invalidates the retained worker before data I/O.
    b.0.borrow_mut().persistence = Persistence::Durable;
    assert_eq!(w.validate(), Err(Error::Configuration));
}
#[test]
fn failed_backend_initialization_stays_retained_and_unavailable() {
    let c = config(8192, 8192, 16384);
    let b = backend(c);
    b.0.borrow_mut().observation.storage = Ownership::Unknown;
    let mut w = Handlers::attach(Profile::validate(c, &[]).unwrap(), b.clone());
    assert!(w.error().is_some());
    assert_eq!(w.validate(), Err(Error::Stopped));
    assert_eq!(b.action_count(), 0);
}
#[test]
fn backend_disk_metadata_must_match_before_service_is_ready() {
    let c = config(8192, 8192, 16384);
    let b = backend(c);
    let mut mismatch = c;
    mismatch.disks[0].bytes += 512;
    let w = Handlers::attach(Profile::validate(mismatch, &[]).unwrap(), b.clone());
    assert_eq!(w.error(), Some(Error::Configuration));
    assert_eq!(b.action_count(), 0);
}
#[test]
fn opens_all_allowlisted_images_and_never_recycles_closed_handles() {
    let (b, mut w) = new_worker();
    let mut ids = Vec::new();
    for name in [
        b"/boot/modem_fs1".as_slice(),
        b"/boot/modem_fs2",
        b"/boot/modem_fsg",
        b"/boot/modem_fsc",
    ] {
        ids.push(open(&mut w, &b, name));
    }
    assert_eq!(ids, [1, 2, 3, 4]);
    for _ in 4..HANDLE_SLOTS {
        open(&mut w, &b, b"/boot/modem_fs1");
    }
    let failed = request(
        &mut w,
        &b,
        Service::RemoteStorage,
        1,
        &[field(1, b"/boot/modem_fs1")],
    );
    assert_eq!(failed.error(), Some(1));
    assert_eq!(
        request(&mut w, &b, Service::RemoteStorage, 2, &[u32f(1, 1)]).error(),
        None
    );
    assert_eq!(open(&mut w, &b, b"/boot/modem_fs1"), 9);
    assert_eq!(
        request(&mut w, &b, Service::RemoteStorage, 5, &[u32f(1, 1)]).error(),
        Some(1)
    );
    assert_eq!(b.action_count(), 0);
}
#[test]
fn handles_are_bound_to_the_full_peer_address() {
    let (b, mut w) = new_worker();
    let h = open(&mut w, &b, b"/boot/modem_fs1");
    for id in [2, 4, 5] {
        let f = if id == 4 {
            vec![u32f(1, h), u32f(2, 4096)]
        } else {
            vec![u32f(1, h)]
        };
        let p = begin(&mut w, Service::RemoteStorage, id, &f, 10);
        assert_eq!(finish(&mut w, &b, p).error(), Some(1));
    }
    assert_eq!(w.open_handles(), 1);
    assert_eq!(b.action_count(), 0);
}
#[test]
fn storage_allocation_scrubs_before_confirmed_grant_and_returns_u64() {
    let (b, mut w) = new_worker();
    let h = open(&mut w, &b, b"/boot/modem_fs1");
    let p = begin(
        &mut w,
        Service::RemoteStorage,
        4,
        &[u32f(1, h), u32f(2, 8192)],
        9,
    );
    assert_eq!(w.step(), Ok(Progress::Progress));
    assert!(w.response().is_none());
    assert_eq!(b.count(Tag::Grant), 0);
    let r = finish(&mut w, &b, p);
    assert_eq!(r.error(), None);
    assert_eq!(
        r.fields(),
        &[
            vec![0x10, 8, 0],
            b.0.borrow().config.storage.base.to_le_bytes().to_vec()
        ]
        .concat()
    );
    assert_eq!(b.count(Tag::Zero), 2);
    assert_eq!(b.count(Tag::Grant), 1);
    assert!(w.leased(Pool::Storage));
    assert!(b.0.borrow().storage.iter().all(|x| *x == 0));
}
#[test]
fn storage_lease_is_shared_by_same_peer_handles_and_closed_only_after_the_last() {
    let (b, mut w) = new_worker();
    let a = open(&mut w, &b, b"/boot/modem_fs1");
    let c = open(&mut w, &b, b"/boot/modem_fs2");
    alloc_storage(&mut w, &b, a, 8192);
    let base = b.0.borrow().config.storage.base as u32;
    assert_eq!(
        request(
            &mut w,
            &b,
            Service::RemoteStorage,
            3,
            &rw_fields(c, false, &[(0, base, 1)], false)
        )
        .error(),
        None
    );
    assert_eq!(&b.0.borrow().storage[..512], &b.0.borrow().disks[1][..512]);
    request(&mut w, &b, Service::RemoteStorage, 2, &[u32f(1, a)]);
    assert_eq!(b.count(Tag::Reclaim), 0);
    assert_eq!(alloc_storage(&mut w, &b, c, 4096).error(), None);
    assert_eq!(b.count(Tag::Grant), 1);
    request(&mut w, &b, Service::RemoteStorage, 2, &[u32f(1, c)]);
    assert_eq!(b.count(Tag::Reclaim), 1);
    assert!(!w.leased(Pool::Storage));
}
#[test]
fn full_iovec_preflight_rejects_invalid_last_entry_before_any_io() {
    for bad in 0..5 {
        let (b, mut w) = new_worker();
        let h = open(&mut w, &b, b"/boot/modem_fs1");
        alloc_storage(&mut w, &b, h, 8192);
        let base = b.0.borrow().config.storage.base as u32;
        let second = match bad {
            0 => (2, base + 8192, 1),
            1 => (32, base + 512, 1),
            2 => (2, base, 1),
            3 => (0, base + 512, 1),
            _ => (2, base + 512, 0),
        };
        b.clear();
        let r = request(
            &mut w,
            &b,
            Service::RemoteStorage,
            3,
            &rw_fields(h, false, &[(0, base, 1), second], false),
        );
        assert_eq!(r.error(), Some(1));
        assert_eq!(b.action_count(), 0, "case {bad}");
    }
}
#[test]
fn read_and_write_use_private_bounded_chunks_and_sync_before_success() {
    let (b, mut w) = new_worker();
    let h = open(&mut w, &b, b"/boot/modem_fs1");
    alloc_storage(&mut w, &b, h, 8192);
    let base = b.0.borrow().config.storage.base as u32;
    let r = request(
        &mut w,
        &b,
        Service::RemoteStorage,
        3,
        &rw_fields(h, false, &[(1, base, 8), (12, base + 4096, 2)], false),
    );
    assert_eq!(r.error(), None);
    assert_eq!(
        &b.0.borrow().storage[..4096],
        &b.0.borrow().disks[0][512..4608]
    );
    b.0.borrow_mut().storage[..8192].fill(0x6d);
    let p = begin(
        &mut w,
        Service::RemoteStorage,
        3,
        &rw_fields(h, true, &[(0, base, 16)], false),
        9,
    );
    assert_eq!(w.step(), Ok(Progress::Progress));
    assert_eq!(w.step(), Ok(Progress::Progress));
    assert_ne!(b.0.borrow().disks[0], b.0.borrow().committed[0]);
    let r = finish(&mut w, &b, p);
    assert_eq!(r.error(), None);
    assert_eq!(b.count(Tag::Sync), 1);
    assert_eq!(b.0.borrow().disks[0], b.0.borrow().committed[0]);
    assert!(
        b.0.borrow()
            .log
            .iter()
            .filter(|op| matches!(
                op.tag,
                Tag::DiskRead | Tag::DiskWrite | Tag::PoolRead | Tag::PoolWrite | Tag::Zero
            ))
            .all(|op| op.bytes <= IO_BYTES)
    );
}
#[test]
fn staged_write_does_not_reread_peer_memory_before_disk_write() {
    let (b, mut w) = new_worker();
    let h = open(&mut w, &b, b"/boot/modem_fs1");
    alloc_storage(&mut w, &b, h, 4096);
    let base = b.0.borrow().config.storage.base as u32;
    b.0.borrow_mut().storage[..512].fill(0x11);
    let p = begin(
        &mut w,
        Service::RemoteStorage,
        3,
        &rw_fields(h, true, &[(0, base, 1)], true),
        9,
    );
    w.step().unwrap();
    b.0.borrow_mut().storage[..512].fill(0x22);
    finish(&mut w, &b, p);
    assert!(b.0.borrow().committed[0][..512].iter().all(|b| *b == 0x11));
}
#[test]
fn readonly_and_nondurable_operations_are_rejected_before_io() {
    let (b, mut w) = new_worker();
    let h = open(&mut w, &b, b"/boot/modem_fsg");
    alloc_storage(&mut w, &b, h, 4096);
    let base = b.0.borrow().config.storage.base as u32;
    b.clear();
    for (write, force) in [(true, false), (false, true)] {
        assert_eq!(
            request(
                &mut w,
                &b,
                Service::RemoteStorage,
                3,
                &rw_fields(h, write, &[(0, base, 1)], force)
            )
            .error(),
            Some(1)
        );
    }
    assert_eq!(b.action_count(), 0);
}
#[test]
fn device_error_reports_a_known_io_failure_without_claiming_partial_write_success() {
    let (b, mut w) = new_worker();
    let h = open(&mut w, &b, b"/boot/modem_fs1");
    alloc_storage(&mut w, &b, h, 8192);
    let base = b.0.borrow().config.storage.base as u32;
    b.0.borrow_mut().storage.fill(0x33);
    let p = begin(
        &mut w,
        Service::RemoteStorage,
        3,
        &rw_fields(h, true, &[(0, base, 16)], false),
        9,
    );
    w.step().unwrap();
    w.step().unwrap();
    w.step().unwrap();
    b.0.borrow_mut().fail = Some((Tag::DiskWrite, Fault::Failed));
    let r = finish(&mut w, &b, p);
    assert_eq!(r.error(), Some(1));
    assert_eq!(b.count(Tag::Sync), 0);
    assert_ne!(b.0.borrow().disks[0], b.0.borrow().committed[0]);
    let r = request(&mut w, &b, Service::RemoteStorage, 5, &[u32f(1, h)]);
    assert_eq!(r.fields(), &[0x10, 1, 0, 1]);
}
#[test]
fn sync_failure_returns_error_and_keeps_the_lease() {
    let (b, mut w) = new_worker();
    let h = open(&mut w, &b, b"/boot/modem_fs1");
    alloc_storage(&mut w, &b, h, 4096);
    let base = b.0.borrow().config.storage.base as u32;
    b.0.borrow_mut().fail = Some((Tag::Sync, Fault::Failed));
    assert_eq!(
        request(
            &mut w,
            &b,
            Service::RemoteStorage,
            3,
            &rw_fields(h, true, &[(0, base, 1)], true)
        )
        .error(),
        Some(1)
    );
    assert!(w.leased(Pool::Storage));
    assert_eq!(w.error(), None);
}
#[test]
fn heap_query_uses_client_and_processor_policy_and_encodes_size() {
    let (b, mut w) = new_worker();
    let r = request(&mut w, &b, Service::HeapMemory, 0x24, &[u32f(1, 1)]);
    assert_eq!(r.fields(), &[0x10, 4, 0, 0, 0x20, 0, 0]);
    for f in [
        vec![u32f(1, 0)],
        vec![u32f(1, 2)],
        vec![u32f(1, 1), u32f(0x10, 1)],
    ] {
        assert_eq!(
            request(&mut w, &b, Service::HeapMemory, 0x24, &f).error(),
            Some(41)
        );
    }
    assert_eq!(b.action_count(), 0);
}
#[test]
fn heap_success_encodes_sequence_and_one_exact_block_after_ownership_confirmation() {
    let (b, mut w) = new_worker();
    let r = mem_alloc(&mut w, &b, 5000, 0x12345678);
    assert_eq!(r.error(), None);
    let base = b.0.borrow().config.heap.base;
    let expected = [
        vec![0x10, 4, 0, 0x78, 0x56, 0x34, 0x12, 0x11, 13, 0, 1],
        base.to_le_bytes().to_vec(),
        5000u32.to_le_bytes().to_vec(),
    ]
    .concat();
    assert_eq!(r.fields(), expected);
    assert_eq!(b.0.borrow().observation.heap, Ownership::ModemExclusive);
    assert_eq!(b.count(Tag::PoolRead) + b.count(Tag::PoolWrite), 0);
    assert_eq!(b.count(Tag::Zero), 2);
}
#[test]
fn heap_allocation_is_idempotent_only_for_the_same_live_lease() {
    let (b, mut w) = new_worker();
    let first = mem_alloc(&mut w, &b, 4096, 7);
    let actions = b.action_count();
    let again = mem_alloc(&mut w, &b, 4096, 7);
    assert_eq!(first.fields(), again.fields());
    assert_eq!(b.action_count(), actions);
    assert!(mem_alloc(&mut w, &b, 4096, 8).error().is_some());
    assert!(mem_alloc(&mut w, &b, 8192, 7).error().is_some());
    assert_eq!(b.action_count(), actions);
}
#[test]
fn heap_rejects_other_clients_processors_and_noncontiguous_policy() {
    let (b, mut w) = new_worker();
    for (client, proc, contiguous, error) in
        [(0, 0, None, 41), (1, 1, None, 41), (1, 0, Some(false), 94)]
    {
        let mut fields = vec![u32f(1, 4096), u32f(2, client), u32f(3, proc), u32f(4, 1)];
        if let Some(c) = contiguous {
            fields.push(field(0x10, &[u8::from(c)]));
        }
        assert_eq!(
            request(&mut w, &b, Service::HeapMemory, 0x22, &fields).error(),
            Some(error)
        );
    }
    assert_eq!(b.action_count(), 0);
}
#[test]
fn free_requires_exact_owned_block_and_omitted_optionals_do_not_erase_ownership() {
    let (b, mut w) = new_worker();
    mem_alloc(&mut w, &b, 4096, 1);
    let base = b.0.borrow().config.heap.base;
    let actions = b.action_count();
    for entries in [
        vec![],
        vec![(base + 1, 4096)],
        vec![(base, 4095)],
        vec![(base, 4096), (base, 4096)],
    ] {
        assert!(free(&mut w, &b, &entries).error().is_some());
    }
    let p = begin(
        &mut w,
        Service::HeapMemory,
        0x23,
        &[blocks(&[(base, 4096)])],
        10,
    );
    assert!(finish(&mut w, &b, p).error().is_some());
    assert_eq!(b.action_count(), actions);
    assert_eq!(free(&mut w, &b, &[(base, 4096)]).error(), None);
    assert!(!w.leased(Pool::Heap));
}
#[test]
fn freed_heap_pages_are_not_reused_and_old_free_cannot_release_new_lease() {
    let (b, mut w) = new_worker();
    mem_alloc(&mut w, &b, 4096, 1);
    let base = b.0.borrow().config.heap.base;
    free(&mut w, &b, &[(base, 4096)]);
    assert!(mem_alloc(&mut w, &b, 4096, 1).error().is_some());
    let newer = mem_alloc(&mut w, &b, 4096, 2);
    assert_eq!(&newer.fields()[11..19], &(base + 4096).to_le_bytes());
    let count = b.count(Tag::Reclaim);
    assert!(free(&mut w, &b, &[(base, 4096)]).error().is_some());
    assert_eq!(b.count(Tag::Reclaim), count);
    assert!(w.leased(Pool::Heap));
    free(&mut w, &b, &[(base + 4096, 4096)]);
    assert_eq!(mem_alloc(&mut w, &b, 4096, 3).error(), Some(2));
}
#[test]
fn all_supported_alignments_return_page_aligned_regions() {
    for alignment in 0..=11 {
        let (b, mut w) = new_worker();
        let r = request(
            &mut w,
            &b,
            Service::HeapMemory,
            0x22,
            &[
                u32f(1, 4096),
                u32f(2, 1),
                u32f(3, 0),
                u32f(4, 1),
                u32f(0x11, alignment),
            ],
        );
        assert_eq!(r.error(), None);
        let base = u64::from_le_bytes(r.fields()[11..19].try_into().unwrap());
        assert_eq!(base % (1u64 << (alignment + 1)), 0);
    }
}
#[test]
fn definite_allocation_failure_returns_no_address_and_uncertain_failure_latches_stop() {
    for tag in [Tag::Zero, Tag::Grant] {
        for fault in [Fault::Failed, Fault::Uncertain] {
            let (b, mut w) = new_worker();
            b.0.borrow_mut().fail = Some((tag, fault));
            let p = begin(
                &mut w,
                Service::HeapMemory,
                0x22,
                &[u32f(1, 4096), u32f(2, 1), u32f(3, 0), u32f(4, 1)],
                9,
            );
            if fault == Fault::Failed {
                let r = finish(&mut w, &b, p);
                assert_eq!(r.error(), Some(3));
                assert!(r.fields().is_empty());
                assert!(!w.leased(Pool::Heap));
            } else {
                assert_eq!(drive(&mut w, &b), Err(Error::BackendUncertain));
                assert!(w.response().is_none());
                let count = b.action_count();
                assert_eq!(w.step(), Err(Error::Stopped));
                assert_eq!(b.action_count(), count);
            }
        }
    }
}
#[test]
fn bad_grant_receipt_and_unconfirmed_reclaim_retain_the_reservation() {
    for reclaim in [false, true] {
        let (b, mut w) = new_worker();
        if !reclaim {
            b.0.borrow_mut().lie_grant = true;
            let _p = begin(
                &mut w,
                Service::HeapMemory,
                0x22,
                &[u32f(1, 4096), u32f(2, 1), u32f(3, 0), u32f(4, 1)],
                9,
            );
            assert_eq!(drive(&mut w, &b), Err(Error::Ownership));
        } else {
            mem_alloc(&mut w, &b, 4096, 1);
            b.0.borrow_mut().lie_reclaim = true;
            let base = b.0.borrow().config.heap.base;
            let _p = begin(
                &mut w,
                Service::HeapMemory,
                0x23,
                &[blocks(&[(base, 4096)])],
                9,
            );
            assert_eq!(drive(&mut w, &b), Err(Error::Ownership));
        }
        assert!(w.leased(Pool::Heap));
        assert!(w.response().is_none());
        assert!(b.0.borrow().grants[1].is_some());
    }
}
#[test]
fn definite_failed_reclaim_does_not_free_a_live_heap_lease() {
    let (b, mut w) = new_worker();
    mem_alloc(&mut w, &b, 4096, 1);
    let base = b.0.borrow().config.heap.base;
    b.0.borrow_mut().fail = Some((Tag::Reclaim, Fault::Failed));
    assert_eq!(free(&mut w, &b, &[(base, 4096)]).error(), Some(3));
    assert!(w.leased(Pool::Heap));
    assert_eq!(b.0.borrow().observation.heap, Ownership::ModemExclusive);
}
#[test]
fn live_generation_identity_ready_and_receipt_changes_stop_before_action() {
    for bad in 0..4 {
        let (b, mut w) = new_worker();
        mem_alloc(&mut w, &b, 4096, 1);
        match bad {
            0 => b.0.borrow_mut().observation.generation = 2,
            1 => b.0.borrow_mut().observation.identity = 72,
            2 => b.0.borrow_mut().observation.ready = false,
            _ => b.0.borrow_mut().corrupt_info = true,
        }
        let actions = b.action_count();
        assert!(w.validate().is_err());
        assert_eq!(b.action_count(), actions);
        assert!(w.leased(Pool::Heap));
    }
}
#[test]
fn unwinding_io_is_never_retried() {
    let (b, mut w) = new_worker();
    let h = open(&mut w, &b, b"/boot/modem_fs1");
    alloc_storage(&mut w, &b, h, 4096);
    let base = b.0.borrow().config.storage.base as u32;
    let _p = begin(
        &mut w,
        Service::RemoteStorage,
        3,
        &rw_fields(h, false, &[(0, base, 1)], false),
        9,
    );
    b.0.borrow_mut().panic = Some(Tag::DiskRead);
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| w.step()));
    assert!(r.is_err());
    let actions = b.action_count();
    assert_eq!(w.step(), Err(Error::Stopped));
    assert_eq!(b.action_count(), actions);
    assert!(w.leased(Pool::Storage));
}
#[test]
fn maximum_observed_storage_and_heap_requests_remain_chunk_bounded() {
    let c = config(
        STORAGE_BUFFER_BYTES,
        HEAP_BUFFER_BYTES,
        u64::from(STORAGE_BUFFER_BYTES),
    );
    let b = backend(c);
    let mut w = Handlers::attach(Profile::validate(c, &[]).unwrap(), b.clone());
    let h = open(&mut w, &b, b"/boot/modem_fs1");
    assert_eq!(
        alloc_storage(&mut w, &b, h, STORAGE_BUFFER_BYTES).error(),
        None
    );
    let base = c.storage.base as u32;
    assert_eq!(
        request(
            &mut w,
            &b,
            Service::RemoteStorage,
            3,
            &rw_fields(h, false, &[(0, base, 4096)], false)
        )
        .error(),
        None
    );
    assert_eq!(mem_alloc(&mut w, &b, HEAP_BUFFER_BYTES, 1).error(), None);
    assert_eq!(&b.0.borrow().storage[..], &b.0.borrow().disks[0][..]);
    assert!(
        b.0.borrow()
            .log
            .iter()
            .filter(|op| matches!(op.tag, Tag::Zero | Tag::DiskRead | Tag::PoolWrite))
            .all(|op| op.bytes <= IO_BYTES)
    );
}

#[test]
fn unallocated_or_cross_pool_storage_requests_cannot_touch_backing_data() {
    let (b, mut w) = new_worker();
    let h = open(&mut w, &b, b"/boot/modem_fs1");
    let base = b.0.borrow().config.storage.base as u32;
    assert_eq!(
        request(
            &mut w,
            &b,
            Service::RemoteStorage,
            3,
            &rw_fields(h, false, &[(0, base, 1)], false)
        )
        .error(),
        Some(1)
    );
    assert_eq!(b.action_count(), 0);
    alloc_storage(&mut w, &b, h, 4096);
    let heap = b.0.borrow().config.heap.base as u32;
    b.clear();
    assert_eq!(
        request(
            &mut w,
            &b,
            Service::RemoteStorage,
            3,
            &rw_fields(h, true, &[(0, heap, 1)], false)
        )
        .error(),
        Some(1)
    );
    assert_eq!(b.action_count(), 0);
}
#[test]
fn mismatched_request_context_stops_without_starting_an_action() {
    for bad in 0..4 {
        let (b, mut w) = new_worker();
        let p = begin(&mut w, Service::HeapMemory, 0x24, &[u32f(1, 1)], 9);
        // Retire the no-action query, then use its authentic sealed handle with
        // deliberately corrupted local routing metadata at the trusted boundary.
        let (mut context, request) = p.session.request(&p.handle).unwrap();
        w.step().unwrap();
        w.reply_queued(&p.handle).unwrap();
        match bad {
            0 => context.generation = 2,
            1 => context.peer.node = 1,
            2 => context.local.port = 0x4000,
            _ => context.message_id = 0x22,
        }
        let actions = b.action_count();
        assert_eq!(
            w.begin(p.handle, context, request),
            Err(Error::InvalidRequest)
        );
        assert_eq!(b.action_count(), actions);
        assert_eq!(w.active_handle(), None);
    }
}
#[test]
fn uncertain_transfer_sync_or_reclaim_never_produces_success_or_retries() {
    for tag in [
        Tag::DiskRead,
        Tag::PoolWrite,
        Tag::PoolRead,
        Tag::DiskWrite,
        Tag::Sync,
        Tag::Reclaim,
    ] {
        let (b, mut w) = new_worker();
        let h = open(&mut w, &b, b"/boot/modem_fs1");
        alloc_storage(&mut w, &b, h, 4096);
        let base = b.0.borrow().config.storage.base as u32;
        let _p = if tag == Tag::Reclaim {
            begin(&mut w, Service::RemoteStorage, 2, &[u32f(1, h)], 9)
        } else {
            begin(
                &mut w,
                Service::RemoteStorage,
                3,
                &rw_fields(
                    h,
                    matches!(tag, Tag::PoolRead | Tag::DiskWrite | Tag::Sync),
                    &[(0, base, 1)],
                    false,
                ),
                9,
            )
        };
        b.0.borrow_mut().fail = Some((tag, Fault::Uncertain));
        assert_eq!(drive(&mut w, &b), Err(Error::BackendUncertain));
        assert!(w.response().is_none());
        assert!(w.leased(Pool::Storage));
        let count = b.action_count();
        assert_eq!(w.step(), Err(Error::Stopped));
        assert_eq!(b.action_count(), count);
    }
}
