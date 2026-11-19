//! Time conversion and wake deadlines for the embedded Slint event loops.
use core::time::Duration;

pub fn elapsed(ticks: u64, frequency: u32) -> Duration {
    let hz = u64::from(frequency);
    Duration::new(ticks / hz, ((ticks % hz) * 1_000_000_000 / hz) as u32)
}

pub fn next_wake_delay(animating: bool, timer: Option<Duration>) -> Option<Duration> {
    const FRAME: Duration = Duration::from_nanos(16_666_667);
    if animating {
        Some(timer.map_or(FRAME, |delay| delay.min(FRAME)))
    } else {
        timer
    }
}
