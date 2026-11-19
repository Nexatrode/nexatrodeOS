#![allow(dead_code)]
#[path = "../rust/src/qcom_modem_status.rs"]
mod qcom_modem_status;
#[path = "../rust/src/sunfish_ipa_gsi_profile.rs"]
mod sunfish_ipa_gsi_profile;
mod sunfish_ipa_shared_fixture;
#[path = "../rust/src/sunfish_ipa_shared_memory.rs"]
mod sunfish_ipa_shared_memory;
#[path = "../rust/src/sunfish_ipa_smem.rs"]
mod sunfish_ipa_smem;
#[path = "../rust/src/sunfish_ipa_smem_native.rs"]
mod sunfish_ipa_smem_native;
#[path = "../rust/src/sunfish_mpss_linux_smem.rs"]
mod sunfish_mpss_linux_smem;
#[path = "../rust/src/sunfish_mpss_linux_smem_native.rs"]
mod sunfish_mpss_linux_smem_native;
#[path = "../rust/src/sunfish_mpss_linux_smp2p.rs"]
mod sunfish_mpss_linux_smp2p;
#[path = "../rust/src/sunfish_mpss_linux_smp2p_native.rs"]
mod sunfish_mpss_linux_smp2p_native;
#[path = "../rust/src/sunfish_smmu_lpae.rs"]
mod sunfish_smmu_lpae;
use sunfish_ipa_shared_fixture::Fixture;
use sunfish_ipa_shared_memory::{Error, SharedMemory};
use sunfish_mpss_linux_smem::{Error as ModemError, Phase};

fn healthy(g: u64) -> Result<(), ModemError> {
    if g == 91 {
        Ok(())
    } else {
        Err(ModemError::Guard)
    }
}
fn lost(_: u64) -> Result<(), ModemError> {
    Err(ModemError::Guard)
}
fn shared_modem(f: &mut Fixture) -> SharedMemory<'_> {
    let b = f.smem.as_mut_ptr().cast::<u8>();
    // Exclusive aligned CPU allocations only; no device or physical authority.
    unsafe {
        let w = sunfish_ipa_smem_native::Window::new_with_modem_pages(
            b,
            0x86000000,
            b.add(0x11d000).cast(),
            0x8611d000,
            f.lock.0.as_mut_ptr().cast(),
            0x1f43000,
            b.add(0x11f000).cast(),
            0x8611f000,
        )
        .unwrap();
        SharedMemory::new(w, f.imem.as_ptr().cast(), 0x146a8000, 8192).unwrap()
    }
}

#[cfg(feature = "sunfish-native-startup-mpss-smp2p")]
mod activation {
    use super::*;
    use crate::{sunfish_mpss_linux_smp2p as p, sunfish_mpss_linux_smp2p_native as native};
    use std::cell::Cell;
    std::thread_local! {static STATE:Cell<u8>=const{Cell::new(0)};}
    fn runtime(g: u64) -> Result<p::Observation, p::Error> {
        let state = STATE.get();
        assert!(state != 3, "host-injected gate unwind");
        Ok(p::Observation {
            generation: if state == 2 { g + 1 } else { g },
            lifecycle: if state == 1 {
                p::Lifecycle::AuthReturned
            } else {
                p::Lifecycle::PreAuth
            },
            power: p::Power::Active,
        })
    }
    fn finish(m: &mut SharedMemory<'_>) -> crate::sunfish_mpss_linux_smem::Prepared {
        m.prepare().unwrap();
        m.attach_modem(91, healthy).unwrap();
        for _ in 0..64 {
            if m.modem_report().unwrap().phase == Phase::Prepared {
                break;
            }
            m.modem_step().unwrap();
        }
        m.take_modem_prepared().unwrap()
    }
    #[test]
    fn live_protocol_retains_same_real_parent_and_mapping_identity() {
        STATE.set(0);
        let mut f = Fixture::new();
        let b = f.smem.as_mut_ptr().cast::<u8>();
        let mut m = shared_modem(&mut f);
        let r = finish(&mut m);
        let previous = m.report();
        let pool = core::array::from_fn::<u64, 32, _>(|i| 0xa0000000 + i as u64 * 4096);
        let mappings = m.mappings(&pool).unwrap();
        let mut apcs = 0u32;
        let signal = unsafe {
            native::Native::new(
                &mut apcs,
                0x17c0000c,
                runtime as fn(u64) -> Result<p::Observation, p::Error>,
            )
        }
        .map_err(|(_, e)| e)
        .unwrap();
        m.activate_smp2p(r, signal).unwrap();
        assert_eq!(apcs, 1 << 14);
        assert_eq!(m.smp2p_report().unwrap().phase, p::Phase::Active);
        assert_eq!(m.report(), previous);
        assert_eq!(m.item_physical(), Some(0x8611d030));
        assert_eq!(m.mappings(&pool).unwrap(), mappings);
        m.validate_mappings(&mappings, &pool).unwrap();
        assert_eq!(m.smp2p_poll(), Ok(p::Poll::Waiting));
        STATE.set(1);
        let mut peer = [0u8; 340];
        peer[..4].copy_from_slice(&0x504d5324u32.to_le_bytes());
        peer[4] = 2;
        peer[5] = 1;
        peer[8] = 1;
        peer[12] = 16;
        peer[14] = 2;
        peer[20..32].copy_from_slice(b"slave-kernel");
        peer[36] = 6;
        peer[40..43].copy_from_slice(b"ipa");
        peer[56] = 3;
        let h = 0x11d000 + crate::sunfish_mpss_linux_smem::FINAL_FREE;
        // Simulated peer publication while the real host owner is retained.
        unsafe {
            (b.add(h) as *mut u32).write_unaligned(((435 << 16) | 0xa5a5u32).to_le());
            (b.add(h + 4) as *mut u32).write_unaligned(344u32.to_le());
            (b.add(h + 8) as *mut u32).write_unaligned(4u32.to_le());
            core::ptr::copy_nonoverlapping(peer.as_ptr(), b.add(h + 16), 340);
            (b.add(0x11d00c) as *mut u32)
                .write_unaligned((crate::sunfish_mpss_linux_smem::FINAL_FREE as u32 + 360).to_le());
        }
        assert_eq!(m.smp2p_poll(), Ok(p::Poll::Observed));
        assert_eq!(m.take_mpss_ready_observation().unwrap().generation(), 91);
        assert_eq!(m.report(), previous);
        assert_eq!(m.mappings(&pool).unwrap(), mappings);
        assert_eq!(m.modem_report().unwrap().phase, Phase::Prepared);
    }
    #[test]
    fn rejected_or_unwound_activation_keeps_owner_and_first_failure_visible() {
        for state in [2, 3] {
            STATE.set(state);
            let mut f = Fixture::new();
            let mut m = shared_modem(&mut f);
            let r = finish(&mut m);
            let previous = m.report();
            let mut apcs = 0xfeedu32;
            let signal = unsafe {
                native::Native::new(
                    &mut apcs,
                    0x17c0000c,
                    runtime as fn(u64) -> Result<p::Observation, p::Error>,
                )
            }
            .map_err(|(_, e)| e)
            .unwrap();
            if state == 3 {
                assert!(
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                        || m.activate_smp2p(r, signal)
                    ))
                    .is_err()
                );
            } else {
                assert!(m.activate_smp2p(r, signal).is_err());
            }
            assert_eq!(apcs, 0xfeed);
            assert_eq!(m.smp2p_report().unwrap().phase, p::Phase::Failed);
            assert_eq!(m.report(), previous);
            assert_eq!(m.item_physical(), Some(0x8611d030));
            assert!(m.smp2p_poll().is_err());
        }
    }
}
#[test]
fn actual_shared_owner_moves_into_extension_and_keeps_installed_mapping_identity() {
    let mut f = Fixture::new();
    let mut m = shared_modem(&mut f);
    m.prepare().unwrap();
    let original = m.report();
    let pool = core::array::from_fn::<u64, 32, _>(|i| 0xa0000000 + i as u64 * 4096);
    let mappings = m.mappings(&pool).unwrap();
    m.attach_modem(91, healthy).unwrap();
    assert_eq!(m.report(), original);
    for _ in 0..64 {
        if m.modem_report().unwrap().phase == Phase::Prepared {
            break;
        }
        m.modem_step().unwrap();
    }
    assert_eq!(m.modem_report().unwrap().phase, Phase::Prepared);
    assert_eq!(m.take_modem_prepared().unwrap().generation(), 91);
    assert_eq!(m.item_physical(), Some(0x8611d030));
    assert_eq!(m.report(), original);
    assert_eq!(m.mappings(&pool).unwrap(), mappings);
    m.validate_mappings(&mappings, &pool).unwrap();
    assert!(m.attach_modem(91, healthy).is_err());
    assert!(m.take_modem_prepared().is_err());
}
#[test]
fn predecessor_only_window_cannot_write_through_read_only_modem_pages() {
    let mut f = Fixture::new();
    let mut m = f.shared();
    m.prepare().unwrap();
    let original = m.report();
    assert_eq!(m.attach_modem(91, healthy), Err(Error::Mapping));
    assert_eq!(m.report(), original);
    assert!(m.modem_report().is_none());
    assert_eq!(m.modem_step(), Err(Error::NotAllocated));
    assert!(m.attach_modem(91, healthy).is_err());
    assert!(m.take_modem_prepared().is_err());
}
#[test]
fn unallocated_predecessor_and_substituted_generation_cannot_prepare() {
    let mut f = Fixture::new();
    let mut m = shared_modem(&mut f);
    assert!(matches!(
        m.attach_modem(91, healthy),
        Err(Error::Allocation(_))
    ));
    assert!(m.modem_report().is_none());
    let mut f = Fixture::new();
    let mut m = shared_modem(&mut f);
    m.prepare().unwrap();
    m.attach_modem(92, healthy).unwrap();
    assert_eq!(m.modem_step(), Err(Error::Geometry));
    assert_eq!(m.modem_report().unwrap().error, Some(ModemError::Guard));
    assert!(m.take_modem_prepared().is_err());
}
#[test]
fn outer_owner_retains_allocation_evidence_and_quarantines_guard_failure() {
    let mut f = Fixture::new();
    let mut m = shared_modem(&mut f);
    m.prepare().unwrap();
    let r = m.report();
    m.attach_modem(91, lost).unwrap();
    assert_eq!(m.modem_step(), Err(Error::Geometry));
    assert_eq!(m.modem_report().unwrap().submitted, 0);
    assert_eq!(m.report(), r);
    assert_eq!(m.item_physical(), Some(0x8611d030));
    assert!(m.take_modem_prepared().is_err());
}
