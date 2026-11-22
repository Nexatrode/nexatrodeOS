//! Bounded receive results identify a chat by stable owner, not its row.
const HEADER: usize = 24;
const MAX_TEXT: usize = 2048;

pub struct Event<'a> { pub status: u64, pub owner: u64, pub text: &'a str }

pub fn encode(out: &mut [u8], status: u64, owner: u64, text: &str) -> Option<usize> {
    if (status == 2 && owner == 0) || text.len() > MAX_TEXT { return None; }
    let end = HEADER.checked_add(text.len())?;
    if end > out.len() { return None; }
    out[..8].copy_from_slice(&status.to_le_bytes());
    out[8..16].copy_from_slice(&owner.to_le_bytes());
    out[16..24].copy_from_slice(&(text.len() as u64).to_le_bytes());
    out[HEADER..end].copy_from_slice(text.as_bytes());
    Some(end)
}

pub fn decode(bytes: &[u8]) -> Option<Event<'_>> {
    let header = bytes.get(..HEADER)?;
    let status = u64::from_le_bytes(header[..8].try_into().ok()?);
    let owner = u64::from_le_bytes(header[8..16].try_into().ok()?);
    let len = usize::try_from(u64::from_le_bytes(header[16..24].try_into().ok()?)).ok()?;
    if (status == 2 && owner == 0) || len > MAX_TEXT || bytes.len() != HEADER.checked_add(len)? { return None; }
    Some(Event { status, owner, text: core::str::from_utf8(&bytes[HEADER..]).ok()? })
}

pub fn chat_for_owner(owners: &[u64], owner: u64) -> Option<usize> {
    (owner != 0).then(|| owners.iter().position(|&value| value == owner)).flatten()
}
