#[path = "../src/photo_cache.rs"]
mod photo_cache;
#[path = "../src/photo_rows.rs"]
mod photo_rows;
use photo_cache::PhotoCache;
use photo_rows::PhotoRows;
use std::{cell::RefCell, rc::Rc};

#[test]
fn offscreen_row_does_not_keep_evicted_preview_alive() {
    let cache = Rc::new(RefCell::new(PhotoCache::new(4)));
    let image = Rc::new(vec![0u8; 4]);
    let weak = Rc::downgrade(&image);
    cache.borrow_mut().insert(7, 11, 4, image.clone());
    let rows = PhotoRows::new(cache.clone(), Some(7), vec![11, 12]);
    let visible = rows.get(0).unwrap();
    drop(image);
    cache.borrow_mut().insert(7, 12, 4, Rc::new(vec![0u8; 4]));
    assert!(rows.get(0).is_none());
    assert!(weak.upgrade().is_some(), "the visible row still owns its image");
    drop(visible);
    assert!(weak.upgrade().is_none(), "an offscreen row must not own its image");
}
