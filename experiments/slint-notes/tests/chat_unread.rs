#[path = "../src/chat_unread.rs"]
mod chat_unread;

#[test]
fn remembers_incoming_messages_arriving_before_baseline() {
    let optimistic = "Alice: hello\nNexatrodeOS: reply\nAlice: again\n";
    assert_eq!(chat_unread::pending_before_baseline(optimistic, false), 2);
    assert_eq!(chat_unread::pending_before_baseline(optimistic, true), 0);
}

#[test]
fn counts_only_complete_new_lines_after_read_position() {
    let text = "Alice: one\nNexatrodeOS: hi\nAlice: two\n";
    assert_eq!(chat_unread::unread_incoming(text, 11), 1);
    assert_eq!(chat_unread::unread_incoming(text, text.len()), 0);
}
