#[path = "../rust/src/pixel_mpss_auth_diagnostic.rs"]
mod diagnostic;

#[test]
fn health_report_keeps_first_failed_predicate() {
    let report = diagnostic::Report::new();
    assert_eq!(report.failure(), diagnostic::Failure::None);
    report.fail(diagnostic::Failure::Domain);
    report.fail(diagnostic::Failure::Receive);
    assert_eq!(report.failure(), diagnostic::Failure::Domain);
}

#[test]
fn each_health_failure_has_a_distinct_stable_screen_value() {
    for (failure, expected) in [
        (diagnostic::Failure::Receive, 1),
        (diagnostic::Failure::Domain, 2),
        (diagnostic::Failure::Preauth, 3),
        (diagnostic::Failure::Rmtfs, 4),
    ] {
        let report = diagnostic::Report::new();
        report.fail(failure);
        assert_eq!(report.failure() as u8, expected);
    }
}

#[test]
fn notification_report_accumulates_all_bits_without_consuming_them() {
    let report = diagnostic::Report::new();
    report.observe(1 << 4);
    report.observe((1 << 5) | (1 << 42));
    assert_eq!(report.seen(), (1 << 4) | (1 << 5) | (1 << 42));
    assert_eq!(report.seen(), (1 << 4) | (1 << 5) | (1 << 42));
}
