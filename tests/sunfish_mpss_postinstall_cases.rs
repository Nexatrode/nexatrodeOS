// CPU RAM only. Exercise the actual Native::install_experiment path rather
// than bypassing its consumed state with the lower-level Installer.
use crate::{sunfish_mpss_linux_smem as smem, sunfish_mpss_linux_smp2p as smp};
fn cold(g: u64) -> Result<(), smem::Error> {
    if g == 1 {
        Ok(())
    } else {
        Err(smem::Error::Guard)
    }
}
fn preauth(g: u64) -> Result<smp::Observation, smp::Error> {
    Ok(smp::Observation {
        generation: g,
        lifecycle: smp::Lifecycle::PreAuth,
        power: smp::Power::Active,
    })
}
pub(super) fn attach<'a>(
    f: &'a mut Fixture,
    shared: &'a mut super::super::shared_fixture::Fixture,
    data: &'a [u64],
    ram: &'a mut [Page],
) -> Native<'a, fn(usize) -> Result<(), Error>> {
    let mut n = f.native_with_pages(data);
    let b = shared.smem.as_mut_ptr().cast::<u8>();
    // Same retained CPU-RAM views as the production adapter's delegated span.
    let memory = unsafe {
        let w = crate::sunfish_ipa_smem_native::Window::new_with_modem_pages(
            b,
            0x86000000,
            b.add(0x11d000).cast(),
            0x8611d000,
            shared.lock.0.as_mut_ptr().cast(),
            0x1f43000,
            b.add(0x11f000).cast(),
            0x8611f000,
        )
        .unwrap();
        crate::sunfish_ipa_shared_memory::SharedMemory::new(
            w,
            shared.imem.as_ptr().cast(),
            0x146a8000,
            8192,
        )
        .unwrap()
    };
    n.attach_shared_memory(memory).unwrap();
    let memory = unsafe {
        crate::sunfish_ipa_linux_receive_native::NativeReceiveRam::new(
            ram.as_mut_ptr().cast(),
            &receive_physical(),
        )
        .unwrap()
    };
    n.attach_receive_memory(memory).unwrap();
    n
}
pub(super) fn finish(n: &mut Native<'_, fn(usize) -> Result<(), Error>>) -> smem::Prepared {
    n.attach_modem_smem(1, cold).unwrap();
    for _ in 0..32 {
        if n.modem_smem_step().unwrap() == smem::Phase::Prepared {
            break;
        }
    }
    let report = n.modem_smem_report().unwrap();
    assert_eq!(
        (
            report.allocations,
            report.submitted,
            report.completed,
            report.zeroed,
            report.verified
        ),
        (3, 4201, 4201, 16756, 16756)
    );
    n.take_modem_smem_prepared().unwrap()
}
#[test]
fn postinstall_full_native_handoff_keeps_installer_closed_through_smem_and_smp2p() {
    let mut f = Fixture::new();
    let mut shared = super::super::shared_fixture::Fixture::new();
    let data = core::array::from_fn::<_, 32, _>(|i| 0xa3000000 + i as u64 * 4096);
    let mut ram = receive_ram();
    let mut extra = core::array::from_fn::<_, 4, _>(|_| Page([0xa5; 512]));
    let mut apcs = 0u32;
    let mut n = attach(&mut f, &mut shared, &data, &mut ram);
    let plan = stage_receive(&mut n, &mut extra);
    n.select(&plan).unwrap();
    n.install_experiment(&plan, 8).unwrap();
    assert!(n.failed && n.installation_completed);
    let _rx = n.take_receive_memory().unwrap();
    let prior = n.shared_report().unwrap();
    let receipt = finish(&mut n);
    assert_eq!(receipt.generation(), 1);
    let signal = unsafe {
        crate::sunfish_mpss_linux_smp2p_native::Native::new(
            &mut apcs,
            0x17c0000c,
            preauth as fn(u64) -> Result<smp::Observation, smp::Error>,
        )
    }
    .map_err(|(_, e)| e)
    .unwrap();
    n.activate_modem_smp2p(receipt, signal).unwrap();
    let report = n.modem_smp2p_report().unwrap();
    assert_eq!(report.phase, smp::Phase::Active);
    assert_eq!(n.modem_smp2p_poll(), Ok(smp::Poll::Waiting));
    assert_eq!(n.shared_report(), Some(prior));
    n.retained_receive_domain().unwrap();
    assert!(n.failed, "installation write path must remain consumed");
    assert!(n.install_experiment(&plan, 8).is_err());
    assert_eq!(apcs, 1 << 14);
}

#[test]
fn postinstall_changed_route_table_context_or_fault_is_terminal_without_smem_writes() {
    for failure in 0..7 {
        let mut f = Fixture::new();
        let mut shared = super::super::shared_fixture::Fixture::new();
        let b = shared.smem.as_mut_ptr().cast::<u8>();
        let data = core::array::from_fn::<_, 32, _>(|i| 0xa3000000 + i as u64 * 4096);
        let mut ram = receive_ram();
        let mut extra = core::array::from_fn::<_, 4, _>(|_| Page([0xa5; 512]));
        let mut n = attach(&mut f, &mut shared, &data, &mut ram);
        let plan = stage_receive(&mut n, &mut extra);
        n.select(&plan).unwrap();
        n.install_experiment(&plan, 8).unwrap();
        let before = unsafe { std::slice::from_raw_parts(b, 0x200000) }.to_vec();
        // Only exclusive host RAM is changed. Restore the stimulus afterwards
        // to prove that a recovered-looking snapshot cannot reopen the owner.
        let (p, value) = match failure {
            0 => (unsafe { n.registers[0].add(0x48).cast::<u32>() }, 1),
            1 => (unsafe { n.registers[2].add(0x58).cast::<u32>() }, 0x402),
            2 => (
                unsafe {
                    n.registers[0]
                        .add(0x800 + n.slot.unwrap() * 4)
                        .cast::<u32>()
                },
                0,
            ),
            3 => (n.registers[2].cast::<u32>(), 0),
            4 => (n.table_pointers[0].cast::<u32>(), 0),
            5 => {
                n.fault.store(true, Ordering::Release);
                (core::ptr::null_mut(), 0)
            }
            _ => {
                n.installation_completed = false;
                (core::ptr::null_mut(), 0)
            }
        };
        let old = if p.is_null() {
            0
        } else {
            unsafe {
                let v = p.read();
                p.write(value);
                v
            }
        };
        assert!(n.attach_modem_smem(1, cold).is_err(), "failure {failure}");
        assert!(n.modem_smem_report().is_none());
        if !p.is_null() {
            unsafe { p.write(old) }
        };
        n.fault.store(false, Ordering::Release);
        n.installation_completed = true;
        n.retained_receive_domain().unwrap();
        assert!(n.attach_modem_smem(1, cold).is_err());
        assert!(n.modem_smem_step().is_err());
        assert!(n.take_modem_smem_prepared().is_err());
        assert!(n.failed, "install remains closed");
        assert_eq!(unsafe { std::slice::from_raw_parts(b, 0x200000) }, before);
    }
}

#[test]
fn postinstall_failed_or_unwound_child_keeps_actual_owners_and_cannot_replay() {
    fn lost(_: u64) -> Result<(), smem::Error> {
        Err(smem::Error::Guard)
    }
    fn unwind(_: u64) -> Result<(), smem::Error> {
        panic!("injected gate unwind")
    }
    for gate in [lost as fn(u64) -> Result<(), smem::Error>, unwind] {
        let mut f = Fixture::new();
        let mut shared = super::super::shared_fixture::Fixture::new();
        let b = shared.smem.as_mut_ptr().cast::<u8>();
        let data = core::array::from_fn::<_, 32, _>(|i| 0xa3000000 + i as u64 * 4096);
        let mut ram = receive_ram();
        let mut extra = core::array::from_fn::<_, 4, _>(|_| Page([0xa5; 512]));
        let mut n = attach(&mut f, &mut shared, &data, &mut ram);
        let plan = stage_receive(&mut n, &mut extra);
        n.select(&plan).unwrap();
        n.install_experiment(&plan, 8).unwrap();
        let prior = n.shared_report();
        n.attach_modem_smem(1, gate).unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| n.modem_smem_step()));
        assert!(result.is_err() || result.unwrap().is_err());
        let before = unsafe { std::slice::from_raw_parts(b, 0x200000) }.to_vec();
        assert!(n.modem_smem_step().is_err());
        assert!(n.take_modem_smem_prepared().is_err());
        assert!(n.modem_smp2p_poll().is_err());
        assert!(n.take_mpss_ready_observation().is_err());
        assert_eq!(n.shared_report(), prior);
        assert!(n.shared_memory.is_some() && n.failed);
        assert_eq!(unsafe { std::slice::from_raw_parts(b, 0x200000) }, before);
    }
}

#[test]
fn postinstall_rechecks_retained_domain_after_child_before_returning_success() {
    std::thread_local! {static CHANGE:std::cell::Cell<usize>=const{std::cell::Cell::new(0)};}
    fn changed(g: u64) -> Result<(), smem::Error> {
        CHANGE.with(|c| {
            let p = c.replace(0);
            if p != 0 {
                unsafe { (p as *mut u64).write(0) }
            }
        });
        cold(g)
    }
    let mut f = Fixture::new();
    let mut shared = super::super::shared_fixture::Fixture::new();
    let data = core::array::from_fn::<_, 32, _>(|i| 0xa3000000 + i as u64 * 4096);
    let mut ram = receive_ram();
    let mut extra = core::array::from_fn::<_, 4, _>(|_| Page([0xa5; 512]));
    let mut n = attach(&mut f, &mut shared, &data, &mut ram);
    let plan = stage_receive(&mut n, &mut extra);
    n.select(&plan).unwrap();
    n.install_experiment(&plan, 8).unwrap();
    n.attach_modem_smem(1, changed).unwrap();
    let p = n.table_pointers[0];
    let old = unsafe { p.read() };
    CHANGE.set(p as usize);
    assert!(n.modem_smem_step().is_err());
    unsafe { p.write(old) };
    n.retained_receive_domain().unwrap();
    assert!(n.modem_smem_step().is_err());
    assert!(n.failed);
}
