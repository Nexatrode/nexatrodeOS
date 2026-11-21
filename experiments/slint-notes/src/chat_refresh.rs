//! Coalesce input-triggered chat refreshes while a frame is deferred.
#[derive(Default)]
pub struct ChatRefresh { pending: bool }
impl ChatRefresh {
    /// Return true only when the caller can refresh immediately.
    pub fn request(&mut self, batching: bool) -> bool {
        self.pending = batching;
        !batching
    }
    pub fn pending(&self) -> bool { self.pending }
    pub fn take(&mut self) -> bool { core::mem::take(&mut self.pending) }
}
