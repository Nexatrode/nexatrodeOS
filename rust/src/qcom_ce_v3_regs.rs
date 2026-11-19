//! Bounded register map for Sunfish's ICNSS/Adrastea CE v3 block.
//!
//! Offsets follow the matched Lineage QCACLD source and its Sunfish build
//! configuration (`CONFIG_ICNSS` selects `CONFIG_WIFI_3_0_ADRASTEA`). These
//! are offsets within ICNSS `membase`, not physical or virtual addresses. This
//! module has no mapping, MMIO, IRQ, power, DMA, or hardware authority.

#![forbid(unsafe_code)]

pub const ICNSS_WINDOW_BYTES: usize = 0x0080_0000;
pub const CE_COUNT: usize = 12;
pub const CE0_OFFSET: usize = 0x0024_0000;
pub const CE_STRIDE: usize = 0x0000_1000;
pub const CE_REGISTER_BYTES: usize = 0x54;
pub const HOST_INTERRUPT_STATUS_MASK: u32 = 0x1f;
/// Sunfish Adrastea CE host status bit set for copy-complete interrupts.
pub const HOST_INTERRUPT_COPY_COMPLETE_MASK: u32 = 0x01;
/// seL4 GIC INTID for CE0, decoded from the PDC-parented Sunfish DT IRQs.
pub const CE0_GIC_INTID: u16 = 0x01be;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidEngine,
    OutsideWindow,
    ReadOnly,
    WriteOneToClear,
    InvalidStatusMask,
    StatusNotPending,
    Io,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Access {
    ReadWrite,
    ReadOnly,
    WriteOneToClear,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub enum Register {
    SourceBaseLow = 0x00,
    SourceBaseHigh = 0x04,
    SourceSize = 0x08,
    DestinationBaseLow = 0x0c,
    DestinationBaseHigh = 0x10,
    DestinationSize = 0x14,
    Control1 = 0x18,
    Control2 = 0x1c,
    Command = 0x20,
    HostInterruptEnable = 0x2c,
    HostInterruptStatus = 0x30,
    MiscInterruptEnable = 0x34,
    MiscInterruptStatus = 0x38,
    SourceWriteIndex = 0x3c,
    DestinationWriteIndex = 0x40,
    CurrentSourceReadIndex = 0x44,
    CurrentDestinationReadIndex = 0x48,
    SourceWatermark = 0x4c,
    DestinationWatermark = 0x50,
}

impl Register {
    pub const fn access(self) -> Access {
        match self {
            Self::CurrentSourceReadIndex | Self::CurrentDestinationReadIndex => Access::ReadOnly,
            Self::HostInterruptStatus => Access::WriteOneToClear,
            Self::MiscInterruptStatus => Access::ReadOnly,
            _ => Access::ReadWrite,
        }
    }

    pub const fn relative_offset(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegisterSpec {
    pub engine: u8,
    pub offset: usize,
    pub access: Access,
}

pub fn register_spec(engine: usize, register: Register) -> Result<RegisterSpec, Error> {
    if engine >= CE_COUNT {
        return Err(Error::InvalidEngine);
    }
    let engine_offset = engine
        .checked_mul(CE_STRIDE)
        .and_then(|value| CE0_OFFSET.checked_add(value))
        .ok_or(Error::OutsideWindow)?;
    let register_offset = register.relative_offset();
    if register_offset
        .checked_add(core::mem::size_of::<u32>())
        .is_none_or(|end| end > CE_REGISTER_BYTES)
    {
        return Err(Error::OutsideWindow);
    }
    let offset = engine_offset
        .checked_add(register_offset)
        .ok_or(Error::OutsideWindow)?;
    if offset
        .checked_add(core::mem::size_of::<u32>())
        .is_none_or(|end| end > ICNSS_WINDOW_BYTES)
    {
        return Err(Error::OutsideWindow);
    }
    Ok(RegisterSpec {
        engine: engine as u8,
        offset,
        access: register.access(),
    })
}

/// Return the seL4 GIC INTID associated with this CE engine. This is an
/// identity lookup only; it neither grants nor acknowledges an IRQ capability.
pub fn gic_intid(engine: usize) -> Result<u16, Error> {
    if engine >= CE_COUNT {
        return Err(Error::InvalidEngine);
    }
    CE0_GIC_INTID
        .checked_add(u16::try_from(engine).map_err(|_| Error::InvalidEngine)?)
        .ok_or(Error::InvalidEngine)
}

/// Raw register operations supplied by a separately authorized native
/// mapping. This interface is host-testable; it does not acquire or validate
/// a mapping, power state, lifecycle lease, or IRQ capability.
pub trait RegisterIo {
    fn read32(&mut self, offset: usize) -> Result<u32, Error>;
    fn write32(&mut self, offset: usize, value: u32) -> Result<(), Error>;
}

pub struct Adapter<I> {
    io: I,
}

impl<I: RegisterIo> Adapter<I> {
    pub const fn new(io: I) -> Self {
        Self { io }
    }

    pub fn read(&mut self, engine: usize, register: Register) -> Result<u32, Error> {
        let spec = register_spec(engine, register)?;
        self.io.read32(spec.offset)
    }

    /// Raw writes are restricted to ordinary read/write control registers.
    /// Status acknowledgements must use `acknowledge_host_status`.
    pub fn write(&mut self, engine: usize, register: Register, value: u32) -> Result<(), Error> {
        let spec = register_spec(engine, register)?;
        match spec.access {
            Access::ReadWrite => {}
            Access::ReadOnly => return Err(Error::ReadOnly),
            Access::WriteOneToClear => return Err(Error::WriteOneToClear),
        }
        self.io.write32(spec.offset, value)
    }

    /// Clear only known host-interrupt bits that are present in a fresh
    /// status read. Miscellaneous error status remains read-only until its
    /// clear semantics are established from a complete target path.
    pub fn acknowledge_host_status(&mut self, engine: usize, bits: u32) -> Result<(), Error> {
        if bits == 0 || bits & !HOST_INTERRUPT_STATUS_MASK != 0 {
            return Err(Error::InvalidStatusMask);
        }
        let spec = register_spec(engine, Register::HostInterruptStatus)?;
        let pending = self.io.read32(spec.offset)?;
        if bits & !pending != 0 {
            return Err(Error::StatusNotPending);
        }
        self.io.write32(spec.offset, bits)
    }

    pub fn into_inner(self) -> I {
        self.io
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adrastea_register_offsets_match_the_qcacld_register_table() {
        assert_eq!(
            register_spec(0, Register::SourceBaseLow),
            Ok(RegisterSpec {
                engine: 0,
                offset: 0x0024_0000,
                access: Access::ReadWrite,
            })
        );
        assert_eq!(
            register_spec(0, Register::SourceWriteIndex).unwrap().offset,
            0x0024_003c
        );
        assert_eq!(
            register_spec(0, Register::DestinationWriteIndex)
                .unwrap()
                .offset,
            0x0024_0040
        );
        assert_eq!(
            register_spec(1, Register::SourceBaseLow).unwrap().offset,
            0x0024_1000
        );
        assert_eq!(
            register_spec(11, Register::DestinationWatermark)
                .unwrap()
                .offset,
            0x0024_b050
        );
    }

    #[test]
    fn register_access_types_preserve_status_and_read_index_semantics() {
        assert_eq!(Register::CurrentSourceReadIndex.access(), Access::ReadOnly);
        assert_eq!(
            Register::CurrentDestinationReadIndex.access(),
            Access::ReadOnly
        );
        assert_eq!(
            Register::HostInterruptStatus.access(),
            Access::WriteOneToClear
        );
        assert_eq!(Register::MiscInterruptStatus.access(), Access::ReadOnly);
        assert_eq!(Register::HostInterruptEnable.access(), Access::ReadWrite);
    }

    #[test]
    fn invalid_engines_and_out_of_window_specs_are_rejected() {
        assert_eq!(
            register_spec(CE_COUNT, Register::SourceBaseLow),
            Err(Error::InvalidEngine)
        );
        assert_eq!(
            register_spec(usize::MAX, Register::SourceBaseLow),
            Err(Error::InvalidEngine)
        );
        assert!(
            register_spec(CE_COUNT - 1, Register::DestinationWatermark)
                .unwrap()
                .offset
                + 4
                <= ICNSS_WINDOW_BYTES
        );
    }

    #[test]
    fn ce_interrupt_ids_match_the_decoded_sunfish_device_tree() {
        for engine in 0..CE_COUNT {
            assert_eq!(gic_intid(engine), Ok(0x01be + engine as u16));
        }
        assert_eq!(gic_intid(CE_COUNT), Err(Error::InvalidEngine));
    }

    #[derive(Default)]
    struct MockIo {
        host_status: u32,
        fail_read: bool,
        fail_write: bool,
        reads: std::vec::Vec<usize>,
        writes: std::vec::Vec<(usize, u32)>,
    }

    impl RegisterIo for MockIo {
        fn read32(&mut self, offset: usize) -> Result<u32, Error> {
            self.reads.push(offset);
            if self.fail_read {
                return Err(Error::Io);
            }
            if offset == register_spec(0, Register::HostInterruptStatus)?.offset {
                Ok(self.host_status)
            } else {
                Ok(0x1234_5678)
            }
        }

        fn write32(&mut self, offset: usize, value: u32) -> Result<(), Error> {
            if self.fail_write {
                return Err(Error::Io);
            }
            self.writes.push((offset, value));
            Ok(())
        }
    }

    #[test]
    fn adapter_blocks_generic_status_and_read_only_writes() {
        let mut adapter = Adapter::new(MockIo::default());
        assert_eq!(
            adapter.write(0, Register::CurrentSourceReadIndex, 1),
            Err(Error::ReadOnly)
        );
        assert_eq!(
            adapter.write(0, Register::HostInterruptStatus, 1),
            Err(Error::WriteOneToClear)
        );
        assert_eq!(
            adapter.write(0, Register::MiscInterruptStatus, 1),
            Err(Error::ReadOnly)
        );
        let io = adapter.into_inner();
        assert!(io.writes.is_empty());
    }

    #[test]
    fn adapter_clears_only_known_observed_host_status_bits() {
        let io = MockIo {
            host_status: 0x05,
            ..MockIo::default()
        };
        let mut adapter = Adapter::new(io);
        assert_eq!(
            adapter.acknowledge_host_status(0, 0x20),
            Err(Error::InvalidStatusMask)
        );
        assert_eq!(
            adapter.acknowledge_host_status(0, 0x10),
            Err(Error::StatusNotPending)
        );
        assert_eq!(adapter.acknowledge_host_status(0, 0x01), Ok(()));
        let io = adapter.into_inner();
        assert_eq!(io.writes, std::vec![(0x0024_0030, 0x01)]);
        assert_eq!(io.reads, std::vec![0x0024_0030, 0x0024_0030]);
    }

    #[test]
    fn adapter_propagates_register_bus_failures_without_retrying_or_mutating() {
        let mut read_adapter = Adapter::new(MockIo {
            fail_read: true,
            ..MockIo::default()
        });
        assert_eq!(
            read_adapter.acknowledge_host_status(0, 0x01),
            Err(Error::Io)
        );
        let io = read_adapter.into_inner();
        assert_eq!(io.reads, std::vec![0x0024_0030]);
        assert!(io.writes.is_empty());

        let mut write_adapter = Adapter::new(MockIo {
            host_status: 0x01,
            fail_write: true,
            ..MockIo::default()
        });
        assert_eq!(
            write_adapter.acknowledge_host_status(0, 0x01),
            Err(Error::Io)
        );
        let io = write_adapter.into_inner();
        assert_eq!(io.reads, std::vec![0x0024_0030]);
        assert!(io.writes.is_empty());
    }
}
