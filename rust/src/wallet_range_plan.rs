//! A single range may be in flight while the preceding verified response is scanned.
//! The download cursor advances by the prefix the daemon actually returned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BlockRange {
    pub start: u64,
    pub count: usize,
}

pub(crate) struct RangePlan {
    next: u64,
    tip: u64,
    limit: usize,
}

/// Bound consecutive productive steps so IPC controls and other apps get a turn.
/// An empty socket with no decoded blocks to scan yields immediately.
pub(crate) fn continue_range_slice(ready: bool, steps: u8, elapsed_us: u64) -> bool {
    ready && steps < 16 && elapsed_us < 3_000
}

impl RangePlan {
    pub fn new(start: u64, tip: u64, limit: usize) -> Self {
        Self {
            next: start,
            tip,
            limit,
        }
    }

    pub fn request(&self) -> Option<BlockRange> {
        let remaining = self.tip.saturating_sub(self.next);
        let count = remaining.min(self.limit as u64) as usize;
        (count > 0).then_some(BlockRange {
            start: self.next,
            count,
        })
    }

    pub fn accept(&mut self, request: BlockRange, received: usize) -> bool {
        if self.request() != Some(request) || received == 0 || received > request.count {
            return false;
        }
        self.next += received as u64;
        true
    }

    pub fn reduce(&mut self, request: BlockRange) -> bool {
        if self.request() != Some(request) || request.count <= 1 {
            return false;
        }
        self.limit = request.count / 2;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn another_ready_step_fits_only_within_the_dispatch_budget() {
        assert!(continue_range_slice(true, 1, 500));
        assert!(!continue_range_slice(false, 1, 500));
        assert!(!continue_range_slice(true, 1, 3_000));
        assert!(!continue_range_slice(true, 16, 10));
    }

    #[test]
    fn short_reply_rebases_lookahead_without_crossing_tip() {
        let mut plan = RangePlan::new(100, 207, 75);
        let first = plan.request().unwrap();
        assert_eq!((first.start, first.count), (100, 75));
        assert!(plan.accept(first, 23));
        let second = plan.request().unwrap();
        assert_eq!((second.start, second.count), (123, 75));
        assert!(!plan.accept(first, 23));
        assert!(plan.accept(second, 75));
        let last = plan.request().unwrap();
        assert_eq!((last.start, last.count), (198, 9));
        assert!(plan.accept(last, 9));
        assert!(plan.request().is_none());
    }

    #[test]
    fn retries_keep_the_same_start_and_oversize_shrinks_the_range() {
        let mut plan = RangePlan::new(u64::MAX - 3, u64::MAX, 75);
        let req = plan.request().unwrap();
        assert!(!plan.accept(req, 0));
        assert!(!plan.accept(req, 4));
        assert_eq!(plan.request(), Some(req));
        assert!(plan.reduce(req));
        let smaller = plan.request().unwrap();
        assert_eq!((smaller.start, smaller.count), (req.start, 1));
        assert!(!plan.reduce(smaller));
        assert!(!plan.accept(req, 3));
        assert!(plan.accept(smaller, 1));
    }
}
