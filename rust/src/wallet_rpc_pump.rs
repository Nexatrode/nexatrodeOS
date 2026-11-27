//! One outstanding short RPC poll alarm; request backoff and bulk own their alarms.
pub const POLL_NS: u64 = 2_000_000;

#[derive(Default)]
pub struct PollTimer { armed: bool }
impl PollTimer {
    pub const fn new() -> Self { Self { armed: false } }
    pub fn fired(&mut self) { self.armed = false; }
    pub fn next(&mut self, polling: bool) -> Option<u64> {
        if !polling { self.armed = false; return None; }
        if self.armed { return None; }
        self.armed = true;
        Some(POLL_NS)
    }
}

pub struct ConnectDeadline { until: u64, reopen_at: u64 }
impl ConnectDeadline {
    pub const fn new() -> Self { Self { until: 0, reopen_at: 0 } }
    pub fn start(&mut self, now_ms: u64) {
        self.until=now_ms.saturating_add(30_000);
        self.reopen_at=now_ms.saturating_add(5_000);
    }
    pub fn expired(&self,now_ms:u64)->bool { self.until!=0 && now_ms>=self.until }
    pub fn reopen(&mut self,now_ms:u64)->bool {
        if self.reopen_at==0 || now_ms<self.reopen_at { return false; }
        self.reopen_at=now_ms.saturating_add(5_000);true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incoming_wakes_cannot_postpone_an_outstanding_poll() {
        let mut t=PollTimer::new();assert_eq!(t.next(true),Some(POLL_NS));
        for _ in 0..100 { assert_eq!(t.next(true),None); }
        t.fired();assert_eq!(t.next(true),Some(POLL_NS));
    }
    #[test]
    fn backoff_bulk_and_idle_are_not_overridden() {
        let mut t=PollTimer::new();assert_eq!(t.next(true),Some(POLL_NS));
        for _ in 0..3 { assert_eq!(t.next(false),None);t.fired();assert_eq!(t.next(false),None); }
        assert_eq!(t.next(true),Some(POLL_NS));
    }
    #[test]
    fn fast_wakes_do_not_shorten_connect_or_reopen_deadlines() {
        let mut d=ConnectDeadline::new();d.start(100);
        for _ in 0..10_000 { assert!(!d.expired(101));assert!(!d.reopen(101)); }
        assert!(!d.expired(5_100));assert!(d.reopen(5_100));assert!(!d.reopen(5_100));
        assert!(!d.expired(30_099));assert!(d.expired(30_100));
    }
}
