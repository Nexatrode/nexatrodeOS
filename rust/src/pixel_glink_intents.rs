//! Bounded diagnostics over owned copies of peer control records.
//! Remote intent sizes/IDs are observations, never buffers or send authority.
use crate::pixel_glink_receive::Header;
use personal_os_rust_server::qcom_modem_status::Error;
pub const MAX_BYTES: usize = 512;
pub const MAX_ENTRIES: usize = 64;
pub const MAX_RECORD_ENTRIES: usize = (MAX_BYTES - 8) / 8;
pub const MAX_GROUPS: usize = 8;
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Group {
    pub size: u32,
    pub count: u32,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Inventory {
    pub records: u32,
    pub intent_records: u32,
    pub entries: u32,
    pub groups: [Group; MAX_GROUPS],
    pub group_count: usize,
    pub min_id: u32,
    pub max_id: u32,
    pub duplicate_ids: u32,
    pub zero_sizes: u32,
    pub signals: u32,
    pub signal_bits: u32,
    pub read_notifications: u32,
    pub intent_requests: u32,
    pub last_request_size: u32,
}
#[derive(Clone, Copy, Debug)]
pub struct Scan {
    pub inventory: Inventory,
    pub parsed_bytes: usize,
    pub error: Option<Error>,
    pub detail: u32,
    pub stop_offset: usize,
    pub stop_header: Option<Header>,
    pub stop_words: [u32; 4],
    pub preview_words: usize,
}
fn empty() -> Inventory {
    Inventory {
        records: 0,
        intent_records: 0,
        entries: 0,
        groups: [Group::default(); MAX_GROUPS],
        group_count: 0,
        min_id: u32::MAX,
        max_id: 0,
        duplicate_ids: 0,
        zero_sizes: 0,
        signals: 0,
        signal_bits: 0,
        read_notifications: 0,
        intent_requests: 0,
        last_request_size: 0,
    }
}
fn header(b: &[u8]) -> Header {
    Header {
        command: u16::from_le_bytes(b[..2].try_into().unwrap()),
        param1: u16::from_le_bytes(b[2..4].try_into().unwrap()),
        param2: u32::from_le_bytes(b[4..8].try_into().unwrap()),
    }
}
/// Details:1=input bound,2=unknown command,3=channel,4=count,5=truncated
/// record,6=entry storage,7=size-group storage. Only whole records commit stats.
pub fn inspect(bytes: &[u8]) -> Scan {
    let mut s = Scan {
        inventory: empty(),
        parsed_bytes: 0,
        error: None,
        detail: 0,
        stop_offset: 0,
        stop_header: None,
        stop_words: [0; 4],
        preview_words: 0,
    };
    let mut ids = [0; MAX_ENTRIES];
    if bytes.len() > MAX_BYTES || bytes.len() % 8 != 0 {
        s.error = Some(Error::Bounds);
        s.detail = 1;
        return s;
    }
    while s.parsed_bytes < bytes.len() {
        let at = s.parsed_bytes;
        let h = header(&bytes[at..at + 8]);
        match record(bytes, at, h, s.inventory, ids) {
            Ok((size, inventory, next_ids)) => {
                s.parsed_bytes += size;
                s.inventory = inventory;
                ids = next_ids
            }
            Err((e, detail)) => {
                s.error = Some(e);
                s.detail = detail;
                s.stop_offset = at;
                s.stop_header = Some(h);
                for j in 0..4 {
                    let p = at + 4 * j;
                    if p + 4 > bytes.len() {
                        break;
                    }
                    s.stop_words[j] = u32::from_le_bytes(bytes[p..p + 4].try_into().unwrap());
                    s.preview_words += 1
                }
                break;
            }
        }
    }
    s
}
fn record(
    bytes: &[u8],
    at: usize,
    h: Header,
    mut r: Inventory,
    mut ids: [u32; MAX_ENTRIES],
) -> Result<(usize, Inventory, [u32; MAX_ENTRIES]), (Error, u32)> {
    let size = match h.command {
        5 => {
            if h.param1 != 2 {
                return Err((Error::Payload, 3));
            }
            if h.param2 == 0 || h.param2 as usize > MAX_RECORD_ENTRIES {
                return Err((Error::Payload, 4));
            }
            8 + 8 * h.param2 as usize
        }
        7 | 15 => {
            if h.param1 != 2 {
                return Err((Error::Payload, 3));
            }
            8
        }
        13 => 8,
        _ => return Err((Error::Payload, 2)),
    };
    if size > bytes.len() - at {
        return Err((Error::Bounds, 5));
    }
    match h.command {
        5 => {
            r.intent_records += 1;
            for i in 0..h.param2 as usize {
                if r.entries as usize >= MAX_ENTRIES {
                    return Err((Error::Bounds, 6));
                }
                let p = at + 8 + i * 8;
                let size = u32::from_le_bytes(bytes[p..p + 4].try_into().unwrap());
                let id = u32::from_le_bytes(bytes[p + 4..p + 8].try_into().unwrap());
                if ids[..r.entries as usize].contains(&id) {
                    r.duplicate_ids += 1
                }
                ids[r.entries as usize] = id;
                r.entries += 1;
                r.min_id = r.min_id.min(id);
                r.max_id = r.max_id.max(id);
                if size == 0 {
                    r.zero_sizes += 1
                }
                let group = match r.groups[..r.group_count]
                    .iter()
                    .position(|g| g.size == size)
                {
                    Some(i) => i,
                    None => {
                        if r.group_count == MAX_GROUPS {
                            return Err((Error::Bounds, 7));
                        }
                        let i = r.group_count;
                        r.groups[i].size = size;
                        r.group_count += 1;
                        i
                    }
                };
                r.groups[group].count += 1;
            }
        }
        7 => {
            r.intent_requests += 1;
            r.last_request_size = h.param2
        }
        13 => r.read_notifications += 1,
        15 => {
            r.signals += 1;
            r.signal_bits = h.param2
        }
        _ => unreachable!(),
    }
    r.records += 1;
    Ok((size, r, ids))
}
pub fn parse(bytes: &[u8]) -> Result<Inventory, Error> {
    let s = inspect(bytes);
    match s.error {
        Some(e) => Err(e),
        None => Ok(s.inventory),
    }
}
