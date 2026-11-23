use super::{Config, LoRig, OwnerError, Service, Session, lo_message, lo_packet, lo_word};
use super::{qcom_modem_owner::ServiceProgress as SP, sunfish_linux_rmtfs_server as server};

#[cfg(feature = "sunfish-native-startup-mpss-runtime")]
#[test]
fn post_auth_runtime_keeps_rmtfs_unavailable_until_bound_and_stages_servreg_once() {
    let runtime = Session::native_runtime_profile(
        Config {
            local_node: 1,
            peer_node: 0,
            local_channel: 1,
            remote_channel: 2,
        },
        1,
    )
    .unwrap();
    let peer = super::Fake::new(0, 0);
    let adapter = super::Adapter::attach(peer, runtime, 1).unwrap();
    let mut owner = super::Owner::attach_linux_ipa(adapter, 1, 0x415, 1000).unwrap();
    assert_eq!(owner.control_status().ready_services, 0);

    owner.prepare_service_registry().unwrap();
    assert_eq!(owner.control_status().ready_services, 0b100);
    assert_eq!(owner.control_status().announced_services, 0);
    owner.start_control().unwrap();

    let sim = Sim::new();
    let (config, source, mut destination) = shadows();
    let mut rmtfs = sr_server(&sim, config, &source, &mut destination);
    let mut irqs = super::Irqs::default();
    owner.activate_linux_rmtfs(&mut rmtfs, &mut irqs).unwrap();
    assert_eq!(owner.control_status().ready_services, 0b101);
    assert_eq!(owner.control_status().announced_services, 0);
    assert_eq!(rmtfs.bound(), Some(owner.identity()));
}

#[cfg(feature = "sunfish-native-startup-mpss-runtime")]
#[test]
fn post_auth_runtime_rejects_duplicate_servreg_preparation() {
    let runtime = Session::native_runtime_profile(
        Config {
            local_node: 1,
            peer_node: 0,
            local_channel: 1,
            remote_channel: 2,
        },
        1,
    )
    .unwrap();
    let peer = super::Fake::new(0, 0);
    let adapter = super::Adapter::attach(peer, runtime, 1).unwrap();
    let mut owner = super::Owner::attach_linux_ipa(adapter, 1, 0x415, 1000).unwrap();
    owner.prepare_service_registry().unwrap();
    assert!(owner.prepare_service_registry().is_err());
    assert!(owner.error().is_some());
}

fn sr_transport() -> LoRig {
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
    for (service, port) in [
        (Service::RemoteStorage, 0x4000),
        (Service::ServiceRegistry, 0x4003),
        (Service::IpaServer, 0x4002),
    ] {
        s.register(1, service, port).unwrap();
        if service != Service::RemoteStorage {
            s.prepare_service_available(1, service).unwrap();
        }
    }
    s.enable_linux_ipa(1, 0x4004).unwrap();
    LoRig::from_session(0, false, s)
}
fn sr_server<'a>(
    s: &Sim,
    mut c: disk::Configuration,
    src: &'a [Vec<u8>; 4],
    dst: &'a mut [Vec<u8>; 4],
) -> server::Server<'a, Sim> {
    c.generation = 1;
    s.0.borrow_mut().observation.generation = 1;
    let maps = s.0.borrow().observation.mappings;
    let mut p = r::Pool::attach(1, maps, s.clone());
    prepare(&mut p);
    p.share().unwrap();
    let [a, b, c0, d] = dst;
    let ram = RamShadow::new(
        c,
        src.each_ref().map(|v| v.as_slice()),
        [a, b, c0, d].map(|v| v.as_mut_slice()),
    )
    .unwrap();
    server::Server::attach(
        server::Configuration {
            generation: 1,
            local: super::qcom_qmi_router::Address {
                node: 1,
                port: 0x4000,
            },
            peer_node: 0,
        },
        r::Resources::attach(p, ram.into_storage().unwrap()),
    )
}
struct SrRig<'a> {
    t: LoRig,
    s: server::Server<'a, Sim>,
    txn: u16,
}
impl<'a> SrRig<'a> {
    fn new(
        s: &Sim,
        c: disk::Configuration,
        src: &'a [Vec<u8>; 4],
        dst: &'a mut [Vec<u8>; 4],
    ) -> Self {
        let mut this = Self {
            t: sr_transport(),
            s: sr_server(s, c, src, dst),
            txn: 100,
        };
        this.t.recycle_intents = true;
        this.t
            .owner
            .activate_linux_rmtfs(&mut this.s, &mut this.t.irqs)
            .unwrap();
        this.drive().unwrap();
        this
    }
    fn drive(&mut self) -> Result<(), OwnerError> {
        for _ in 0..4096 {
            let p = self.t.owner.notified(8, &mut self.t.irqs)?;
            let before = self.s.report().chunks;
            let service = self
                .t
                .owner
                .dispatch_linux_rmtfs_step(&mut self.s, &mut self.t.irqs)?;
            assert!(
                self.s.report().chunks - before <= 1,
                "one bounded chunk per dispatch"
            );
            let registry = self
                .t
                .owner
                .dispatch_service_registry_step(&mut self.t.irqs)?;
            let ipa = self.t.owner.service_linux_ipa_step(0, &mut self.t.irqs)?;
            let output = self.t.drain();
            let busy = matches!(service, SP::Progress | SP::ReplyQueued)
                || matches!(registry, SP::Progress | SP::ReplyQueued)
                || matches!(
                    ipa,
                    super::LP::Prepared
                        | super::LP::Queued
                        | super::LP::Committed
                        | super::LP::ResponseAccepted
                        | super::LP::Discovered
                );
            if p.received == 0 && p.published == 0 && !busy && !output {
                return Ok(());
            }
        }
        panic!("storage owner failed to yield");
    }
    fn send(&mut self, port: u32, id: u16, fields: &[(u8, &[u8])]) -> u16 {
        let txn = self.txn;
        self.txn += 1;
        self.t.enqueue(&lo_packet(
            1,
            port,
            0x4000,
            true,
            &lo_message(0, id, txn, fields),
        ));
        self.drive().unwrap();
        txn
    }
    fn message(&self, txn: u16) -> super::qcom_qmi::Message<'_> {
        let packet = self
            .t
            .host
            .iter()
            .find(|p| {
                lo_word(p, 12) == 0x4000 && u16::from_le_bytes(p[33..35].try_into().unwrap()) == txn
            })
            .expect("response sent");
        super::qcom_qmi::Message::decode(&packet[32..32 + lo_word(packet, 20) as usize]).unwrap()
    }
    fn open(&mut self, path: &[u8]) -> u32 {
        let txn = self.send(9, 1, &[(1, path)]);
        self.success(txn);
        self.message(txn).u32(0x10).unwrap()
    }
    fn success(&self, txn: u16) {
        assert_eq!(self.message(txn).get(2), Some([0, 0, 0, 0].as_slice()));
    }
    fn iov(&mut self, port: u32, handle: u32, vectors: &[(u32, u32, u32)], write: bool) -> u16 {
        let mut v = vec![vectors.len() as u8];
        for (sector, pa, count) in vectors {
            for x in [sector, pa, count] {
                v.extend(x.to_le_bytes());
            }
        }
        self.send(
            port,
            3,
            &[
                (1, &handle.to_le_bytes()),
                (2, &[u8::from(write)]),
                (3, &v),
                (4, &[0]),
            ],
        )
    }
}
#[test]
fn rmtfsserver_real_owner_storage_only_repeated_alloc_close_and_node_scoping() {
    let sim = Sim::new();
    let (c, src, mut dst) = shadows();
    let mut r = SrRig::new(&sim, c, &src, &mut dst);
    assert_eq!(r.t.owner.control_status().announced_services, 0x15);
    let h = r.open(b"/boot/modem_fs1");
    assert_eq!(h, 0);
    let txn = r.send(19, 1, &[(1, b"/boot/modem_fs1")]);
    assert_eq!(r.message(txn).u32(0x10).unwrap(), h);
    for bytes in [1u32, 4096, 0x200000] {
        let txn = r.send(19, 4, &[(1, &h.to_le_bytes()), (2, &bytes.to_le_bytes())]);
        r.success(txn);
        assert_eq!(
            u64::from_le_bytes(r.message(txn).required(0x10).unwrap().try_into().unwrap()),
            r::BUFFER_PA
        );
    }
    let txn = r.send(19, 2, &[(1, &h.to_le_bytes())]);
    r.success(txn);
    assert_eq!(r.s.resource_status().0.phase, r::Phase::Shared);
    assert_eq!(sim.count("invoke"), 1);
    let reopened = r.open(b"/boot/modem_fs1");
    assert_ne!(reopened, h);
    let txn = r.send(9, 5, &[(1, &h.to_le_bytes())]);
    assert_eq!(r.message(txn).required(2).unwrap(), [1, 0, 1, 0]);
    let txn = r.send(9, 5, &[(1, &reopened.to_le_bytes())]);
    assert_eq!(r.message(txn).get(0x10), Some([0].as_slice()));
}
#[test]
fn rmtfsserver_long_read_needs_more_than_one_native_budget() {
    let sim = Sim::new();
    let (c, src, mut dst) = shadows();
    let mut r = SrRig::new(&sim, c, &src, &mut dst);
    let h = r.open(b"/boot/modem_fs1");
    let txn = r.txn;
    r.txn += 1;
    let mut vectors = vec![20u8];
    for _ in 0..20 {
        for value in [0u32, r::BUFFER_PA as u32, 8u32] {
            vectors.extend(value.to_le_bytes());
        }
    }
    let payload = lo_message(0, 3, txn, &[
        (1, &h.to_le_bytes()), (2, &[0]), (3, &vectors), (4, &[0]),
    ]);
    r.t.enqueue(&lo_packet(1, 19, 0x4000, true, &payload));
    let before = r.s.report().chunks;
    for _ in 0..super::WORK_BUDGET {
        r.t.owner.notified(8, &mut r.t.irqs).unwrap();
        r.t.owner.dispatch_linux_rmtfs_step(&mut r.s, &mut r.t.irqs).unwrap();
        r.t.drain();
    }
    let report = r.s.report();
    assert!(report.active_request);
    assert_eq!(report.active_message, Some(3));
    assert!(report.chunks - before <= super::WORK_BUDGET as u64);
    assert_eq!(report.replies_queued, 1); // Only the earlier open replied.
    r.drive().unwrap();
    assert!(!r.s.report().active_request);
    r.success(txn);
}

#[test]
fn rmtfsserver_reads_128k_in_one_checked_step() {
    let sim = Sim::new();
    let (mut c, _, _) = shadows();
    let src: [Vec<u8>; 4] = core::array::from_fn(|n| vec![n as u8 + 1; 262144]);
    for (i, binding) in c.bindings.iter_mut().enumerate() {
        binding.source = hash(&src[i]);
        binding.disk.bytes = 262144;
    }
    let mut dst: [Vec<u8>; 4] = core::array::from_fn(|_| vec![0; 262144]);
    let mut r = SrRig::new(&sim, c, &src, &mut dst);
    let handle = r.open(b"/boot/modem_fs1");
    let before = r.s.report().chunks;
    let txn = r.iov(19, handle, &[(1, r::BUFFER_PA as u32 + 512, 256)], false);
    r.success(txn);
    assert_eq!(r.s.report().chunks - before, 1);
    let state = sim.0.borrow();
    assert_eq!(&state.bytes[512..512 + 131072], vec![1; 131072]);
    assert_eq!(state.bytes[511], 0);
    assert_eq!(state.bytes[512 + 131072], 0);
}

#[test]
fn rmtfsserver_burst_finishes_linux_sized_read_in_bounded_steps() {
    fn clock() -> u64 {
        static TICKS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
        TICKS.fetch_add(1, core::sync::atomic::Ordering::Relaxed)
    }
    for glink_first in [true, false] {
    let sim = Sim::new();
    let (mut c, _, _) = shadows();
    let src: [Vec<u8>; 4] = core::array::from_fn(|n| vec![n as u8 + 1; 0x200000]);
    for (i, binding) in c.bindings.iter_mut().enumerate() {
        binding.source = hash(&src[i]);
        binding.disk.bytes = 0x200000;
    }
    let mut dst: [Vec<u8>; 4] = core::array::from_fn(|_| vec![0; 0x200000]);
    let mut r = SrRig::new(&sim, c, &src, &mut dst);
    r.t.owner.set_rmtfs_event_clock(clock);
    let handle = r.open(b"/boot/modem_fs1");
    r.t.hold_host = true;
    let before = r.s.report().chunks;
    let replies_before = r.s.report().replies_queued;
    let txn = r.txn;
    r.txn += 1;
    let mut vectors = vec![1u8];
    for value in [2u32, r::BUFFER_PA as u32 + 1024, 4094u32] {
        vectors.extend(value.to_le_bytes());
    }
    let payload = lo_message(0, 3, txn, &[
        (1, &handle.to_le_bytes()), (2, &[0]), (3, &vectors), (4, &[0]),
    ]);
    r.t.enqueue(&lo_packet(1, 19, 0x4000, true, &payload));
    let mut completed = false;
    for _ in 0..128 {
        r.t.owner.notified(8, &mut r.t.irqs).unwrap();
        let previous = r.s.report().chunks;
        r.t.owner.dispatch_linux_rmtfs_burst(&mut r.s, &mut r.t.irqs).unwrap();
        assert!(r.s.report().chunks - previous <= 2);
        r.t.drain();
        if r.s.report().replies_queued > replies_before { completed = true; break; }
    }
    assert!(completed);
    assert_eq!(r.s.report().chunks - before, 16);
    assert!(!r.s.report().active_request);
    assert_eq!(r.t.owner.rmtfs_large_reply_flags(), 0b10001);
    let queued = r.t.owner.rmtfs_event_time(0).unwrap();
    assert_eq!(r.t.owner.rmtfs_event_time(1), None);
    assert_eq!(r.t.owner.rmtfs_event_time(4), None);
    r.drive().unwrap();
    r.success(txn);
    assert_eq!(r.t.owner.rmtfs_large_reply_flags(), 0b10011);
    let published = r.t.owner.rmtfs_event_time(1).unwrap();
    assert!(queued < published);
    assert_eq!(r.t.held_host.len(), 1);
    let reply = r.t.held_host.remove(0);
    #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
    assert_eq!(lo_word(&reply.1, 16), 0, "first packet to port 19 requests no QRTR credit");
    r.t.ack_one(&reply, glink_first).unwrap();
    #[cfg(not(feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
    assert_eq!(r.t.owner.rmtfs_large_reply_flags(), if glink_first { 0b10111 } else { 0b11011 });
    #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
    assert_eq!(r.t.owner.rmtfs_large_reply_flags(), if glink_first { 0b10111 } else { 0b10011 });
    r.t.ack_one(&reply, !glink_first).unwrap();
    #[cfg(not(feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
    assert_eq!(r.t.owner.rmtfs_large_reply_flags(), 0b11111);
    #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
    assert_eq!(r.t.owner.rmtfs_large_reply_flags(), 0b10111);
    let rx_done = r.t.owner.rmtfs_event_time(2).unwrap();
    #[cfg(not(feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
    let resumed = r.t.owner.rmtfs_event_time(3).unwrap();
    #[cfg(not(feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
    assert!(published < rx_done && published < resumed);
    #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
    { assert!(published < rx_done); assert_eq!(r.t.owner.rmtfs_event_time(3), None); }
    #[cfg(not(feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
    assert_eq!(rx_done < resumed, glink_first);
    assert_eq!(r.t.owner.rmtfs_event_time(0), Some(queued));
    assert_eq!(r.t.owner.rmtfs_event_time(1), Some(published));
    assert_eq!(r.t.owner.rmtfs_event_time(99), None);
    let state = sim.0.borrow();
    assert!(state.bytes[1024..0x200000].iter().all(|b| *b == 1));
    assert_eq!(state.bytes[1023], 0);
    }
}

#[test]
fn rmtfsserver_burst_is_bounded_and_retains_request_until_reply() {
    let sim = Sim::new();
    let (c, src, mut dst) = shadows();
    let mut r = SrRig::new(&sim, c, &src, &mut dst);
    let h = r.open(b"/boot/modem_fs1");
    let txn = r.txn;
    r.txn += 1;
    let mut vectors = vec![20u8];
    for _ in 0..20 {
        for value in [0u32, r::BUFFER_PA as u32, 16u32] {
            vectors.extend(value.to_le_bytes());
        }
    }
    let payload = lo_message(0, 3, txn, &[
        (1, &h.to_le_bytes()), (2, &[0]), (3, &vectors), (4, &[0]),
    ]);
    r.t.enqueue(&lo_packet(1, 19, 0x4000, true, &payload));
    let before = r.s.report().chunks;
    for _ in 0..32 {
        r.t.owner.notified(8, &mut r.t.irqs).unwrap();
        let previous = r.s.report().chunks;
        r.t.owner.dispatch_linux_rmtfs_burst(&mut r.s, &mut r.t.irqs).unwrap();
        assert!(r.s.report().chunks - previous <= 2);
        r.t.drain();
        if r.s.report().chunks > before { break; }
    }
    assert_eq!(r.s.report().chunks - before, 2);
    assert!(r.s.report().active_request);
    assert_eq!(r.s.report().active_write, Some(false));
    assert_eq!(r.s.report().active_sectors_total, Some(320));
    assert_eq!(r.s.report().active_sectors_remaining, Some(288));
    assert_eq!(r.s.report().replies_queued, 1);
    r.drive().unwrap();
    r.success(txn);
    assert_eq!(r.s.report().chunks - before, 20);
}

#[test]
fn rmtfsserver_burst_stops_on_uncertain_copy_without_retry() {
    for revoke in [false, true] {
    let sim = Sim::new();
    let (mut c, _, _) = shadows();
    let src: [Vec<u8>; 4] = core::array::from_fn(|n| vec![n as u8 + 1; 262144]);
    for (i, binding) in c.bindings.iter_mut().enumerate() {
        binding.source = hash(&src[i]);
        binding.disk.bytes = 262144;
    }
    let mut dst: [Vec<u8>; 4] = core::array::from_fn(|_| vec![0; 262144]);
    let mut r = SrRig::new(&sim, c, &src, &mut dst);
    let h = r.open(b"/boot/modem_fs1");
    if revoke { sim.0.borrow_mut().revoke = Some("write"); }
    else { sim.0.borrow_mut().fail = Some("write"); }
    let mut vectors = vec![1u8];
    for value in [0u32, r::BUFFER_PA as u32, 256u32] {
        vectors.extend(value.to_le_bytes());
    }
    let payload = lo_message(0, 3, 500, &[
        (1, &h.to_le_bytes()), (2, &[0]), (3, &vectors), (4, &[0]),
    ]);
    r.t.enqueue(&lo_packet(1, 19, 0x4000, true, &payload));
    let mut failed = false;
    for _ in 0..32 {
        if r.t.owner.notified(8, &mut r.t.irqs).is_err()
            || r.t.owner.dispatch_linux_rmtfs_burst(&mut r.s, &mut r.t.irqs).is_err() {
            failed = true;
            break;
        }
        r.t.drain();
    }
    assert!(failed);
    assert!(r.s.report().failed);
    assert!(r.t.owner.error().is_some());
    let calls = sim.0.borrow().calls.len();
    assert!(r.t.owner.dispatch_linux_rmtfs_burst(&mut r.s, &mut r.t.irqs).is_err());
    assert_eq!(sim.0.borrow().calls.len(), calls);
    }
}

#[test]
fn rmtfsserver_wire_matches_original_userspace_encoder() {
    let sim = Sim::new();
    let (c, src, mut dst) = shadows();
    let mut r = SrRig::new(&sim, c, &src, &mut dst);
    let h = r.open(b"/boot/modem_fs1");
    let allocated = r.send(9, 4, &[(1, &h.to_le_bytes()), (2, &4096u32.to_le_bytes())]);
    let status = r.send(9, 5, &[(1, &h.to_le_bytes())]);
    let closed = r.send(9, 2, &[(1, &h.to_le_bytes())]);
    let failed = r.send(9, 5, &[(1, &h.to_le_bytes())]);
    let golden = include_str!(env!("SUNFISH_RMTFS_WIRE_C"));
    for (name, txn) in [
        ("open", 100),
        ("alloc", allocated),
        ("status", status),
        ("close", closed),
        ("failure", failed),
    ] {
        let expected = golden
            .lines()
            .find_map(|s| s.strip_prefix(&format!("{name} ")))
            .unwrap();
        let mut actual = r.message(txn).as_bytes().to_vec();
        actual[1..3].copy_from_slice(&0x1234u16.to_le_bytes());
        assert_eq!(
            actual
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
            expected
        );
    }
}
#[test]
fn rmtfsserver_all_partitions_both_directions_and_boot_ram_persists_close() {
    let sim = Sim::new();
    let (c, src, mut dst) = shadows();
    let original = src.clone();
    {
        let mut r = SrRig::new(&sim, c, &src, &mut dst);
        for (i, path) in [
            b"/boot/modem_fs1".as_slice(),
            b"/boot/modem_fs2",
            b"/boot/modem_fsg",
            b"/boot/modem_fsc",
        ]
        .into_iter()
        .enumerate()
        {
            let h = r.open(path);
            let txn = r.iov(19, h, &[(0, r::BUFFER_PA as u32, 12)], false);
            r.success(txn);
            assert!(
                sim.0.borrow().bytes[..6144]
                    .iter()
                    .all(|b| *b == i as u8 + 1)
            );
            sim.0.borrow_mut().bytes[..6144].fill(0x70 + i as u8);
            let txn = r.iov(19, h, &[(1, r::BUFFER_PA as u32, 12)], true);
            r.success(txn);
            assert_eq!(r.s.resource_status().1.dirty, [false; 4]);
            let txn = r.send(19, 2, &[(1, &h.to_le_bytes())]);
            r.success(txn);
            let h = r.open(path);
            let txn = r.iov(9, h, &[(1, r::BUFFER_PA as u32, 12)], false);
            r.success(txn);
            assert!(
                sim.0.borrow().bytes[..6144]
                    .iter()
                    .all(|b| *b == 0x70 + i as u8)
            );
        }
        assert_eq!(r.s.report().chunks, 12);
        assert!(!r.s.report().failed);
    }
    assert_eq!(src, original);
    for (i, d) in dst.iter().enumerate() {
        assert!(d[512..6656].iter().all(|b| *b == 0x70 + i as u8));
        assert_eq!(&d[..512], &src[i][..512]);
        assert_eq!(&d[6656..], &src[i][6656..]);
    }
}
#[test]
fn rmtfsserver_all_vectors_checked_before_any_effect() {
    let sim = Sim::new();
    let (c, src, mut dst) = shadows();
    {
        let mut r = SrRig::new(&sim, c, &src, &mut dst);
        let h = r.open(b"/boot/modem_fs1");
        for bad in [
            (16, r::BUFFER_PA as u32, 1),
            (0, r::BUFFER_PA as u32 - 512, 1),
            (0, (r::BUFFER_PA + u64::from(r::BUFFER_BYTES)) as u32, 1),
            (0, r::BUFFER_PA as u32 + 1, 1),
            (0, r::BUFFER_PA as u32, 0),
        ] {
            let txn = r.iov(9, h, &[(0, r::BUFFER_PA as u32, 1), bad], true);
            assert_eq!(r.message(txn).required(2).unwrap(), [1, 0, 1, 0]);
        }
        assert_eq!((sim.count("read"), sim.count("write")), (0, 0));
        assert_eq!(r.s.report().chunks, 0);
    }
    assert_eq!(dst, src);
}
#[test]
#[cfg(not(feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
fn rmtfsserver_backpressure_retains_result_until_both_transport_acks() {
    for glink_first in [false, true] {
        let sim = Sim::new();
        let (c, src, mut dst) = shadows();
        let mut r = SrRig::new(&sim, c, &src, &mut dst);
        let h = r.open(b"/boot/modem_fs1");
        r.t.hold_host = true;
        let first = r.iov(9, h, &[(0, r::BUFFER_PA as u32, 12)], true);
        r.success(first);
        let second = r.iov(9, h, &[(0, r::BUFFER_PA as u32, 8)], true);
        assert!(r.s.report().active_request);
        assert_eq!(r.s.report().chunks, 2);
        let before = sim.count("read");
        for _ in 0..4 {
            r.drive().unwrap();
        }
        assert_eq!(sim.count("read"), before);
        assert_eq!(r.t.held_host.len(), 1);
        let held = r.t.held_host.remove(0);
        r.t.ack_one(&held, glink_first).unwrap();
        r.drive().unwrap();
        assert!(r.s.report().active_request);
        r.t.ack_one(&held, !glink_first).unwrap();
        r.drive().unwrap();
        r.success(second);
        assert!(!r.s.report().active_request);
        assert_eq!(sim.count("read"), before);
        assert_eq!(r.t.held_host.len(), 1);
    }
}
#[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
#[test]
fn rmtfsserver_backpressure_releases_only_after_glink_without_replaying_io() {
    for glink_first in [false, true] {
        let sim = Sim::new();
        let (c, src, mut dst) = shadows();
        let mut r = SrRig::new(&sim, c, &src, &mut dst);
        let h = r.open(b"/boot/modem_fs1");
        r.t.hold_host = true;
        let first = r.iov(9, h, &[(0, r::BUFFER_PA as u32, 12)], true);
        r.success(first);
        let second = r.iov(9, h, &[(0, r::BUFFER_PA as u32, 8)], true);
        assert!(r.s.report().active_request);
        let before = sim.count("read");
        let held = r.t.held_host.remove(0);
        r.t.ack_one(&held, glink_first).unwrap();
        r.drive().unwrap();
        assert_eq!(r.s.report().active_request, !glink_first);
        r.t.ack_one(&held, !glink_first).unwrap();
        r.drive().unwrap();
        r.success(second);
        assert!(!r.s.report().active_request);
        assert_eq!(sim.count("read"), before);
    }
}
#[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
#[test]
fn rmtfsserver_applies_linux_credit_limit_without_replaying_storage() {
    let sim = Sim::new();
    let (c, src, mut dst) = shadows();
    let mut r = SrRig::new(&sim, c, &src, &mut dst);
    let h = r.open(b"/boot/modem_fs1"); // First outbound packet to port 9.
    r.t.hold_host = true;
    let mut confirmation = None;
    for count in 2..=10 {
        let txn = r.iov(9, h, &[(0, r::BUFFER_PA as u32, 1)], true);
        r.success(txn);
        let reply = r.t.held_host.remove(0);
        assert_eq!(lo_word(&reply.1, 16), u32::from(count == 5));
        r.t.ack_one(&reply, true).unwrap();
        if count == 5 { confirmation = Some(reply); }
    }
    let txn = r.iov(9, h, &[(0, r::BUFFER_PA as u32, 1)], true);
    assert!(r.s.report().active_request, "eleventh reply must wait for flow credit");
    let operations = sim.count("read");
    r.drive().unwrap();
    assert_eq!(sim.count("read"), operations);
    r.t.ack_one(&confirmation.unwrap(), false).unwrap();
    r.drive().unwrap();
    r.success(txn);
    assert!(!r.s.report().active_request);
    assert_eq!(sim.count("read"), operations);
}
#[test]
fn rmtfsserver_uncertain_copy_or_lifecycle_loss_stops_both_owners() {
    for op in ["read", "write"] {
        for revoke in [false, true] {
            let sim = Sim::new();
            let (c, src, mut dst) = shadows();
            let mut r = SrRig::new(&sim, c, &src, &mut dst);
            let h = r.open(b"/boot/modem_fs1");
            if revoke {
                sim.0.borrow_mut().revoke = Some(op);
            } else {
                sim.0.borrow_mut().fail = Some(op);
            }
            let mut v = vec![1];
            for x in [0, r::BUFFER_PA as u32, 1] {
                v.extend(x.to_le_bytes());
            }
            r.t.enqueue(&lo_packet(
                1,
                9,
                0x4000,
                true,
                &lo_message(
                    0,
                    3,
                    500,
                    &[
                        (1, &h.to_le_bytes()),
                        (2, &[u8::from(op == "read")]),
                        (3, &v),
                        (4, &[0]),
                    ],
                ),
            ));
            assert!(r.drive().is_err());
            assert!(r.s.report().failed);
            assert!(r.t.owner.error().is_some());
            let before = sim.0.borrow().calls.len();
            assert!(r.drive().is_err());
            assert_eq!(before, sim.0.borrow().calls.len());
        }
    }
}
#[test]
fn rmtfsserver_binding_cannot_move_between_owner_lifetimes() {
    let sim = Sim::new();
    let (c, src, mut dst) = shadows();
    let mut r = SrRig::new(&sim, c, &src, &mut dst);
    let mut other = sr_transport();
    assert!(
        other
            .owner
            .activate_linux_rmtfs(&mut r.s, &mut other.irqs)
            .is_err()
    );
    assert!(r.s.report().failed);
    assert!(other.owner.error().is_some());
}
#[test]
fn rmtfsserver_unwind_in_transfer_retains_and_stops_everything() {
    for op in ["read", "write"] {
        let sim = Sim::new();
        let (c, src, mut dst) = shadows();
        let mut r = SrRig::new(&sim, c, &src, &mut dst);
        let h = r.open(b"/boot/modem_fs1");
        sim.0.borrow_mut().panic = Some(op);
        let mut v = vec![1];
        for x in [0, r::BUFFER_PA as u32, 1] {
            v.extend(x.to_le_bytes());
        }
        r.t.enqueue(&lo_packet(
            1,
            9,
            0x4000,
            true,
            &lo_message(
                0,
                3,
                500,
                &[
                    (1, &h.to_le_bytes()),
                    (2, &[u8::from(op == "read")]),
                    (3, &v),
                    (4, &[0]),
                ],
            ),
        ));
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| r.drive())).is_err());
        assert!(r.s.report().failed);
        let before = sim.0.borrow().calls.len();
        assert!(r.drive().is_err());
        assert_eq!(before, sim.0.borrow().calls.len());
    }
}

#[test]
fn rmtfsserver_forged_context_cannot_reach_resources() {
    for wrong in 0..4 {
        let sim = Sim::new();
        let (c, src, mut dst) = shadows();
        let mut r = SrRig::new(&sim, c, &src, &mut dst);
        r.t.enqueue(&lo_packet(
            1,
            9,
            0x4000,
            true,
            &lo_message(0, 1, 500, &[(1, b"/boot/modem_fs1")]),
        ));
        r.t.owner.notified(8, &mut r.t.irqs).unwrap();
        let handle =
            r.t.owner
                .pending_request_for_service(Service::RemoteStorage)
                .unwrap()
                .unwrap();
        let (mut context, request) = r.t.owner.request(&handle).unwrap();
        match wrong {
            0 => context.peer.node = 2,
            1 => context.local.port = 0x4001,
            2 => context.generation += 1,
            _ => context.message_id = 4,
        }
        assert!(r.s.begin(handle, context, request).is_err());
        assert_eq!(r.s.report().open_handles, 0);
        assert!(r.s.report().failed);
        assert_eq!((sim.count("read"), sim.count("write")), (0, 0));
    }
}

#[test]
fn rmtfsserver_core_binding_is_one_use() {
    let sim = Sim::new();
    let (c, src, mut dst) = shadows();
    let mut r = SrRig::new(&sim, c, &src, &mut dst);
    assert!(r.s.bind(r.t.owner.identity()).is_err());
    assert!(r.s.report().failed);
}

#[test]
fn rmtfsserver_ipa_handshake_progresses_while_storage_reply_is_blocked() {
    let sim = Sim::new();
    let (c, src, mut dst) = shadows();
    let mut r = SrRig::new(&sim, c, &src, &mut dst);
    let h = r.open(b"/boot/modem_fs1");
    r.t.hold_host = true;
    r.iov(9, h, &[(0, r::BUFFER_PA as u32, 8)], true);
    let second = r.iov(9, h, &[(0, r::BUFFER_PA as u32, 8)], true);
    assert!(r.s.report().active_request);
    let initial = r.t.next();
    r.t.ack(&initial, true);
    r.t.init_response(0, 0, 0).unwrap();
    for id in [0x20, 0x35] {
        r.t.request(id, 1);
        let record = r.t.next();
        r.t.ack(&record, true);
    }
    let indication = r.t.next();
    r.t.ack(&indication, true);
    assert!(r.t.owner.linux_ipa_report().unwrap().protocol_complete);
    assert!(r.s.report().active_request);
    let held = r.t.held_host.remove(0);
    r.t.ack_one(&held, false).unwrap();
    r.t.ack_one(&held, true).unwrap();
    r.drive().unwrap();
    r.success(second);
}

#[test]
fn rmtfsserver_old_pool_handler_mode_is_exclusive() {
    let sim = Sim::new();
    let (c, src, mut dst) = shadows();
    let mut r = SrRig::new(&sim, c, &src, &mut dst);
    let legacy = super::services_fixture::config(8192, 8192, 16384);
    let backend = super::services_fixture::backend(legacy);
    let mut handlers = h::Handlers::attach(h::Profile::validate(legacy, &[]).unwrap(), backend);
    assert!(
        r.t.owner
            .activate_handlers(&mut handlers, &mut r.t.irqs)
            .is_err()
    );
    assert!(r.drive().is_err());
    assert!(
        r.t.owner
            .dispatch_linux_rmtfs_step(&mut r.s, &mut r.t.irqs)
            .is_err()
    );
    assert!(r.s.report().failed);
    let sim2 = Sim::new();
    let (c2, src2, mut dst2) = shadows();
    let mut server2 = sr_server(&sim2, c2, &src2, &mut dst2);
    let mut legacy_owner = LoRig::at(0, true);
    assert!(
        legacy_owner
            .owner
            .activate_linux_rmtfs(&mut server2, &mut legacy_owner.irqs)
            .is_err()
    );
    assert!(server2.report().failed);
}

#[test]
fn rmtfsserver_original_encoder_requests_reach_real_storage() {
    let sim = Sim::new();
    let (c, src, mut dst) = shadows();
    let mut r = SrRig::new(&sim, c, &src, &mut dst);
    assert_eq!(r.open(b"/boot/modem_fs1"), 0);
    let golden = include_str!(env!("SUNFISH_RMTFS_WIRE_C"));
    for name in ["alloc_req", "read_req"] {
        let line = golden
            .lines()
            .find_map(|s| s.strip_prefix(&format!("{name} ")))
            .unwrap();
        let bytes: Vec<u8> = (0..line.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&line[i..i + 2], 16).unwrap())
            .collect();
        r.t.host.clear();
        r.t.enqueue(&lo_packet(1, 19, 0x4000, true, &bytes));
        r.drive().unwrap();
        r.success(0x1234);
    }
    assert!(sim.0.borrow().bytes[..4096].iter().all(|b| *b == 1));
    assert_eq!(r.s.report().chunks, 1);
}

#[cfg(feature="sunfish-original-final-ack-turn")]
#[test]
fn final_ack_uses_one_bounded_router_turn_without_oack_helper() {
    use super::elf_blocker_irq::{Kernel, Router};
    use super::qcom_modem_owner::TftpServiceProgress;
    use super::qcom_tftp_server_check as tftp;
    struct Irq;
    impl Kernel for Irq { fn acknowledge(&mut self,_:u8)->Result<(),()>{Ok(())} }
    for (pending_grant,blocked) in [(false,false),(true,false),(false,true),(true,true)] {
        let mut session=super::lo_session();session.prepare_tftp_server(1,0x5000).unwrap();
        let mut rig=LoRig::from_session(0,false,session);
        let mut router=Router::new(Irq,1);let mut server=tftp::ServerCheck::new();
        rig.hold_host=true;
        while rig.available.len()<4 {
            rig.owner.offer_receive().unwrap();
            super::qcom_modem_owner::Interrupts::schedule(&mut router).unwrap();
            router.dispatch(&mut rig.owner).unwrap();rig.drain();
        }
        let is_tftp=|p:&[u8],op:u8|lo_word(p,4)==1 && lo_word(p,28)==0x1f && p.len()>=34 && p[32..34]==[0,op];
        rig.enqueue(&lo_packet(1,0x1f,0x5000,false,tftp::REQUEST));
        router.capture(1<<4).unwrap();router.dispatch(&mut rig.owner).unwrap();rig.drain();
        assert_eq!(rig.owner.service_tftp_server_check_step(&mut server,&mut router).unwrap(),TftpServiceProgress::ResponseQueued);
        assert_eq!(router.dispatch_queued_original_final_ack(&mut rig.owner),Ok(false),"an OACK cannot consume the ACK turn");
        assert!(!router.original_final_ack_turn());
        router.dispatch(&mut rig.owner).unwrap();rig.drain();
        let (id,oack)=rig.held_host.iter().find(|(_,p)|is_tftp(p,6)).unwrap().clone();
        let port=lo_word(&oack,12);
        assert_eq!(port,0x5001);assert_eq!(lo_word(&oack,8),1);assert_eq!(lo_word(&oack,24),0);
        let exact_ack=|p:&[u8]|is_tftp(p,4) && p.len()==36 && lo_word(p,20)==4
            && lo_word(p,8)==1 && lo_word(p,12)==port && lo_word(p,24)==0 && p[32..36]==[0,4,0,1];
        rig.peer.push(&super::control(6,2,id));
        if lo_word(&oack,16)==1 { let credit=rig.credit(&oack);rig.enqueue(&credit); }
        router.capture(1<<4).unwrap();router.dispatch(&mut rig.owner).unwrap();rig.drain();
        assert_eq!(rig.owner.service_registry_trace().first_server_check.unwrap().oack.phase,4);
        if rig.available.is_empty(){rig.owner.offer_receive().unwrap();super::qcom_modem_owner::Interrupts::schedule(&mut router).unwrap();router.dispatch(&mut rig.owner).unwrap();rig.drain();}
        rig.enqueue(&lo_packet(1,0x1f,port,false,b"\0\x03\0\x01test\n"));
        router.capture(1<<4).unwrap();router.dispatch(&mut rig.owner).unwrap();rig.drain();
        assert_eq!(rig.owner.service_tftp_server_check_step(&mut server,&mut router).unwrap(),TftpServiceProgress::ResponseQueued);
        let w=rig.owner.service_registry_trace().first_server_check.unwrap();
        assert_eq!(w.ack.phase,2);assert!(w.data.unwrap().stages.final_queued);
        assert!(!rig.held_host.iter().any(|(_,p)|exact_ack(p)));
        assert!(router.report().continuation_pending);
        assert_eq!(router.dispatch_queued_server_check_oack(&mut rig.owner,||0),Ok(false));
        assert_eq!(router.dispatch_queued_tftp_retry(&mut rig.owner),Ok(false));
        assert_eq!(rig.owner.service_registry_trace().first_server_check.unwrap().ack.phase,2);
        router.capture(1<<2).unwrap();
        assert_eq!(router.dispatch_queued_original_final_ack(&mut rig.owner),Ok(false));
        assert!(!router.original_final_ack_turn());
        assert!(router.report().continuation_pending);
        assert_eq!(router.take_other(),1<<2);
        if pending_grant { rig.peer.push(&super::control(7,2,52)); }
        if blocked {
            // Valid receive backlog defers the attempted DATA write. Do not
            // forge a TX tail beyond bytes actually published by the host.
            for peer in [0x60,0x61] {rig.enqueue(&lo_packet(1,peer,0x5000,false,tftp::REQUEST));}
        }
        assert_eq!(router.dispatch_queued_original_final_ack(&mut rig.owner),Ok(true));
        assert!(router.original_final_ack_turn());
        assert_eq!(router.dispatch_queued_original_final_ack(&mut rig.owner),Ok(false));
        if blocked {
            assert_eq!(rig.owner.service_registry_trace().first_server_check.unwrap().ack.phase,2);
            assert!(router.report().continuation_pending);
            for _ in 0..4 {
                router.dispatch(&mut rig.owner).unwrap();
                if rig.owner.service_registry_trace().first_server_check.unwrap().ack.phase==3 {break;}
                assert!(router.report().continuation_pending);
                assert_eq!(router.dispatch_queued_original_final_ack(&mut rig.owner),Ok(false));
            }
        }
        let wire={let m=rig.peer.0.borrow();let tail=m.words[0] as usize;let n=(m.words[1] as usize+super::qcom_glink_fifo::FIFO_BYTES-tail)%super::qcom_glink_fifo::FIFO_BYTES;
            (0..n).map(|i|m.tx[(tail+i)%super::qcom_glink_fifo::FIFO_BYTES]).collect::<Vec<_>>()};
        let mut at=0;let mut advertisements=Vec::new();let mut grants=Vec::new();
        while at<wire.len(){let cmd=u16::from_le_bytes([wire[at],wire[at+1]]);let size=match cmd{
            5=>{assert_eq!(lo_word(&wire,at)>>16,1);assert_eq!(lo_word(&wire,at+4),1);
                advertisements.push((at,lo_word(&wire,at+8),lo_word(&wire,at+12)));16},
            8=>{assert_eq!(lo_word(&wire,at)>>16,1);assert_eq!(lo_word(&wire,at+4),1,"positive grant required");grants.push(at);8},
            9|12=>{let n=lo_word(&wire,at+8) as usize;(16+n+7)&!7},
            6|7|13|14=>8,other=>panic!("unexpected GLINK command {other}")};at+=size;}
        if pending_grant {
            assert_eq!(advertisements.len(),1);assert_eq!(grants.len(),1);
            assert!(advertisements[0].1>=52 && advertisements[0].2!=0);
            assert!(advertisements[0].0<grants[0],"backing advertisement must commit before positive grant ACK");
        }
        rig.drain();
        assert_eq!(rig.owner.service_registry_trace().first_server_check.unwrap().ack.phase,3);
        assert!(rig.held_host.iter().any(|(_,p)|exact_ack(p)));
        println!("ACK TURN pending_grant={pending_grant}, blocked={blocked}: OACK-only helpers returned false; ordinary checked router turn published exact final ACK; grant advertisements={:?}, grants={:?}",rig.available,rig.intent_acks);
    }
}
