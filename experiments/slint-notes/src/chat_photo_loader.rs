//! Bounded, resumable reads for saved chat-photo previews.
extern crate alloc;
use alloc::{collections::VecDeque, vec::Vec};

pub const PHOTO_CHUNK: usize = 3072;
pub const PHOTO_MAX: usize = 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum PhotoOutcome {
    Pending,
    Complete { row: usize, bytes: Vec<u8> },
    Failed { row: usize },
}

#[derive(Default)]
pub struct PhotoLoader {
    stamp: u64,
    chat: u64,
    jobs: VecDeque<PhotoJob>,
}

struct PhotoJob {
    row: usize,
    id: u64,
    visible: bool,
    total: Option<usize>,
    bytes: Vec<u8>,
}

impl PhotoLoader {
    pub fn set_visible(&mut self, row: usize, visible: bool) {
        if let Some(job) = self.jobs.iter_mut().find(|job| job.row == row) {
            if job.visible && !visible {
                // A long chat must not retain a partial megabyte for every
                // photo the user briefly scrolled past.
                job.bytes = Vec::new();
                job.total = None;
            }
            job.visible = visible;
        }
    }
    /// Replacing a transcript discards its partial reads and row indices.
    pub fn reset(&mut self, stamp: u64, chat: u64, photos: &[(usize, u64)]) {
        self.stamp = stamp;
        self.chat = chat;
        self.jobs.clear();
        self.jobs.extend(
            photos
                .iter()
                .filter(|(_, id)| *id != 0)
                .map(|&(row, id)| PhotoJob {
                    row,
                    id,
                    visible: false,
                    total: None,
                    bytes: Vec::new(),
                }),
        );
    }

    pub fn matches(&self, stamp: u64, chat: u64) -> bool {
        self.stamp == stamp && self.chat == chat
    }

    /// Transfer one visible job to the background reader.
    pub fn claim_visible(&mut self) -> Option<(u64, u64, usize)> {
        let index = self.jobs.iter().position(|job| job.visible)?;
        let job = self.jobs.remove(index)?;
        Some((self.chat, job.id, job.row))
    }

    /// At most one bounded vault request is needed before returning to input.
    pub fn next_request(&self) -> Option<(u64, u64, usize)> {
        self.jobs
            .iter()
            .find(|job| job.visible)
            .map(|job| (self.chat, job.id, job.bytes.len() / PHOTO_CHUNK))
    }

    pub fn accept(&mut self, chunk: Option<(usize, &[u8])>) -> PhotoOutcome {
        let index = self
            .jobs
            .iter()
            .position(|job| job.visible)
            .expect("photo response without a request");
        let job = &mut self.jobs[index];
        let valid = chunk.filter(|(total, payload)| {
            (1..=PHOTO_MAX).contains(total)
                && job.total.is_none_or(|prior| prior == *total)
                && job.bytes.len() < *total
                && payload.len() == PHOTO_CHUNK.min(*total - job.bytes.len())
        });
        let Some((total, payload)) = valid else {
            return PhotoOutcome::Failed {
                row: self.jobs.remove(index).unwrap().row,
            };
        };
        job.total = Some(total);
        job.bytes.extend_from_slice(payload);
        if job.bytes.len() == total {
            let job = self.jobs.remove(index).unwrap();
            PhotoOutcome::Complete {
                row: job.row,
                bytes: job.bytes,
            }
        } else {
            PhotoOutcome::Pending
        }
    }
}
