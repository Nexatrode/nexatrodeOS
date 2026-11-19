//! Closed routing policy for the shared notification endpoint. This supplies
//! no IRQ authority or ready receipt; the root retains the actual owners.
use personal_os_rust_server::sunfish_mpss_linux_smp2p as smp;
pub(super) const SMP_CHANNEL: usize = 5;
pub(super) const WDOG_CHANNEL: usize = 6;
pub(super) const SMP: u64 = 1 << SMP_CHANNEL;
pub(super) const WDOG: u64 = 1 << WDOG_CHANNEL;
#[cfg(feature = "sunfish-native-startup-mpss-runtime")]
pub(super) const GLINK: u64 = 1 << 4;
#[cfg(feature = "sunfish-native-startup-mpss-runtime")]
pub(super) const OWNED: u64 = SMP | WDOG | GLINK;
#[cfg(not(feature = "sunfish-native-startup-mpss-runtime"))]
pub(super) const OWNED: u64 = SMP | WDOG;
pub(super) fn split(badge: u64) -> (u64, u64) {
    (badge & OWNED, badge & !OWNED)
}
pub(super) fn monitor_healthy(rest: u64, pending: u64) -> bool {
    rest & !0xf == 0 && rest & 3 == 0 && pending & WDOG == 0
}
pub(super) fn ack_sample(pending: u64, poll: smp::Poll) -> bool {
    // Keep the delivered IRQ pending across a version downgrade. The next
    // stable negotiated sample may be the only opportunity to observe ready.
    pending & SMP != 0 && poll == smp::Poll::Observed
}
pub(super) fn ready_sample(report: &smp::Report) -> bool {
    // A historical ready bit followed by an unrelated SMP2P IRQ is insufficient.
    report.ready_seen
        && report
            .peer
            .and_then(|p| p.slave_kernel)
            .is_some_and(|v| v & 2 != 0)
}
