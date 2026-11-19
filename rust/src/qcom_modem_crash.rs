//! Read-only SMEM v12 global failure-reason snapshot. Item421 is pinned by
//! the captured Sunfish modem DTS and the matched pil-q6v5-mss source trace.
//! No legacy TOC fallback, private-host lookup, address from payload, or clear.
use crate::qcom_modem_status::{Directory, Error, Item, Read, Route, SMEM_BYTES};
pub const ITEM_ID: u16 = 421;
pub const MAX_BYTES: usize = 256;
const TABLE: usize = SMEM_BYTES - 4096;
fn fits(at: usize, n: usize, limit: usize) -> bool {
    at <= limit && n <= limit - at
}
fn u16_at(m: &impl Read, at: usize) -> Result<u16, Error> {
    if !fits(at, 2, SMEM_BYTES) {
        return Err(Error::Bounds);
    }
    Ok(u16::from_le_bytes([m.read8(at), m.read8(at + 1)]))
}
fn u32_at(m: &impl Read, at: usize) -> Result<u32, Error> {
    if !fits(at, 4, SMEM_BYTES) {
        return Err(Error::Bounds);
    }
    Ok(u32::from_le_bytes([
        m.read8(at),
        m.read8(at + 1),
        m.read8(at + 2),
        m.read8(at + 3),
    ]))
}
fn record(found: &mut Option<Item>, item: Item) -> Result<(), Error> {
    if found.is_some() {
        return Err(Error::Duplicate);
    }
    *found = Some(item);
    Ok(())
}
pub fn directory(m: &impl Read) -> Result<Directory, Error> {
    if u32_at(m, 192)? != 1 || u32_at(m, 204)? != 0 || u32_at(m, 92)? >> 16 != 12 {
        return Err(Error::Header);
    }
    if u32_at(m, TABLE)? != 0x434f5424 || u32_at(m, TABLE + 4)? != 1 {
        return Err(Error::Table);
    }
    let count = u32_at(m, TABLE + 8)? as usize;
    if count == 0 || count > 80 || !fits(32, count * 48, 4096) {
        return Err(Error::Table);
    }
    let mut route = None;
    for i in 0..count {
        let at = TABLE + 32 + i * 48;
        if (u16_at(m, at + 12)?, u16_at(m, at + 14)?) == (0xfffe, 0xfffe) {
            if route.is_some() {
                return Err(Error::Route);
            }
            route = Some(Route {
                index: i as u32,
                offset: u32_at(m, at)? as usize,
                size: u32_at(m, at + 4)? as usize,
                cacheline: u32_at(m, at + 16)? as usize,
            });
        }
    }
    let r = route.ok_or(Error::Route)?;
    if r.offset < 4096 || r.size < 32 || !fits(r.offset, r.size, TABLE) {
        return Err(Error::Partition);
    }
    // The global lookup validates only its unique global route and header.
    // Other host-pair entries describe separate heaps and are not followed or
    // range-checked here; all reads remain bounded by the selected global route.
    let p = r.offset;
    let header_hosts = (u16_at(m, p + 4)?, u16_at(m, p + 6)?);
    if u32_at(m, p)? != 0x54525024
        || (header_hosts.0 != 0xfffe && header_hosts.1 != 0xfffe)
        || u32_at(m, p + 8)? as usize != r.size
    {
        return Err(Error::Partition);
    }
    let uncached = u32_at(m, p + 12)? as usize;
    let cached = u32_at(m, p + 16)? as usize;
    if uncached < 32 || uncached > cached || cached > r.size {
        return Err(Error::Partition);
    }
    let mut found = None;
    let mut cursor = 32;
    let mut steps = 0;
    while cursor < uncached {
        steps += 1;
        if steps > 4096 || !fits(cursor, 16, uncached) {
            return Err(Error::Allocation);
        }
        let at = p + cursor;
        let size = u32_at(m, at + 4)? as usize;
        let pad = u16_at(m, at + 8)? as usize;
        let header_pad = u16_at(m, at + 10)? as usize;
        if u16_at(m, at)? != 0xa5a5
            || size == 0
            || pad >= size
            || header_pad > uncached - cursor - 16
        {
            return Err(Error::Allocation);
        }
        let data = cursor + 16 + header_pad;
        if !fits(data, size, uncached) {
            return Err(Error::Allocation);
        }
        if u16_at(m, at + 2)? == ITEM_ID {
            record(
                &mut found,
                Item {
                    offset: p + data,
                    size: size - pad,
                },
            )?
        }
        cursor = data + size;
    }
    if cursor != uncached {
        return Err(Error::Allocation);
    }
    if cached < r.size {
        let line = r.cacheline;
        if line < 16 || line > 4096 || !line.is_power_of_two() {
            return Err(Error::Allocation);
        }
        let reserved = (32 + line - 1) & !(line - 1);
        let stride = (16 + line - 1) & !(line - 1);
        if reserved >= r.size {
            return Err(Error::Allocation);
        }
        let mut entry = r.size - reserved;
        let mut steps = 0;
        let mut last_data = r.size;
        while entry > cached {
            steps += 1;
            if steps > 4096 || !fits(entry, 16, r.size) {
                return Err(Error::Allocation);
            }
            let at = p + entry;
            let size = u32_at(m, at + 4)? as usize;
            let pad = u16_at(m, at + 8)? as usize;
            if u16_at(m, at)? != 0xa5a5 || size == 0 || pad >= size || size > entry - cached {
                return Err(Error::Allocation);
            }
            last_data = entry - size;
            if u16_at(m, at + 2)? == ITEM_ID {
                record(
                    &mut found,
                    Item {
                        offset: p + last_data,
                        size: size - pad,
                    },
                )?
            }
            if entry < size + stride {
                return Err(Error::Allocation);
            }
            entry -= size + stride;
        }
        if last_data != cached {
            return Err(Error::Allocation);
        }
    }
    if found.is_some_and(|i| i.size == 0 || i.size > 4096) {
        return Err(Error::ItemSize);
    }
    Ok(Directory {
        route: r,
        free_uncached: uncached,
        free_cached: cached,
        item: found,
    })
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Snapshot {
    pub directory: Directory,
    pub raw: [u8; MAX_BYTES],
    pub copied: usize,
    pub text_len: usize,
    pub terminated: bool,
    pub truncated: bool,
}
/// Read-only snapshots captured before and at a peer-fatal indication.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Evidence {
    pub before: Option<Result<Snapshot, Error>>,
    pub after: Option<Result<Snapshot, Error>>,
}
/// Caller holds lock3. The remote writer may ignore that lock, so compare two
/// complete bounded copies and the directory before and after both copies.
pub fn snapshot(m: &impl Read) -> Result<Snapshot, Error> {
    let d = directory(m)?;
    let mut first = [0; MAX_BYTES];
    let mut second = [0; MAX_BYTES];
    let copied = d.item.map_or(0, |i| i.size.min(MAX_BYTES));
    if let Some(item) = d.item {
        for i in 0..copied {
            first[i] = m.read8(item.offset + i)
        }
        for i in 0..copied {
            second[i] = m.read8(item.offset + i)
        }
    }
    let after = directory(m)?;
    if d != after || first != second {
        return Err(Error::Unstable);
    }
    let end = first[..copied].iter().position(|b| *b == 0);
    Ok(Snapshot {
        directory: d,
        raw: first,
        copied,
        text_len: end.unwrap_or(copied),
        terminated: end.is_some(),
        truncated: end.is_none() && d.item.is_some_and(|i| i.size > copied),
    })
}
/// Difference applies only to the bounded raw prefix/presence, not attribution
/// to this boot or a claim about uncaptured bytes or the cause of a failure.
/// A change is established only when both bounded snapshots succeeded.
pub fn evidence_changed(before: Option<Result<Snapshot, Error>>, after: Option<Result<Snapshot, Error>>) -> Option<bool> {
    Some(changed(&before?.ok()?, &after?.ok()?))
}

pub fn changed(before: &Snapshot, after: &Snapshot) -> bool {
    before.directory.item.is_some() != after.directory.item.is_some()
        || before.copied != after.copied
        || before.raw != after.raw
}
pub fn text_row(s: &Snapshot, row: usize) -> [u8; 24] {
    let mut b = [b' '; 24];
    if row >= MAX_BYTES.div_ceil(24) {
        return b;
    }
    for (i, v) in b.iter_mut().enumerate() {
        let at = row * 24 + i;
        if at >= s.text_len {
            break;
        }
        let c = s.raw[at].to_ascii_uppercase();
        *v = if c.is_ascii_alphanumeric() { c } else { b' ' };
    }
    b
}

/// Diagnostic discriminator for Error::Partition only. This mirrors the four
/// bounded global-route checks without reading item payloads or following any
/// address not already validated by the same metadata guard.
pub fn partition_stage(m: &impl Read) -> u32 {
    if u32_at(m, 192).ok() != Some(1)
        || u32_at(m, 204).ok() != Some(0)
        || u32_at(m, 92).ok().map(|v| v >> 16) != Some(12)
        || u32_at(m, TABLE).ok() != Some(0x434f5424)
        || u32_at(m, TABLE + 4).ok() != Some(1)
    {
        return 0;
    }
    let count = match u32_at(m, TABLE + 8) {
        Ok(v) if (1..=80).contains(&(v as usize)) => v as usize,
        _ => return 0,
    };
    let mut route = None;
    for i in 0..count {
        let at = TABLE + 32 + i * 48;
        if (u16_at(m, at + 12).ok(), u16_at(m, at + 14).ok()) == (Some(0xfffe), Some(0xfffe)) {
            if route.is_some() {
                return 0;
            }
            route = Some(Route {
                index: i as u32,
                offset: u32_at(m, at).unwrap_or(0) as usize,
                size: u32_at(m, at + 4).unwrap_or(0) as usize,
                cacheline: u32_at(m, at + 16).unwrap_or(0) as usize,
            });
        }
    }
    let Some(r) = route else { return 0 };
    if r.offset < 4096 || r.size < 32 || !fits(r.offset, r.size, TABLE) {
        return 1;
    }
    let p = r.offset;
    if u32_at(m, p).ok() != Some(0x54525024)
        || (u16_at(m, p + 4).ok().is_none_or(|v| v != 0xfffe)
            && u16_at(m, p + 6).ok().is_none_or(|v| v != 0xfffe))
        || u32_at(m, p + 8).ok().map(|v| v as usize) != Some(r.size)
    {
        return 2;
    }
    let uncached = u32_at(m, p + 12).unwrap_or(u32::MAX) as usize;
    let cached = u32_at(m, p + 16).unwrap_or(u32::MAX) as usize;
    if uncached < 32 || uncached > cached || cached > r.size {
        return 3;
    }
    0
}
