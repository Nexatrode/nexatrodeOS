//! Closed native register/badge policy; does not mint hardware authority.
use personal_os_rust_server::{
    sunfish_ipa_linux_config::Snapshot,
    sunfish_ipa_linux_receive::{self as rx, Space, Word},
    sunfish_ipa_linux_receive_dispatch::Notice,
    sunfish_ipa_linux_receive_runtime::Register,
    sunfish_ipa_linux_receive_start::GuardState,
};
/// Metadata captured at the first failed guard, using values already read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct GuardFailure {
    pub stage: u32,
    pub space: u32,
    pub offset: u32,
    pub observed: u32,
    pub expected: u32,
    pub mask: u32,
}
pub(super) fn first_guard_failure(slot: &mut Option<GuardFailure>, failure: GuardFailure) {
    if slot.is_none() { *slot = Some(failure); }
}
/// Diagnostic summary of the snapshot the guard already read; no MMIO.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ConfigTrace {
    pub actual: [u32; 9],
    pub mismatch: u32,
    pub clkon_baseline: u32,
    pub last_good_clkon: u32,
    pub successful_snapshots: u32,
}
pub(super) fn config_trace(expected: Snapshot, actual: Snapshot, last_good_clkon: Option<u32>, successful_snapshots: u32) -> ConfigTrace {
    let mut mismatch = 0u32;
    for index in 0..actual.config.len() {
        if actual.config[index] != expected.config[index] { mismatch |= 1 << index; }
    }
    ConfigTrace { actual: actual.config, mismatch, clkon_baseline: expected.config[2],
        last_good_clkon: last_good_clkon.unwrap_or(u32::MAX), successful_snapshots }
}
pub(super) fn startup(word: Word) -> bool {
    match (word.space, word.offset) {
        (Space::Gsi, 0x1f0b8) => word.value == 3,
        (Space::Gsi, 0x1f0a0) => matches!(word.value, 4 | u32::MAX),
        (Space::Gsi, 0x1f098) => matches!(word.value, 0 | 4),
        (Space::Gsi, 0x1f088) => matches!(word.value, 0 | 0x4c | 0x4d),
        (Space::Gsi, 0x1f008) => word.value == 0x01000002,
        (Space::Gsi, 0x1e010) => rx::Plan::new().is_ok_and(|p| {
            (0..rx::DOORBELL_COUNT).any(|n| p.doorbell(n).is_ok_and(|d| d.word.value == word.value))
        }),
        (Space::Ipa, 0x3034) => word.value == 0x100,
        (Space::Ipa, 0x300c) => matches!(word.value, 0 | 0x400c),
        (Space::Ipa, 0x48) => word.value == rx::DEFAULT_ROUTE.value,
        _ => false,
    }
}
pub(super) fn receive(register: Register, value: u32) -> bool {
    match register {
        Register::IeobMask => matches!(value, 1 | 3),
        Register::IeobClear => value == 2,
        Register::EventDoorbell => (0x80008000..0x80009000).contains(&value) && value % 16 == 0,
        Register::ChannelDoorbell => (0x80009000..0x8000a000).contains(&value) && value % 16 == 0,
    }
}
pub(super) fn notice(badge: u64) -> Notice {
    match badge {
        4 => Notice::Ipa,
        8 => Notice::Gsi,
        12 => Notice::Both,
        _ => Notice::Fault,
    }
}
pub(super) fn config(expected: Snapshot, mut actual: Snapshot, state: GuardState) -> bool {
    if expected.guards[0] != 0x08090811
        || expected.guards[1] != 0x415
        || expected.guards[5] != 0xe4046b59
        || expected.guards[2..5] != [0, 0, 0]
    {
        return false;
    }
    let mut wanted = expected;
    wanted.guards[3] = state.ipa_mask();
    wanted.guards[4] = 1;
    // Only endpoint 8 can aggregate after this owner's receive enable.
    if state.rx_running() && state.ipa_mask() == 0x400c && state.suspend_mask() == 0x100 {
        if actual.guards[2] & !0x100 != 0 {
            return false;
        }
        actual.guards[2] = 0;
    }
    actual == wanted
}
/// Experimental exact pair observed in the working Linux reference after
/// MPSS/WLAN startup and IPA runtime resume. The caller must establish a
/// same-generation Q6-ready receipt and retain all other runtime guards.
pub(super) fn post_modem_config(expected: Snapshot, mut actual: Snapshot, state: GuardState, ready: bool) -> bool {
    if !ready || !state.rx_running() || !state.route_set()
        || expected.config[2] != 0x30000000 || expected.config[4] != 0x08
        || actual.config[2] != 0 || actual.config[4] != 0x78 {
        return false;
    }
    actual.config[2] = expected.config[2];
    actual.config[4] = expected.config[4];
    config(expected, actual, state)
}
/// Explain a failed config check using only its existing snapshot.
pub(super) fn config_failure(expected: Snapshot, mut actual: Snapshot, state: GuardState) -> GuardFailure {
    let unknown = GuardFailure { stage: 3, space: u32::MAX, offset: u32::MAX,
        observed: u32::MAX, expected: u32::MAX, mask: 0 };
    if expected.guards[0] != 0x08090811 || expected.guards[1] != 0x415
        || expected.guards[5] != 0xe4046b59 || expected.guards[2..5] != [0, 0, 0] { return unknown; }
    for (index, spec) in personal_os_rust_server::sunfish_ipa_linux_config::PROFILE.into_iter().enumerate() {
        if actual.config[index] != expected.config[index] {
            return GuardFailure { stage: 3, space: 0, offset: spec.offset as u32,
                observed: actual.config[index], expected: expected.config[index], mask: u32::MAX };
        }
    }
    let mut wanted = expected;
    wanted.guards[3] = state.ipa_mask();
    wanted.guards[4] = 1;
    if state.rx_running() && state.ipa_mask() == 0x400c && state.suspend_mask() == 0x100 {
        if actual.guards[2] & 0xfffffeff != 0 {
            return GuardFailure { stage: 3, space: 0, offset: 0xb4, observed: actual.guards[2], expected: 0, mask: !0x100 };
        }
        actual.guards[2] = 0;
    }
    let regs = [(0, 0x210), (0, 0x54), (0, 0xb4), (0, 0x300c), (1, 0x1f000), (1, 0x1f040)];
    for (index, (space, offset)) in regs.into_iter().enumerate() {
        if actual.guards[index] != wanted.guards[index] {
            return GuardFailure { stage: 3, space, offset, observed: actual.guards[index], expected: wanted.guards[index], mask: u32::MAX };
        }
    }
    unknown
}
pub(super) fn rx_state(state: GuardState, observed: u32) -> bool {
    if state.rx_running() {
        observed == 2
    } else if state.gsi_channel_mask() == 4 {
        matches!(observed, 1 | 2)
    } else {
        observed == 1
    }
}
