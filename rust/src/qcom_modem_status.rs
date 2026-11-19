//! Bounded read-only SMEM v12 directory and SMP2P status decoding.
//! Caller serializes directory reads with TCSR lock3. SMP2P remains remote
//! single-writer data: two equal snapshots are evidence, not an IRQ/lifecycle
//! service or permission to initialize AP/shared transport state.
pub const SMEM_BYTES: usize = 0x200000;
pub const PAYLOAD_BYTES: usize = 340;
pub const ITEM_ID: u16 = 435;
pub trait Read {
    fn read8(&self, at: usize) -> u8;
    /// Metadata word, little-endian. Backends may use one aligned read;
    /// callers still validate bounds and perform their stability snapshots.
    fn read_le32(&self, at: usize) -> u32 {
        u32::from_le_bytes(core::array::from_fn(|i| self.read8(at + i)))
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Header,
    Table,
    Route,
    Partition,
    Allocation,
    Duplicate,
    ItemSize,
    Payload,
    Unstable,
    Lock,
    LockLost,
    BaselinePresent,
    RouteChanged,
    Fatal,
    Clock,
    Timeout,
    Bounds,
    Auth,
    Publication,
    Outbound,
    Restart,
    ReadyLost,
}
impl Error {
    /// Local diagnostic identifier, not the peer's raw `slave-kernel` value.
    pub const fn code(self) -> u32 {
        self as u32 + 1
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Route {
    pub index: u32,
    pub offset: usize,
    pub size: usize,
    pub cacheline: usize,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Item {
    pub offset: usize,
    pub size: usize,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Directory {
    pub route: Route,
    pub free_uncached: usize,
    pub free_cached: usize,
    pub item: Option<Item>,
}
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
fn metadata_u32_at(m: &impl Read, at: usize) -> Result<u32, Error> {
    if !fits(at, 4, SMEM_BYTES) { return Err(Error::Bounds); }
    Ok(m.read_le32(at))
}
fn record(found: &mut Option<Item>, item: Item) -> Result<(), Error> {
    if found.is_some() {
        return Err(Error::Duplicate);
    }
    *found = Some(item);
    Ok(())
}
pub fn directory(m: &impl Read) -> Result<Directory, Error> {
    directory_for(m, ITEM_ID)
}
pub fn directory_for(m: &impl Read, target: u16) -> Result<Directory, Error> {
    let d = allocation_directory(m, target)?;
    if d.item
        .is_some_and(|i| i.size < PAYLOAD_BYTES || i.size > 4096)
    {
        return Err(Error::ItemSize);
    }
    Ok(d)
}
/// Allocation metadata only, with no payload/type interpretation. In
/// particular a32-byte GLINK descriptor is valid metadata, never an SMP2P item.
pub fn allocation_directory(m: &impl Read, target: u16) -> Result<Directory, Error> {
    if u32_at(m, 192)? != 1 || u32_at(m, 204)? != 0 || u32_at(m, 92)? >> 16 != 12 {
        return Err(Error::Header);
    }
    let table = SMEM_BYTES - 4096;
    if u32_at(m, table)? != 0x434f5424 || u32_at(m, table + 4)? != 1 {
        return Err(Error::Table);
    }
    let count = u32_at(m, table + 8)? as usize;
    if count == 0 || count > 80 || !fits(32, count * 48, 4096) {
        return Err(Error::Table);
    }
    let mut route = None;
    for i in 0..count {
        let at = table + 32 + i * 48;
        let h0 = u16_at(m, at + 12)?;
        let h1 = u16_at(m, at + 14)?;
        if (h0, h1) == (0, 1) || (h0, h1) == (1, 0) {
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
    if r.offset == 0 || r.size < 32 || !fits(r.offset, r.size, table) {
        return Err(Error::Partition);
    }
    let p = r.offset;
    let hosts = (u16_at(m, p + 4)?, u16_at(m, p + 6)?);
    if u32_at(m, p)? != 0x54525024
        || !matches!(hosts, (0, 1) | (1, 0))
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
        let id = u16_at(m, at + 2)?;
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
        if id == target {
            record(
                &mut found,
                Item {
                    offset: p + data,
                    size: size - pad,
                },
            )?;
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
            if u16_at(m, at + 2)? == target {
                record(
                    &mut found,
                    Item {
                        offset: p + entry - size,
                        size: size - pad,
                    },
                )?;
            }
            if entry < size + stride {
                return Err(Error::Allocation);
            }
            entry -= size + stride;
        }
    }
    Ok(Directory {
        route: r,
        free_uncached: uncached,
        free_cached: cached,
        item: found,
    })
}
/// Resolve several item IDs in one bounded pass of the same allocation list.
pub fn allocation_directories<const N: usize>(m: &impl Read, targets: [u16; N]) -> Result<[Directory; N], Error> {
    if metadata_u32_at(m, 192)? != 1 || metadata_u32_at(m, 204)? != 0 || metadata_u32_at(m, 92)? >> 16 != 12 {
        return Err(Error::Header);
    }
    let table = SMEM_BYTES - 4096;
    if metadata_u32_at(m, table)? != 0x434f5424 || metadata_u32_at(m, table + 4)? != 1 {
        return Err(Error::Table);
    }
    let count = metadata_u32_at(m, table + 8)? as usize;
    if count == 0 || count > 80 || !fits(32, count * 48, 4096) {
        return Err(Error::Table);
    }
    let mut route = None;
    for i in 0..count {
        let at = table + 32 + i * 48;
        let h0 = u16_at(m, at + 12)?;
        let h1 = u16_at(m, at + 14)?;
        if (h0, h1) == (0, 1) || (h0, h1) == (1, 0) {
            if route.is_some() {
                return Err(Error::Route);
            }
            route = Some(Route {
                index: i as u32,
                offset: metadata_u32_at(m, at)? as usize,
                size: metadata_u32_at(m, at + 4)? as usize,
                cacheline: metadata_u32_at(m, at + 16)? as usize,
            });
        }
    }
    let r = route.ok_or(Error::Route)?;
    if r.offset == 0 || r.size < 32 || !fits(r.offset, r.size, table) {
        return Err(Error::Partition);
    }
    let p = r.offset;
    let hosts = (u16_at(m, p + 4)?, u16_at(m, p + 6)?);
    if metadata_u32_at(m, p)? != 0x54525024
        || !matches!(hosts, (0, 1) | (1, 0))
        || metadata_u32_at(m, p + 8)? as usize != r.size
    {
        return Err(Error::Partition);
    }
    let uncached = metadata_u32_at(m, p + 12)? as usize;
    let cached = metadata_u32_at(m, p + 16)? as usize;
    if uncached < 32 || uncached > cached || cached > r.size {
        return Err(Error::Partition);
    }
    let mut found = [None; N];
    let mut cursor = 32;
    let mut steps = 0;
    while cursor < uncached {
        steps += 1;
        if steps > 4096 || !fits(cursor, 16, uncached) {
            return Err(Error::Allocation);
        }
        let at = p + cursor;
        let id = u16_at(m, at + 2)?;
        let size = metadata_u32_at(m, at + 4)? as usize;
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
        for (i, target) in targets.iter().enumerate() {
            if id == *target {
                record(&mut found[i], Item { offset: p + data, size: size - pad })?;
            }
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
        while entry > cached {
            steps += 1;
            if steps > 4096 || !fits(entry, 16, r.size) {
                return Err(Error::Allocation);
            }
            let at = p + entry;
            let size = metadata_u32_at(m, at + 4)? as usize;
            let pad = u16_at(m, at + 8)? as usize;
            if u16_at(m, at)? != 0xa5a5 || size == 0 || pad >= size || size > entry - cached {
                return Err(Error::Allocation);
            }
            let id = u16_at(m, at + 2)?;
            for (i, target) in targets.iter().enumerate() {
                if id == *target {
                    record(&mut found[i], Item { offset: p + entry - size, size: size - pad })?;
                }
            }
            if entry < size + stride {
                return Err(Error::Allocation);
            }
            entry -= size + stride;
        }
    }
    Ok(core::array::from_fn(|i| Directory {
        route: r,
        free_uncached: uncached,
        free_cached: cached,
        item: found[i],
    }))
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Status {
    pub features: u32,
    pub flags: u32,
    pub valid_entries: u16,
    /// Remote modem-to-AP `slave-kernel` value from the Atoll modem SMP2P
    /// edge. It is distinct from the WLAN SMP2P `wlan` entry.
    pub slave_kernel: Option<u32>,
}
impl Status {
    /// Atoll's modem DTS maps bit 1 of `slave-kernel` to `qcom,err-ready`.
    pub fn ready_bit(&self) -> bool {
        self.slave_kernel
            .is_some_and(|v| v & 2 != 0 && v & (1 | 8 | 128) == 0)
    }
    /// Atoll's modem DTS maps bit 0 of `slave-kernel` to `qcom,err-fatal`.
    pub fn fatal_bit(&self) -> bool {
        self.slave_kernel.is_some_and(|v| v & 1 != 0)
    }
}
pub fn decode(b: &[u8; PAYLOAD_BYTES]) -> Result<Status, Error> {
    fn u16b(b: &[u8], at: usize) -> u16 {
        u16::from_le_bytes(b[at..at + 2].try_into().unwrap())
    }
    fn u32b(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
    }
    let features = u32b(b, 4) >> 8;
    let valid = u16b(b, 14);
    let flags = u32b(b, 16);
    if u32b(b, 0) != 0x504d5324
        || b[4] != 1
        || features & !1 != 0
        || u16b(b, 8) != 1
        || u16b(b, 10) != 0
        || u16b(b, 12) != 16
        || valid > 16
        || flags & !3 != 0
    {
        return Err(Error::Payload);
    }
    let mut target = None;
    for i in 0..valid as usize {
        let at = 20 + i * 20;
        let name = &b[at..at + 16];
        let end = name.iter().position(|c| *c == 0).unwrap_or(16);
        if end == 0 || !name[..end].iter().all(|v| (32..=126).contains(v)) {
            return Err(Error::Payload);
        }
        if &name[..end] == b"slave-kernel" {
            if target.is_some() {
                return Err(Error::Duplicate);
            }
            target = Some(u32b(b, at + 16));
        }
    }
    Ok(Status {
        features,
        flags,
        valid_entries: valid,
        slave_kernel: target,
    })
}
/// Repeated exact reads stabilize remote-owned data without modifying it.
pub fn payload(m: &impl Read, item: Item) -> Result<Status, Error> {
    if item.size < PAYLOAD_BYTES
        || item.size > 4096
        || !fits(item.offset, item.size, SMEM_BYTES - 4096)
    {
        return Err(Error::ItemSize);
    }
    let mut first = [0; PAYLOAD_BYTES];
    let mut second = [0; PAYLOAD_BYTES];
    for (i, v) in first.iter_mut().enumerate() {
        *v = m.read8(item.offset + i);
    }
    for (i, v) in second.iter_mut().enumerate() {
        *v = m.read8(item.offset + i);
    }
    if first != second {
        return Err(Error::Unstable);
    }
    decode(&first)
}
