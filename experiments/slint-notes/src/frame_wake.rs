//! One-shot frame deadline shared by a Slint PD and its phone's input poller.
use core::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

/// A zero request disarms the poller. Only the Slint PD writes this word.
pub fn publish(request: &AtomicU64, now_ticks: u64, frequency_hz: u32, delay: Option<Duration>) {
    let next = delay.map_or(0, |delay| {
        let ticks = (delay.as_nanos() * u128::from(frequency_hz)).div_ceil(1_000_000_000);
        // Even a zero-delay rearm needs a deadline distinct from one just delivered.
        let deadline = now_ticks.saturating_add(u64::try_from(ticks.max(1)).unwrap_or(u64::MAX));
        let pending = request.load(Ordering::Relaxed);
        if pending > now_ticks {
            pending.min(deadline)
        } else {
            deadline
        }
    });
    request.store(next, Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_while_animating_never_postpones_a_pending_frame() {
        let request = AtomicU64::new(0);
        let frame = Some(Duration::from_millis(16));
        publish(&request, 10_000, 19_200_000, frame);
        assert_eq!(request.load(Ordering::Relaxed), 317_200);
        publish(&request, 50_000, 19_200_000, frame);
        assert_eq!(request.load(Ordering::Relaxed), 317_200);
        publish(&request, 50_000, 19_200_000, Some(Duration::from_nanos(70)));
        assert_eq!(request.load(Ordering::Relaxed), 50_002);
    }

    #[test]
    fn expired_frame_can_rearm_immediately_and_idle_cancels() {
        let request = AtomicU64::new(123);
        publish(&request, 123, 24_000_000, Some(Duration::ZERO));
        assert_eq!(request.load(Ordering::Relaxed), 124);
        publish(&request, 123, 24_000_000, None);
        assert_eq!(request.load(Ordering::Relaxed), 0);
    }
}
