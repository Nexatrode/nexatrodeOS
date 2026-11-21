#[path = "../src/transcript_queue.rs"]
mod transcript_queue;

use transcript_queue::Queue;

#[test]
fn prioritizes_open_chat_and_coalesces_duplicate_requests() {
    let mut queue = Queue::new();
    assert!(queue.request(1));
    assert!(queue.request(4));
    assert!(queue.request(1));
    queue.prioritize(4);
    let first = queue.start().unwrap();
    assert_eq!(first.index(), 4);
    assert!(queue.start().is_none());
    assert!(queue.finish(first));
    let second = queue.start().unwrap();
    assert_eq!(second.index(), 1);
    assert!(queue.finish(second));
    assert!(queue.start().is_none());
}

#[test]
fn discards_an_old_result_after_chat_list_changes() {
    let mut queue = Queue::new();
    queue.request(3);
    let old = queue.start().unwrap();
    queue.invalidate();
    queue.request(3);
    assert!(!queue.finish(old));
    let new = queue.start().unwrap();
    assert_eq!(new.index(), 3);
    assert!(queue.finish(new));
}

#[test]
fn changes_during_active_read_request_a_fresh_result() {
    let mut queue = Queue::new();
    queue.request(2);
    let first = queue.start().unwrap();
    queue.changed(2);
    assert!(!queue.finish(first));
    let again = queue.start().unwrap();
    assert_eq!(again.index(), 2);
    assert!(queue.finish(again));
    assert!(queue.start().is_none());
}

#[test]
fn repeated_reads_do_not_starve_an_active_job() {
    let mut queue = Queue::new();
    queue.request(2);
    let first = queue.start().unwrap();
    queue.request(2);
    assert!(queue.finish(first));
    assert!(queue.start().is_none());
}

#[test]
fn rejects_out_of_range_and_wrong_completion() {
    let mut queue = Queue::new();
    assert!(!queue.request(16));
    queue.request(0);
    let first = queue.start().unwrap();
    assert!(queue.finish(first));
    queue.request(1);
    let second = queue.start().unwrap();
    assert!(!queue.finish(first));
    assert!(queue.finish(second));
}

#[test]
fn associates_results_with_the_owner_from_the_visible_snapshot() {
    use transcript_queue::owners_from_snapshot;
    let owners = owners_from_snapshot("CHAT Alice\nCHAT Bob\nCHAT_HANDLE 0 0000000000000011\nCHAT_HANDLE 1 0000000000000022\n");
    assert_eq!(owners[0], 17);
    assert_eq!(owners[1], 34);
    assert_eq!(owners[2], 0);
    assert_eq!(owners_from_snapshot("CHAT_HANDLE 0 invalid\nCHAT_HANDLE 17 0000000000000001")[0], 0);
}

#[test]
fn maps_existing_chat_state_by_owner_when_rows_reorder() {
    use transcript_queue::remap_by_owner;
    let mut old = [0u64; 16]; old[0] = 11; old[1] = 22;
    let mut new = [0u64; 16]; new[0] = 22; new[1] = 33; new[2] = 11;
    let map = remap_by_owner(&old, &new);
    assert_eq!(map[0], Some(1));
    assert_eq!(map[1], None);
    assert_eq!(map[2], Some(0));
    assert_eq!(map[3], None);
}
