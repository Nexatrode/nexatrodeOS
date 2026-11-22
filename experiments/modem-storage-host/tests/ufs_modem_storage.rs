use modem_storage_host::{
    qcom_modem_backend,
    qcom_modem_handlers::{self as handlers, Backend as HandlerBackend, Disk, Fault},
    qcom_modem_pools::{self as pools, Pools, Reservations},
    qcom_modem_services::Partition,
    qcom_modem_storage::{self as storage, Profile, Storage},
    qcom_tftp::ReadSourceIo,
    qcom_ufs_gpt::{self as gpt_parser, VerifiedModemPartitions},
    qcom_ufs_modem_storage::{self as adapter, BlockIo, MediaObservation},
    sunfish_smmu::SUNFISH_MAX_SMR_SLOTS,
};
use std::{cell::RefCell, rc::Rc};

#[path = "common/mod.rs"]
mod common;

const LUN_SECTORS: u64 = 256;
const LUN: u8 = 0;
const DEVICE: storage::Digest = [0x31; 32];
const DISK_GUID: [u8; 16] = [0x42; 16];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Op {
    Read,
    Write,
    Sync,
}

struct State {
    observation: MediaObservation,
    bytes: Vec<u8>,
    operations: Vec<(Op, u64, usize)>,
    fault: Option<(Op, Fault)>,
}

#[derive(Clone)]
struct FakeBlock(Rc<RefCell<State>>);

impl FakeBlock {
    fn new() -> Self {
        let mut bytes = vec![0; LUN_SECTORS as usize * 512];
        for (lba, sector) in bytes.chunks_exact_mut(512).enumerate() {
            sector.fill(lba as u8);
        }
        Self(Rc::new(RefCell::new(State {
            observation: observation(),
            bytes,
            operations: Vec::new(),
            fault: None,
        })))
    }

    fn set_fault(&self, op: Op, fault: Fault) {
        self.0.borrow_mut().fault = Some((op, fault));
    }

    fn count(&self, op: Op) -> usize {
        self.0
            .borrow()
            .operations
            .iter()
            .filter(|operation| operation.0 == op)
            .count()
    }
}

impl BlockIo for FakeBlock {
    fn observe(&mut self) -> Result<MediaObservation, Fault> {
        Ok(self.0.borrow().observation)
    }

    fn read_blocks(&mut self, first_lba: u64, out: &mut [u8]) -> Result<(), Fault> {
        let mut state = self.0.borrow_mut();
        state.operations.push((Op::Read, first_lba, out.len()));
        if state.fault.is_some_and(|(op, _)| op == Op::Read) {
            return Err(state.fault.take().unwrap().1);
        }
        let start = first_lba as usize * 512;
        out.copy_from_slice(&state.bytes[start..start + out.len()]);
        Ok(())
    }

    fn write_blocks(&mut self, first_lba: u64, bytes: &[u8]) -> Result<(), Fault> {
        let mut state = self.0.borrow_mut();
        state.operations.push((Op::Write, first_lba, bytes.len()));
        if state.fault.is_some_and(|(op, _)| op == Op::Write) {
            return Err(state.fault.take().unwrap().1);
        }
        let start = first_lba as usize * 512;
        state.bytes[start..start + bytes.len()].copy_from_slice(bytes);
        Ok(())
    }

    fn synchronize_cache(&mut self) -> Result<(), Fault> {
        let mut state = self.0.borrow_mut();
        state.operations.push((Op::Sync, 0, 0));
        if state.fault.is_some_and(|(op, _)| op == Op::Sync) {
            return Err(state.fault.take().unwrap().1);
        }
        Ok(())
    }
}

fn observation() -> MediaObservation {
    MediaObservation {
        generation: 7,
        identity: 19,
        device: DEVICE,
        gpt_disk_guid: DISK_GUID,
        source_digests: configuration().bindings.map(|binding| binding.source),
        fresh_boot: true,
        exclusive: true,
        platform_ready: true,
        dma_uncached: true,
        dma_exclusive: true,
        dma_translated: true,
        dma_stream_id: 0x300,
        smmu_route_slot: Some(4),
        smmu_route_id: 0x300,
        smmu_route_mask: 0,
        smmu_route_translated: true,
        smmu_route_context_bank: Some(8),
        smmu_context_bank: Some(8),
        smmu_context_enabled: true,
        lun: LUN,
        lun_sectors: LUN_SECTORS,
        logical_block_bytes: 512,
        write_supported: true,
        sync_cache_supported: true,
    }
}

fn gpt() -> VerifiedModemPartitions {
    let mut entries = vec![0; gpt_parser::MAX_ENTRY_ARRAY_BYTES];
    add_entry(&mut entries, 0, b"modemst1", 40, 47, 1);
    add_entry(&mut entries, 1, b"modemst2", 64, 71, 2);
    add_entry(&mut entries, 2, b"fsg", 88, 91, 3);
    add_entry(&mut entries, 3, b"fsc", 104, 107, 4);
    add_entry(&mut entries, 4, b"super", 128, 191, 5);
    let array_crc = crc32(&entries);
    let primary_header = make_header(1, LUN_SECTORS - 1, 2, array_crc);
    let backup_header = make_header(
        LUN_SECTORS - 1,
        1,
        LUN_SECTORS - 1 - (entries.len() / 512) as u64,
        array_crc,
    );
    gpt_parser::parse_sunfish_pair_verified(
        &primary_header,
        &entries,
        &backup_header,
        &entries,
        LUN_SECTORS,
    )
    .unwrap()
}

fn gpt_native_4k() -> VerifiedModemPartitions {
    const BLOCKS: u64 = 64;
    let mut entries = vec![0; gpt_parser::MAX_ENTRY_ARRAY_BYTES];
    add_entry(&mut entries, 0, b"modemst1", 8, 8, 1);
    add_entry(&mut entries, 1, b"modemst2", 10, 10, 2);
    add_entry(&mut entries, 2, b"fsg", 12, 12, 3);
    add_entry(&mut entries, 3, b"fsc", 14, 14, 4);
    add_entry(&mut entries, 4, b"super", 16, 23, 5);
    let array_crc = crc32(&entries);
    let mut primary = make_header(1, BLOCKS - 1, 2, array_crc);
    put64(&mut primary, 40, 6);
    put64(&mut primary, 48, BLOCKS - 6);
    refresh_crcs(&mut primary, &entries);
    let mut backup = make_header(BLOCKS - 1, 1, BLOCKS - 5, array_crc);
    put64(&mut backup, 40, 6);
    put64(&mut backup, 48, BLOCKS - 6);
    refresh_crcs(&mut backup, &entries);
    gpt_parser::parse_sunfish_pair_verified_with_block_size(
        &primary, &entries, &backup, &entries, BLOCKS, 4096,
    )
    .unwrap()
}

fn make_header(my_lba: u64, alternate_lba: u64, array_lba: u64, array_crc: u32) -> [u8; 512] {
    let mut header = [0; 512];
    header[..8].copy_from_slice(b"EFI PART");
    put32(&mut header, 8, 0x0001_0000);
    put32(&mut header, 12, 92);
    put64(&mut header, 24, my_lba);
    put64(&mut header, 32, alternate_lba);
    put64(&mut header, 40, 34);
    put64(&mut header, 48, LUN_SECTORS - 34);
    header[56..72].copy_from_slice(&DISK_GUID);
    put64(&mut header, 72, array_lba);
    put32(&mut header, 80, 128);
    put32(&mut header, 84, 128);
    put32(&mut header, 88, array_crc);
    let header_crc = crc32(&header[..92]);
    put32(&mut header, 16, header_crc);
    header
}

fn add_entry(entries: &mut [u8], index: usize, name: &[u8], first: u64, last: u64, guid: u8) {
    let at = index * 128;
    entries[at..at + 16].fill(0x81);
    entries[at + 16..at + 32].fill(guid);
    put64(entries, at + 32, first);
    put64(entries, at + 40, last);
    for (position, byte) in name.iter().enumerate() {
        entries[at + 56 + position * 2..at + 58 + position * 2]
            .copy_from_slice(&u16::from(*byte).to_le_bytes());
    }
}

fn refresh_crcs(header: &mut [u8; 512], entries: &[u8]) {
    put32(header, 88, crc32(entries));
    put32(header, 16, 0);
    let crc = crc32(&header[..92]);
    put32(header, 16, crc);
}

fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn put64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn configuration() -> storage::Configuration {
    let sizes = [8 * 512, 8 * 512, 4 * 512, 4 * 512];
    storage::Configuration {
        generation: 7,
        device: DEVICE,
        bindings: std::array::from_fn(|index| storage::Binding {
            partition: storage::PARTITIONS[index],
            object: index as u64 + 1,
            source: [index as u8 + 1; 32],
            disk: Disk {
                bytes: sizes[index],
                writable: index == 1,
                durable: true,
            },
        }),
    }
}

fn configuration_4k() -> storage::Configuration {
    storage::Configuration {
        generation: 7,
        device: DEVICE,
        bindings: std::array::from_fn(|index| storage::Binding {
            partition: storage::PARTITIONS[index],
            object: index as u64 + 1,
            source: [index as u8 + 1; 32],
            disk: Disk {
                bytes: 4096,
                writable: index == 1,
                durable: true,
            },
        }),
    }
}

#[test]
fn native_4k_gpt_extents_are_normalized_to_the_512_byte_service_api() {
    let map = adapter::PartitionMap::from_verified_gpt(
        configuration_4k(),
        DISK_GUID,
        gpt_native_4k(),
        LUN,
    )
    .unwrap();
    assert_eq!(map.extent(Partition::ModemFs1).first_lba, 64);
    assert_eq!(map.extent(Partition::ModemFs1).sectors, 8);
    assert_eq!(map.extent(Partition::ModemFs2).first_lba, 80);
    assert_eq!(map.extent(Partition::ModemFsg).first_lba, 96);
    assert_eq!(map.extent(Partition::ModemFsc).first_lba, 112);
    assert_eq!(map.super_extent().first_lba, 128);
    assert_eq!(map.super_extent().sectors, 64);
}

#[test]
fn provider_requires_the_observed_native_geometry_to_match_the_verified_gpt() {
    let block = FakeBlock::new();
    block.0.borrow_mut().observation.logical_block_bytes = 4096;
    let map = adapter::PartitionMap::from_verified_gpt(
        configuration_4k(),
        DISK_GUID,
        gpt_native_4k(),
        LUN,
    )
    .unwrap();
    let mut provider = adapter::UfsModemStorage::new(map, block);
    assert!(storage::Io::observe(&mut provider).is_ok());

    let block = FakeBlock::new();
    block.0.borrow_mut().observation.logical_block_bytes = 4096;
    let map =
        adapter::PartitionMap::from_verified_gpt(configuration(), DISK_GUID, gpt(), LUN).unwrap();
    let mut provider = adapter::UfsModemStorage::new(map, block);
    assert_eq!(storage::Io::observe(&mut provider), Err(Fault::Failed));
}

fn provider(block: FakeBlock) -> adapter::UfsModemStorage<FakeBlock> {
    adapter::UfsModemStorage::new(
        adapter::PartitionMap::from_verified_gpt(configuration(), DISK_GUID, gpt(), LUN).unwrap(),
        block,
    )
}

fn service(block: FakeBlock) -> Storage<adapter::UfsModemStorage<FakeBlock>> {
    Storage::attach(Profile::validate(configuration()).unwrap(), provider(block))
}

#[test]
fn gpt_extents_map_to_the_remote_storage_names_in_protocol_order() {
    let map =
        adapter::PartitionMap::from_verified_gpt(configuration(), DISK_GUID, gpt(), LUN).unwrap();
    assert_eq!(map.extent(Partition::ModemFs1).first_lba, 40);
    assert_eq!(map.extent(Partition::ModemFs2).first_lba, 64);
    assert_eq!(map.extent(Partition::ModemFsg).first_lba, 88);
    assert_eq!(map.extent(Partition::ModemFsc).first_lba, 104);
    assert_eq!(map.super_extent().first_lba, 128);
    assert_eq!(map.super_extent().sectors, 64);
    assert_eq!(map.disk_guid(), DISK_GUID);
}

#[test]
fn super_source_reads_only_bounded_aligned_bytes_inside_the_verified_extent() {
    let block = FakeBlock::new();
    let mut provider = provider(block.clone());
    let mut source = provider.super_source().unwrap();
    assert_eq!(source.len().unwrap(), 64 * 512);

    let mut bytes = vec![0; 5000];
    source.read_exact_at(7, &mut bytes).unwrap();
    assert!(bytes[..505].iter().all(|byte| *byte == 128));
    assert!(bytes[505..1017].iter().all(|byte| *byte == 129));
    assert!(bytes[1017..1529].iter().all(|byte| *byte == 130));

    let state = block.0.borrow();
    assert_eq!(
        state.operations,
        [(Op::Read, 128, 4096), (Op::Read, 136, 1024)]
    );
    assert_eq!(
        state
            .operations
            .iter()
            .filter(|op| op.0 != Op::Read)
            .count(),
        0
    );
}

#[test]
fn super_source_rejects_out_of_bounds_without_io_and_can_read_afterward() {
    let block = FakeBlock::new();
    let mut provider = provider(block.clone());
    let mut source = provider.super_source().unwrap();
    let end = source.len().unwrap();
    assert!(source.read_exact_at(end - 1, &mut [0; 2]).is_err());
    assert_eq!(block.count(Op::Read), 0);

    let mut last_byte = [0];
    source.read_exact_at(end - 1, &mut last_byte).unwrap();
    assert_eq!(last_byte, [191]);
    assert_eq!(block.0.borrow().operations, [(Op::Read, 191, 512)]);
}

#[test]
fn super_source_latches_off_after_an_uncertain_read() {
    let block = FakeBlock::new();
    let mut provider = provider(block.clone());
    let mut source = provider.super_source().unwrap();
    block.set_fault(Op::Read, Fault::Uncertain);
    assert!(source.read_exact_at(0, &mut [0; 512]).is_err());
    drop(source);

    assert!(matches!(provider.super_source(), Err(Fault::Failed)));
    assert_eq!(block.count(Op::Read), 1);
}

#[test]
fn super_source_latches_off_if_its_live_lease_changes() {
    let block = FakeBlock::new();
    let mut provider = provider(block.clone());
    let mut source = provider.super_source().unwrap();
    block.0.borrow_mut().observation.identity += 1;
    assert!(source.read_exact_at(0, &mut [0; 512]).is_err());
    drop(source);

    assert!(matches!(provider.super_source(), Err(Fault::Failed)));
    assert_eq!(block.count(Op::Read), 0);
}

#[test]
fn service_reads_and_writes_only_the_requested_partition_extent() {
    let block = FakeBlock::new();
    let mut service = service(block.clone());
    assert_eq!(service.observe().unwrap(), 19);

    let mut first = [0; 512];
    service.read(Partition::ModemFs1, 0, &mut first).unwrap();
    assert!(first.iter().all(|byte| *byte == 40));

    service
        .write(Partition::ModemFs2, 512, &[0x77; 1024])
        .unwrap();
    let state = block.0.borrow();
    let start = 65 * 512;
    assert_eq!(&state.bytes[start..start + 1024], &[0x77; 1024]);
    assert_eq!(state.operations[0], (Op::Read, 40, 512));
    assert_eq!(state.operations[1], (Op::Write, 65, 1024));
}

#[test]
fn read_only_partition_rejects_writes_and_durable_write_flushes_lun() {
    let block = FakeBlock::new();
    let mut service = service(block.clone());
    assert_eq!(
        service.write(Partition::ModemFs1, 0, &[0x55; 512]),
        Err(Fault::Failed)
    );
    assert_eq!(block.count(Op::Write), 0);

    service.write(Partition::ModemFs2, 0, &[0x66; 512]).unwrap();
    service.flush(Partition::ModemFs2).unwrap();
    assert_eq!(block.count(Op::Write), 1);
    assert_eq!(block.count(Op::Sync), 1);
}

#[test]
fn provider_refuses_inexact_or_unsafe_smmu_media_observations() {
    for mutate in [
        |o: &mut MediaObservation| o.fresh_boot = false,
        |o: &mut MediaObservation| o.exclusive = false,
        |o: &mut MediaObservation| o.dma_translated = false,
        |o: &mut MediaObservation| o.smmu_route_slot = Some(SUNFISH_MAX_SMR_SLOTS as u8),
        |o: &mut MediaObservation| o.smmu_route_mask = 0x0f,
        |o: &mut MediaObservation| o.smmu_route_translated = false,
        |o: &mut MediaObservation| o.smmu_route_context_bank = Some(9),
        |o: &mut MediaObservation| o.smmu_context_enabled = false,
        |o: &mut MediaObservation| o.device[0] ^= 1,
        |o: &mut MediaObservation| o.lun_sectors = 100,
    ] {
        let block = FakeBlock::new();
        mutate(&mut block.0.borrow_mut().observation);
        let mut service = service(block.clone());
        assert!(service.observe().is_err());
        assert!(service.read(Partition::ModemFs1, 0, &mut [0; 512]).is_err());
        assert_eq!(block.count(Op::Read), 0);
    }
}

#[test]
fn mismatched_guid_or_wrong_partition_capacity_is_rejected() {
    assert_eq!(
        adapter::PartitionMap::from_verified_gpt(configuration(), [0x99; 16], gpt(), LUN)
            .unwrap_err(),
        adapter::Error::DiskGuidMismatch
    );

    let mut wrong_size = configuration();
    wrong_size.bindings[0].disk.bytes -= 512;
    assert_eq!(
        adapter::PartitionMap::from_verified_gpt(wrong_size, DISK_GUID, gpt(), LUN).unwrap_err(),
        adapter::Error::InvalidExtent
    );
}

#[test]
fn uncertain_write_is_quarantined_and_never_retried() {
    let block = FakeBlock::new();
    let mut service = service(block.clone());
    block.set_fault(Op::Write, Fault::Uncertain);
    assert_eq!(
        service.write(Partition::ModemFs2, 0, &[0x11; 512]),
        Err(Fault::Uncertain)
    );
    assert!(service.observe().is_err());
    assert_eq!(block.count(Op::Write), 1);
}

#[test]
fn transfer_bound_and_partition_end_are_checked_before_block_io() {
    let block = FakeBlock::new();
    let mut service = service(block.clone());
    assert_eq!(
        service.read(Partition::ModemFs1, 0, &mut [0; handlers::IO_BYTES + 512]),
        Err(Fault::Failed)
    );
    assert_eq!(
        service.read(Partition::ModemFs1, 7 * 512, &mut [0; 1024]),
        Err(Fault::Failed)
    );
    assert_eq!(block.count(Op::Read), 0);
}

#[test]
fn verified_ufs_provider_composes_with_the_scm_pool_backend() {
    let block = FakeBlock::new();
    let pool_io = common::pool();
    pool_io.0.borrow_mut().observation.generation = 7;
    let pool_manager = Pools::attach(
        Reservations::validate(7, 0, pools::MAPPINGS, &[]).unwrap(),
        pool_io,
    );
    let ufs_store = Storage::attach(
        Profile::validate(configuration()).unwrap(),
        provider(block.clone()),
    );
    let handler_configuration = handlers::Configuration {
        generation: 7,
        local_node: 1,
        peer_node: 0,
        storage_port: 0x4000,
        memory_port: 0x4001,
        storage: pools::STORAGE,
        heap: pools::HEAP,
        disks: configuration().bindings.map(|binding| binding.disk),
    };
    let mut backend =
        qcom_modem_backend::Backend::attach(handler_configuration, pool_manager, ufs_store);

    assert_eq!(backend.error(), None);
    assert!(backend.observe().unwrap().ready);
    assert_eq!(backend.disk(Partition::ModemFs1).unwrap().bytes, 8 * 512);
    let mut sector = [0; 512];
    backend
        .disk_read(Partition::ModemFs1, 0, &mut sector)
        .unwrap();
    assert!(sector.iter().all(|byte| *byte == 40));
    assert_eq!(block.0.borrow().operations[0], (Op::Read, 40, 512));

    backend.zero(handlers::Pool::Storage, 0, 4096).unwrap();
    let grant = handlers::Grant {
        serial: 1,
        pool: handlers::Pool::Storage,
        region: handlers::Region {
            base: pools::STORAGE.base,
            bytes: 4096,
        },
        generation: 7,
        peer: modem_storage_host::qcom_qmi_router::Address { node: 0, port: 9 },
        ownership: handlers::Ownership::SharedStorage,
    };
    let receipt = backend.grant(grant).unwrap();
    assert_eq!(backend.info(&receipt).unwrap(), grant);
    backend.reclaim(&receipt).unwrap();
}
