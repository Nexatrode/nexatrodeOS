//! Pure shared-endpoint routing; no device access or interrupt acknowledgements.
extern crate self as personal_os_rust_server;
pub mod sunfish_mpss_linux_smp2p {
    #[derive(PartialEq)] pub enum Poll { Observed }
    #[derive(Clone, Copy)] pub struct Peer { pub slave_kernel: Option<u32> }
    pub struct Report { pub ready_seen: bool, pub peer: Option<Peer> }
}
#[allow(dead_code)]
#[path = "../rust/src/pixel_mpss_auth_irq.rs"]
mod policy;

#[test]
fn runtime_retains_glink_and_smp_without_forwarding_them_to_ipa() {
    for ipa in [0, 4, 8, 12] {
        let badge = (1 << 4) | (1 << 5) | ipa;
        let (owned, rest) = policy::split(badge);
        assert_eq!(owned, (1 << 4) | (1 << 5));
        assert_eq!(rest, ipa);
        assert!(policy::monitor_healthy(rest, owned));
    }
}

#[test]
fn runtime_never_hides_watchdog_smmu_or_unknown_faults() {
    for bad in [1, 2, 1 << 7, 1 << 40] {
        let (owned, rest) = policy::split((1 << 4) | bad);
        assert!(!policy::monitor_healthy(rest, owned));
    }
    let (owned, rest) = policy::split((1 << 4) | (1 << 6));
    assert!(!policy::monitor_healthy(rest, owned));
}

#[test]
fn smp_ack_does_not_discard_glink_handoff() {
    use std::sync::atomic::{AtomicU64, Ordering};
    let pending = AtomicU64::new(0);
    let (owned, rest) = policy::split((1 << 4) | (1 << 5) | 4);
    pending.fetch_or(owned, Ordering::AcqRel);
    assert_eq!(rest, 4);
    pending.fetch_and(!(1 << 5), Ordering::AcqRel);
    assert_eq!(pending.swap(0, Ordering::AcqRel), 1 << 4);
    assert_eq!(pending.load(Ordering::Acquire), 0);
}
