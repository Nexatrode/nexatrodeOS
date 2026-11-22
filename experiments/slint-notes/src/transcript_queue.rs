//! Scheduling for bounded background transcript requests.

const SLOTS: usize = 16;

pub fn remap_by_owner(old: &[u64; SLOTS], new: &[u64; SLOTS]) -> [Option<usize>; SLOTS] {
    core::array::from_fn(|index| {
        let owner = new[index];
        (owner != 0).then(|| old.iter().position(|&previous| previous == owner)).flatten()
    })
}

pub fn owners_from_snapshot(snapshot: &str) -> [u64; SLOTS] {
    let mut owners = [0; SLOTS];
    for line in snapshot.lines() {
        let Some((index, owner)) = line.strip_prefix("CHAT_HANDLE ").and_then(|line| line.split_once(' ')) else { continue; };
        let Some(index) = index.parse::<usize>().ok().filter(|&index| index < SLOTS) else { continue; };
        if owner.len() == 16 {
            if let Ok(owner) = u64::from_str_radix(owner, 16) {
                if owner != u64::MAX { owners[index] = owner; }
            }
        }
    }
    owners
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ticket {
    index: usize,
    generation: u64,
    revision: u64,
    serial: u64,
}

impl Ticket {
    pub fn index(self) -> usize { self.index }
}

pub struct Queue {
    pending: u16,
    preferred: Option<usize>,
    active: Option<Ticket>,
    generation: u64,
    serial: u64,
    revisions: [u64; SLOTS],
}

impl Queue {
    pub const fn new() -> Self {
        Self { pending: 0, preferred: None, active: None, generation: 0, serial: 0, revisions: [0; SLOTS] }
    }

    pub fn request(&mut self, index: usize) -> bool {
        if index >= SLOTS { return false; }
        if self.active.is_some_and(|active| active.index == index && active.generation == self.generation) { return true; }
        self.pending |= 1 << index;
        true
    }

    pub fn changed(&mut self, index: usize) -> bool {
        if index >= SLOTS { return false; }
        self.revisions[index] = self.revisions[index].wrapping_add(1);
        self.pending |= 1 << index;
        true
    }

    pub fn prioritize(&mut self, index: usize) {
        if index < SLOTS { self.preferred = Some(index); }
    }

    pub fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.pending = 0;
        self.preferred = None;
    }

    pub fn start(&mut self) -> Option<Ticket> {
        if self.active.is_some() || self.pending == 0 { return None; }
        let index = self.preferred.filter(|i| self.pending & (1 << i) != 0)
            .unwrap_or(self.pending.trailing_zeros() as usize);
        self.pending &= !(1 << index);
        self.serial = self.serial.wrapping_add(1);
        let ticket = Ticket { index, generation: self.generation, revision: self.revisions[index], serial: self.serial };
        self.active = Some(ticket);
        Some(ticket)
    }

    pub fn finish(&mut self, ticket: Ticket) -> bool {
        if self.active != Some(ticket) { return false; }
        self.active = None;
        ticket.generation == self.generation && ticket.revision == self.revisions[ticket.index]
    }
}
