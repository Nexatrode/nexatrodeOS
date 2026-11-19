mod linuxrmtfs {
    use super::{qcom_modem_handlers as h, qcom_modem_ram_storage::RamShadow};
    use super::{qcom_modem_services::Partition, qcom_modem_storage as disk};
    use super::{
        sunfish_linux_rmtfs as r, sunfish_linux_rmtfs_native as native, sunfish_lpd::Sha256,
    };
    use h::Fault;
    use r::{Call, Io};
    use std::{cell::RefCell, rc::Rc};

    fn mappings() -> r::Mappings {
        let m = |va, pa, bytes| r::Mapping {
            virtual_address: va,
            physical_address: pa,
            bytes,
            writable: true,
            executable: false,
            cached: false,
        };
        r::Mappings {
            buffer: m(0x77001000, r::BUFFER_PA, r::BUFFER_BYTES),
            scratch: m(0x78001000, 0x81000000, r::PAGE),
        }
    }
    fn observation() -> r::Observation {
        r::Observation {
            generation: 7,
            identity: 99,
            ready: true,
            mappings: mappings(),
            modem_stopped: true,
            initial_hlos: true,
        }
    }
    struct State {
        observation: r::Observation,
        bytes: Vec<u8>,
        calls: Vec<(&'static str, usize, usize)>,
        reply: [u64; 8],
        fail: Option<&'static str>,
        panic: Option<&'static str>,
        revoke: Option<&'static str>,
    }
    #[derive(Clone)]
    struct Sim(Rc<RefCell<State>>);
    impl Sim {
        fn new() -> Self {
            Self(Rc::new(RefCell::new(State {
                observation: observation(),
                bytes: vec![0xa5; r::BUFFER_BYTES as usize],
                calls: vec![],
                reply: [0; 8],
                fail: None,
                panic: None,
                revoke: None,
            })))
        }
        fn effect(&self, name: &'static str, at: usize, n: usize) -> Result<(), Fault> {
            let mut s = self.0.borrow_mut();
            s.calls.push((name, at, n));
            if s.panic == Some(name) {
                panic!("effect after {name}");
            }
            if s.revoke == Some(name) {
                s.observation.identity += 1;
            }
            if s.fail == Some(name) {
                Err(Fault::Uncertain)
            } else {
                Ok(())
            }
        }
        fn count(&self, op: &str) -> usize {
            self.0.borrow().calls.iter().filter(|c| c.0 == op).count()
        }
    }
    impl Io for Sim {
        fn observe(&mut self) -> Result<r::Observation, Fault> {
            self.effect("observe", 0, 0)?;
            Ok(self.0.borrow().observation)
        }
        fn zero(&mut self, at: u32, n: usize) -> Result<(), Fault> {
            self.0.borrow_mut().bytes[at as usize..at as usize + n].fill(0);
            self.effect("zero", at as usize, n)
        }
        fn stage(&mut self, call: &Call) -> Result<(), Fault> {
            assert_eq!(call.bytes().len(), 320);
            self.effect("stage", 0, 320)
        }
        fn invoke(&mut self, _: &Call) -> Result<[u64; 8], Fault> {
            self.effect("invoke", 0, 0)?;
            Ok(self.0.borrow().reply)
        }
        fn complete(&mut self) -> Result<(), Fault> {
            self.effect("complete", 0, 0)
        }
        fn read(&mut self, at: u32, out: &mut [u8]) -> Result<(), Fault> {
            out.copy_from_slice(&self.0.borrow().bytes[at as usize..at as usize + out.len()]);
            self.effect("read", at as usize, out.len())
        }
        fn write(&mut self, at: u32, b: &[u8]) -> Result<(), Fault> {
            self.0.borrow_mut().bytes[at as usize..at as usize + b.len()].copy_from_slice(b);
            self.effect("write", at as usize, b.len())
        }
    }
    fn pool(s: &Sim) -> r::Pool<Sim> {
        let mappings = s.0.borrow().observation.mappings;
        r::Pool::attach(7, mappings, s.clone())
    }
    fn prepare(p: &mut r::Pool<Sim>) {
        for i in 0..512 {
            assert_eq!(p.prepare_step(), Ok(i == 511));
        }
    }
    fn shared(s: &Sim) -> r::Pool<Sim> {
        let mut p = pool(s);
        prepare(&mut p);
        p.share().unwrap();
        p
    }

    #[test]
    fn linuxrmtfs_original_linux_scm_bytes_and_pointer_arguments() {
        let c = Call::new(mappings()).unwrap();
        let golden = include_str!(env!("SUNFISH_LINUX_RMTFS_C"));
        let bytes: Vec<u8> = (0..golden.trim().len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&golden[i..i + 2], 16).unwrap())
            .collect();
        assert_eq!(c.bytes().as_slice(), bytes);
        assert_eq!(
            c.words(),
            &[
                0x42000c16, 0x1117, 0x81000040, 16, 0x81000000, 0x81000100, 0, 0
            ]
        );
        assert_eq!(r::CLIENT_ID, 1);
        for pa in [0x81234000, 0xa2a01000, 0x100001000] {
            let mut m = mappings();
            m.scratch.physical_address = pa;
            let c = Call::new(m).unwrap();
            assert_eq!(c.words()[2], pa + 64);
            assert_eq!(
                u64::from_le_bytes(c.bytes()[264..272].try_into().unwrap()),
                pa + 128
            );
        }
    }
    #[test]
    fn linuxrmtfs_exact_reservation_and_no_heap_or_guard_subtraction() {
        assert_eq!(r::BUFFER_PA, 0xfde01000);
        assert_eq!(r::BUFFER_BYTES, 0x200000);
        let m = mappings();
        for bad in [
            r::Mappings {
                buffer: r::Mapping {
                    physical_address: 0xa2001000,
                    ..m.buffer
                },
                ..m
            },
            r::Mappings {
                buffer: r::Mapping {
                    bytes: r::BUFFER_BYTES - 8192,
                    ..m.buffer
                },
                ..m
            },
            r::Mappings {
                scratch: r::Mapping {
                    physical_address: r::BUFFER_PA,
                    ..m.scratch
                },
                ..m
            },
            r::Mappings {
                scratch: r::Mapping {
                    virtual_address: m.buffer.virtual_address,
                    ..m.scratch
                },
                ..m
            },
            r::Mappings {
                buffer: r::Mapping {
                    cached: true,
                    ..m.buffer
                },
                ..m
            },
            r::Mappings {
                buffer: r::Mapping {
                    executable: true,
                    ..m.buffer
                },
                ..m
            },
            r::Mappings {
                buffer: r::Mapping {
                    writable: false,
                    ..m.buffer
                },
                ..m
            },
            r::Mappings {
                scratch: r::Mapping {
                    virtual_address: u64::MAX - 4095,
                    ..m.scratch
                },
                ..m
            },
            r::Mappings {
                scratch: r::Mapping {
                    physical_address: u64::MAX - 4095,
                    ..m.scratch
                },
                ..m
            },
        ] {
            assert_eq!(bad.validate(), Err(Fault::Failed));
            assert!(Call::new(bad).is_err());
        }
    }
    #[test]
    fn linuxrmtfs_attach_requires_cold_live_identity_without_effects() {
        for n in 0..6 {
            let s = Sim::new();
            {
                let mut b = s.0.borrow_mut();
                match n {
                    0 => b.observation.initial_hlos = false,
                    1 => b.observation.modem_stopped = false,
                    2 => b.observation.ready = false,
                    3 => b.observation.generation = 8,
                    4 => b.observation.identity = 0,
                    _ => b.observation.mappings.buffer.cached = true,
                }
            }
            let mut p = pool(&s);
            assert_eq!(p.status().error, Some(r::Error::Attach));
            assert!(p.prepare_step().is_err());
            assert!(p.share().is_err());
            assert_eq!(
                (s.count("zero"), s.count("stage"), s.count("invoke")),
                (0, 0, 0)
            );
        }
    }
    #[test]
    fn linuxrmtfs_bounded_preparation_share_once_and_repeated_allocations() {
        let s = Sim::new();
        let mut p = pool(&s);
        assert_eq!(p.share(), Err(Fault::Failed));
        assert_eq!(p.allocation(512), Err(Fault::Failed));
        prepare(&mut p);
        assert!(s.0.borrow().bytes.iter().all(|b| *b == 0));
        assert_eq!(p.prepare_step(), Ok(true));
        assert_eq!(s.count("zero"), 512);
        p.share().unwrap();
        assert_eq!(p.status().phase, r::Phase::Shared);
        assert_eq!(p.status().assignments_attempted, 1);
        s.0.borrow_mut().observation.modem_stopped = false;
        for n in [1, 512, 4097, r::BUFFER_BYTES] {
            assert_eq!(p.allocation(n), Ok(r::BUFFER_PA));
        }
        assert_eq!(p.allocation(0), Err(Fault::Failed));
        assert_eq!(p.allocation(r::BUFFER_BYTES + 1), Err(Fault::Failed));
        assert!(p.prepare_step().is_err());
        assert!(p.share().is_err());
        assert_eq!(s.count("invoke"), 1);
        assert_eq!(s.count("zero"), 512);
    }
    #[test]
    fn linuxrmtfs_monitor_status_in_both_registers_and_no_retry() {
        for (a0, a1) in [(1, 0), (0, 1), (0, u64::MAX), (u64::MAX, 0)] {
            let s = Sim::new();
            let mut p = pool(&s);
            prepare(&mut p);
            s.0.borrow_mut().reply[0] = a0;
            s.0.borrow_mut().reply[1] = a1;
            assert!(p.share().is_err());
            assert!(p.share().is_err());
            assert_eq!(s.count("invoke"), 1);
            assert_eq!(s.count("complete"), 0);
            assert_eq!(p.status().phase, r::Phase::Unknown);
        }
    }
    #[test]
    fn linuxrmtfs_failures_and_lifecycle_changes_quarantine_resources() {
        for op in ["zero", "stage", "invoke", "complete"] {
            for revoke in [false, true] {
                let s = Sim::new();
                let mut p = pool(&s);
                if op != "zero" {
                    prepare(&mut p);
                }
                if revoke {
                    s.0.borrow_mut().revoke = Some(op);
                } else {
                    s.0.borrow_mut().fail = Some(op);
                }
                assert!(
                    if op == "zero" {
                        p.prepare_step().map(|_| ())
                    } else {
                        p.share()
                    }
                    .is_err()
                );
                let calls = s.0.borrow().calls.len();
                assert!(p.share().is_err());
                assert!(p.allocation(512).is_err());
                assert_eq!(s.0.borrow().calls.len(), calls);
            }
        }
    }
    #[test]
    fn linuxrmtfs_unwind_after_effect_cannot_report_ready_or_resubmit() {
        use std::panic::{AssertUnwindSafe, catch_unwind};
        for op in ["zero", "stage", "invoke", "complete"] {
            let s = Sim::new();
            let mut p = pool(&s);
            if op != "zero" {
                prepare(&mut p);
            }
            s.0.borrow_mut().panic = Some(op);
            assert!(
                catch_unwind(AssertUnwindSafe(|| if op == "zero" {
                    p.prepare_step().map(|_| ())
                } else {
                    p.share()
                }))
                .is_err()
            );
            assert_eq!(p.status().phase, r::Phase::Unknown);
            assert!(p.status().error.is_some());
            let before = s.0.borrow().calls.len();
            assert!(p.share().is_err());
            assert_eq!(before, s.0.borrow().calls.len());
        }
    }
    fn hash(b: &[u8]) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(b);
        h.finish()
    }
    fn shadows() -> (disk::Configuration, [Vec<u8>; 4], [Vec<u8>; 4]) {
        let sources = core::array::from_fn(|n| vec![n as u8 + 1; 8192]);
        let c = disk::Configuration {
            generation: 7,
            device: [9; 32],
            bindings: core::array::from_fn(|i| disk::Binding {
                partition: disk::PARTITIONS[i],
                object: i as u64 + 1,
                source: hash(&sources[i]),
                disk: h::Disk {
                    bytes: 8192,
                    writable: true,
                    durable: false,
                },
            }),
        };
        (c, sources, core::array::from_fn(|_| vec![0; 8192]))
    }
    #[test]
    fn linuxrmtfs_larger_ram_transfer_crosses_pages_and_retains_bounds() {
        let s = Sim::new();
        let (mut c, _, _) = shadows();
        let src: [Vec<u8>; 4] = core::array::from_fn(|n| vec![n as u8 + 1; 262144]);
        for (i, binding) in c.bindings.iter_mut().enumerate() {
            binding.source = hash(&src[i]);
            binding.disk.bytes = 262144;
        }
        let mut dst: [Vec<u8>; 4] = core::array::from_fn(|_| vec![0; 262144]);
        let original = src.clone();
        {
            let mut r = resources(&s, c, &src, &mut dst);
            let pa = r::BUFFER_PA + 512;
            r.transfer(Partition::ModemFs1, 1, pa, 256, false).unwrap();
            let state = s.0.borrow();
            assert_eq!(&state.bytes[512..512+131072], vec![1u8; 131072]);
            assert_eq!(state.bytes[511], 0);
            assert_eq!(state.bytes[512+131072], 0);
            drop(state);
            s.0.borrow_mut().bytes[512..512+131072].fill(0x77);
            r.transfer(Partition::ModemFs1, 1, pa, 256, true).unwrap();
            let calls = s.count("read") + s.count("write");
            assert_eq!(r.transfer(Partition::ModemFs1, 0, pa, 257, false), Err(Fault::Failed));
            assert_eq!(s.count("read") + s.count("write"), calls);
        }
        assert_eq!(&dst[0][512..512+131072], vec![0x77; 131072]);
        assert_eq!(src, original);
    }
    fn resources<'a>(
        s: &Sim,
        c: disk::Configuration,
        src: &'a [Vec<u8>; 4],
        dst: &'a mut [Vec<u8>; 4],
    ) -> r::Resources<'a, Sim> {
        let [a, b, c0, d] = dst;
        let ram = RamShadow::new(
            c,
            src.each_ref().map(|v| v.as_slice()),
            [a, b, c0, d].map(|v| v.as_mut_slice()),
        )
        .unwrap();
        r::Resources::attach(shared(s), ram.into_storage().unwrap())
    }
    #[test]
    fn linuxrmtfs_real_verified_ram_shadows_both_directions_all_partitions() {
        let s = Sim::new();
        let (c, src, mut dst) = shadows();
        let original = src.clone();
        {
            let mut r = resources(&s, c, &src, &mut dst);
            s.0.borrow_mut().observation.modem_stopped = false;
            for (i, p) in disk::PARTITIONS.into_iter().enumerate() {
                r.transfer(p, 1, r::BUFFER_PA + 512, 8, false).unwrap();
                assert_eq!(&s.0.borrow().bytes[512..4608], vec![i as u8 + 1; 4096]);
                s.0.borrow_mut().bytes[512..4608].fill(0x40 + i as u8);
                r.transfer(p, 2, r::BUFFER_PA + 512, 8, true).unwrap();
                assert!(r.status().1.dirty[i]);
                r.sync(p).unwrap();
                assert!(!r.status().1.dirty[i]);
                assert_eq!(r.allocation(0x200000), Ok(r::BUFFER_PA));
            }
            assert!(!r.status().2);
            assert_eq!(s.count("invoke"), 1);
        }
        assert_eq!(src, original);
        for (i, b) in dst.iter().enumerate() {
            assert_eq!(&b[1024..5120], vec![0x40 + i as u8; 4096]);
            assert_eq!(&b[..1024], &src[i][..1024]);
            assert_eq!(&b[5120..], &src[i][5120..]);
        }
    }
    #[test]
    fn linuxrmtfs_bad_geometry_performs_no_data_io_or_shadow_write() {
        let s = Sim::new();
        let (c, src, mut dst) = shadows();
        {
            let mut r = resources(&s, c, &src, &mut dst);
            for (sector, pa, count) in [
                (0, r::BUFFER_PA - 512, 1),
                (0, r::BUFFER_PA + r::BUFFER_BYTES as u64, 1),
                (0, r::BUFFER_PA + 1, 1),
                (0, u64::MAX, 1),
                (0, r::BUFFER_PA, 0),
                (0, r::BUFFER_PA, 257),
                (16, r::BUFFER_PA, 1),
                (u32::MAX, r::BUFFER_PA, u32::MAX),
            ] {
                for write in [true, false] {
                    assert_eq!(
                        r.transfer(Partition::ModemFs1, sector, pa, count, write),
                        Err(Fault::Failed)
                    );
                }
            }
            assert_eq!((s.count("read"), s.count("write")), (0, 0));
            r.transfer(
                Partition::ModemFs1,
                15,
                r::BUFFER_PA + r::BUFFER_BYTES as u64 - 512,
                1,
                false,
            )
            .unwrap();
            assert!(!r.status().2);
        }
        assert_eq!(dst, src);
    }
    #[test]
    fn linuxrmtfs_uncertain_shared_copy_stops_without_repeating_effect() {
        for op in ["read", "write"] {
            for revoke in [false, true] {
                let s = Sim::new();
                let (c, src, mut dst) = shadows();
                let mut r = resources(&s, c, &src, &mut dst);
                if revoke {
                    s.0.borrow_mut().revoke = Some(op);
                } else {
                    s.0.borrow_mut().fail = Some(op);
                }
                assert!(
                    r.transfer(Partition::ModemFs1, 0, r::BUFFER_PA, 1, op == "read")
                        .is_err()
                );
                let before = s.count(op);
                assert!(
                    r.transfer(Partition::ModemFs1, 0, r::BUFFER_PA, 1, op == "read")
                        .is_err()
                );
                assert_eq!(s.count(op), before);
                assert!(r.status().2);
            }
        }
    }
    #[test]
    fn linuxrmtfs_storage_generation_mismatch_is_not_ready() {
        let s = Sim::new();
        let (mut c, src, mut dst) = shadows();
        c.generation = 8;
        let [a, b, c0, d] = &mut dst;
        let ram = RamShadow::new(
            c,
            src.each_ref().map(|v| v.as_slice()),
            [a, b, c0, d].map(|v| v.as_mut_slice()),
        )
        .unwrap();
        let mut resources = r::Resources::attach(pool(&s), ram.into_storage().unwrap());
        assert!(resources.status().2);
        assert!(resources.allocation(4096).is_err());
        assert!(
            resources
                .transfer(Partition::ModemFs1, 0, r::BUFFER_PA, 1, true)
                .is_err()
        );
        assert_eq!((s.count("read"), s.count("write")), (0, 0));
        assert_eq!((s.count("zero"), s.count("invoke")), (0, 0));
    }
    #[test]
    fn linuxrmtfs_unwind_during_shared_transfer_is_terminal() {
        use std::panic::{AssertUnwindSafe, catch_unwind};
        for op in ["read", "write"] {
            let s = Sim::new();
            let (c, src, mut dst) = shadows();
            let mut resources = resources(&s, c, &src, &mut dst);
            s.0.borrow_mut().panic = Some(op);
            assert!(
                catch_unwind(AssertUnwindSafe(|| resources.transfer(
                    Partition::ModemFs1,
                    0,
                    r::BUFFER_PA,
                    1,
                    op == "read"
                )))
                .is_err()
            );
            assert!(resources.status().2);
            let before = s.0.borrow().calls.len();
            assert!(resources.shared().is_err());
            assert_eq!(before, s.0.borrow().calls.len());
        }
    }
    struct NativeGate(r::Observation);
    impl native::Gate for NativeGate {
        fn observe(&mut self) -> Result<r::Observation, Fault> {
            Ok(self.0)
        }
    }
    struct Monitor(Rc<RefCell<usize>>);
    impl native::Smc for Monitor {
        fn invoke(&mut self, _: &Call) -> Result<[u64; 8], Fault> {
            *self.0.borrow_mut() += 1;
            Ok([0; 8])
        }
    }
    struct Pages {
        ptr: *mut u8,
        layout: std::alloc::Layout,
    }
    impl Pages {
        fn new(bytes: usize) -> Self {
            let layout = std::alloc::Layout::from_size_align(bytes, 4096).unwrap();
            let ptr = unsafe { std::alloc::alloc(layout) };
            assert!(!ptr.is_null());
            unsafe { std::ptr::write_bytes(ptr, 0x5a, bytes) };
            Self { ptr, layout }
        }
        fn base(&self) -> u64 {
            self.ptr as u64 + 4096
        }
        fn guards(&self) {
            for at in [0, self.layout.size() - 4096] {
                for i in 0..4096 {
                    assert_eq!(unsafe { *self.ptr.add(at + i) }, 0x5a);
                }
            }
        }
    }
    impl Drop for Pages {
        fn drop(&mut self) {
            unsafe { std::alloc::dealloc(self.ptr, self.layout) }
        }
    }
    #[test]
    fn linuxrmtfs_native_larger_copy_spans_pages_and_preserves_guards() {
        let buffer = Pages::new(r::BUFFER_BYTES as usize + 8192);
        let scratch = Pages::new(12288);
        let mut m = mappings();
        m.buffer.virtual_address = buffer.base();
        m.scratch.virtual_address = scratch.base();
        let observation = r::Observation { mappings: m, ..observation() };
        let mut io = unsafe { native::Native::new(NativeGate(observation), Monitor(Rc::new(RefCell::new(0))), m) }
            .unwrap_or_else(|_| panic!("native geometry"));
        let data = vec![0x91; 131072];
        io.write(512, &data).unwrap();
        let mut result = vec![0; 131072];
        io.read(512, &mut result).unwrap();
        assert_eq!(result, data);
        assert_eq!(unsafe { *(buffer.base() as *const u8).add(511) }, 0x5a);
        assert_eq!(unsafe { *(buffer.base() as *const u8).add(512 + 131072) }, 0x5a);
        assert!(io.write(r::BUFFER_BYTES - 512, &data).is_err());
        assert!(io.write(0, &vec![0; 131073]).is_err());
        buffer.guards();
        scratch.guards();
    }
    #[test]
    fn linuxrmtfs_native_capability_checks_staging_and_consumes_invoke() {
        let buffer = Pages::new(r::BUFFER_BYTES as usize + 8192);
        let scratch = Pages::new(12288);
        let mut m = mappings();
        m.buffer.virtual_address = buffer.base();
        m.scratch.virtual_address = scratch.base();
        let o = r::Observation {
            mappings: m,
            ..observation()
        };
        let count = Rc::new(RefCell::new(0));
        let mut native = unsafe { native::Native::new(NativeGate(o), Monitor(count.clone()), m) }
            .unwrap_or_else(|_| panic!("native geometry"));
        let call = Call::new(m).unwrap();
        let mut other = m;
        other.scratch.physical_address += 4096;
        let wrong = Call::new(other).unwrap();
        assert_eq!(native.invoke(&call), Err(Fault::Failed));
        assert_eq!(native.stage(&wrong), Err(Fault::Failed));
        native.stage(&call).unwrap();
        assert_eq!(native.invoke(&wrong), Err(Fault::Failed));
        assert_eq!(native.zero(0, 4096), Err(Fault::Failed));
        native.invoke(&call).unwrap();
        assert_eq!(native.invoke(&call), Err(Fault::Failed));
        assert_eq!(native.stage(&call), Err(Fault::Failed));
        assert_eq!(*count.borrow(), 1);
        buffer.guards();
        scratch.guards();
    }
    #[test]
    fn linuxrmtfs_actual_volatile_adapter_with_guarded_host_pages() {
        let buffer = Pages::new(r::BUFFER_BYTES as usize + 8192);
        let scratch = Pages::new(12288);
        let mut m = mappings();
        m.buffer.virtual_address = buffer.base();
        m.scratch.virtual_address = scratch.base();
        let o = r::Observation {
            mappings: m,
            ..observation()
        };
        let calls = Rc::new(RefCell::new(0));
        let io = unsafe { native::Native::new(NativeGate(o), Monitor(calls.clone()), m) }
            .unwrap_or_else(|_| panic!("native geometry"));
        let mut p = r::Pool::attach(7, m, io);
        for _ in 0..512 {
            p.prepare_step().unwrap();
        }
        p.share().unwrap();
        assert_eq!(p.allocation(4096), Ok(r::BUFFER_PA));
        assert_eq!(*calls.borrow(), 1);
        let call = Call::new(m).unwrap();
        for i in 0..320 {
            assert_eq!(
                unsafe { *(scratch.base() as *const u8).add(i) },
                call.bytes()[i]
            );
        }
        let (c, src, mut dst) = shadows();
        {
            let [a, b, c0, d] = &mut dst;
            let ram = RamShadow::new(
                c,
                src.each_ref().map(|v| v.as_slice()),
                [a, b, c0, d].map(|v| v.as_mut_slice()),
            )
            .unwrap();
            let mut resources = r::Resources::attach(p, ram.into_storage().unwrap());
            let last = r::BUFFER_PA + u64::from(r::BUFFER_BYTES) - 4096;
            resources
                .transfer(Partition::ModemFs2, 8, last, 8, false)
                .unwrap();
            for i in 0..4096 {
                assert_eq!(
                    unsafe {
                        *((buffer.base() + u64::from(r::BUFFER_BYTES) - 4096) as *const u8).add(i)
                    },
                    2
                );
                unsafe {
                    *((buffer.base() + u64::from(r::BUFFER_BYTES) - 4096) as *mut u8).add(i) = 0x77;
                }
            }
            resources
                .transfer(Partition::ModemFs2, 0, last, 8, true)
                .unwrap();
            resources.sync(Partition::ModemFs2).unwrap();
            assert!(!resources.status().2);
        }
        assert!(dst[1][..4096].iter().all(|b| *b == 0x77));
        assert_eq!(&dst[1][4096..], &src[1][4096..]);
        buffer.guards();
        scratch.guards();
    }

    #[cfg(feature = "sunfish-linux-rmtfs")]
    include!("sunfish_linux_rmtfs_server_cases.rs");
}
