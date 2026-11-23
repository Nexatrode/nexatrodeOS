//! Defer presentation while the live router has work; retain a settled redraw.
//! This gate never changes protocol state, dispatch, or fault handling.
#[derive(Default)]
pub(super) struct Gate {
    dirty: bool,
    pub(super) deferred: u32,
    ticks: u64,
    unknown: bool,
}
impl Gate {
    fn defer(&mut self) { self.dirty = true; self.deferred = self.deferred.saturating_add(1); }
    pub(super) fn progress(&mut self, error: u32, busy: bool) -> bool {
        if error == 0 && busy { self.defer(); false } else { true }
    }
    pub(super) fn full(&mut self, error: u32, busy: bool, requested: bool) -> bool {
        if error != 0 { self.dirty = false; return true; }
        if busy { if requested { self.defer(); } return false; }
        requested | core::mem::take(&mut self.dirty)
    }
    /// Clip actual rendered spans to the exact post-queue / first-writer window.
    /// These wall times enclose rendering and must not be added to outer work.
    pub(super) fn record(&mut self, queue: Option<u64>, first: Option<u64>, start: u64, end: u64) {
        let Some(queue) = queue else { return; };
        if end < start || first.is_some_and(|at| at < queue) { self.unknown = true; return; }
        let begin = start.max(queue); let end = end.min(first.unwrap_or(end));
        if end > begin { self.ticks = self.ticks.saturating_add(end - begin); }
    }
    pub(super) fn ticks(&self) -> Option<u64> {
        (!self.unknown && self.ticks != u64::MAX).then_some(self.ticks)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn busy_repaints_keep_one_settled_redraw_and_never_consume_dispatch() {
        let mut g=Gate::default();assert!(!g.full(0,false,false));
        assert!(!g.full(0,true,true));assert!(!g.full(0,true,false));assert!(!g.progress(0,true));assert_eq!(g.deferred,2);
        assert!(g.progress(0,false)); // A footer update cannot consume full-panel debt.
        assert!(g.full(0,false,false));assert!(!g.full(0,false,false));assert!(g.full(0,false,true));
        assert!(!g.full(0,true,true));assert!(g.full(1,true,false));assert!(!g.full(0,false,false));assert!(g.progress(1,true));
    }
    #[test] fn clipped_paint_spans_do_not_invent_prequeue_or_postwriter_cost() {
        let mut g=Gate::default();g.record(None,None,0,99);assert_eq!(g.ticks(),Some(0));
        g.record(Some(100),None,90,120);g.record(Some(100),Some(150),120,170);g.record(Some(100),Some(150),170,200);assert_eq!(g.ticks(),Some(50));
        g.record(Some(100),Some(90),200,210);assert_eq!(g.ticks(),None);
        let mut g=Gate::default();g.deferred=u32::MAX;g.progress(0,true);assert_eq!(g.deferred,u32::MAX);g.record(Some(0),None,0,u64::MAX);assert_eq!(g.ticks(),None);
    }
}
