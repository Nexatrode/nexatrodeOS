use personal_os_slint_notes_spike::snapshot_refresh::Refresh;

#[test]
fn a_refresh_started_before_a_newer_snapshot_cannot_replace_it() {
    let mut refresh = Refresh::new();
    let old = refresh.start().unwrap();
    assert!(refresh.start().is_none());
    refresh.invalidate();
    assert!(!refresh.finish(old));
    let current = refresh.start().unwrap();
    assert!(refresh.finish(current));
}

#[test]
fn a_stale_completion_does_not_consume_the_active_request() {
    let mut refresh = Refresh::new();
    let first = refresh.start().unwrap();
    assert!(refresh.finish(first));
    let second = refresh.start().unwrap();
    assert!(!refresh.finish(first));
    assert!(refresh.finish(second));
}
