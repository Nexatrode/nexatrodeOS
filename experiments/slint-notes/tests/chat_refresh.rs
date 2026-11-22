#[path = "../src/chat_refresh.rs"]
mod chat_refresh;
use chat_refresh::ChatRefresh;

#[test]
fn queued_taps_coalesce_into_one_refresh_after_the_frame() {
    let mut refresh = ChatRefresh::default();
    assert!(!refresh.request(true));
    assert!(!refresh.request(true));
    assert!(refresh.pending());
    assert!(refresh.take());
    assert!(!refresh.pending());
    assert!(!refresh.take());
}

#[test]
fn directly_dispatched_input_refreshes_without_waiting_for_a_timer() {
    let mut refresh = ChatRefresh::default();
    assert!(refresh.request(false));
    assert!(!refresh.pending());
}

#[test]
fn a_direct_refresh_supersedes_an_older_queued_request() {
    let mut refresh = ChatRefresh::default();
    assert!(!refresh.request(true));
    assert!(refresh.request(false));
    assert!(!refresh.take());
}
