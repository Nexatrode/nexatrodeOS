//! A bounded in-memory cache of decoded conversation previews.
extern crate alloc;
use alloc::collections::VecDeque;

struct Entry<T> {
    owner: u64,
    id: u64,
    bytes: usize,
    image: T,
}

pub struct PhotoCache<T> {
    entries: VecDeque<Entry<T>>,
    bytes: usize,
    budget: usize,
    generation: u64,
}

impl<T: Clone> PhotoCache<T> {
    pub fn new(budget: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            bytes: 0,
            budget,
            generation: 0,
        }
    }

    pub fn insert(&mut self, owner: u64, id: u64, bytes: usize, image: T) {
        if owner == 0 || id == 0 || bytes == 0 || bytes > self.budget {
            return;
        }
        if let Some(index) = self
            .entries
            .iter()
            .position(|e| e.owner == owner && e.id == id)
        {
            self.bytes -= self.entries.remove(index).unwrap().bytes;
        }
        while self.bytes > self.budget - bytes {
            let Some(entry) = self.entries.pop_front() else {
                break;
            };
            self.bytes -= entry.bytes;
        }
        self.entries.push_back(Entry {
            owner,
            id,
            bytes,
            image,
        });
        self.bytes += bytes;
    }

    pub fn get(&mut self, owner: u64, id: u64) -> Option<T> {
        let index = self
            .entries
            .iter()
            .position(|e| e.owner == owner && e.id == id)?;
        let entry = self.entries.remove(index)?;
        let image = entry.image.clone();
        self.entries.push_back(entry);
        Some(image)
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
        self.generation = self.generation.wrapping_add(1);
    }
}
