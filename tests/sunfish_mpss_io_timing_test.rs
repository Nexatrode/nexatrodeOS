#[path = "../rust/src/sunfish_mpss_io_timing.rs"]
mod timing;

#[test]
fn only_records_inside_dispatch_and_saturates() {
    let mut t = timing::IoTiming::default();
    t.record(0, 99);
    assert_eq!(t.total[0], 0);
    t.enable(true);
    assert!(t.enabled());
    t.record(0, 10);
    t.record(1, 7);
    t.record(0, u64::MAX);
    t.enable(false);
    t.record(1, 20);
    assert_eq!(t.total, [u64::MAX, 7, 0, 0]);
    assert_eq!(t.calls, [2, 1, 0, 0]);
}

#[cfg(feature = "sunfish-tftp-publish-op-observer")]
#[test]
fn detailed_operations_obey_enable_and_saturate_independently() {
    let mut t = timing::IoTiming::default();
    t.record_detail(1, 50);
    assert_eq!(t.detail_total[1], 0);
    t.enable(true);
    t.record(3, 8);
    t.record_detail(1, 5);
    t.record_detail(4, 3);
    t.record_detail(1, u64::MAX);
    t.enable(false);
    t.record_detail(4, 9);
    assert_eq!(t.total[3], 8);
    assert_eq!(t.detail_total, [0, u64::MAX, 0, 0, 3, 0]);
    assert_eq!(t.detail_calls, [0, 2, 0, 0, 1, 0]);
}

#[cfg(feature = "sunfish-tftp-barrier-domain-observer")]
#[test]
fn barrier_domain_samples_keep_partial_failures_and_saturate() {
    let mut t = timing::IoTiming::default();
    t.record_barrier_domains([Some(1), Some(4), Some(5), Some(10)]);
    assert_eq!(t.barrier_domain_calls, [0, 0]);
    t.enable(true);
    let before = t;
    t.record_barrier_domains([Some(1), Some(4), None, None]);
    assert_eq!(t.barrier_domain_observation(&before), [3, u64::MAX]);
    t.record_barrier_domains([Some(10), Some(20), Some(22), Some(29)]);
    assert_eq!(t.barrier_domain_total, [13, 7]);
    assert_eq!(t.barrier_domain_calls, [2, 1]);
    t.record_barrier_domains([Some(0), Some(u64::MAX), None, None]);
    assert_eq!(t.barrier_domain_total[0], u64::MAX);
}
