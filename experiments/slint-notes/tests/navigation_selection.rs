use personal_os_slint_notes_spike::navigation_selection::{Completion, Selection};

#[test]
fn only_the_most_recent_requested_chat_can_open() {
    let mut selection = Selection::new();
    selection.request(11, 2);
    let first = selection.start().unwrap();
    selection.request(22, 3);
    assert_eq!(selection.finish(first, true), Completion::Superseded);
    let second = selection.start().unwrap();
    assert_eq!(selection.finish(second, true), Completion::Open { owner: 22, row: 3 });
}

#[test]
fn leaving_the_list_or_a_failed_selection_cannot_open_a_chat() {
    let mut selection = Selection::new();
    selection.request(11, 2);
    let first = selection.start().unwrap();
    selection.cancel();
    assert_eq!(selection.finish(first, true), Completion::Superseded);
    selection.request(22, 3);
    let second = selection.start().unwrap();
    assert_eq!(selection.finish(second, false), Completion::Failed);
    assert!(selection.start().is_none());
}

#[test]
fn empty_or_invalid_owners_are_not_sent_to_the_worker() {
    let mut selection = Selection::new();
    assert!(!selection.request(0, 0));
    assert!(!selection.request(u64::MAX, 0));
    assert!(selection.start().is_none());
}
