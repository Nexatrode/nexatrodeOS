//! Row IDs reference the bounded cache; rows do not retain decoded images.
extern crate alloc;
use alloc::{rc::Rc, vec::Vec};
use core::cell::RefCell;
use crate::photo_cache::PhotoCache;

pub struct PhotoRows<T> {
    cache: Rc<RefCell<PhotoCache<T>>>,
    owner: Option<u64>,
    ids: Vec<u64>,
}

impl<T: Clone> PhotoRows<T> {
    pub fn new(cache: Rc<RefCell<PhotoCache<T>>>, owner: Option<u64>, ids: Vec<u64>) -> Self {
        Self { cache, owner, ids }
    }
    pub fn len(&self) -> usize { self.ids.len() }
    pub fn get(&self, row: usize) -> Option<T> {
        self.cache.borrow_mut().get(self.owner?, *self.ids.get(row)?)
    }
}
