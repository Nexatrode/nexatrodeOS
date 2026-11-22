#[path = "../src/photo_cache.rs"]
mod photo_cache;
use photo_cache::PhotoCache;

#[test]
fn entries_are_isolated_by_owner_and_id() {
    let mut cache = PhotoCache::new(8);
    cache.insert(1, 2, 4, "first");
    assert_eq!(cache.get(1, 2), Some("first"));
    assert_eq!(cache.get(2, 2), None);
    assert_eq!(cache.get(1, 3), None);
}

#[test]
fn least_recently_used_entries_are_evicted_within_budget() {
    let mut cache = PhotoCache::new(8);
    cache.insert(1, 1, 4, "one");
    cache.insert(1, 2, 4, "two");
    assert_eq!(cache.get(1, 1), Some("one"));
    cache.insert(1, 3, 4, "three");
    assert_eq!(cache.get(1, 2), None);
    assert_eq!(cache.get(1, 1), Some("one"));
    assert_eq!(cache.get(1, 3), Some("three"));
    cache.clear();
    assert_eq!(cache.get(1, 1), None);
}

#[test]
fn oversize_and_replaced_entries_never_exceed_the_budget() {
    let mut cache = PhotoCache::new(8);
    cache.insert(1, 1, 4, "old");
    cache.insert(1, 1, 7, "new");
    assert_eq!(cache.get(1, 1), Some("new"));
    cache.insert(1, 2, 9, "too-big");
    assert_eq!(cache.get(1, 2), None);
    assert_eq!(cache.get(1, 1), Some("new"));
}

#[test]
fn clearing_invalidates_inflight_generation() {
    let mut cache = PhotoCache::<u8>::new(8);
    let before = cache.generation();
    cache.clear();
    assert_ne!(before, cache.generation());
}
