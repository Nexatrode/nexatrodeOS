//! Private host vectors and recorded SMC envelopes. Never invokes hardware.
#[allow(dead_code)]
#[path = "../rust/src/qcom_wlan_power.rs"]
pub mod qcom_wlan_power;
#[path = "../rust/src/qcom_glink_qmi.rs"]
pub mod qcom_glink_qmi;
#[path = "../rust/src/qcom_ipa.rs"]
pub mod qcom_ipa;
#[path = "../rust/src/qcom_ipa_service.rs"]
#[allow(dead_code)]
pub mod qcom_ipa_service;
#[path = "../rust/src/qcom_ipa_client.rs"]
pub mod qcom_ipa_client;
#[path = "../rust/src/qcom_modem_backend.rs"]
pub mod qcom_modem_backend;
#[path = "../rust/src/qcom_modem_handlers.rs"]
pub mod qcom_modem_handlers;
#[path = "../rust/src/qcom_modem_pools.rs"]
pub mod qcom_modem_pools;
#[path = "../rust/src/qcom_modem_services.rs"]
pub mod qcom_modem_services;
#[path = "../rust/src/qcom_modem_storage.rs"]
pub mod qcom_modem_storage;
#[path = "../rust/src/qcom_modem_ram_storage.rs"]
pub mod qcom_modem_ram_storage;
#[path = "../rust/src/sunfish_lpd.rs"]
pub mod sunfish_lpd;
#[path = "../rust/src/qcom_msa.rs"]
pub mod qcom_msa;
#[path = "../rust/src/qcom_msa_handoff.rs"]
pub mod qcom_msa_handoff;
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
use qcom_modem_handlers::{Fault, Grant, Ownership, Pool, Region};
use qcom_modem_pools::*;
use qcom_qmi_router::Address;
use std::{
    cell::RefCell,
    panic::{catch_unwind, AssertUnwindSafe},
    rc::Rc,
};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Tag {
    Observe,
    Stage,
    Invoke,
    Complete,
    Zero,
    Read,
    Write,
}
struct State {
    observation: Observation,
    data: [Vec<u8>; 2],
    calls: Vec<([u64; 8], [u8; ARG_BYTES])>,
    ops: Vec<(Tag, usize)>,
    fault: Option<(Tag, Fault)>,
    panic: Option<Tag>,
    after: Option<(Tag, fn(&mut Observation))>,
    raw: [u64; 8],
}
#[derive(Clone)]
struct Fake(Rc<RefCell<State>>);
impl Fake {
    fn new() -> Self {
        Self(Rc::new(RefCell::new(State {
            observation: Observation {
                generation: 1,
                identity: 101,
                ready: true,
                mappings: MAPPINGS,
                contract: true,
                initial_hlos: true,
            },
            data: [
                vec![0xa5; STORAGE.bytes as usize],
                vec![0x5a; HEAP.bytes as usize],
            ],
            calls: vec![],
            ops: vec![],
            fault: None,
            panic: None,
            after: None,
            raw: [0; 8],
        })))
    }
    fn op(&self, tag: Tag, n: usize) -> Result<(), Fault> {
        let mut s = self.0.borrow_mut();
        s.ops.push((tag, n));
        if s.panic == Some(tag) {
            s.panic = None;
            drop(s);
            panic!("injected {tag:?}");
        }
        if s.fault.is_some_and(|(t, _)| t == tag) {
            return Err(s.fault.take().unwrap().1);
        }
        if s.after.is_some_and(|(t, _)| t == tag) {
            let (_, f) = s.after.take().unwrap();
            f(&mut s.observation);
        }
        Ok(())
    }
    fn count(&self, tag: Tag) -> usize {
        self.0
            .borrow()
            .ops
            .iter()
            .filter(|(t, _)| *t == tag)
            .count()
    }
}
fn idx(p: Pool) -> usize {
    if p == Pool::Storage {
        0
    } else {
        1
    }
}
impl Io for Fake {
    fn observe(&mut self) -> Result<Observation, Fault> {
        self.op(Tag::Observe, 0)?;
        Ok(self.0.borrow().observation)
    }
    fn stage(&mut self, c: &Call) -> Result<(), Fault> {
        self.op(Tag::Stage, ARG_BYTES)?;
        self.0.borrow_mut().calls.push((*c.words(), *c.bytes()));
        Ok(())
    }
    fn invoke(&mut self, _: &Call) -> Result<[u64; 8], Fault> {
        self.op(Tag::Invoke, 0)?;
        Ok(self.0.borrow().raw)
    }
    fn complete(&mut self) -> Result<(), Fault> {
        self.op(Tag::Complete, 0)
    }
    fn zero(&mut self, p: Pool, at: u32, n: usize) -> Result<(), Fault> {
        self.op(Tag::Zero, n)?;
        self.0.borrow_mut().data[idx(p)][at as usize..at as usize + n].fill(0);
        Ok(())
    }
    fn read(&mut self, p: Pool, at: u32, out: &mut [u8]) -> Result<(), Fault> {
        self.op(Tag::Read, out.len())?;
        out.copy_from_slice(&self.0.borrow().data[idx(p)][at as usize..at as usize + out.len()]);
        Ok(())
    }
    fn write(&mut self, p: Pool, at: u32, b: &[u8]) -> Result<(), Fault> {
        self.op(Tag::Write, b.len())?;
        self.0.borrow_mut().data[idx(p)][at as usize..at as usize + b.len()].copy_from_slice(b);
        Ok(())
    }
}
fn attached(f: &Fake) -> Pools<Fake> {
    Pools::attach(
        Reservations::validate(1, 0, MAPPINGS, &[]).unwrap(),
        f.clone(),
    )
}
fn rig() -> (Fake, Pools<Fake>) {
    let f = Fake::new();
    let p = attached(&f);
    assert_eq!(p.status().error, None);
    (f, p)
}
fn request(p: Pool, serial: u64, at: u32, bytes: u32) -> Grant {
    Grant {
        serial,
        pool: p,
        region: Region {
            base: MAPPINGS[idx(p)].region.base + u64::from(at),
            bytes,
        },
        generation: 1,
        peer: Address { node: 0, port: 9 },
        ownership: if p == Pool::Storage {
            Ownership::SharedStorage
        } else {
            Ownership::ModemExclusive
        },
    }
}
fn scrub(p: &mut Pools<Fake>, pool: Pool, at: u32, bytes: u32) {
    for offset in (at..at + bytes).step_by(4096) {
        p.zero(pool, offset, 4096).unwrap();
    }
}
fn small(p: &mut Pools<Fake>, pool: Pool, n: u64) -> Receipt {
    scrub(p, pool, 0, 4096);
    p.grant(request(pool, n, 0, 4096)).unwrap()
}
fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
fn le64(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

mod composite_backend_tests {
    use super::*;
    use qcom_modem_backend::Backend as CompositeBackend;
    use qcom_modem_handlers::{Backend as ServiceBackend, Configuration as ServiceConfig, Disk};
    use qcom_modem_storage::{Binding, Configuration as DiskConfig, Io as DiskIo};

    struct Disks {
        configuration: DiskConfig,
        identity: u64,
        data: [Vec<u8>; 4],
    }

    impl DiskIo for Disks {
        fn observe(&mut self) -> Result<qcom_modem_storage::Observation, Fault> {
            Ok(qcom_modem_storage::Observation {
                configuration: self.configuration,
                identity: self.identity,
                ready: true,
            })
        }

        fn read(
            &mut self,
            partition: qcom_modem_services::Partition,
            offset: u64,
            out: &mut [u8],
        ) -> Result<usize, Fault> {
            let index = qcom_modem_storage::index(partition);
            let start = usize::try_from(offset).map_err(|_| Fault::Failed)?;
            let end = start.checked_add(out.len()).ok_or(Fault::Failed)?;
            let source = self.data[index].get(start..end).ok_or(Fault::Failed)?;
            out.copy_from_slice(source);
            Ok(out.len())
        }

        fn write(
            &mut self,
            partition: qcom_modem_services::Partition,
            offset: u64,
            bytes: &[u8],
        ) -> Result<usize, Fault> {
            let index = qcom_modem_storage::index(partition);
            let start = usize::try_from(offset).map_err(|_| Fault::Failed)?;
            let end = start.checked_add(bytes.len()).ok_or(Fault::Failed)?;
            let destination = self.data[index].get_mut(start..end).ok_or(Fault::Failed)?;
            destination.copy_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self, _partition: qcom_modem_services::Partition) -> Result<(), Fault> {
            Ok(())
        }
    }

    fn disk_configuration() -> DiskConfig {
        let disk = Disk {
            bytes: 4096,
            writable: true,
            durable: true,
        };
        DiskConfig {
            generation: 1,
            device: [0x51; 32],
            bindings: [
                qcom_modem_services::Partition::ModemFs1,
                qcom_modem_services::Partition::ModemFs2,
                qcom_modem_services::Partition::ModemFsg,
                qcom_modem_services::Partition::ModemFsc,
            ]
            .map(|partition| Binding {
                partition,
                object: match partition {
                    qcom_modem_services::Partition::ModemFs1 => 1,
                    qcom_modem_services::Partition::ModemFs2 => 2,
                    qcom_modem_services::Partition::ModemFsg => 3,
                    qcom_modem_services::Partition::ModemFsc => 4,
                },
                source: [0x62; 32],
                disk,
            }),
        }
    }

    fn backend_config(disks: [Disk; 4]) -> ServiceConfig {
        ServiceConfig {
            generation: 1,
            local_node: 1,
            peer_node: 0,
            storage_port: 0x4000,
            memory_port: 0x4001,
            storage: STORAGE,
            heap: HEAP,
            disks,
        }
    }

    fn attached_backend() -> (Fake, CompositeBackend<Fake, Disks>) {
        let pool_io = Fake::new();
        let pools = super::attached(&pool_io);
        let configuration = disk_configuration();
        let disk_profile = qcom_modem_storage::Profile::validate(configuration).unwrap();
        let storage = qcom_modem_storage::Storage::attach(
            disk_profile,
            Disks {
                configuration,
                identity: 202,
                data: std::array::from_fn(|_| vec![0x33; 4096]),
            },
        );
        let disks = configuration.bindings.map(|binding| binding.disk);
        let backend = CompositeBackend::attach(backend_config(disks), pools, storage);
        assert_eq!(backend.error(), None);
        (pool_io, backend)
    }

    #[test]
    fn durable_storage_keeps_4k_limit_when_disk_is_larger() {
        let mut configuration = disk_configuration();
        for binding in &mut configuration.bindings { binding.disk.bytes = 8192; }
        let profile = qcom_modem_storage::Profile::validate(configuration).unwrap();
        let mut storage = qcom_modem_storage::Storage::attach(profile, Disks {
            configuration, identity: 202, data: std::array::from_fn(|_| vec![0x33; 8192]),
        });
        let mut out = vec![0; 4608];
        assert_eq!(storage.read(qcom_modem_services::Partition::ModemFs1, 0, &mut out), Err(Fault::Failed));
        assert_eq!(out, vec![0; 4608]);
    }

    #[test]
    fn ram_shadow_composes_with_the_real_pool_backend_without_claiming_durability() {
        use qcom_modem_handlers::Persistence;
        let source = [[0x6b; 4096]; 4];
        let mut working = [[0; 4096]; 4];
        let mut configuration = disk_configuration();
        for binding in &mut configuration.bindings {
            let mut hash = sunfish_lpd::Sha256::new();
            hash.update(&source[qcom_modem_storage::index(binding.partition)]);
            binding.source = hash.finish();
            binding.disk.durable = false;
        }
        let storage = qcom_modem_ram_storage::RamShadow::new(configuration,
            source.each_ref().map(|s| &s[..]), working.each_mut().map(|s| &mut s[..]))
            .unwrap().into_storage().unwrap();
        let pool_io = Fake::new();
        let c = backend_config(configuration.bindings.map(|b| b.disk));
        let mut backend = CompositeBackend::attach(c, super::attached(&pool_io), storage);
        assert_eq!(backend.error(), None);
        assert_eq!(ServiceBackend::persistence(&backend), Persistence::RamShadow);
        assert!(!ServiceBackend::disk(&mut backend, qcom_modem_services::Partition::ModemFs1).unwrap().durable);
        ServiceBackend::disk_write(&mut backend, qcom_modem_services::Partition::ModemFs1, 0, &[0x99; 512]).unwrap();
        ServiceBackend::sync(&mut backend, qcom_modem_services::Partition::ModemFs1).unwrap();
        let mut bytes = [0; 512];
        ServiceBackend::disk_read(&mut backend, qcom_modem_services::Partition::ModemFs1, 0, &mut bytes).unwrap();
        assert_eq!(bytes, [0x99; 512]);
        assert_eq!(source, [[0x6b; 4096]; 4]);
        let handlers = qcom_modem_handlers::Handlers::attach(
            qcom_modem_handlers::Profile::validate_ram_shadow(c, &[]).unwrap(), backend);
        assert_eq!(handlers.error(), None);
    }

    #[test]
    fn composite_backend_gates_both_owners_and_forwards_disk_and_pool_io() {
        let (_pool_io, mut backend) = attached_backend();
        let observation = ServiceBackend::observe(&mut backend).unwrap();
        assert_eq!(observation.generation, 1);
        assert_eq!(observation.storage, Ownership::Host);
        assert_eq!(observation.heap, Ownership::Host);

        let disk =
            ServiceBackend::disk(&mut backend, qcom_modem_services::Partition::ModemFs1).unwrap();
        assert_eq!(disk.bytes, 4096);

        let page = [0x7c; 4096];
        let grant = Grant {
            serial: 1,
            pool: Pool::Storage,
            region: Region {
                base: STORAGE.base,
                bytes: 4096,
            },
            generation: 1,
            peer: Address { node: 0, port: 9 },
            ownership: Ownership::SharedStorage,
        };
        ServiceBackend::zero(&mut backend, Pool::Storage, 0, 4096).unwrap();
        let receipt = ServiceBackend::grant(&mut backend, grant).unwrap();
        assert_eq!(ServiceBackend::info(&mut backend, &receipt).unwrap(), grant);
        ServiceBackend::pool_write(&mut backend, &receipt, 0, &page).unwrap();
        let mut copied = [0; 4096];
        ServiceBackend::pool_read(&mut backend, &receipt, 0, &mut copied).unwrap();
        assert_eq!(copied, page);
        ServiceBackend::reclaim(&mut backend, &receipt).unwrap();

        let data = [0x9d; 512];
        ServiceBackend::disk_write(
            &mut backend,
            qcom_modem_services::Partition::ModemFs1,
            0,
            &data,
        )
        .unwrap();
        ServiceBackend::sync(&mut backend, qcom_modem_services::Partition::ModemFs1).unwrap();
        let mut readback = [0; 512];
        ServiceBackend::disk_read(
            &mut backend,
            qcom_modem_services::Partition::ModemFs1,
            0,
            &mut readback,
        )
        .unwrap();
        assert_eq!(readback, data);
    }

    #[test]
    fn composite_backend_quarantines_pool_identity_changes() {
        let (pool_io, mut backend) = attached_backend();
        pool_io.0.borrow_mut().observation.identity += 1;
        assert_eq!(ServiceBackend::observe(&mut backend), Err(Fault::Uncertain));
        assert_eq!(backend.error(), Some(qcom_modem_backend::Error::Pool));
    }
}

#[test]
fn reservation_exact_profile_and_injected_mapping_mismatches() {
    assert!(Reservations::validate(1, 0, MAPPINGS, &[]).is_ok());
    for i in 0..3 {
        for which in 0..6 {
            let mut m = MAPPINGS;
            match which {
                0 => m[i].region.base += 4096,
                1 => m[i].region.bytes += 4096,
                2 => m[i].virtual_address += 4096,
                3 => m[i].cached = true,
                4 => m[i].executable = true,
                _ => m[i].writable = false,
            };
            assert!(Reservations::validate(1, 0, m, &[]).is_err());
        }
    }
    assert!(Reservations::validate(0, 0, MAPPINGS, &[]).is_err());
    assert!(Reservations::validate(1, u32::MAX, MAPPINGS, &[]).is_err());
}
#[test]
fn exclusion_guards_overflow_zero_and_count() {
    for m in MAPPINGS {
        for base in [
            m.region.base - 4096,
            m.region.base,
            m.region.base + u64::from(m.region.bytes),
        ] {
            assert!(
                Reservations::validate(1, 0, MAPPINGS, &[Region { base, bytes: 4096 }]).is_err()
            );
        }
    }
    for r in [
        Region { base: 0, bytes: 0 },
        Region {
            base: u64::MAX,
            bytes: 2,
        },
    ] {
        assert!(Reservations::validate(1, 0, MAPPINGS, &[r]).is_err());
    }
    assert!(Reservations::validate(
        1,
        0,
        MAPPINGS,
        &[Region {
            base: 0x86000000,
            bytes: 0x200000
        }; 65]
    )
    .is_err());
}
#[test]
fn attach_requires_explicit_contract_and_initial_hlos() {
    for contract in [false, true] {
        let f = Fake::new();
        f.0.borrow_mut().observation.contract = contract;
        f.0.borrow_mut().observation.initial_hlos = !contract;
        let mut p = attached(&f);
        assert_eq!(p.status().owners, [Ownership::Unknown; 2]);
        assert!(p.zero(Pool::Storage, 0, 4096).is_err());
        assert_eq!(f.count(Tag::Zero), 0);
    }
}
#[test]
fn attachment_rejects_wrong_generation_identity_readiness_and_maps() {
    for n in 0..5 {
        let f = Fake::new();
        {
            let mut s = f.0.borrow_mut();
            match n {
                0 => s.observation.generation = 2,
                1 => s.observation.identity = 0,
                2 => s.observation.ready = false,
                3 => s.observation.mappings[0].cached = true,
                _ => s.fault = Some((Tag::Observe, Fault::Failed)),
            };
        }
        let p = attached(&f);
        assert_eq!(p.status().error, Some(Error::Attach));
        assert_eq!(f.count(Tag::Invoke), 0);
    }
}
#[test]
fn unscrubbed_or_partial_pages_cannot_be_granted() {
    let (f, mut p) = rig();
    assert!(p.grant(request(Pool::Storage, 1, 0, 4096)).is_err());
    p.zero(Pool::Storage, 0, 2048).unwrap();
    p.zero(Pool::Storage, 2048, 2048).unwrap();
    assert!(p.grant(request(Pool::Storage, 1, 0, 4096)).is_err());
    assert_eq!(f.count(Tag::Invoke), 0);
}
#[test]
fn full_capacity_scrub_is_bounded_and_both_pools_grant() {
    let (f, mut p) = rig();
    scrub(&mut p, Pool::Storage, 0, STORAGE.bytes);
    scrub(&mut p, Pool::Heap, 0, HEAP.bytes);
    let a = p
        .grant(request(Pool::Storage, 1, 0, STORAGE.bytes))
        .unwrap();
    let b = p.grant(request(Pool::Heap, 2, 0, HEAP.bytes)).unwrap();
    assert_eq!(p.info(&a).unwrap().region, STORAGE);
    assert_eq!(p.info(&b).unwrap().region, HEAP);
    assert!(f.0.borrow().ops.iter().all(|(_, n)| *n <= 4096));
    assert_eq!(f.count(Tag::Invoke), 2);
}
#[test]
fn storage_scm_envelope_matches_pinned_abi_and_zero_padding() {
    let (f, mut p) = rig();
    let _r = small(&mut p, Pool::Storage, 1);
    let s = f.0.borrow();
    let (w, b) = s.calls[0];
    assert_eq!(
        w,
        [0x42000c16, 0x1117, 0xa2a01000, 16, 0xa2a01040, 0xa2a01100, 0, 0]
    );
    assert_eq!(le64(&b, 0), STORAGE.base);
    assert_eq!(le64(&b, 8), 4096);
    assert_eq!(le32(&b, 64), 3);
    assert_eq!(le64(&b, 256), 4);
    assert_eq!(le64(&b, 264), SCRATCH.base + 128);
    assert_eq!(le64(&b, 272), 48);
    assert_eq!(&b[128..136], &[3, 0, 0, 0, 6, 0, 0, 0]);
    assert_eq!(&b[152..160], &[15, 0, 0, 0, 6, 0, 0, 0]);
    for span in [16..64, 68..128, 136..152, 160..256, 280..320] {
        assert!(b[span].iter().all(|v| *v == 0));
    }
}
#[test]
fn heap_grant_removes_hlos_and_uses_only_modem_rw() {
    let (f, mut p) = rig();
    let _r = small(&mut p, Pool::Heap, 1);
    let b = f.0.borrow().calls[0].1;
    assert_eq!(le32(&b, 64), 3);
    assert_eq!(le64(&b, 256), 4);
    assert_eq!(le64(&b, 272), 24);
    assert_eq!(&b[128..136], &[15, 0, 0, 0, 6, 0, 0, 0]);
    assert!(b[136..256].iter().all(|x| *x == 0));
}
#[test]
fn storage_reclaim_lists_both_previous_owners_and_only_hlos_destination() {
    let (f, mut p) = rig();
    let r = small(&mut p, Pool::Storage, 1);
    p.reclaim(&r).unwrap();
    let b = f.0.borrow().calls[1].1;
    assert_eq!(&b[64..72], &[3, 0, 0, 0, 15, 0, 0, 0]);
    assert_eq!(le64(&b, 256), 8);
    assert_eq!(le64(&b, 272), 24);
    assert_eq!(le32(&b, 128), 3);
    assert_eq!(le32(&b, 132), 6);
    assert_eq!(p.status().owners[0], Ownership::Host);
    assert!(p.info(&r).is_err());
}

#[test]
fn msa_calls_encode_all_three_fixed_destinations_without_overlapping_scm_args() {
    let reservation = qcom_msa::Reservation::validate(
        1,
        2,
        qcom_msa::SUNFISH_MSA_BASE,
        qcom_msa::SUNFISH_MSA_BYTES,
        true,
        true,
        &[],
    )
    .unwrap();
    let regions = [
        qcom_msa::MsaRegion {
            base: qcom_msa::SUNFISH_MSA_BASE,
            bytes: 0x4000,
            secure: true,
        },
        qcom_msa::MsaRegion {
            base: qcom_msa::SUNFISH_MSA_BASE + 0x4000,
            bytes: 0x1fc000,
            secure: false,
        },
    ];
    let plan = qcom_msa::plan_handoff(reservation, &regions).unwrap();
    let [secure_plan, nonsecure_plan] = plan.transfers();
    let secure = Call::for_msa_transfer(secure_plan).unwrap();
    let nonsecure = Call::for_msa_transfer(nonsecure_plan).unwrap();
    let expected_words = [
        0x42000c16,
        0x1117,
        SCRATCH.base,
        16,
        SCRATCH.base + 64,
        SCRATCH.base + 256,
        0,
        0,
    ];
    assert_eq!(secure.words(), &expected_words);
    assert_eq!(nonsecure.words(), &expected_words);

    let a = secure.bytes();
    assert_eq!(le64(a, 0), qcom_msa::SUNFISH_MSA_BASE);
    assert_eq!(le64(a, 8), 0x4000);
    assert_eq!(le32(a, 64), 3);
    assert_eq!(&a[128..136], &[15, 0, 0, 0, 6, 0, 0, 0]);
    assert_eq!(&a[152..160], &[24, 0, 0, 0, 6, 0, 0, 0]);
    assert_eq!(le64(a, 256), 4);
    assert_eq!(le64(a, 264), SCRATCH.base + 128);
    assert_eq!(le64(a, 272), 48);
    assert!(a[176..256].iter().all(|byte| *byte == 0));
    assert!(a[280..320].iter().all(|byte| *byte == 0));

    let b = nonsecure.bytes();
    assert_eq!(le64(b, 0), qcom_msa::SUNFISH_MSA_BASE + 0x4000);
    assert_eq!(le64(b, 8), 0x1fc000);
    assert_eq!(le32(b, 64), 3);
    assert_eq!(&b[128..136], &[15, 0, 0, 0, 6, 0, 0, 0]);
    assert_eq!(&b[152..160], &[24, 0, 0, 0, 6, 0, 0, 0]);
    assert_eq!(&b[176..184], &[25, 0, 0, 0, 6, 0, 0, 0]);
    assert_eq!(le64(b, 256), 4);
    assert_eq!(le64(b, 264), SCRATCH.base + 128);
    assert_eq!(le64(b, 272), 72);
    assert!(b[200..256].iter().all(|byte| *byte == 0));
    assert!(b[280..320].iter().all(|byte| *byte == 0));
}
#[test]
fn heap_reclaim_lists_modem_source_and_hlos_rw() {
    let (f, mut p) = rig();
    let r = small(&mut p, Pool::Heap, 1);
    p.reclaim(&r).unwrap();
    let b = f.0.borrow().calls[1].1;
    assert_eq!(le32(&b, 64), 15);
    assert_eq!(le64(&b, 256), 4);
    assert_eq!(le32(&b, 128), 3);
    assert_eq!(le32(&b, 132), 6);
}
#[test]
fn zero_and_io_bounds_never_access_guards() {
    let (f, mut p) = rig();
    for (at, n) in [(0, 0), (0, 4097), (STORAGE.bytes, 1), (u32::MAX, 4096)] {
        assert!(p.zero(Pool::Storage, at, n).is_err());
    }
    assert_eq!(f.count(Tag::Zero), 0);
    let r = small(&mut p, Pool::Storage, 1);
    for (at, n) in [(0, 0), (0, 4097), (4096, 1), (u32::MAX, 8)] {
        assert!(p.read(&r, at, &mut vec![0; n]).is_err());
        assert!(p.write(&r, at, &vec![0; n]).is_err());
    }
    assert_eq!(f.count(Tag::Read), 0);
    assert_eq!(f.count(Tag::Write), 0);
}
#[test]
fn all_invalid_grants_fail_before_staging() {
    let (f, mut p) = rig();
    scrub(&mut p, Pool::Storage, 0, 4096);
    for n in 0..12 {
        let mut g = request(Pool::Storage, 1, 0, 4096);
        match n {
            0 => g.region.base -= 4096,
            1 => g.region.base += 1,
            2 => g.region.bytes = 0,
            3 => g.region.bytes = 1,
            4 => g.region.bytes = STORAGE.bytes + 4096,
            5 => g.region.base = u64::MAX - 4095,
            6 => g.generation = 2,
            7 => g.peer.node = 1,
            8 => g.peer.port = 0,
            9 => g.peer.port = u32::MAX,
            10 => g.ownership = Ownership::ModemExclusive,
            _ => g.serial = 0,
        };
        assert!(p.grant(g).is_err());
    }
    assert_eq!(f.count(Tag::Stage), 0);
}
#[test]
fn host_heap_access_and_zero_forbidden_after_assignment() {
    let (f, mut p) = rig();
    let r = small(&mut p, Pool::Heap, 1);
    assert!(p.read(&r, 0, &mut [0; 8]).is_err());
    assert!(p.write(&r, 0, &[0; 8]).is_err());
    assert!(p.zero(Pool::Heap, 4096, 4096).is_err());
    assert_eq!(f.count(Tag::Read), 0);
    assert_eq!(f.count(Tag::Write), 0);
}
#[test]
fn receipt_cannot_cross_manager_even_same_grant() {
    let (f, mut p) = rig();
    let r = small(&mut p, Pool::Storage, 1);
    let f2 = Fake::new();
    let mut p2 = attached(&f2);
    let _r2 = small(&mut p2, Pool::Storage, 1);
    assert!(p2.info(&r).is_err());
    assert!(p2.reclaim(&r).is_err());
    assert_eq!(f2.count(Tag::Invoke), 1);
    assert_eq!(f.count(Tag::Invoke), 1);
}
#[test]
fn stale_receipt_cannot_free_later_allocation() {
    let (f, mut p) = rig();
    let old = small(&mut p, Pool::Heap, 1);
    p.reclaim(&old).unwrap();
    scrub(&mut p, Pool::Heap, 4096, 4096);
    let new = p.grant(request(Pool::Heap, 2, 4096, 4096)).unwrap();
    assert!(p.reclaim(&old).is_err());
    assert_eq!(f.count(Tag::Invoke), 3);
    assert_eq!(p.info(&new).unwrap().serial, 2);
}
#[test]
fn nonzero_subregion_uses_relative_receipt_offsets() {
    let (f, mut p) = rig();
    scrub(&mut p, Pool::Storage, 4096, 4096);
    let r = p.grant(request(Pool::Storage, 1, 4096, 4096)).unwrap();
    p.write(&r, 12, &[1, 2, 3, 4]).unwrap();
    let mut out = [0; 4];
    p.read(&r, 12, &mut out).unwrap();
    assert_eq!(out, [1, 2, 3, 4]);
    assert_eq!(&f.0.borrow().data[0][4108..4112], &out);
    assert_eq!(f.0.borrow().data[0][0], 0xa5);
}
#[test]
fn stage_definite_failure_keeps_host_owner_and_allows_one_later_attempt() {
    let (f, mut p) = rig();
    scrub(&mut p, Pool::Storage, 0, 4096);
    f.0.borrow_mut().fault = Some((Tag::Stage, Fault::Failed));
    assert!(matches!(
        p.grant(request(Pool::Storage, 1, 0, 4096)),
        Err(Fault::Failed)
    ));
    assert_eq!(p.status().owners[0], Ownership::Host);
    assert_eq!(p.status().error, None);
    assert_eq!(f.count(Tag::Invoke), 0);
    p.grant(request(Pool::Storage, 1, 0, 4096)).unwrap();
    assert_eq!(f.count(Tag::Invoke), 1);
}
#[test]
fn uncertain_stage_is_terminal_and_never_submitted() {
    let (f, mut p) = rig();
    scrub(&mut p, Pool::Storage, 0, 4096);
    f.0.borrow_mut().fault = Some((Tag::Stage, Fault::Uncertain));
    assert!(p.grant(request(Pool::Storage, 1, 0, 4096)).is_err());
    assert!(p.observe().is_err());
    assert!(p.grant(request(Pool::Storage, 1, 0, 4096)).is_err());
    assert_eq!(f.count(Tag::Invoke), 0);
}
#[test]
fn lifecycle_change_during_staging_prevents_smc() {
    let (f, mut p) = rig();
    scrub(&mut p, Pool::Heap, 0, 4096);
    f.0.borrow_mut().after = Some((Tag::Stage, |o| o.ready = false));
    assert!(p.grant(request(Pool::Heap, 1, 0, 4096)).is_err());
    assert_eq!(f.count(Tag::Invoke), 0);
    assert_eq!(p.status().owners[1], Ownership::Unknown);
}
#[test]
fn any_kernel_or_transport_error_quarantines_without_retry() {
    for fault in [Fault::Failed, Fault::Uncertain] {
        let (f, mut p) = rig();
        scrub(&mut p, Pool::Storage, 0, 4096);
        f.0.borrow_mut().fault = Some((Tag::Invoke, fault));
        assert!(matches!(
            p.grant(request(Pool::Storage, 1, 0, 4096)),
            Err(Fault::Uncertain)
        ));
        assert_eq!(p.status().owners[0], Ownership::Unknown);
        assert_eq!(p.status().attempted, 1);
        assert!(p.grant(request(Pool::Storage, 1, 0, 4096)).is_err());
        assert_eq!(f.count(Tag::Invoke), 1);
    }
}
#[test]
fn interrupted_busy_and_unknown_transport_or_command_statuses_never_retry() {
    for (a0, a1) in [
        (1, 0),
        ((-12i64) as u64, 0),
        ((-55i64) as u64, 0),
        ((-1i64) as u64, 0),
        (0x1234, 0),
        (0, 1),
    ] {
        let (f, mut p) = rig();
        scrub(&mut p, Pool::Heap, 0, 4096);
        f.0.borrow_mut().raw[0] = a0;
        f.0.borrow_mut().raw[1] = a1;
        assert!(p.grant(request(Pool::Heap, 1, 0, 4096)).is_err());
        assert_eq!(p.status().owners[1], Ownership::Unknown);
        assert_eq!(p.status().completed, 0);
        assert!(p.observe().is_err());
        assert_eq!(f.count(Tag::Invoke), 1);
    }
}
#[test]
fn post_success_completion_failure_withholds_receipt() {
    let (f, mut p) = rig();
    scrub(&mut p, Pool::Heap, 0, 4096);
    f.0.borrow_mut().fault = Some((Tag::Complete, Fault::Failed));
    assert!(p.grant(request(Pool::Heap, 1, 0, 4096)).is_err());
    assert_eq!(p.status().owners[1], Ownership::Unknown);
    assert_eq!(p.status().completed, 0);
    assert_eq!(f.count(Tag::Invoke), 1);
}
#[test]
fn post_success_lifecycle_failure_withholds_receipt() {
    let (f, mut p) = rig();
    scrub(&mut p, Pool::Heap, 0, 4096);
    f.0.borrow_mut().after = Some((Tag::Complete, |o| o.identity += 1));
    assert!(p.grant(request(Pool::Heap, 1, 0, 4096)).is_err());
    assert_eq!(p.status().owners[1], Ownership::Unknown);
    assert_eq!(p.status().completed, 0);
}
#[test]
fn uncertain_reclaim_retains_lease_and_permanently_denies_access() {
    let (f, mut p) = rig();
    let r = small(&mut p, Pool::Storage, 1);
    f.0.borrow_mut().raw[0] = (-12i64) as u64;
    assert!(p.reclaim(&r).is_err());
    assert_eq!(p.status().owners[0], Ownership::Unknown);
    assert!(p.read(&r, 0, &mut [0; 8]).is_err());
    assert!(p.reclaim(&r).is_err());
    assert!(p.zero(Pool::Storage, 0, 4096).is_err());
    assert_eq!(f.count(Tag::Invoke), 2);
}
#[test]
fn definite_reclaim_pre_submit_failure_keeps_valid_receipt() {
    let (f, mut p) = rig();
    let r = small(&mut p, Pool::Heap, 1);
    f.0.borrow_mut().fault = Some((Tag::Stage, Fault::Failed));
    assert_eq!(p.reclaim(&r), Err(Fault::Failed));
    assert_eq!(p.info(&r).unwrap().serial, 1);
    assert_eq!(f.count(Tag::Invoke), 1);
    p.reclaim(&r).unwrap();
    assert_eq!(f.count(Tag::Invoke), 2);
}
#[test]
fn uncertain_pool_data_access_stops_without_scm_cleanup() {
    for tag in [Tag::Read, Tag::Write] {
        let (f, mut p) = rig();
        let r = small(&mut p, Pool::Storage, 1);
        f.0.borrow_mut().fault = Some((tag, Fault::Uncertain));
        let result = if tag == Tag::Read {
            p.read(&r, 0, &mut [0; 8])
        } else {
            p.write(&r, 0, &[0; 8])
        };
        assert_eq!(result, Err(Fault::Uncertain));
        assert!(p.reclaim(&r).is_err());
        assert_eq!(f.count(Tag::Invoke), 1);
    }
}
#[test]
fn unwind_at_every_external_step_latches_and_cannot_repeat() {
    for tag in [
        Tag::Observe,
        Tag::Stage,
        Tag::Invoke,
        Tag::Complete,
        Tag::Zero,
    ] {
        let (f, mut p) = rig();
        scrub(&mut p, Pool::Heap, 0, 4096);
        f.0.borrow_mut().panic = Some(tag);
        assert!(catch_unwind(AssertUnwindSafe(|| {
            if tag == Tag::Zero {
                p.zero(Pool::Heap, 0, 4096).unwrap();
            } else {
                let _ = p.grant(request(Pool::Heap, 1, 0, 4096));
            }
        }))
        .is_err());
        let before = f.count(Tag::Invoke);
        assert!(p.observe().is_err());
        assert!(p.grant(request(Pool::Heap, 1, 0, 4096)).is_err());
        assert_eq!(f.count(Tag::Invoke), before);
    }
}
#[test]
fn lost_generation_or_contract_stops_all_pool_operations() {
    for n in 0..4 {
        let (f, mut p) = rig();
        let r = small(&mut p, Pool::Storage, 1);
        {
            let mut s = f.0.borrow_mut();
            match n {
                0 => s.observation.generation = 2,
                1 => s.observation.contract = false,
                2 => s.observation.ready = false,
                _ => s.observation.mappings[0].virtual_address += 4096,
            }
        }
        assert!(p.read(&r, 0, &mut [0; 4]).is_err());
        assert!(p.reclaim(&r).is_err());
        assert_eq!(f.count(Tag::Invoke), 1);
    }
}
#[test]
fn no_implicit_reclaim_or_io_on_drop() {
    let (f, mut p) = rig();
    let _r = small(&mut p, Pool::Heap, 1);
    let before = f.0.borrow().ops.len();
    drop(p);
    assert_eq!(f.0.borrow().ops.len(), before);
}
#[test]
fn serials_do_not_reuse_across_storage_and_heap() {
    let (f, mut p) = rig();
    let a = small(&mut p, Pool::Storage, 8);
    scrub(&mut p, Pool::Heap, 0, 4096);
    assert!(p.grant(request(Pool::Heap, 8, 0, 4096)).is_err());
    let _b = p.grant(request(Pool::Heap, 9, 0, 4096)).unwrap();
    p.reclaim(&a).unwrap();
    scrub(&mut p, Pool::Storage, 0, 4096);
    assert!(p.grant(request(Pool::Storage, 8, 0, 4096)).is_err());
    assert_eq!(f.count(Tag::Invoke), 3);
}
#[test]
fn active_pool_cannot_grow_or_overwrite_lease() {
    let (f, mut p) = rig();
    let r = small(&mut p, Pool::Storage, 1);
    assert!(p.grant(request(Pool::Storage, 2, 4096, 4096)).is_err());
    assert!(p.zero(Pool::Storage, 4096, 4096).is_err());
    assert!(p.info(&r).is_ok());
    assert_eq!(f.count(Tag::Invoke), 1);
}
#[test]
fn successful_reclaim_requires_new_scrub_before_next_grant() {
    let (f, mut p) = rig();
    let r = small(&mut p, Pool::Heap, 1);
    p.reclaim(&r).unwrap();
    assert!(p.grant(request(Pool::Heap, 2, 0, 4096)).is_err());
    scrub(&mut p, Pool::Heap, 0, 4096);
    p.grant(request(Pool::Heap, 2, 0, 4096)).unwrap();
    assert_eq!(f.count(Tag::Invoke), 3);
}
#[test]
fn known_zero_failure_does_not_certify_clean_page() {
    let (f, mut p) = rig();
    f.0.borrow_mut().fault = Some((Tag::Zero, Fault::Failed));
    assert_eq!(p.zero(Pool::Storage, 0, 4096), Err(Fault::Failed));
    assert!(p.grant(request(Pool::Storage, 1, 0, 4096)).is_err());
    assert_eq!(f.count(Tag::Invoke), 0);
}
#[test]
fn unrelated_pool_is_retained_after_one_transition_becomes_unknown() {
    let (f, mut p) = rig();
    let storage = small(&mut p, Pool::Storage, 1);
    scrub(&mut p, Pool::Heap, 0, 4096);
    f.0.borrow_mut().raw[0] = 1;
    assert!(p.grant(request(Pool::Heap, 2, 0, 4096)).is_err());
    assert_eq!(
        p.status().owners,
        [Ownership::SharedStorage, Ownership::Unknown]
    );
    assert!(p.reclaim(&storage).is_err());
    assert_eq!(f.count(Tag::Invoke), 2);
}

// Compose the real service handler with Pools, not the earlier simulated
// ownership receipt. Disk methods deliberately have no backing store here.
struct Actions {
    pools: Pools<Fake>,
}
impl qcom_modem_handlers::Backend for Actions {
    type Receipt = Receipt;
    fn observe(&mut self) -> Result<qcom_modem_handlers::Observation, Fault> {
        let (identity, owners) = self.pools.observe()?;
        Ok(qcom_modem_handlers::Observation {
            generation: 1,
            identity,
            ready: true,
            storage: owners[0],
            heap: owners[1],
        })
    }
    fn disk(
        &mut self,
        _: qcom_modem_services::Partition,
    ) -> Result<qcom_modem_handlers::Disk, Fault> {
        Ok(qcom_modem_handlers::Disk {
            bytes: 16384,
            writable: false,
            durable: true,
        })
    }
    fn zero(&mut self, p: Pool, at: u32, n: usize) -> Result<(), Fault> {
        self.pools.zero(p, at, n)
    }
    fn grant(&mut self, g: Grant) -> Result<Receipt, Fault> {
        self.pools.grant(g)
    }
    fn info(&mut self, r: &Receipt) -> Result<Grant, Fault> {
        self.pools.info(r)
    }
    fn reclaim(&mut self, r: &Receipt) -> Result<(), Fault> {
        self.pools.reclaim(r)
    }
    fn pool_read(&mut self, r: &Receipt, at: u32, b: &mut [u8]) -> Result<(), Fault> {
        self.pools.read(r, at, b)
    }
    fn pool_write(&mut self, r: &Receipt, at: u32, b: &[u8]) -> Result<(), Fault> {
        self.pools.write(r, at, b)
    }
    fn disk_read(
        &mut self,
        _: qcom_modem_services::Partition,
        _: u64,
        _: &mut [u8],
    ) -> Result<(), Fault> {
        Err(Fault::Failed)
    }
    fn disk_write(
        &mut self,
        _: qcom_modem_services::Partition,
        _: u64,
        _: &[u8],
    ) -> Result<(), Fault> {
        Err(Fault::Failed)
    }
    fn sync(&mut self, _: qcom_modem_services::Partition) -> Result<(), Fault> {
        Err(Fault::Failed)
    }
}
fn worker(f: &Fake) -> qcom_modem_handlers::Handlers<Actions> {
    use qcom_modem_handlers::{Configuration, Disk, Handlers, Profile};
    let c = Configuration {
        generation: 1,
        local_node: 1,
        peer_node: 0,
        storage_port: 0x4000,
        memory_port: 0x4001,
        storage: STORAGE,
        heap: HEAP,
        disks: [Disk {
            bytes: 16384,
            writable: false,
            durable: true,
        }; 4],
    };
    let h = Handlers::attach(
        Profile::validate(c, &[]).unwrap(),
        Actions { pools: attached(f) },
    );
    assert_eq!(h.error(), None);
    h
}
fn field(k: u8, b: &[u8]) -> Vec<u8> {
    let mut v = vec![k];
    v.extend((b.len() as u16).to_le_bytes());
    v.extend(b);
    v
}
fn number(k: u8, n: u32) -> Vec<u8> {
    field(k, &n.to_le_bytes())
}
fn action(
    w: &mut qcom_modem_handlers::Handlers<Actions>,
    memory: bool,
    id: u16,
    fields: &[Vec<u8>],
) -> Result<qcom_modem_handlers::Response, qcom_modem_handlers::Error> {
    use qcom_glink_qmi::{Config, Event, Session};
    use qcom_modem_services::Service;
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
    let mut q = vec![0, 1, 0];
    q.extend(id.to_le_bytes());
    q.extend((payload.len() as u16).to_le_bytes());
    q.extend(payload);
    let mut packet: Vec<u8> = [
        1u32,
        1,
        0,
        9,
        0,
        q.len() as u32,
        1,
        if memory { 0x4001 } else { 0x4000 },
    ]
    .iter()
    .flat_map(|v| v.to_le_bytes())
    .collect();
    packet.extend(q);
    packet.resize((packet.len() + 3) & !3, 0);
    let mut record: Vec<u8> = [0x20009u32, intent, packet.len() as u32, 0]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    record.extend(packet);
    record.resize((record.len() + 7) & !7, 0);
    let Event::Request(handle) = s.ingest(1, &record).unwrap() else {
        panic!("request")
    };
    let (context, request) = s.request(&handle).unwrap();
    w.begin(handle, context, request)?;
    for _ in 0..2048 {
        if w.step()? == qcom_modem_handlers::Progress::ReplyReady {
            let reply = w.response().unwrap().clone();
            s.queue_reply(&handle, reply.reply()).unwrap();
            w.reply_queued(&handle).unwrap();
            return Ok(reply);
        }
    }
    panic!("handler did not complete")
}
#[test]
fn real_storage_handler_grants_and_last_close_reclaims_reserved_pool() {
    let f = Fake::new();
    let mut w = worker(&f);
    let open = action(&mut w, false, 1, &[field(1, b"/boot/modem_fs1")]).unwrap();
    let handle = le32(open.fields(), 3);
    let response = action(&mut w, false, 4, &[number(1, handle), number(2, 8192)]).unwrap();
    assert_eq!(response.error(), None);
    assert_eq!(le64(response.fields(), 3), STORAGE.base);
    assert!(w.leased(Pool::Storage));
    assert_eq!(f.count(Tag::Invoke), 1);
    assert_eq!(
        action(&mut w, false, 2, &[number(1, handle)])
            .unwrap()
            .error(),
        None
    );
    assert!(!w.leased(Pool::Storage));
    assert_eq!(f.count(Tag::Invoke), 2);
}
#[test]
fn real_heap_handler_scrubs_rounds_grants_and_exact_free_reclaims() {
    let f = Fake::new();
    let mut w = worker(&f);
    let response = action(
        &mut w,
        true,
        0x22,
        &[number(1, 5000), number(2, 1), number(3, 0), number(4, 1)],
    )
    .unwrap();
    assert_eq!(response.error(), None);
    assert_eq!(f.count(Tag::Zero), 2);
    assert_eq!(le64(&f.0.borrow().calls[0].1, 8), 8192);
    let mut block = vec![1];
    block.extend(HEAP.base.to_le_bytes());
    block.extend(5000u32.to_le_bytes());
    assert_eq!(
        action(&mut w, true, 0x23, &[field(1, &block)])
            .unwrap()
            .error(),
        None
    );
    assert!(!w.leased(Pool::Heap));
    assert_eq!(f.count(Tag::Invoke), 2);
}
#[test]
fn real_handler_never_publishes_address_after_ambiguous_assignment() {
    let f = Fake::new();
    let mut w = worker(&f);
    f.0.borrow_mut().raw[0] = 1;
    assert!(action(
        &mut w,
        true,
        0x22,
        &[number(1, 4096), number(2, 1), number(3, 0), number(4, 1)]
    )
    .is_err());
    assert!(w.error().is_some());
    assert!(!w.leased(Pool::Heap));
    assert!(w.response().is_none());
    assert_eq!(f.count(Tag::Invoke), 1);
    assert!(w.step().is_err());
    assert_eq!(f.count(Tag::Invoke), 1);
}

#[test]
fn failed_lifecycle_observation_invalidates_both_ownership_claims() {
    let (f, mut p) = rig();
    let storage = small(&mut p, Pool::Storage, 1);
    f.0.borrow_mut().fault = Some((Tag::Observe, Fault::Failed));
    assert!(p.info(&storage).is_err());
    assert_eq!(p.status().owners, [Ownership::Unknown; 2]);
    assert!(p.reclaim(&storage).is_err());
    assert_eq!(f.count(Tag::Invoke), 1);
}
