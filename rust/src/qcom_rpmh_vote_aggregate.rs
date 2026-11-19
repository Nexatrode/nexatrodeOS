//! Bounded, pure model of RPMh VRM regulator-device vote aggregation.
//!
//! The regulator core first combines each regulator device's consumer requests
//! (including summed load for mode selection). For each shared RPMh resource,
//! the pinned provider then max-aggregates each valid register across the
//! regulator devices associated with that resource, separately for active and
//! sleep sets. This module models that latter calculation for one resource at
//! a time; its inputs are already-resolved regulator-device requests, not raw
//! consumer requests. It performs no I/O, discovery, or authentication, and is
//! not an RPMh broker. A caller must group by the exact RPMh resource and
//! provide its complete trusted regulator-device vote set.

pub const MAX_CLIENTS: usize = 16;
pub const REGISTER_COUNT: usize = 4;
pub const VOLTAGE_REGISTER: usize = 0;
pub const ENABLE_REGISTER: usize = 1;
pub const MODE_REGISTER: usize = 2;
pub const HEADROOM_REGISTER: usize = 3;

const VALID_REGISTER_MASK: u8 = (1 << REGISTER_COUNT) - 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// One already-resolved regulator-device request associated with the RPMh
/// resource being aggregated. Do not store raw consumer voltage/load requests
/// here; combine those first using regulator-core semantics.
pub struct ClientVote {
    pub set_active: bool,
    pub set_sleep: bool,
    pub valid_mask: u8,
    pub registers: [u32; REGISTER_COUNT],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Aggregate {
    pub active_valid_mask: u8,
    pub active_registers: [u32; REGISTER_COUNT],
    pub sleep_valid_mask: u8,
    pub sleep_registers: [u32; REGISTER_COUNT],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    TooManyClients,
    InvalidRegisterMask,
}

/// Calculate active and sleep max votes for one complete bounded set of
/// regulator devices that share the same RPMh resource.
///
/// Invalid bits are rejected even for currently inactive clients so a stale or
/// malformed record cannot become effective after a later state transition.
pub fn aggregate(votes: &[ClientVote]) -> Result<Aggregate, Error> {
    if votes.len() > MAX_CLIENTS {
        return Err(Error::TooManyClients);
    }
    if votes
        .iter()
        .any(|vote| vote.valid_mask & !VALID_REGISTER_MASK != 0)
    {
        return Err(Error::InvalidRegisterMask);
    }

    let mut result = Aggregate {
        active_valid_mask: 0,
        active_registers: [0; REGISTER_COUNT],
        sleep_valid_mask: 0,
        sleep_registers: [0; REGISTER_COUNT],
    };

    for vote in votes {
        if vote.set_active {
            merge_vote(
                &mut result.active_valid_mask,
                &mut result.active_registers,
                vote,
            );
        }
        if vote.set_sleep {
            merge_vote(
                &mut result.sleep_valid_mask,
                &mut result.sleep_registers,
                vote,
            );
        }
    }

    Ok(result)
}

fn merge_vote(valid_mask: &mut u8, registers: &mut [u32; REGISTER_COUNT], vote: &ClientVote) {
    for (index, value) in registers.iter_mut().enumerate() {
        let bit = 1 << index;
        if vote.valid_mask & bit != 0 {
            *value = if *valid_mask & bit != 0 {
                (*value).max(vote.registers[index])
            } else {
                vote.registers[index]
            };
            *valid_mask |= bit;
        }
    }
}
