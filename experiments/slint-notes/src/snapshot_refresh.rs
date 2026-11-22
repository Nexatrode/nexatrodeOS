//! Reject a background snapshot if a newer foreground load superseded it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ticket { generation: u64, serial: u64 }

pub struct Refresh { generation: u64, serial: u64, active: Option<Ticket> }

impl Refresh {
    pub const fn new() -> Self { Self { generation: 0, serial: 0, active: None } }
    pub fn start(&mut self) -> Option<Ticket> {
        if self.active.is_some() { return None; }
        self.serial = self.serial.wrapping_add(1);
        let ticket = Ticket { generation: self.generation, serial: self.serial };
        self.active = Some(ticket);
        Some(ticket)
    }
    pub fn invalidate(&mut self) { self.generation = self.generation.wrapping_add(1); }
    pub fn finish(&mut self, ticket: Ticket) -> bool {
        if self.active != Some(ticket) { return false; }
        self.active = None;
        ticket.generation == self.generation
    }
}
