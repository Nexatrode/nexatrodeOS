use modem_storage_host::qcom_modem_handlers::Fault;
use modem_storage_host::qcom_ufs_host as host;
use modem_storage_host::qcom_ufs_irq as ufs_irq;
use modem_storage_host::qcom_ufs_modem_storage::{BlockIo, MediaObservation};
use modem_storage_host::qcom_ufs_protocol as protocol;
use modem_storage_host::qcom_ufs_scsi_block_io::{
    Error as ScsiBlockIoError, ObservationSource, UfsScsiBlockIo,
};
use std::cell::Cell;
use std::rc::Rc;

const CAP_64_BIT: u32 = 0x0100_0000;
const HCS_DEVICE_PRESENT: u32 = 1;
const HCS_ALL_READY: u32 = 0x0e;
const HCS_UIC_READY: u32 = 1 << 3;
const INT_TRANSFER_COMPLETE: u32 = 1;
const INT_FATAL: u32 = 1 << 16;

struct FakeIo {
    registers: [u32; 64],
    ready: bool,
    power_ready: bool,
    complete_link_startup: bool,
    complete_enable: bool,
    complete_transfer: bool,
    uic_result: u32,
    device_present: bool,
    fail_read: Option<u32>,
    fail_write: Option<u32>,
    write_log: Vec<(u32, u32)>,
    read_log: Vec<u32>,
    delays: Vec<u32>,
    status_acknowledged: Rc<Cell<bool>>,
}

impl FakeIo {
    fn new() -> Self {
        let mut io = Self {
            registers: [0; 64],
            ready: true,
            power_ready: true,
            complete_link_startup: true,
            complete_enable: true,
            complete_transfer: true,
            uic_result: 0,
            device_present: true,
            fail_read: None,
            fail_write: None,
            write_log: Vec::new(),
            read_log: Vec::new(),
            delays: Vec::new(),
            status_acknowledged: Rc::new(Cell::new(false)),
        };
        io.set(host::REG_VERSION, 0x200);
        io.set(host::REG_CAPABILITIES, CAP_64_BIT | 1 | (1 << 16));
        io.set(host::REG_CONTROLLER_STATUS, HCS_UIC_READY);
        io
    }

    fn set(&mut self, offset: u32, value: u32) {
        self.registers[(offset / 4) as usize] = value;
    }

    fn get(&self, offset: u32) -> u32 {
        self.registers[(offset / 4) as usize]
    }

    fn start(&mut self) -> host::Host {
        let mut h = host::Host::new();
        h.enable(self, 4).unwrap();
        h.link_startup(self, 4).unwrap();
        h
    }
}

impl host::RegisterIo for FakeIo {
    fn controller_power_ready(&mut self) -> bool {
        self.power_ready
    }

    fn platform_ready(&mut self) -> bool {
        self.ready
    }

    fn read32(&mut self, offset: u32) -> Result<u32, host::Error> {
        self.read_log.push(offset);
        if self.fail_read == Some(offset) {
            return Err(host::Error::Io);
        }
        Ok(self.get(offset))
    }

    fn write32(&mut self, offset: u32, value: u32) -> Result<(), host::Error> {
        if self.fail_write == Some(offset) {
            return Err(host::Error::Io);
        }
        self.write_log.push((offset, value));
        if offset == host::REG_INTERRUPT_STATUS {
            self.set(offset, self.get(offset) & !value);
            self.status_acknowledged.set(true);
        } else if offset == host::REG_CONTROLLER_ENABLE && !self.complete_enable {
            self.set(offset, 0);
        } else if offset == host::REG_UTRL_DOORBELL && value == 1 && self.complete_transfer {
            self.set(offset, 0);
            self.set(
                host::REG_INTERRUPT_STATUS,
                self.get(host::REG_INTERRUPT_STATUS) | INT_TRANSFER_COMPLETE,
            );
        } else {
            self.set(offset, value);
        }
        if offset == host::REG_UIC_COMMAND && self.complete_link_startup {
            self.set(host::REG_UIC_ARGUMENT_2, self.uic_result);
            self.set(
                host::REG_CONTROLLER_STATUS,
                HCS_UIC_READY
                    | HCS_ALL_READY
                    | if self.device_present {
                        HCS_DEVICE_PRESENT
                    } else {
                        0
                    },
            );
            self.set(host::REG_INTERRUPT_STATUS, 1 << 10);
        }
        Ok(())
    }

    fn dma_write_barrier(&mut self) -> Result<(), host::Error> {
        Ok(())
    }

    fn dma_read_barrier(&mut self) -> Result<(), host::Error> {
        Ok(())
    }

    fn delay_us(&mut self, microseconds: u32) {
        self.delays.push(microseconds);
    }
}

struct FakeKernelAck {
    status_acknowledged: Rc<Cell<bool>>,
    channel_ids: Vec<usize>,
    fail: bool,
}

impl ufs_irq::KernelAcknowledger for FakeKernelAck {
    fn acknowledge(&mut self, channel_id: usize) -> Result<(), ()> {
        if !self.status_acknowledged.get() || self.fail {
            return Err(());
        }
        self.channel_ids.push(channel_id);
        Ok(())
    }
}

struct FakeDma {
    transfer_list: u64,
    command_descriptor: u64,
    transfer: [u8; protocol::UTRD_BYTES],
    ucd: [u8; 1040],
    ready: bool,
    ocs: u32,
    response_status: u8,
    fail_next_read: bool,
    lose_after_first_write: bool,
    data_address: Option<u64>,
    data_bytes: [u8; protocol::MAX_DATA_BYTES],
    data_write_lengths: Vec<usize>,
    inquiry_data: [u8; protocol::STANDARD_INQUIRY_BYTES],
    capacity_data: [u8; protocol::READ_CAPACITY10_BYTES],
    data_read_lengths: Vec<usize>,
    command_log: Vec<(u8, u8, u8)>,
    fail_data_read: bool,
}

impl FakeDma {
    fn new(transfer_list: u64, command_descriptor: u64) -> Self {
        Self {
            transfer_list,
            command_descriptor,
            transfer: [0; protocol::UTRD_BYTES],
            ucd: [0; 1040],
            ready: true,
            ocs: 0,
            response_status: 0,
            fail_next_read: false,
            lose_after_first_write: false,
            data_address: None,
            data_bytes: [0; protocol::MAX_DATA_BYTES],
            data_write_lengths: Vec::new(),
            inquiry_data: [0; protocol::STANDARD_INQUIRY_BYTES],
            capacity_data: [0; protocol::READ_CAPACITY10_BYTES],
            data_read_lengths: Vec::new(),
            command_log: Vec::new(),
            fail_data_read: false,
        }
    }

    fn configure_discovery_data(&mut self, address: u64) {
        self.data_address = Some(address);
        self.inquiry_data[0] = 0;
        self.inquiry_data[4] = 31;
        self.inquiry_data[8..16].copy_from_slice(b"GOOGLE  ");
        self.inquiry_data[16..32].copy_from_slice(b"SUNFISH UFS     ");
        self.inquiry_data[32..36].copy_from_slice(b"0001");
        self.capacity_data = [0, 0, 0x3f, 0xff, 0, 0, 2, 0];
    }
}

impl host::DmaIo for FakeDma {
    fn ready(&mut self) -> bool {
        self.ready
    }

    fn write(&mut self, address: u64, bytes: &[u8]) -> Result<(), host::Error> {
        if address == self.transfer_list && bytes.len() == self.transfer.len() {
            self.transfer.copy_from_slice(bytes);
        } else if address == self.command_descriptor && bytes.len() == self.ucd.len() {
            self.ucd.copy_from_slice(bytes);
            self.command_log
                .push((self.ucd[2], self.ucd[3], self.ucd[16]));
        } else if Some(address) == self.data_address && bytes.len() <= self.data_bytes.len() {
            self.data_bytes[..bytes.len()].copy_from_slice(bytes);
            self.data_write_lengths.push(bytes.len());
        } else {
            return Err(host::Error::Io);
        }
        if self.lose_after_first_write {
            self.lose_after_first_write = false;
            self.ready = false;
        }
        Ok(())
    }

    fn read(&mut self, address: u64, bytes: &mut [u8]) -> Result<(), host::Error> {
        if self.fail_next_read {
            self.fail_next_read = false;
            return Err(host::Error::Io);
        }
        if address == self.transfer_list && bytes.len() == self.transfer.len() {
            bytes.copy_from_slice(&self.transfer);
            bytes[8..12].copy_from_slice(&self.ocs.to_le_bytes());
        } else if address == self.command_descriptor + 512 && bytes.len() == protocol::UPIU_BYTES {
            bytes.fill(0);
            bytes[0] = 0x21;
            bytes[2] = self.ucd[2];
            bytes[3] = self.ucd[3];
            bytes[7] = self.response_status;
        } else if Some(address) == self.data_address
            && bytes.len() == protocol::STANDARD_INQUIRY_BYTES
        {
            if self.fail_data_read {
                self.fail_data_read = false;
                return Err(host::Error::Io);
            }
            bytes.copy_from_slice(&self.inquiry_data);
            self.data_read_lengths.push(bytes.len());
        } else if Some(address) == self.data_address
            && bytes.len() == protocol::READ_CAPACITY10_BYTES
        {
            if self.fail_data_read {
                self.fail_data_read = false;
                return Err(host::Error::Io);
            }
            bytes.copy_from_slice(&self.capacity_data);
            self.data_read_lengths.push(bytes.len());
        } else if Some(address) == self.data_address && bytes.len() <= self.data_bytes.len() {
            bytes.copy_from_slice(&self.data_bytes[..bytes.len()]);
            self.data_read_lengths.push(bytes.len());
        } else {
            return Err(host::Error::Io);
        }
        Ok(())
    }
}

struct FakeObservation(MediaObservation);

impl FakeObservation {
    fn new() -> Self {
        Self(MediaObservation {
            generation: 7,
            identity: 19,
            device: [0x31; 32],
            gpt_disk_guid: [0x42; 16],
            source_digests: [[0x53; 32]; 4],
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
            lun: 0,
            lun_sectors: 256,
            logical_block_bytes: 512,
            write_supported: true,
            sync_cache_supported: true,
        })
    }
}

impl ObservationSource for FakeObservation {
    fn observe_media(&mut self) -> Result<MediaObservation, Fault> {
        Ok(self.0)
    }
}

#[test]
fn decodes_and_retains_observed_sunfish_hci_capabilities() {
    const SUNFISH_CAP: u32 = 0x1587_031f;
    let expected = host::ControllerCapabilities {
        raw: SUNFISH_CAP,
        transfer_request_slots: 32,
        ready_to_transfer_requests: 4,
        task_management_slots: 8,
        auto_hibern8: true,
        addressing_64_bit: true,
        out_of_order_data_delivery: false,
        uic_dme_test_mode: true,
        crypto: true,
    };
    assert_eq!(
        host::ControllerCapabilities::from_register(SUNFISH_CAP),
        expected
    );

    let mut io = FakeIo::new();
    io.set(host::REG_VERSION, 0x210);
    io.set(host::REG_CAPABILITIES, SUNFISH_CAP);
    let mut controller = host::Host::new();
    controller.enable(&mut io, 4).unwrap();
    assert_eq!(controller.capabilities(), Some(expected));
}

#[test]
fn pre_capability_snapshot_is_read_only_and_needs_only_controller_power() {
    let mut io = FakeIo::new();
    io.ready = false;
    io.power_ready = true;
    io.set(host::REG_VERSION, 0x210);

    let snapshot = host::Host::read_only_snapshot(&mut io).unwrap();

    assert_eq!(snapshot.version, protocol::HciVersion::V2_1);
    assert_eq!(snapshot.capabilities.raw, io.get(host::REG_CAPABILITIES));
    assert_eq!(
        snapshot.controller_status,
        io.get(host::REG_CONTROLLER_STATUS)
    );
    assert_eq!(
        io.read_log,
        [
            host::REG_CAPABILITIES,
            host::REG_VERSION,
            host::REG_CONTROLLER_STATUS
        ]
    );
    assert!(io.write_log.is_empty());
}

#[test]
fn hce_enable_requires_full_platform_after_pre_capability_snapshot() {
    let mut io = FakeIo::new();
    io.ready = false;
    io.power_ready = true;
    let mut controller = host::Host::new();

    assert_eq!(
        controller.enable(&mut io, 4),
        Err(host::Error::PlatformNotReady)
    );
    assert_eq!(controller.state(), host::State::Disabled);
    assert_eq!(
        io.read_log,
        [
            host::REG_CAPABILITIES,
            host::REG_VERSION,
            host::REG_CONTROLLER_STATUS
        ]
    );
    assert!(io.write_log.is_empty());
}

#[test]
fn pre_capability_snapshot_rejects_missing_controller_power() {
    let mut io = FakeIo::new();
    io.power_ready = false;
    assert_eq!(
        host::Host::read_only_snapshot(&mut io),
        Err(host::Error::PlatformNotReady)
    );
    assert!(io.read_log.is_empty());
    assert!(io.write_log.is_empty());
}

#[test]
fn scsi_block_adapter_rejects_invalid_host_and_dma_layout_before_use() {
    let mut io = FakeIo::new();
    let mut host = host::Host::new();
    let mut dma = FakeDma::new(0x1000, 0x4000);
    dma.data_address = Some(0x6000);
    let mut observation = FakeObservation::new();
    assert!(matches!(
        UfsScsiBlockIo::new(
            &mut host,
            &mut io,
            &mut dma,
            &mut observation,
            0,
            0x4000,
            0x6000,
            4,
        ),
        Err(ScsiBlockIoError::HostNotOperational)
    ));

    host = io.start();
    host.make_operational(&mut io, 0x1000, 0x2000).unwrap();
    assert!(matches!(
        UfsScsiBlockIo::new(
            &mut host,
            &mut io,
            &mut dma,
            &mut observation,
            8,
            0x4000,
            0x6000,
            4,
        ),
        Err(ScsiBlockIoError::InvalidLun)
    ));
    assert!(matches!(
        UfsScsiBlockIo::new(
            &mut host,
            &mut io,
            &mut dma,
            &mut observation,
            0,
            0x4001,
            0x6000,
            4,
        ),
        Err(ScsiBlockIoError::InvalidDmaAddress)
    ));
    assert!(matches!(
        UfsScsiBlockIo::new(
            &mut host,
            &mut io,
            &mut dma,
            &mut observation,
            0,
            0x4000,
            0x4400,
            4,
        ),
        Err(ScsiBlockIoError::InvalidDmaAddress)
    ));
    assert!(matches!(
        UfsScsiBlockIo::new(
            &mut host,
            &mut io,
            &mut dma,
            &mut observation,
            0,
            0x4000,
            0x6000,
            0,
        ),
        Err(ScsiBlockIoError::InvalidPollLimit)
    ));
    assert!(dma.command_log.is_empty());
}

#[test]
fn scsi_block_adapter_moves_bounded_read_write_and_sync_commands() {
    let mut io = FakeIo::new();
    let mut h = io.start();
    h.make_operational(&mut io, 0x1000, 0x2000).unwrap();
    let mut dma = FakeDma::new(0x1000, 0x4000);
    dma.data_address = Some(0x6000);
    dma.data_bytes.fill(0x6a);
    let mut observation = FakeObservation::new();

    {
        let mut block = UfsScsiBlockIo::new(
            &mut h,
            &mut io,
            &mut dma,
            &mut observation,
            0,
            0x4000,
            0x6000,
            4,
        )
        .unwrap();
        assert_eq!(block.observe().unwrap().smmu_route_context_bank, Some(8));

        let mut out = [0; 1024];
        block.read_blocks(40, &mut out).unwrap();
        assert_eq!(out, [0x6a; 1024]);
        block.write_blocks(64, &[0x77; 512]).unwrap();
        block.synchronize_cache().unwrap();
    }

    assert_eq!(dma.command_log, [(0, 0, 0x28), (0, 1, 0x2a), (0, 2, 0x35)]);
    assert_eq!(dma.data_write_lengths, [512]);
    assert_eq!(&dma.data_bytes[..512], &[0x77; 512]);
}

#[test]
fn scsi_block_adapter_reads_across_native_4k_boundaries() {
    let mut io = FakeIo::new();
    let mut h = io.start();
    h.make_operational(&mut io, 0x1000, 0x2000).unwrap();
    let mut dma = FakeDma::new(0x1000, 0x4000);
    dma.data_address = Some(0x6000);
    dma.data_bytes.fill(0x6a);
    let mut observation = FakeObservation::new();
    observation.0.logical_block_bytes = 4096;

    let mut block = UfsScsiBlockIo::new(
        &mut h,
        &mut io,
        &mut dma,
        &mut observation,
        0,
        0x4000,
        0x6000,
        4,
    )
    .unwrap();
    let mut out = [0; 1024];
    block.read_blocks(7, &mut out).unwrap();
    assert_eq!(out, [0x6a; 1024]);
    drop(block);
    assert_eq!(dma.command_log, [(0, 0, 0x28), (0, 1, 0x28)]);
}

#[test]
fn scsi_block_adapter_uses_read_modify_write_for_sub_4k_updates() {
    let mut io = FakeIo::new();
    let mut h = io.start();
    h.make_operational(&mut io, 0x1000, 0x2000).unwrap();
    let mut dma = FakeDma::new(0x1000, 0x4000);
    dma.data_address = Some(0x6000);
    dma.data_bytes.fill(0x6a);
    let mut observation = FakeObservation::new();
    observation.0.logical_block_bytes = 4096;

    let mut block = UfsScsiBlockIo::new(
        &mut h,
        &mut io,
        &mut dma,
        &mut observation,
        0,
        0x4000,
        0x6000,
        4,
    )
    .unwrap();
    block.write_blocks(2, &[0x77; 512]).unwrap();
    drop(block);
    assert_eq!(dma.command_log, [(0, 0, 0x28), (0, 1, 0x2a)]);
    assert_eq!(dma.data_write_lengths, [4096]);
    assert!(dma.data_bytes[..1024].iter().all(|byte| *byte == 0x6a));
    assert!(dma.data_bytes[1024..1536].iter().all(|byte| *byte == 0x77));
    assert!(dma.data_bytes[1536..].iter().all(|byte| *byte == 0x6a));
}

#[test]
fn scsi_block_adapter_preflights_ranges_and_never_retries_uncertain_commands() {
    let mut io = FakeIo::new();
    let mut h = io.start();
    h.make_operational(&mut io, 0x1000, 0x2000).unwrap();
    io.complete_transfer = false;
    let mut dma = FakeDma::new(0x1000, 0x4000);
    dma.data_address = Some(0x6000);
    let mut observation = FakeObservation::new();
    {
        let mut block = UfsScsiBlockIo::new(
            &mut h,
            &mut io,
            &mut dma,
            &mut observation,
            0,
            0x4000,
            0x6000,
            2,
        )
        .unwrap();

        assert_eq!(block.read_blocks(0, &mut []), Err(Fault::Failed));
        assert_eq!(block.write_blocks(0, &[0x55; 513]), Err(Fault::Failed));
        assert_eq!(block.synchronize_cache(), Err(Fault::Uncertain));
    }
    assert_eq!(dma.command_log, [(0, 0, 0x35)]);
    assert_eq!(h.state(), host::State::Failed);
}

#[test]
fn bringup_orders_hce_link_and_operational_lists() {
    let mut io = FakeIo::new();
    let mut h = io.start();
    assert_eq!(h.state(), host::State::LinkUp);
    h.make_operational(&mut io, 0x1000, 0x2000).unwrap();
    assert_eq!(h.state(), host::State::Operational);
    assert_eq!(io.get(host::REG_UTRL_BASE_LO), 0x1000);
    assert_eq!(io.get(host::REG_UTMRL_BASE_LO), 0x2000);
    assert_eq!(io.get(host::REG_UTRL_RUN_STOP), 1);
    assert_eq!(io.get(host::REG_UTMRL_RUN_STOP), 1);

    let hce = io
        .write_log
        .iter()
        .position(|e| *e == (host::REG_CONTROLLER_ENABLE, 1))
        .unwrap();
    let link = io
        .write_log
        .iter()
        .position(|e| *e == (host::REG_UIC_COMMAND, 0x16))
        .unwrap();
    let lists = io
        .write_log
        .iter()
        .position(|e| *e == (host::REG_UTRL_BASE_LO, 0x1000))
        .unwrap();
    assert!(hce < link && link < lists);
}

#[test]
fn platform_and_version_gates_precede_controller_writes() {
    let mut io = FakeIo::new();
    io.ready = false;
    let mut h = host::Host::new();
    assert_eq!(h.enable(&mut io, 4), Err(host::Error::PlatformNotReady));
    assert_eq!(h.state(), host::State::Disabled);
    assert!(io.write_log.is_empty());

    io.ready = true;
    io.set(host::REG_VERSION, 0x100);
    assert_eq!(h.enable(&mut io, 4), Err(host::Error::HciVersion));
    assert!(io.write_log.is_empty());

    io.set(host::REG_VERSION, 0x200);
    io.set(host::REG_CONTROLLER_ENABLE, 1);
    assert_eq!(h.enable(&mut io, 4), Err(host::Error::ControllerActive));
    assert_eq!(h.state(), host::State::Failed);
    assert!(!io
        .write_log
        .iter()
        .any(|(r, _)| *r == host::REG_CONTROLLER_ENABLE));

    let mut io = FakeIo::new();
    let mut h = host::Host::new();
    assert_eq!(h.enable(&mut io, 65), Err(host::Error::PollLimit));
    assert!(io.write_log.is_empty());
}

#[test]
fn enable_and_link_startup_timeouts_quarantine_host() {
    let mut io = FakeIo::new();
    io.complete_enable = false;
    let mut h = host::Host::new();
    assert_eq!(h.enable(&mut io, 3), Err(host::Error::Timeout));
    assert_eq!(h.state(), host::State::Failed);
    assert_eq!(io.delays, vec![1_000, 5_000, 5_000, 5_000]);

    let mut io = FakeIo::new();
    io.complete_link_startup = false;
    let mut h = host::Host::new();
    h.enable(&mut io, 1).unwrap();
    assert_eq!(h.link_startup(&mut io, 2), Err(host::Error::Timeout));
    assert_eq!(h.state(), host::State::Failed);
}

#[test]
fn link_startup_requires_uic_success_and_present_device() {
    let mut io = FakeIo::new();
    io.uic_result = 4;
    let mut h = host::Host::new();
    h.enable(&mut io, 2).unwrap();
    assert_eq!(h.link_startup(&mut io, 2), Err(host::Error::UicFailure));
    assert_eq!(h.state(), host::State::Failed);

    let mut io = FakeIo::new();
    io.device_present = false;
    let mut h = host::Host::new();
    h.enable(&mut io, 2).unwrap();
    assert_eq!(h.link_startup(&mut io, 2), Err(host::Error::DeviceMissing));

    let mut io = FakeIo::new();
    io.set(host::REG_CONTROLLER_STATUS, 0);
    let mut h = host::Host::new();
    h.enable(&mut io, 2).unwrap();
    assert_eq!(h.link_startup(&mut io, 2), Err(host::Error::UicNotReady));

    let mut io = FakeIo::new();
    let mut h = host::Host::new();
    h.enable(&mut io, 2).unwrap();
    io.set(host::REG_INTERRUPT_STATUS, INT_FATAL);
    assert_eq!(h.link_startup(&mut io, 2), Err(host::Error::HostError));
    assert!(!io
        .write_log
        .iter()
        .any(|(r, _)| *r == host::REG_UIC_COMMAND));
}

#[test]
fn queue_memory_alignment_address_width_and_slot_caps_are_checked() {
    let mut io = FakeIo::new();
    let mut h = io.start();
    assert_eq!(
        h.make_operational(&mut io, 0x1001, 0x2000),
        Err(host::Error::InvalidDma)
    );
    assert_eq!(h.state(), host::State::LinkUp);

    let mut io = FakeIo::new();
    io.set(host::REG_CAPABILITIES, 1 | (1 << 16));
    let mut h = io.start();
    assert_eq!(
        h.make_operational(&mut io, 0x1_0000_0000, 0x2000),
        Err(host::Error::AddressWidth)
    );

    // Zero in the HCI slot-count fields encodes one supported slot.
    io.set(host::REG_CAPABILITIES, 0);
    assert_eq!(h.make_operational(&mut io, 0x1000, 0x2000), Ok(()));
    assert_eq!(h.state(), host::State::Operational);

    let mut io = FakeIo::new();
    let mut h = io.start();
    assert_eq!(
        h.make_operational(&mut io, 0x1000, 0x1000),
        Err(host::Error::InvalidDma)
    );
    assert_eq!(h.state(), host::State::LinkUp);

    let mut io = FakeIo::new();
    let mut h = io.start();
    io.set(host::REG_CONTROLLER_STATUS, HCS_ALL_READY);
    assert_eq!(
        h.make_operational(&mut io, 0x1000, 0x2000),
        Err(host::Error::DeviceMissing)
    );
}

#[test]
fn queue_lists_must_be_ready_before_run_stop() {
    let mut io = FakeIo::new();
    let mut h = io.start();
    io.set(
        host::REG_CONTROLLER_STATUS,
        HCS_UIC_READY | HCS_DEVICE_PRESENT | (1 << 2),
    );
    assert_eq!(
        h.make_operational(&mut io, 0x1000, 0x2000),
        Err(host::Error::ListsNotReady)
    );
    assert_eq!(h.state(), host::State::Failed);
    assert!(!io
        .write_log
        .iter()
        .any(|(r, _)| *r == host::REG_UTRL_RUN_STOP));
}

#[test]
fn irq_snapshot_acknowledges_status_and_quarantines_fatal_errors() {
    let mut io = FakeIo::new();
    let mut h = io.start();
    h.make_operational(&mut io, 0x1000, 0x2000).unwrap();
    io.set(host::REG_INTERRUPT_STATUS, INT_TRANSFER_COMPLETE);
    let irq = h.handle_interrupt(&mut io).unwrap();
    assert!(irq.transfer_complete);
    assert_eq!(irq.errors, 0);
    assert_eq!(io.get(host::REG_INTERRUPT_STATUS), 0);

    io.set(host::REG_INTERRUPT_STATUS, INT_FATAL);
    let irq = h.handle_interrupt(&mut io).unwrap();
    assert_eq!(irq.errors, INT_FATAL);
    assert_eq!(h.state(), host::State::Failed);
    assert_eq!(io.get(host::REG_INTERRUPT_STATUS), 0);
}

#[test]
fn routed_irq_clears_one_controller_snapshot_before_kernel_ack() {
    let mut io = FakeIo::new();
    let mut h = io.start();
    h.make_operational(&mut io, 0x1000, 0x2000).unwrap();
    io.set(host::REG_INTERRUPT_STATUS, INT_TRANSFER_COMPLETE);
    io.status_acknowledged.set(false);
    let mut ack = FakeKernelAck {
        status_acknowledged: io.status_acknowledged.clone(),
        channel_ids: Vec::new(),
        fail: false,
    };

    let snapshot =
        ufs_irq::dispatch_one(ufs_irq::UFS_IRQ_CHANNEL_ID, &mut h, &mut io, &mut ack).unwrap();
    assert!(snapshot.transfer_complete);
    assert_eq!(io.get(host::REG_INTERRUPT_STATUS), 0);
    assert_eq!(ack.channel_ids, [ufs_irq::UFS_IRQ_CHANNEL_ID]);
}

#[test]
fn wrong_route_and_failed_mmio_do_not_ack_kernel_irq() {
    let mut io = FakeIo::new();
    let mut h = io.start();
    h.make_operational(&mut io, 0x1000, 0x2000).unwrap();
    io.set(host::REG_INTERRUPT_STATUS, INT_TRANSFER_COMPLETE);
    io.status_acknowledged.set(false);
    let mut ack = FakeKernelAck {
        status_acknowledged: io.status_acknowledged.clone(),
        channel_ids: Vec::new(),
        fail: false,
    };

    assert_eq!(
        ufs_irq::dispatch_one(3, &mut h, &mut io, &mut ack),
        Err(ufs_irq::Error::WrongChannel)
    );
    assert!(ack.channel_ids.is_empty());
    assert_eq!(io.get(host::REG_INTERRUPT_STATUS), INT_TRANSFER_COMPLETE);

    io.fail_read = Some(host::REG_INTERRUPT_STATUS);
    assert_eq!(
        ufs_irq::dispatch_one(ufs_irq::UFS_IRQ_CHANNEL_ID, &mut h, &mut io, &mut ack),
        Err(ufs_irq::Error::Host(host::Error::Io))
    );
    assert!(ack.channel_ids.is_empty());
}

#[test]
fn kernel_ack_failure_is_reported_after_controller_status_is_cleared() {
    let mut io = FakeIo::new();
    let mut h = io.start();
    h.make_operational(&mut io, 0x1000, 0x2000).unwrap();
    io.set(host::REG_INTERRUPT_STATUS, INT_TRANSFER_COMPLETE);
    io.status_acknowledged.set(false);
    let mut ack = FakeKernelAck {
        status_acknowledged: io.status_acknowledged.clone(),
        channel_ids: Vec::new(),
        fail: true,
    };

    assert_eq!(
        ufs_irq::dispatch_one(ufs_irq::UFS_IRQ_CHANNEL_ID, &mut h, &mut io, &mut ack),
        Err(ufs_irq::Error::KernelAcknowledge)
    );
    assert_eq!(io.get(host::REG_INTERRUPT_STATUS), 0);
    assert!(ack.channel_ids.is_empty());
}

#[test]
fn fatal_controller_irq_is_masked_before_kernel_rearm() {
    let mut io = FakeIo::new();
    let mut h = io.start();
    h.make_operational(&mut io, 0x1000, 0x2000).unwrap();
    io.set(host::REG_INTERRUPT_STATUS, INT_FATAL);
    io.status_acknowledged.set(false);
    let mut ack = FakeKernelAck {
        status_acknowledged: io.status_acknowledged.clone(),
        channel_ids: Vec::new(),
        fail: false,
    };

    let snapshot =
        ufs_irq::dispatch_one(ufs_irq::UFS_IRQ_CHANNEL_ID, &mut h, &mut io, &mut ack).unwrap();
    assert_eq!(snapshot.errors, INT_FATAL);
    assert_eq!(h.state(), host::State::Failed);
    assert_eq!(io.get(host::REG_INTERRUPT_ENABLE), 0);
    assert_eq!(ack.channel_ids, [ufs_irq::UFS_IRQ_CHANNEL_ID]);
}

#[test]
fn native_lifecycle_loss_stops_operational_host() {
    let mut io = FakeIo::new();
    let mut h = io.start();
    h.make_operational(&mut io, 0x1000, 0x2000).unwrap();
    io.ready = false;
    assert_eq!(
        h.handle_interrupt(&mut io),
        Err(host::Error::PlatformNotReady)
    );
    assert_eq!(h.state(), host::State::Failed);
}

#[test]
fn partial_register_failures_quarantine_host() {
    let mut io = FakeIo::new();
    let mut h = io.start();
    io.fail_write = Some(host::REG_UTMRL_BASE_HI);
    assert_eq!(
        h.make_operational(&mut io, 0x1000, 0x2000),
        Err(host::Error::Io)
    );
    assert_eq!(h.state(), host::State::Failed);

    // Recreate the pre-link stage so the failed read occurs inside startup.
    let mut io = FakeIo::new();
    let mut h = host::Host::new();
    h.enable(&mut io, 2).unwrap();
    io.fail_read = Some(host::REG_INTERRUPT_STATUS);
    assert_eq!(h.link_startup(&mut io, 2), Err(host::Error::Io));
    assert_eq!(h.state(), host::State::Failed);
}

#[test]
fn high_dma_lists_are_accepted_only_with_hci_capability() {
    let mut io = FakeIo::new();
    let mut h = io.start();
    h.make_operational(&mut io, 0x1_0000_0000, 0x1_0000_1000)
        .unwrap();
    assert_eq!(io.get(host::REG_UTRL_BASE_HI), 1);
    assert_eq!(io.get(host::REG_UTMRL_BASE_HI), 1);
}

#[test]
fn bounded_scsi_submit_stages_descriptors_and_validates_irq_completion() {
    let mut io = FakeIo::new();
    let mut h = io.start();
    h.make_operational(&mut io, 0x1000, 0x2000).unwrap();
    let mut dma = FakeDma::new(0x1000, 0x4000);
    let command = protocol::ScsiCommand::read10(0x1234, 1).unwrap();
    assert_eq!(
        h.execute_command(&mut io, &mut dma, command, 8, 9, 0x4000, Some(0x6000), 3),
        Err(host::Error::Packet(protocol::Error::InvalidLun))
    );
    assert_eq!(h.state(), host::State::Operational);
    h.execute_command(&mut io, &mut dma, command, 0, 9, 0x4000, Some(0x6000), 3)
        .unwrap();

    assert_eq!(h.state(), host::State::Operational);
    assert_eq!(dma.ucd[0], 0x01);
    assert_eq!(dma.ucd[1], 0x40);
    assert_eq!(dma.ucd[3], 9);
    assert_eq!(&dma.ucd[16..26], &[0x28, 0, 0, 0, 0x12, 0x34, 0, 0, 1, 0]);
    assert_eq!(
        u64::from_le_bytes(dma.ucd[1024..1032].try_into().unwrap()),
        0x6000
    );
    assert_eq!(io.get(host::REG_UTRL_DOORBELL), 0);
    assert!(io
        .write_log
        .iter()
        .any(|(register, value)| *register == host::REG_UTRL_DOORBELL && *value == 1));
}

#[test]
fn read_only_lun_discovery_submits_tur_inquiry_and_capacity_in_order() {
    let mut io = FakeIo::new();
    let mut h = io.start();
    h.make_operational(&mut io, 0x1000, 0x2000).unwrap();
    let mut dma = FakeDma::new(0x1000, 0x4000);
    dma.configure_discovery_data(0x6000);

    let found = h
        .discover_lun(&mut io, &mut dma, 0, 4, 0x4000, 0x6000, 3)
        .unwrap();

    assert_eq!(found.lun, 0);
    assert_eq!(&found.inquiry.vendor_id, b"GOOGLE  ");
    assert_eq!(&found.inquiry.product_id, b"SUNFISH UFS     ");
    assert_eq!(found.capacity.sectors, 16_384);
    assert_eq!(dma.command_log, [(0, 4, 0x00), (0, 5, 0x12), (0, 6, 0x25)]);
    assert_eq!(dma.data_read_lengths, [36, 8]);
    assert!(
        io.write_log
            .iter()
            .filter(|(register, value)| *register == host::REG_UTRL_DOORBELL && *value == 1)
            .count()
            == 3
    );
    assert_eq!(h.state(), host::State::Operational);
}

#[test]
fn read_only_lun_discovery_stops_after_invalid_inquiry_without_capacity_or_retry() {
    let mut io = FakeIo::new();
    let mut h = io.start();
    h.make_operational(&mut io, 0x1000, 0x2000).unwrap();
    let mut dma = FakeDma::new(0x1000, 0x4000);
    dma.configure_discovery_data(0x6000);
    dma.inquiry_data[0] = 5; // Unsupported CD/DVD peripheral type.

    assert_eq!(
        h.discover_lun(&mut io, &mut dma, 0, 7, 0x4000, 0x6000, 3),
        Err(host::Error::Packet(protocol::Error::UnsupportedPeripheral))
    );
    assert_eq!(dma.command_log, [(0, 7, 0x00), (0, 8, 0x12)]);
    assert_eq!(h.state(), host::State::Failed);
}

#[test]
fn read_only_lun_discovery_preflights_tag_window_and_quarantines_dma_read_failure() {
    let mut io = FakeIo::new();
    let mut h = io.start();
    h.make_operational(&mut io, 0x1000, 0x2000).unwrap();
    let mut dma = FakeDma::new(0x1000, 0x4000);
    dma.configure_discovery_data(0x6000);
    assert_eq!(
        h.discover_lun(&mut io, &mut dma, 0, 30, 0x4000, 0x6000, 3),
        Err(host::Error::Packet(protocol::Error::InvalidTag))
    );
    assert!(dma.command_log.is_empty());
    assert_eq!(h.state(), host::State::Operational);

    let mut io = FakeIo::new();
    let mut h = io.start();
    h.make_operational(&mut io, 0x1000, 0x2000).unwrap();
    let mut dma = FakeDma::new(0x1000, 0x4000);
    dma.configure_discovery_data(0x6000);
    dma.fail_data_read = true;
    assert_eq!(
        h.discover_lun(&mut io, &mut dma, 0, 0, 0x4000, 0x6000, 3),
        Err(host::Error::Io)
    );
    assert_eq!(dma.command_log, [(0, 0, 0x00), (0, 1, 0x12)]);
    assert_eq!(h.state(), host::State::Failed);
}

#[test]
fn bad_ocs_and_dma_completion_failure_quarantine_host() {
    let mut io = FakeIo::new();
    let mut h = io.start();
    h.make_operational(&mut io, 0x1000, 0x2000).unwrap();
    let mut dma = FakeDma::new(0x1000, 0x4000);
    dma.ocs = 1;
    assert_eq!(
        h.execute_command(
            &mut io,
            &mut dma,
            protocol::ScsiCommand::synchronize_cache10(),
            0,
            1,
            0x4000,
            None,
            2
        ),
        Err(host::Error::Packet(protocol::Error::Ocs))
    );
    assert_eq!(h.state(), host::State::Failed);

    let mut io = FakeIo::new();
    let mut h = io.start();
    h.make_operational(&mut io, 0x1000, 0x2000).unwrap();
    let mut dma = FakeDma::new(0x1000, 0x4000);
    dma.lose_after_first_write = true;
    assert_eq!(
        h.execute_command(
            &mut io,
            &mut dma,
            protocol::ScsiCommand::synchronize_cache10(),
            0,
            1,
            0x4000,
            None,
            2
        ),
        Err(host::Error::PlatformNotReady)
    );
    assert_eq!(h.state(), host::State::Failed);
    assert_eq!(io.get(host::REG_UTRL_DOORBELL), 0);

    let mut io = FakeIo::new();
    let mut h = io.start();
    h.make_operational(&mut io, 0x1000, 0x2000).unwrap();
    let mut dma = FakeDma::new(0x1000, 0x4000);
    dma.fail_next_read = true;
    assert_eq!(
        h.execute_command(
            &mut io,
            &mut dma,
            protocol::ScsiCommand::synchronize_cache10(),
            0,
            1,
            0x4000,
            None,
            2
        ),
        Err(host::Error::Io)
    );
    assert_eq!(h.state(), host::State::Failed);
}

#[test]
fn missing_transfer_irq_times_out_without_replay() {
    let mut io = FakeIo::new();
    io.complete_transfer = false;
    let mut h = io.start();
    h.make_operational(&mut io, 0x1000, 0x2000).unwrap();
    let mut dma = FakeDma::new(0x1000, 0x4000);
    assert_eq!(
        h.execute_command(
            &mut io,
            &mut dma,
            protocol::ScsiCommand::synchronize_cache10(),
            0,
            1,
            0x4000,
            None,
            2
        ),
        Err(host::Error::Timeout)
    );
    assert_eq!(h.state(), host::State::Failed);
    assert_eq!(io.get(host::REG_UTRL_DOORBELL), 1);
    assert_eq!(
        io.write_log
            .iter()
            .filter(|(register, _)| *register == host::REG_UTRL_DOORBELL)
            .count(),
        1
    );
}
