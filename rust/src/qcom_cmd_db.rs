//! Bounded decoder for copied Qualcomm command-database bytes.
//!
//! Wire reference: matched Lineage cmd-db.c; ARC auxiliary format and consumer
//! offset from rpmh-regulator.c. No MMIO, votes, or resource ownership is
//! supplied here. Header version/checksum are reported, not authenticated or
//! interpreted as a supported hardware command ABI.

pub const MAGIC: u32 = 0x0c03_30db;
pub const HEADER_BYTES: usize = 144;
pub const MAX_BYTES: usize = 65_536;
const TABLES: usize = 8;
const ENTRY_BYTES: usize = 24;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Header,
    Magic,
    Range,
    TableLayout,
    ResourceId,
    SlaveId,
    DuplicateResource,
    NotFound,
    ArcData,
    NoLevel,
}

impl Error {
    pub const fn code(self) -> u32 {
        match self {
            Self::Header => 1,
            Self::Magic => 2,
            Self::Range => 3,
            Self::TableLayout => 4,
            Self::ResourceId => 5,
            Self::SlaveId => 6,
            Self::DuplicateResource => 7,
            Self::NotFound => 8,
            Self::ArcData => 9,
            Self::NoLevel => 10,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Fault {
    pub error: Error,
    pub slot: Option<u8>,
    pub entry: Option<u16>,
}

impl Fault {
    fn header(error: Error) -> Self {
        Self {
            error,
            slot: None,
            entry: None,
        }
    }
    fn table(error: Error, slot: usize) -> Self {
        Self {
            error,
            slot: Some(slot as u8),
            entry: None,
        }
    }
    fn entry(error: Error, slot: usize, entry: usize) -> Self {
        Self {
            error,
            slot: Some(slot as u8),
            entry: Some(entry as u16),
        }
    }
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16, Error> {
    let p = bytes
        .get(offset..offset.checked_add(2).ok_or(Error::Range)?)
        .ok_or(Error::Range)?;
    Ok(u16::from_le_bytes([p[0], p[1]]))
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, Error> {
    let p = bytes
        .get(offset..offset.checked_add(4).ok_or(Error::Range)?)
        .ok_or(Error::Range)?;
    Ok(u32::from_le_bytes([p[0], p[1], p[2], p[3]]))
}

#[derive(Clone, Copy, Default)]
struct Table {
    source_slot: usize,
    slave: u16,
    start: usize,
    end: usize,
    auxiliary: usize,
    count: usize,
    version: u16,
}

pub struct Database<'a> {
    bytes: &'a [u8],
    tables: [Table; TABLES],
    count: usize,
    entries: usize,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Resource<'a> {
    pub id: [u8; 8],
    pub address: u32,
    pub priority: [u32; 2],
    /// Hardware type derived from the address, as in cmd_db_get_slave_id().
    pub slave: u16,
    /// Raw table category. It does not establish an entry's usable address.
    pub table_slave: u16,
    pub auxiliary_version: u16,
    pub auxiliary: &'a [u8],
}

impl Resource<'_> {
    /// Check a selected resource against the consumer's expected hardware type.
    /// Parsing/lookup alone does not make an address usable or grant ownership.
    pub fn command_address(&self, expected_slave: u16) -> Result<u32, Error> {
        if self.address == 0
            || expected_slave == 0
            || expected_slave > 7
            || self.slave != expected_slave
        {
            return Err(Error::SlaveId);
        }
        Ok(self.address)
    }

    /// Scheduling-priority metadata, not permission to use this resource.
    pub fn priority_for_driver(&self, driver: u8) -> Option<bool> {
        if driver >= 32 {
            None
        } else {
            Some(self.priority[0] & (1u32 << driver) != 0)
        }
    }

    pub fn arc_levels(&self) -> Result<ArcLevels, Error> {
        self.command_address(3)?;
        ArcLevels::parse(self.auxiliary)
    }
}

fn valid_id(id: &[u8]) -> bool {
    let mut padding = false;
    if id.first().copied().unwrap_or(0) == 0 {
        return false;
    }
    for byte in id {
        if *byte == 0 {
            padding = true;
        } else if padding || !(0x21..=0x7e).contains(byte) {
            return false;
        }
    }
    true
}

impl<'a> Database<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        Self::parse_detailed(bytes).map_err(|fault| fault.error)
    }

    pub fn parse_detailed(bytes: &'a [u8]) -> Result<Self, Fault> {
        if bytes.len() < HEADER_BYTES || bytes.len() > MAX_BYTES {
            return Err(Fault::header(Error::Header));
        }
        if u32_at(bytes, 4).map_err(Fault::header)? != MAGIC {
            return Err(Fault::header(Error::Magic));
        }
        let mut db = Self {
            bytes,
            tables: [Table::default(); TABLES],
            count: 0,
            entries: 0,
        };
        for slot in 0..TABLES {
            let offset = 8 + slot * 16;
            let read = |offset| u16_at(bytes, offset).map_err(|error| Fault::table(error, slot));
            let slave = read(offset)?;
            if slave == 0 {
                // The matched reader terminates here. Remaining slots are
                // unused storage, not additional tables to validate or use.
                break;
            }
            let count = usize::from(read(offset + 6)?);
            if count == 0 {
                // No entries will be dereferenced. Firmware may retain
                // arbitrary offsets in an empty hardware-category slot.
                continue;
            }
            if slave > 7 {
                return Err(Fault::table(Error::TableLayout, slot));
            }
            // Multiple populated headers may describe the same hardware
            // category; require disjoint tables and unambiguous resource IDs.
            let start = HEADER_BYTES + usize::from(read(offset + 2)?);
            let end = start
                .checked_add(
                    count
                        .checked_mul(ENTRY_BYTES)
                        .ok_or_else(|| Fault::table(Error::Range, slot))?,
                )
                .ok_or_else(|| Fault::table(Error::Range, slot))?;
            let auxiliary = HEADER_BYTES + usize::from(read(offset + 4)?);
            bytes
                .get(start..end)
                .ok_or_else(|| Fault::table(Error::Range, slot))?;
            if db.tables[..db.count]
                .iter()
                .any(|t| start < t.end && t.start < end)
            {
                return Err(Fault::table(Error::TableLayout, slot));
            }
            db.tables[db.count] = Table {
                source_slot: slot,
                slave,
                start,
                end,
                auxiliary,
                count,
                version: read(offset + 8)?,
            };
            db.count += 1;
            db.entries += count;
        }
        for table in &db.tables[..db.count] {
            for index in 0..table.count {
                db.resource(*table, index)
                    .map_err(|error| Fault::entry(error, table.source_slot, index))?;
            }
        }
        Ok(db)
    }

    pub fn version(&self) -> u32 {
        u32_at(self.bytes, 0).unwrap_or(0)
    }
    pub fn checksum(&self) -> u32 {
        u32_at(self.bytes, 136).unwrap_or(0)
    }
    pub fn standalone(&self) -> bool {
        u32_at(self.bytes, 140).unwrap_or(0) & 1 != 0
    }
    pub const fn entry_count(&self) -> usize {
        self.entries
    }

    fn resource(&self, table: Table, index: usize) -> Result<Resource<'a>, Error> {
        let offset = table.start + index * ENTRY_BYTES;
        let mut id = [0; 8];
        id.copy_from_slice(self.bytes.get(offset..offset + 8).ok_or(Error::Range)?);
        if !valid_id(&id) {
            return Err(Error::ResourceId);
        }
        let address = u32_at(self.bytes, offset + 16)?;
        // The matched reader permits zero addresses and does not compare
        // each entry's address type with its table category. Consumers check
        // their selected resource separately; unrelated entries must not
        // invalidate the entire structurally bounded database.
        let length = usize::from(u16_at(self.bytes, offset + 20)?);
        let auxiliary = if length == 0 {
            &self.bytes[0..0]
        } else {
            let start = table.auxiliary + usize::from(u16_at(self.bytes, offset + 22)?);
            let end = start.checked_add(length).ok_or(Error::Range)?;
            let value = self.bytes.get(start..end).ok_or(Error::Range)?;
            if self.tables[..self.count]
                .iter()
                .any(|t| start < t.end && t.start < end)
            {
                return Err(Error::TableLayout);
            }
            value
        };
        Ok(Resource {
            id,
            address,
            priority: [
                u32_at(self.bytes, offset + 8)?,
                u32_at(self.bytes, offset + 12)?,
            ],
            slave: ((address >> 16) & 7) as u16,
            table_slave: table.slave,
            auxiliary_version: table.version,
            auxiliary,
        })
    }

    /// Reject duplicate matches instead of choosing an ambiguous first record.
    pub fn lookup(&self, name: &str) -> Result<Resource<'a>, Error> {
        if name.is_empty()
            || name.len() > 8
            || !valid_id(name.as_bytes())
            || name.as_bytes().contains(&0)
        {
            return Err(Error::ResourceId);
        }
        let mut id = [0; 8];
        id[..name.len()].copy_from_slice(name.as_bytes());
        let mut found = None;
        for table in &self.tables[..self.count] {
            for index in 0..table.count {
                let resource = self.resource(*table, index)?;
                if resource.id == id {
                    if found.is_some() {
                        return Err(Error::DuplicateResource);
                    }
                    found = Some(resource);
                }
            }
        }
        found.ok_or(Error::NotFound)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct ArcLevels {
    levels: [u16; 16],
    count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArcSelection {
    pub selector: u8,
    pub raw_level: u16,
    pub consumer_level: u32,
}

impl ArcLevels {
    pub fn parse(auxiliary: &[u8]) -> Result<Self, Error> {
        if auxiliary.is_empty() || auxiliary.len() > 32 || auxiliary.len() % 2 != 0 {
            return Err(Error::ArcData);
        }
        let mut result = Self {
            levels: [0; 16],
            count: 0,
        };
        let mut padding = false;
        for index in 0..auxiliary.len() / 2 {
            let level = u16_at(auxiliary, index * 2)?;
            if index > 0 && level == 0 {
                padding = true;
                continue;
            }
            if padding || (index > 0 && level <= result.levels[index - 1]) {
                return Err(Error::ArcData);
            }
            result.levels[result.count] = level;
            result.count += 1;
        }
        Ok(result)
    }

    /// Linux adds one to each raw level to keep consumer voltage zero invalid.
    /// Choose the least supported consumer level at least as high as the
    /// request, corresponding to regulator_set_voltage(min, INT_MAX).
    pub fn select_at_least(&self, consumer_level: u32) -> Result<ArcSelection, Error> {
        if consumer_level == 0 {
            return Err(Error::NoLevel);
        }
        for (selector, raw_level) in self.levels[..self.count].iter().copied().enumerate() {
            let actual = u32::from(raw_level) + 1;
            if actual >= consumer_level {
                return Ok(ArcSelection {
                    selector: selector as u8,
                    raw_level,
                    consumer_level: actual,
                });
            }
        }
        Err(Error::NoLevel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn put32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn fixture() -> std::vec::Vec<u8> {
        let mut bytes = std::vec![0; HEADER_BYTES + 96 + 36];
        put32(&mut bytes, 0, 1);
        put32(&mut bytes, 4, MAGIC);
        put16(&mut bytes, 8, 3);
        put16(&mut bytes, 12, 96);
        put16(&mut bytes, 14, 4);
        put16(&mut bytes, 16, 0x100);
        for (i, name) in ["cx.lvl", "mss.lvl", "mx.lvl", "xo.lvl"].iter().enumerate() {
            let start = HEADER_BYTES + i * 24;
            bytes[start..start + name.len()].copy_from_slice(name.as_bytes());
            put32(&mut bytes, start + 8, 4);
            put32(&mut bytes, start + 16, 0x30000 + i as u32 * 4);
            if i < 3 {
                put16(&mut bytes, start + 20, 12);
                put16(&mut bytes, start + 22, i as u16 * 12);
                for (j, value) in [0, 16, 256, 384, 416, 0].iter().copied().enumerate() {
                    put16(&mut bytes, HEADER_BYTES + 96 + i * 12 + j * 2, value);
                }
            }
        }
        bytes
    }

    #[test]
    fn decodes_relative_tables_and_maps_consumer_turbo_to_selector() {
        let bytes = fixture();
        let db = Database::parse(&bytes).unwrap();
        assert_eq!(db.version(), 1);
        assert_eq!(db.entry_count(), 4);
        let cx = db.lookup("cx.lvl").unwrap();
        assert_eq!(cx.address, 0x30000);
        assert_eq!(cx.auxiliary_version, 0x100);
        assert_eq!(cx.priority_for_driver(2), Some(true));
        assert_eq!(cx.priority_for_driver(32), None);
        let levels = cx.arc_levels().unwrap();
        assert_eq!(
            levels.select_at_least(385),
            Ok(ArcSelection {
                selector: 3,
                raw_level: 384,
                consumer_level: 385
            })
        );
        assert_eq!(levels.select_at_least(300).unwrap().consumer_level, 385);
        assert_eq!(db.lookup("missing"), Err(Error::NotFound));
        assert_eq!(db.lookup("xo.lvl").unwrap().auxiliary, &[]);
    }

    #[test]
    fn every_truncation_and_outside_auxiliary_is_rejected() {
        let mut bytes = fixture();
        for end in 0..bytes.len() {
            assert!(Database::parse(&bytes[..end]).is_err(), "prefix {end}");
        }
        put16(&mut bytes, HEADER_BYTES + 22, u16::MAX);
        assert!(matches!(Database::parse(&bytes), Err(Error::Range)));
        let mut bytes = fixture();
        put16(&mut bytes, 10, u16::MAX);
        assert!(matches!(Database::parse(&bytes), Err(Error::Range)));
    }

    #[test]
    fn rejects_overlapping_tables_and_auxiliary_in_table() {
        let mut bytes = fixture();
        put16(&mut bytes, 24, 4);
        put16(&mut bytes, 30, 1);
        assert!(matches!(Database::parse(&bytes), Err(Error::TableLayout)));
        let mut bytes = fixture();
        put16(&mut bytes, 12, 0);
        assert!(matches!(Database::parse(&bytes), Err(Error::TableLayout)));
    }

    #[test]
    fn address_validation_is_per_consumer_and_uses_address_not_table_category() {
        let mut bytes = fixture();
        put32(&mut bytes, HEADER_BYTES + 16, 0x40000);
        let db = Database::parse(&bytes).unwrap();
        let cx = db.lookup("cx.lvl").unwrap();
        assert_eq!((cx.table_slave, cx.slave), (3, 4));
        assert_eq!(cx.command_address(3), Err(Error::SlaveId));
        assert_eq!(cx.arc_levels(), Err(Error::SlaveId));
        assert_eq!(cx.command_address(4), Ok(0x40000));
        assert_eq!(cx.command_address(0), Err(Error::SlaveId));
        assert_eq!(cx.command_address(8), Err(Error::SlaveId));
        // A mismatched unrelated record does not prevent a valid rail lookup.
        assert_eq!(
            db.lookup("mss.lvl").unwrap().command_address(3),
            Ok(0x30004)
        );
        // A table claiming VRM must not cause an ARC address to be treated as VRM.
        put16(&mut bytes, 8, 4);
        let db = Database::parse(&bytes).unwrap();
        let mss = db.lookup("mss.lvl").unwrap();
        assert_eq!((mss.table_slave, mss.slave), (4, 3));
        assert_eq!(mss.command_address(4), Err(Error::SlaveId));
    }

    #[test]
    fn zero_address_entry_retains_bounded_auxiliary_but_cannot_be_a_command() {
        let mut bytes = fixture();
        put32(&mut bytes, HEADER_BYTES + 16, 0);
        let db = Database::parse(&bytes).unwrap();
        let cx = db.lookup("cx.lvl").unwrap();
        assert_eq!((cx.table_slave, cx.slave), (3, 0));
        assert_eq!(cx.auxiliary.len(), 12);
        for slave in 0..=8 {
            assert_eq!(cx.command_address(slave), Err(Error::SlaveId));
        }
        assert_eq!(cx.arc_levels(), Err(Error::SlaveId));
        assert_eq!(
            db.lookup("mss.lvl")
                .unwrap()
                .arc_levels()
                .unwrap()
                .select_at_least(385)
                .unwrap()
                .selector,
            3
        );
        // Zero addresses do not bypass auxiliary bounds or ID checks.
        put16(&mut bytes, HEADER_BYTES + 22, u16::MAX);
        assert!(matches!(Database::parse(&bytes), Err(Error::Range)));
    }

    #[test]
    fn photographed_four_table_layout_can_contain_unaddressed_entries() {
        // The table geometry is observed. Names/addresses/auxiliary content
        // below are synthetic; this is not a captured command-DB dump.
        let mut bytes = std::vec![0; 0x1078];
        put32(&mut bytes, 0, 1);
        put32(&mut bytes, 4, MAGIC);
        for (slot, (slave, count, header, auxiliary)) in [
            (3, 0x14, 0, 0x1e0),
            (3, 0x0a, 0x2e0, 0x3d0),
            (5, 0x28, 0x3e8, 0x7a8),
            (4, 0x40, 0x8e8, 0xee8),
        ]
        .into_iter()
        .enumerate()
        {
            let start = 8 + slot * 16;
            put16(&mut bytes, start, slave);
            put16(&mut bytes, start + 2, header);
            put16(&mut bytes, start + 4, auxiliary);
            put16(&mut bytes, start + 6, count);
            for index in 0..usize::from(count) {
                let offset = HEADER_BYTES + usize::from(header) + index * ENTRY_BYTES;
                let name = std::format!("s{slot}e{index:03}");
                bytes[offset..offset + name.len()].copy_from_slice(name.as_bytes());
                put32(
                    &mut bytes,
                    offset + 16,
                    u32::from(slave) << 16 | index as u32 * 4,
                );
            }
        }
        let unaddressed = HEADER_BYTES + 0x2e0;
        put32(&mut bytes, unaddressed + 16, 0);
        put16(&mut bytes, unaddressed + 20, 2);
        let db = Database::parse(&bytes).unwrap();
        assert_eq!(db.entry_count(), 134);
        assert_eq!(
            db.lookup("s1e000").unwrap().command_address(3),
            Err(Error::SlaveId)
        );
        assert_eq!(db.lookup("s1e001").unwrap().command_address(3), Ok(0x30004));
        assert_eq!(db.lookup("s2e039").unwrap().command_address(5), Ok(0x5009c));
        assert_eq!(db.lookup("s3e063").unwrap().command_address(4), Ok(0x400fc));
    }

    #[test]
    fn empty_hardware_slots_do_not_create_ranges_or_validate_unused_offsets() {
        for offset in [0, 98, u16::MAX] {
            let mut bytes = fixture();
            put16(&mut bytes, 24, 4);
            put16(&mut bytes, 26, offset);
            put16(&mut bytes, 28, u16::MAX);
            // Header slot 1 has count zero. Its offsets must not cause an
            // overlap/range error or any read through an unused pointer.
            let db = Database::parse(&bytes).unwrap();
            assert_eq!(db.entry_count(), 4);
            assert_eq!(db.lookup("cx.lvl").unwrap().address, 0x30000);
        }
    }

    #[test]
    fn first_zero_slave_terminates_lookup_without_inspecting_trailing_slots() {
        let mut bytes = fixture();
        bytes[40..136].fill(0xff); // Slot 1 is still the zero terminator.
        let db = Database::parse(&bytes).unwrap();
        assert_eq!(db.entry_count(), 4);
        assert_eq!(db.lookup("mss.lvl").unwrap().address, 0x30004);
    }

    #[test]
    fn disjoint_headers_of_one_hardware_category_remain_unambiguous() {
        let mut bytes = fixture();
        put16(&mut bytes, 14, 2); // First ARC table covers first two entries.
        put16(&mut bytes, 24, 3); // Second table is also ARC.
        put16(&mut bytes, 26, 48);
        put16(&mut bytes, 28, 96);
        put16(&mut bytes, 30, 2);
        put16(&mut bytes, 32, 0x100);
        let db = Database::parse(&bytes).unwrap();
        assert_eq!(db.entry_count(), 4);
        assert_eq!(db.lookup("mx.lvl").unwrap().address, 0x30008);
        let id: [u8; 8] = bytes[HEADER_BYTES..HEADER_BYTES + 8].try_into().unwrap();
        bytes[HEADER_BYTES + 48..HEADER_BYTES + 56].copy_from_slice(&id);
        let db = Database::parse(&bytes).unwrap();
        assert_eq!(db.lookup("cx.lvl"), Err(Error::DuplicateResource));
    }

    #[test]
    fn detailed_fault_preserves_the_original_header_slot_and_entry() {
        let mut bytes = fixture();
        put16(&mut bytes, 24, 4);
        put16(&mut bytes, 30, 1);
        assert!(matches!(
            Database::parse_detailed(&bytes),
            Err(Fault {
                error: Error::TableLayout,
                slot: Some(1),
                entry: None
            })
        ));
        let mut bytes = fixture();
        put16(&mut bytes, 12, 0);
        assert!(matches!(
            Database::parse_detailed(&bytes),
            Err(Fault {
                error: Error::TableLayout,
                slot: Some(0),
                entry: Some(0)
            })
        ));
        let mut bytes = fixture();
        // Empty slot 1 does not renumber populated slot 2 in a fault report.
        put16(&mut bytes, 24, 4);
        put16(&mut bytes, 40, 5);
        put16(&mut bytes, 46, 1); // Overlapping nonempty table at original slot 2.
        assert!(matches!(
            Database::parse_detailed(&bytes),
            Err(Fault {
                error: Error::TableLayout,
                slot: Some(2),
                entry: None
            })
        ));
    }

    #[test]
    fn rejects_ambiguous_ids_and_invalid_wire_or_query_keys() {
        let mut bytes = fixture();
        let id: [u8; 8] = bytes[HEADER_BYTES..HEADER_BYTES + 8].try_into().unwrap();
        bytes[HEADER_BYTES + 24..HEADER_BYTES + 32].copy_from_slice(&id);
        let db = Database::parse(&bytes).unwrap();
        assert_eq!(db.lookup("cx.lvl"), Err(Error::DuplicateResource));
        for key in ["", "too-long-id", "cx\0.lvl", "bad key"] {
            assert_eq!(db.lookup(key), Err(Error::ResourceId));
        }
        let mut bytes = fixture();
        bytes[HEADER_BYTES + 7] = b'X';
        assert!(matches!(Database::parse(&bytes), Err(Error::ResourceId)));
        let mut bytes = fixture();
        put32(&mut bytes, 4, 0);
        assert!(matches!(Database::parse(&bytes), Err(Error::Magic)));
    }

    #[test]
    fn arc_padding_order_bounds_and_consumer_offset_are_checked() {
        for bytes in [
            &[][..],
            &[0][..],
            &[0, 0, 16, 0, 0, 0, 1, 0][..],
            &[0, 0, 16, 0, 16, 0][..],
            &[0, 0, 32, 0, 16, 0][..],
            &[0; 34][..],
        ] {
            assert_eq!(ArcLevels::parse(bytes), Err(Error::ArcData));
        }
        let levels = ArcLevels::parse(&[0, 0, 255, 255, 0, 0]).unwrap();
        assert_eq!(
            levels.select_at_least(65_536).unwrap().consumer_level,
            65_536
        );
        assert_eq!(levels.select_at_least(65_537), Err(Error::NoLevel));
        assert_eq!(levels.select_at_least(0), Err(Error::NoLevel));
        assert_eq!(levels.select_at_least(1).unwrap().selector, 0);
    }
}
