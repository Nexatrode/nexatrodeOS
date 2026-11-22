#[path = "../src/frame_clock.rs"]
mod frame_clock;

use core::time::Duration;
use frame_clock::{elapsed, next_wake_delay};

#[test]
fn counter_time_starts_at_zero_and_advances_at_board_frequency() {
    let origin = u64::MAX - 700;
    for hz in [19_200_000, 24_000_000, 62_500_000] {
        assert_eq!(elapsed(0, hz), Duration::ZERO);
        let now = origin.wrapping_add(hz as u64 * 3 / 2);
        assert_eq!(
            elapsed(now.wrapping_sub(origin), hz),
            Duration::from_millis(1500)
        );
    }
    assert_eq!(elapsed(19_201, 19_200_000), Duration::from_nanos(1_000_052));
    assert_eq!(
        elapsed(24_000_000 * 86400 * 365, 24_000_000).as_secs(),
        31_536_000
    );
}

#[test]
fn wake_only_for_pending_work_and_keep_the_earliest_deadline() {
    let frame = Duration::from_nanos(16_666_667);
    assert_eq!(next_wake_delay(false, None), None);
    assert_eq!(next_wake_delay(true, None), Some(frame));
    assert_eq!(
        next_wake_delay(false, Some(Duration::from_secs(1))),
        Some(Duration::from_secs(1))
    );
    assert_eq!(
        next_wake_delay(true, Some(Duration::from_millis(2))),
        Some(Duration::from_millis(2))
    );
    assert_eq!(
        next_wake_delay(true, Some(Duration::from_secs(4))),
        Some(frame)
    );
}
