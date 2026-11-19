#[path = "../rust/src/qcom_ufs_host.rs"]
pub mod qcom_ufs_host;
#[path = "../rust/src/qcom_ufs_irq.rs"]
pub mod qcom_ufs_irq;
#[path = "../rust/src/qcom_ufs_native_io.rs"]
pub mod qcom_ufs_native_io;
#[path = "../rust/src/qcom_ufs_platform.rs"]
pub mod qcom_ufs_platform;
#[path = "../rust/src/qcom_ufs_protocol.rs"]
pub mod qcom_ufs_protocol;
#[path = "../rust/src/sunfish_smmu.rs"]
pub mod sunfish_smmu;

#[cfg(test)]
mod tests {
    use super::{
        qcom_ufs_host as host, qcom_ufs_native_io as native, qcom_ufs_platform as platform,
    };
    use core::cell::RefCell;
    use host::{DmaIo, RegisterIo};
    use std::rc::Rc;

    const TEST_DMA_IOVA: u64 = 0x8100_0000;
    const TEST_DMA_VIRTUAL: u64 = 0x6500_0000;

    #[repr(align(4096))]
    struct Hci([u8; platform::HOST_BYTES as usize]);

    #[repr(align(4096))]
    struct Dma([u8; 4096]);

    #[derive(Clone)]
    struct FakeGate(Rc<RefCell<native::Observation>>);

    struct FakeGuard<'a>(std::cell::Ref<'a, native::Observation>);

    impl native::Guard for FakeGuard<'_> {
        fn observation(&self) -> native::Observation {
            *self.0
        }
    }

    // SAFETY: tests are single-threaded and the RefCell borrow pins the fixture
    // observation for the duration of each fake MMIO/DMA operation.
    unsafe impl native::Gate for FakeGate {
        type Guard<'a> = FakeGuard<'a>;

        fn lock(&mut self) -> Result<Self::Guard<'_>, host::Error> {
            self.0
                .try_borrow()
                .map(FakeGuard)
                .map_err(|_| host::Error::PlatformNotReady)
        }
    }

    struct FakeDelay;
    impl native::Delay for FakeDelay {
        fn delay_us(&mut self, _: u32) {}
    }

    fn observation(hci: native::HciMapping, dma: native::DmaMapping) -> native::Observation {
        native::Observation {
            generation: 7,
            fresh_boot: true,
            exclusive: true,
            controller_power_ready: true,
            platform_ready: true,
            host_device_uncached: true,
            host_virtual: hci.virtual_base,
            host_physical: platform::HOST_BASE,
            host_bytes: platform::HOST_BYTES,
            dma_virtual: dma.virtual_base,
            dma_iova: dma.iova_base,
            dma_bytes: dma.bytes,
            dma_stream_id: platform::SMMU_STREAM_ID,
            smmu_route_slot: Some(7),
            smmu_route_id: platform::SMMU_STREAM_ID,
            smmu_route_mask: 0,
            smmu_route_translated: true,
            smmu_context_bank: Some(3),
            smmu_context_enabled: true,
            dma_uncached: true,
            dma_exclusive: true,
            dma_translated: true,
        }
    }

    fn make_authority(
        hci: native::HciMapping,
        dma: native::DmaMapping,
    ) -> Rc<RefCell<native::Observation>> {
        Rc::new(RefCell::new(observation(hci, dma)))
    }

    #[test]
    fn constructor_requires_fresh_exclusive_ready_mapping() {
        let mut hci_mem = Hci([0; platform::HOST_BYTES as usize]);
        let mut dma_mem = Dma([0; 4096]);
        let hci = native::HciMapping {
            base: hci_mem.0.as_mut_ptr(),
            virtual_base: native::HCI_VIRTUAL_BASE as u64,
            bytes: platform::HOST_BYTES as usize,
        };
        let dma = native::DmaMapping {
            cpu_base: dma_mem.0.as_mut_ptr(),
            virtual_base: TEST_DMA_VIRTUAL,
            iova_base: TEST_DMA_IOVA,
            bytes: dma_mem.0.len() as u64,
        };
        let state = make_authority(hci, dma);
        state.borrow_mut().controller_power_ready = false;
        // SAFETY: pointer/shape are valid test arrays; the fake gate is offline.
        let result = unsafe { native::NativeRegisterIo::new(hci, FakeGate(state), FakeDelay) };
        assert!(matches!(result, Err(host::Error::PlatformNotReady)));

        let state = Rc::new(RefCell::new(observation(hci, dma)));
        state.borrow_mut().dma_virtual = native::HCI_VIRTUAL_BASE as u64 + 0x2000;
        // SAFETY: pointer/shape are valid test arrays; the fake gate is offline.
        let mut io =
            unsafe { native::NativeRegisterIo::new(hci, FakeGate(state), FakeDelay) }.unwrap();
        assert!(!io.platform_ready());
        assert_eq!(
            io.read32(host::REG_CONTROLLER_ENABLE),
            Err(host::Error::PlatformNotReady)
        );
    }

    #[test]
    fn constructor_requires_the_audited_stream_and_a_valid_live_context_bank() {
        let mut hci_mem = Hci([0; platform::HOST_BYTES as usize]);
        let mut dma_mem = Dma([0; 4096]);
        let hci = native::HciMapping {
            base: hci_mem.0.as_mut_ptr(),
            virtual_base: native::HCI_VIRTUAL_BASE as u64,
            bytes: platform::HOST_BYTES as usize,
        };
        let dma = native::DmaMapping {
            cpu_base: dma_mem.0.as_mut_ptr(),
            virtual_base: TEST_DMA_VIRTUAL,
            iova_base: TEST_DMA_IOVA,
            bytes: dma_mem.0.len() as u64,
        };

        let state = make_authority(hci, dma);
        state.borrow_mut().dma_stream_id = platform::SMMU_STREAM_ID + 1;
        // SAFETY: pointer/shape are valid test arrays; the fake gate is offline.
        let result = unsafe { native::NativeDmaIo::new(dma, FakeGate(state)) };
        assert!(matches!(result, Err(host::Error::PlatformNotReady)));

        for context_bank in [
            None,
            Some(crate::sunfish_smmu::SUNFISH_MAX_CONTEXT_BANKS as u8),
        ] {
            let state = make_authority(hci, dma);
            state.borrow_mut().smmu_context_bank = context_bank;
            // SAFETY: pointer/shape are valid test arrays; the fake gate is offline.
            let result = unsafe { native::NativeDmaIo::new(dma, FakeGate(state)) };
            assert!(matches!(result, Err(host::Error::PlatformNotReady)));
        }
    }

    #[test]
    fn constructor_rejects_bypass_or_broad_smmu_routes() {
        let mut hci_mem = Hci([0; platform::HOST_BYTES as usize]);
        let mut dma_mem = Dma([0; 4096]);
        let hci = native::HciMapping {
            base: hci_mem.0.as_mut_ptr(),
            virtual_base: native::HCI_VIRTUAL_BASE as u64,
            bytes: platform::HOST_BYTES as usize,
        };
        let dma = native::DmaMapping {
            cpu_base: dma_mem.0.as_mut_ptr(),
            virtual_base: TEST_DMA_VIRTUAL,
            iova_base: TEST_DMA_IOVA,
            bytes: dma_mem.0.len() as u64,
        };

        let mutations: [fn(&mut native::Observation); 4] = [
            |state: &mut native::Observation| state.smmu_route_id += 1,
            |state: &mut native::Observation| state.smmu_route_mask = 0x0f,
            |state: &mut native::Observation| state.smmu_route_translated = false,
            |state: &mut native::Observation| state.smmu_context_enabled = false,
        ];
        for mutate in mutations {
            let state = make_authority(hci, dma);
            mutate(&mut state.borrow_mut());
            // SAFETY: pointer/shape are valid test arrays; the fake gate is offline.
            let mut hci_io =
                unsafe { native::NativeRegisterIo::new(hci, FakeGate(state.clone()), FakeDelay) }
                    .unwrap();
            assert!(!hci_io.platform_ready());
            assert_eq!(
                hci_io.read32(host::REG_CONTROLLER_ENABLE),
                Err(host::Error::PlatformNotReady)
            );
            // SAFETY: pointer/shape are valid test arrays; the fake gate is offline.
            let result = unsafe { native::NativeDmaIo::new(dma, FakeGate(state)) };
            assert!(matches!(result, Err(host::Error::PlatformNotReady)));
        }

        let state = make_authority(hci, dma);
        state.borrow_mut().smmu_route_slot = Some(crate::sunfish_smmu::SUNFISH_MAX_SMR_SLOTS as u8);
        // SAFETY: pointer/shape are valid test arrays; the fake gate is offline.
        let mut hci_io =
            unsafe { native::NativeRegisterIo::new(hci, FakeGate(state.clone()), FakeDelay) }
                .unwrap();
        assert!(!hci_io.platform_ready());
        assert_eq!(
            hci_io.read32(host::REG_CONTROLLER_ENABLE),
            Err(host::Error::PlatformNotReady)
        );
        // SAFETY: pointer/shape are valid test arrays; the fake gate is offline.
        let result = unsafe { native::NativeDmaIo::new(dma, FakeGate(state)) };
        assert!(matches!(result, Err(host::Error::PlatformNotReady)));
    }

    #[test]
    fn register_adapter_allows_only_host_driver_registers() {
        let mut hci_mem = Hci([0; platform::HOST_BYTES as usize]);
        let mut dma_mem = Dma([0; 4096]);
        let hci = native::HciMapping {
            base: hci_mem.0.as_mut_ptr(),
            virtual_base: native::HCI_VIRTUAL_BASE as u64,
            bytes: platform::HOST_BYTES as usize,
        };
        let dma = native::DmaMapping {
            cpu_base: dma_mem.0.as_mut_ptr(),
            virtual_base: TEST_DMA_VIRTUAL,
            iova_base: TEST_DMA_IOVA,
            bytes: dma_mem.0.len() as u64,
        };
        let state = make_authority(hci, dma);
        // SAFETY: pointer/shape are valid test arrays; the fake gate is offline.
        let mut io =
            unsafe { native::NativeRegisterIo::new(hci, FakeGate(state), FakeDelay) }.unwrap();
        assert!(io.platform_ready());
        io.write32(host::REG_CONTROLLER_ENABLE, 1).unwrap();
        let value = unsafe {
            hci.base
                .add(host::REG_CONTROLLER_ENABLE as usize)
                .cast::<u32>()
                .read_volatile()
        };
        assert_eq!(u32::from_le(value), 1);
        assert_eq!(
            io.write32(host::REG_CAPABILITIES, u32::MAX),
            Err(host::Error::Io)
        );
        assert_eq!(io.read32(0x2), Err(host::Error::Io));
    }

    #[test]
    fn read_only_snapshot_needs_power_but_not_dma_or_smmu() {
        let mut hci_mem = Hci([0; platform::HOST_BYTES as usize]);
        let mut dma_mem = Dma([0; 4096]);
        let hci = native::HciMapping {
            base: hci_mem.0.as_mut_ptr(),
            virtual_base: native::HCI_VIRTUAL_BASE as u64,
            bytes: platform::HOST_BYTES as usize,
        };
        let dma = native::DmaMapping {
            cpu_base: dma_mem.0.as_mut_ptr(),
            virtual_base: TEST_DMA_VIRTUAL,
            iova_base: TEST_DMA_IOVA,
            bytes: dma_mem.0.len() as u64,
        };
        let state = make_authority(hci, dma);
        {
            let mut observation = state.borrow_mut();
            observation.platform_ready = false;
            observation.dma_virtual = 0;
            observation.dma_iova = 0;
            observation.dma_bytes = 0;
            observation.smmu_route_slot = None;
            observation.smmu_context_bank = None;
            observation.smmu_context_enabled = false;
            observation.dma_translated = false;
        }
        unsafe {
            hci.base
                .add(host::REG_CAPABILITIES as usize)
                .cast::<u32>()
                .write_volatile(0x1587_031f_u32.to_le());
            hci.base
                .add(host::REG_VERSION as usize)
                .cast::<u32>()
                .write_volatile(0x210_u32.to_le());
            hci.base
                .add(host::REG_CONTROLLER_STATUS as usize)
                .cast::<u32>()
                .write_volatile(0x9_u32.to_le());
        }
        // SAFETY: pointer/shape are valid test arrays; power readiness is a
        // live fake observation and the three reads are allowlisted.
        let mut io =
            unsafe { native::NativeRegisterIo::new(hci, FakeGate(state), FakeDelay) }.unwrap();

        let snapshot = host::Host::read_only_snapshot(&mut io).unwrap();

        assert_eq!(snapshot.capabilities.raw, 0x1587_031f);
        assert_eq!(snapshot.version, crate::qcom_ufs_protocol::HciVersion::V2_1);
        assert_eq!(snapshot.controller_status, 0x9);
        assert!(!io.platform_ready());
        assert_eq!(
            io.read32(host::REG_CONTROLLER_ENABLE),
            Err(host::Error::PlatformNotReady)
        );
        assert_eq!(
            io.write32(host::REG_CONTROLLER_ENABLE, 1),
            Err(host::Error::PlatformNotReady)
        );
    }

    #[test]
    fn generation_change_closes_the_mmio_gate() {
        let mut hci_mem = Hci([0; platform::HOST_BYTES as usize]);
        let mut dma_mem = Dma([0; 4096]);
        let hci = native::HciMapping {
            base: hci_mem.0.as_mut_ptr(),
            virtual_base: native::HCI_VIRTUAL_BASE as u64,
            bytes: platform::HOST_BYTES as usize,
        };
        let dma = native::DmaMapping {
            cpu_base: dma_mem.0.as_mut_ptr(),
            virtual_base: TEST_DMA_VIRTUAL,
            iova_base: TEST_DMA_IOVA,
            bytes: dma_mem.0.len() as u64,
        };
        let state = make_authority(hci, dma);
        // SAFETY: pointer/shape are valid test arrays; the fake gate is offline.
        let mut io =
            unsafe { native::NativeRegisterIo::new(hci, FakeGate(state.clone()), FakeDelay) }
                .unwrap();
        state.borrow_mut().generation += 1;
        assert_eq!(
            io.read32(host::REG_VERSION),
            Err(host::Error::PlatformNotReady)
        );
        assert!(!io.platform_ready());
    }

    #[test]
    fn dma_adapter_checks_translation_and_both_range_edges() {
        let mut hci_mem = Hci([0; platform::HOST_BYTES as usize]);
        let mut dma_mem = Dma([0; 4096]);
        let hci = native::HciMapping {
            base: hci_mem.0.as_mut_ptr(),
            virtual_base: native::HCI_VIRTUAL_BASE as u64,
            bytes: platform::HOST_BYTES as usize,
        };
        let dma = native::DmaMapping {
            cpu_base: dma_mem.0.as_mut_ptr(),
            virtual_base: TEST_DMA_VIRTUAL,
            iova_base: TEST_DMA_IOVA,
            bytes: dma_mem.0.len() as u64,
        };
        let state = make_authority(hci, dma);
        // SAFETY: pointer/shape are valid test arrays; the fake gate is offline.
        let mut io = unsafe { native::NativeDmaIo::new(dma, FakeGate(state.clone())) }.unwrap();
        assert!(io.ready());
        io.write(TEST_DMA_IOVA + 12, b"offline").unwrap();
        let mut result = [0; 7];
        io.read(TEST_DMA_IOVA + 12, &mut result).unwrap();
        assert_eq!(&result, b"offline");
        assert_eq!(
            io.read(TEST_DMA_IOVA - 1, &mut [0]),
            Err(host::Error::InvalidDma)
        );
        assert_eq!(
            io.write(TEST_DMA_IOVA + 4095, b"xx"),
            Err(host::Error::InvalidDma)
        );

        state.borrow_mut().dma_translated = false;
        assert!(!io.ready());
        assert_eq!(
            io.read(TEST_DMA_IOVA, &mut [0]),
            Err(host::Error::PlatformNotReady)
        );
    }
}
