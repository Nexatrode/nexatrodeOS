//! Coalesce navigation requests while the Messages service is busy.
//! A completed request opens a view only if it is still the newest request.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ticket { pub owner: u64, pub row: usize, generation: u64 }

#[derive(Debug, PartialEq, Eq)]
pub enum Completion { Open { owner: u64, row: usize }, Failed, Superseded }

pub struct Selection {
    generation: u64,
    pending: Option<Ticket>,
    active: Option<Ticket>,
}

impl Selection {
    pub const fn new() -> Self { Self { generation: 0, pending: None, active: None } }

    pub fn request(&mut self, owner: u64, row: usize) -> bool {
        if owner == 0 || owner == u64::MAX { return false; }
        self.generation = self.generation.wrapping_add(1);
        self.pending = Some(Ticket { owner, row, generation: self.generation });
        true
    }

    pub fn cancel(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.pending = None;
    }

    pub fn start(&mut self) -> Option<Ticket> {
        if self.active.is_some() { return None; }
        let ticket = self.pending.take()?;
        self.active = Some(ticket);
        Some(ticket)
    }

    pub fn finish(&mut self, ticket: Ticket, success: bool) -> Completion {
        if self.active != Some(ticket) { return Completion::Superseded; }
        self.active = None;
        if self.generation != ticket.generation { return Completion::Superseded; }
        if success { Completion::Open { owner: ticket.owner, row: ticket.row } }
        else { Completion::Failed }
    }
}
