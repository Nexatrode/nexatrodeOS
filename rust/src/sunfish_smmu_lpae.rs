//! Bounded ARM SMMUv2 AArch64 LPAE plan for Sunfish IPA AP ring pages.
//!
//! This emits sparse descriptor values and context-register values for a
//! caller-owned four-page table set and can materialize them into caller-
//! supplied page buffers. It does not allocate pages, prove their physical
//! identity, write MMIO, perform cache maintenance/TLB invalidation, select an
//! SMMU route, or establish authority to use the Apps SMMU.
#![forbid(unsafe_code)]

use crate::sunfish_ipa_gsi_profile::{AP_DMA_PAGE_MAPPING_COUNT, DmaPageMapPlan, PAGE_BYTES};

pub const SMMU_LPAE_PAGE_BYTES: u64 = PAGE_BYTES as u64;
pub const LPAE_ENTRIES_PER_TABLE: usize = PAGE_BYTES as usize / core::mem::size_of::<u64>();
pub const AP_LPAE_TABLE_PAGE_COUNT: usize = 4;
pub const AP_LPAE_DESCRIPTOR_COUNT: usize = AP_DMA_PAGE_MAPPING_COUNT + 2;
/// Keep planner storage bounded to a 512-KiB DMA window even though an L3
/// table itself contains 512 entries. This covers the modeled CE rings and
/// bounded WMI control buffers.
pub const MAX_LPAE_PAGE_MAPPINGS: usize = 128;
pub const MAX_LPAE_DESCRIPTOR_COUNT: usize = MAX_LPAE_PAGE_MAPPINGS + 2;
/// Bounded extension for larger pools: eight leaf tables cover up to 16 MiB
/// of mappings within one 1-GiB TTBR0 root region.
pub const MAX_MULTI_LPAE_LEVEL3_TABLES: usize = 8;
pub const MAX_MULTI_LPAE_PAGE_MAPPINGS: usize =
    MAX_MULTI_LPAE_LEVEL3_TABLES * LPAE_ENTRIES_PER_TABLE;

const PAGE_MASK: u64 = SMMU_LPAE_PAGE_BYTES - 1;
pub const SMMU_LPAE_ADDRESS_MASK: u64 = 0x0000_ffff_ffff_f000;
const DESC_VALID: u64 = 1;
const DESC_TYPE_TABLE_OR_PAGE: u64 = 3;
const DESC_AF: u64 = 1 << 10;
const DESC_SH_OUTER_SHAREABLE: u64 = 2 << 8;
const DESC_AP_UNPRIV_RW: u64 = 1 << 6;
const DESC_NOT_GLOBAL: u64 = 1 << 11;
const DESC_XN: u64 = 3 << 53;
const DESC_NS: u64 = 1 << 5;
const DESC_NS_TABLE: u64 = 1 << 63;

const TCR_EPD1: u64 = 1 << 23;
const TCR_TG0_4K: u64 = 0;
const TCR_SH0_OUTER_SHAREABLE: u64 = 2 << 12;
const TCR_IRGN0_SHIFT: u32 = 8;
const TCR_ORGN0_SHIFT: u32 = 10;
const TCR_RGN_NC: u64 = 0;
const TCR_RGN_WBWA: u64 = 1;
const TCR_IPS_SHIFT: u32 = 32;
const TCR_T0SZ_MASK: u64 = 0x3f;
const TTBCR2_SEP_UPSTREAM: u32 = 0x7 << 15;
const TTBCR2_AS_16BIT: u32 = 1 << 4;
const TTBR_ASID_SHIFT: u32 = 48;

/// Linux's `qcom,use-3-lvl-tables` caps an AArch64 stage-1 context at 39 input
/// address bits. The actual value is the lesser of that cap and the SMMU's
/// reported input width, so it remains an explicit caller input.
pub const SUNFISH_LPAE_MAX_INPUT_BITS: u8 = 39;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableWalkPolicy {
    /// Linux's default when the SMMU page-table walker is not DMA-coherent.
    NonCoherent,
    /// Linux's `IO_PGTABLE_QUIRK_NO_DMA` policy for coherent table walks.
    Coherent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RingMemoryAttribute {
    /// MAIR AttrIdx 0: Normal non-cacheable.
    NormalNonCacheable,
    /// MAIR AttrIdx 1: Normal write-back, read/write allocate.
    NormalWriteBack,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LpaeS1Config {
    /// Hardware-reported input width, capped to 39 by the matched Sunfish DTS
    /// option. The selected width must cover every planned ring IOVA.
    pub input_address_bits: u8,
    /// Hardware-reported output/physical address width from SMMU ID registers.
    pub output_address_bits: u8,
    /// Caller-selected nonzero ASID for the delegated context bank.
    pub asid: u16,
    pub table_walk_policy: TableWalkPolicy,
    /// Ring data uses a normal-memory MAIR entry, never a Device entry.
    pub ring_memory_attribute: RingMemoryAttribute,
    /// Mirrors the matched kernel's `IO_PGTABLE_QUIRK_ARM_NS`, when explicitly
    /// selected by the trusted SMMU owner.
    pub arm_ns_quirk: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LpaeTablePages {
    /// Page-table root referenced by TTBR0.
    pub ttbr0_root: u64,
    /// Child table referenced by the single used TTBR0 root entry.
    pub level2: u64,
    /// Leaf page table for the contiguous ring mapping.
    pub level3: u64,
    /// Empty root referenced by TTBR1 while TCR.EPD1 disables TTBR1 walks.
    pub ttbr1_root: u64,
}

/// Caller-supplied table-frame identities for a bounded multi-L3 mapping.
/// One level-2 table covers all mappings, which must stay within one 1-GiB
/// TTBR0 root region. `level3` contains one frame for each occupied 2-MiB
/// level-2 region, in ascending IOVA order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LpaeMultiTablePages<'a> {
    pub ttbr0_root: u64,
    pub level2: u64,
    pub level3: &'a [u64],
    pub ttbr1_root: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LpaeTableLevel {
    Ttbr0Root,
    Level2,
    Level3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LpaeDescriptorWrite {
    pub table: LpaeTableLevel,
    pub index: u16,
    pub value: u64,
}

/// One page-granular DMA mapping for any bounded device pool.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LpaePageMapping {
    pub iova_page: u64,
    pub physical_page: u64,
}

/// Four page-sized, caller-owned CPU mappings for the table frames named by
/// an [`LpaeS1Plan`]. The caller is responsible for proving that these virtual
/// buffers refer to the supplied physical pages and are writable by this PD.
pub struct LpaeTableBuffers<'a> {
    pub physical_pages: LpaeTablePages,
    pub ttbr0_root: &'a mut [u64; LPAE_ENTRIES_PER_TABLE],
    pub level2: &'a mut [u64; LPAE_ENTRIES_PER_TABLE],
    pub level3: &'a mut [u64; LPAE_ENTRIES_PER_TABLE],
    pub ttbr1_root: &'a mut [u64; LPAE_ENTRIES_PER_TABLE],
}

/// CPU-writable views of multi-L3 table frames. The leaf-table storage is a
/// flat sequence of `level3.len() * LPAE_ENTRIES_PER_TABLE` entries.
pub struct LpaeMultiTableBuffers<'pages, 'buffers> {
    pub physical_pages: LpaeMultiTablePages<'pages>,
    pub ttbr0_root: &'buffers mut [u64],
    pub level2: &'buffers mut [u64],
    pub level3: &'buffers mut [u64],
    pub ttbr1_root: &'buffers mut [u64],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LpaeTableImageReceipt {
    pub written_descriptors: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LpaeContextRegisters {
    /// 64-bit TCR value; the low word is TTBCR and high IPS bits go to TTBCR2.
    pub tcr: u64,
    /// TTBCR2 value including the matched driver's AArch64 SEP and 16-bit ASID
    /// settings, plus the high TCR/IPS bits.
    pub ttbcr2: u32,
    pub ttbr0: u64,
    pub ttbr1: u64,
    pub mair0: u32,
    pub mair1: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LpaePlanError {
    InvalidInputAddressWidth,
    UnsupportedOutputAddressWidth,
    InvalidAsid,
    TablePageZero,
    TablePageMisaligned,
    TablePageOutOfRange,
    DuplicateTablePage,
    TablePageAliasesDmaPage,
    TableBackingIdentityMismatch,
    DmaPageOutOfRange,
    IovaOutOfRange,
    NonContiguousIovaPages,
    MappingSpansMultipleL1Entries,
    MappingSpansMultipleL2Entries,
    DuplicateDmaPage,
    DuplicateIovaPage,
    IovaPagesNotSorted,
    TooManyLevel3Tables,
    Level3TableCountMismatch,
    InvalidTableBufferLength,
    TooManyDmaPages,
    InvalidDescriptorIndex,
    DescriptorCountInvariant,
    RootTableCountMismatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LpaeS1Plan {
    table_pages: LpaeTablePages,
    context: LpaeContextRegisters,
    writes: [LpaeDescriptorWrite; MAX_LPAE_DESCRIPTOR_COUNT],
    write_count: usize,
}

/// Compact plan for a bounded, sorted page map with multiple L3 tables.
/// Mappings and table-frame addresses are borrowed; planning does not allocate
/// table storage or make an ownership claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LpaeMultiS1Plan<'a> {
    table_pages: LpaeMultiTablePages<'a>,
    mappings: &'a [LpaePageMapping],
    context: LpaeContextRegisters,
    l1_index: u16,
    l3_table_count: usize,
    arm_ns_quirk: bool,
    ring_memory_attribute: RingMemoryAttribute,
}

impl LpaeMultiS1Plan<'_> {
    pub const fn table_pages(&self) -> LpaeMultiTablePages<'_> {
        self.table_pages
    }

    pub const fn context(&self) -> LpaeContextRegisters {
        self.context
    }

    pub const fn mapping_count(&self) -> usize {
        self.mappings.len()
    }

    pub const fn level3_table_count(&self) -> usize {
        self.l3_table_count
    }

    /// Root entry, level-2 links, and leaf pages materialized by this plan.
    pub const fn descriptor_count(&self) -> usize {
        self.mappings.len() + self.l3_table_count + 1
    }

    /// Zero and fill the supplied ordinary-memory table images. This does not
    /// clean caches, issue barriers/TLB invalidations, access MMIO, or transfer
    /// table-page ownership.
    pub fn materialize_table_image(
        &self,
        buffers: &mut LpaeMultiTableBuffers<'_, '_>,
    ) -> Result<LpaeTableImageReceipt, LpaePlanError> {
        let expected_l3_entries = self
            .l3_table_count
            .checked_mul(LPAE_ENTRIES_PER_TABLE)
            .ok_or(LpaePlanError::InvalidTableBufferLength)?;
        if buffers.physical_pages != self.table_pages {
            return Err(LpaePlanError::TableBackingIdentityMismatch);
        }
        if buffers.ttbr0_root.len() != LPAE_ENTRIES_PER_TABLE
            || buffers.level2.len() != LPAE_ENTRIES_PER_TABLE
            || buffers.level3.len() != expected_l3_entries
            || buffers.ttbr1_root.len() != LPAE_ENTRIES_PER_TABLE
        {
            return Err(LpaePlanError::InvalidTableBufferLength);
        }

        buffers.ttbr0_root.fill(0);
        buffers.level2.fill(0);
        buffers.level3.fill(0);
        buffers.ttbr1_root.fill(0);

        let ns_table = if self.arm_ns_quirk { DESC_NS_TABLE } else { 0 };
        buffers.ttbr0_root[usize::from(self.l1_index)] =
            self.table_pages.level2 | DESC_VALID | DESC_TYPE_TABLE_OR_PAGE | ns_table;

        let ns_page = if self.arm_ns_quirk { DESC_NS } else { 0 };
        let attr_index = match self.ring_memory_attribute {
            RingMemoryAttribute::NormalNonCacheable => 0u64,
            RingMemoryAttribute::NormalWriteBack => 1u64,
        };
        let mut previous_l2 = None;
        let mut l3_table_index = 0usize;
        for mapping in self.mappings.iter().copied() {
            let l2_index = ((mapping.iova_page >> 21) & 0x1ff) as usize;
            if previous_l2 != Some(l2_index) {
                let table_page = self.table_pages.level3[l3_table_index];
                buffers.level2[l2_index] =
                    table_page | DESC_VALID | DESC_TYPE_TABLE_OR_PAGE | ns_table;
                previous_l2 = Some(l2_index);
                l3_table_index += 1;
            }
            let l3_index = ((mapping.iova_page >> 12) & 0x1ff) as usize;
            let entry = l3_table_index - 1;
            buffers.level3[entry * LPAE_ENTRIES_PER_TABLE + l3_index] = mapping.physical_page
                | DESC_VALID
                | DESC_TYPE_TABLE_OR_PAGE
                | DESC_AF
                | DESC_SH_OUTER_SHAREABLE
                | DESC_AP_UNPRIV_RW
                | DESC_NOT_GLOBAL
                | DESC_XN
                | (attr_index << 2)
                | ns_page;
        }

        Ok(LpaeTableImageReceipt {
            written_descriptors: self.descriptor_count() as u16,
        })
    }
}

impl LpaeS1Plan {
    pub const fn table_pages(&self) -> LpaeTablePages {
        self.table_pages
    }

    pub const fn context(&self) -> LpaeContextRegisters {
        self.context
    }

    pub fn descriptor_writes(&self) -> &[LpaeDescriptorWrite] {
        &self.writes[..self.write_count]
    }

    /// Zero the four caller-provided page buffers and write the bounded
    /// descriptors into them. This mutates ordinary memory only; it does not
    /// clean caches, issue barriers or TLB invalidations, touch MMIO, or make
    /// any page/table ownership claim.
    pub fn materialize_table_image(
        &self,
        buffers: &mut LpaeTableBuffers<'_>,
    ) -> Result<LpaeTableImageReceipt, LpaePlanError> {
        if buffers.physical_pages != self.table_pages {
            return Err(LpaePlanError::TableBackingIdentityMismatch);
        }
        if self
            .descriptor_writes()
            .iter()
            .any(|write| usize::from(write.index) >= LPAE_ENTRIES_PER_TABLE)
        {
            return Err(LpaePlanError::InvalidDescriptorIndex);
        }

        buffers.ttbr0_root.fill(0);
        buffers.level2.fill(0);
        buffers.level3.fill(0);
        buffers.ttbr1_root.fill(0);
        for write in self.descriptor_writes().iter().copied() {
            let table = match write.table {
                LpaeTableLevel::Ttbr0Root => &mut buffers.ttbr0_root[..],
                LpaeTableLevel::Level2 => &mut buffers.level2[..],
                LpaeTableLevel::Level3 => &mut buffers.level3[..],
            };
            table[usize::from(write.index)] = write.value;
        }
        Ok(LpaeTableImageReceipt {
            written_descriptors: self.write_count as u16,
        })
    }
}

/// Build the matched LPAE plan for the IPA AP's fourteen contiguous ring pages.
/// This compatibility wrapper keeps the IPA AP map type at its existing
/// source-derived size; generic bounded page maps use [`plan_lpae_s1`].
pub fn plan_ap_ring_lpae_s1(
    mappings: &DmaPageMapPlan,
    table_pages: LpaeTablePages,
    config: LpaeS1Config,
) -> Result<LpaeS1Plan, LpaePlanError> {
    let pages: [LpaePageMapping; AP_DMA_PAGE_MAPPING_COUNT] =
        core::array::from_fn(|index| LpaePageMapping {
            iova_page: mappings.pages()[index].iova_page,
            physical_page: mappings.pages()[index].physical_page,
        });
    plan_lpae_s1(&pages, table_pages, config)
}

/// Build a bounded 4-KiB, three-level AArch64 LPAE plan for contiguous
/// page-granular DMA mappings. Four caller-supplied table pages are required:
/// TTBR0 root, one level-2 table, one level-3 table, and the disabled TTBR1
/// root. Mappings must fit within one level-1 and one level-2 entry.
pub fn plan_lpae_s1(
    mappings: &[LpaePageMapping],
    table_pages: LpaeTablePages,
    config: LpaeS1Config,
) -> Result<LpaeS1Plan, LpaePlanError> {
    if mappings.is_empty() {
        return Err(LpaePlanError::DescriptorCountInvariant);
    }
    if mappings.len() > MAX_LPAE_PAGE_MAPPINGS {
        return Err(LpaePlanError::TooManyDmaPages);
    }
    if !(31..=SUNFISH_LPAE_MAX_INPUT_BITS).contains(&config.input_address_bits) {
        return Err(LpaePlanError::InvalidInputAddressWidth);
    }
    let output_encoding = output_size_encoding(config.output_address_bits)
        .ok_or(LpaePlanError::UnsupportedOutputAddressWidth)?;
    if config.asid == 0 || config.asid == u16::MAX {
        return Err(LpaePlanError::InvalidAsid);
    }

    let output_limit = 1u64 << config.output_address_bits;
    let pages = [
        table_pages.ttbr0_root,
        table_pages.level2,
        table_pages.level3,
        table_pages.ttbr1_root,
    ];
    for page in pages {
        if page == 0 {
            return Err(LpaePlanError::TablePageZero);
        }
        if page & PAGE_MASK != 0 {
            return Err(LpaePlanError::TablePageMisaligned);
        }
        let end = page
            .checked_add(SMMU_LPAE_PAGE_BYTES - 1)
            .ok_or(LpaePlanError::TablePageOutOfRange)?;
        if end >= output_limit || end >= (1u64 << 48) {
            return Err(LpaePlanError::TablePageOutOfRange);
        }
    }
    for i in 0..pages.len() {
        if pages[..i].contains(&pages[i]) {
            return Err(LpaePlanError::DuplicateTablePage);
        }
        if mappings
            .iter()
            .any(|mapping| mapping.physical_page == pages[i])
        {
            return Err(LpaePlanError::TablePageAliasesDmaPage);
        }
    }

    let iova_limit = 1u64 << config.input_address_bits;
    let mut first_l1 = None;
    let mut first_l2 = None;
    let mut previous_iova = None;
    for (index, mapping) in mappings.iter().enumerate() {
        if mapping.iova_page & PAGE_MASK != 0 {
            return Err(LpaePlanError::IovaOutOfRange);
        }
        let iova_end = mapping
            .iova_page
            .checked_add(SMMU_LPAE_PAGE_BYTES)
            .ok_or(LpaePlanError::IovaOutOfRange)?;
        if iova_end > iova_limit {
            return Err(LpaePlanError::IovaOutOfRange);
        }
        let pa_end = mapping
            .physical_page
            .checked_add(SMMU_LPAE_PAGE_BYTES - 1)
            .ok_or(LpaePlanError::DmaPageOutOfRange)?;
        if mapping.physical_page & PAGE_MASK != 0
            || pa_end >= output_limit
            || pa_end >= (1u64 << 48)
        {
            return Err(LpaePlanError::DmaPageOutOfRange);
        }
        if mappings[..index]
            .iter()
            .any(|old| old.physical_page == mapping.physical_page)
        {
            return Err(LpaePlanError::DuplicateDmaPage);
        }
        if let Some(previous) = previous_iova {
            if mapping.iova_page != previous + SMMU_LPAE_PAGE_BYTES {
                return Err(LpaePlanError::NonContiguousIovaPages);
            }
        }
        previous_iova = Some(mapping.iova_page);
        let l1 = ((mapping.iova_page >> 30) & 0x1ff) as u16;
        let l2 = ((mapping.iova_page >> 21) & 0x1ff) as u16;
        if let Some(root_index) = first_l1 {
            if l1 != root_index {
                return Err(LpaePlanError::MappingSpansMultipleL1Entries);
            }
        } else {
            first_l1 = Some(l1);
        }
        if let Some(level2_index) = first_l2 {
            if l2 != level2_index {
                return Err(LpaePlanError::MappingSpansMultipleL2Entries);
            }
        } else {
            first_l2 = Some(l2);
        }
    }

    let l1_index = first_l1.ok_or(LpaePlanError::DescriptorCountInvariant)?;
    let l2_index = first_l2.ok_or(LpaePlanError::DescriptorCountInvariant)?;
    let ns_table = if config.arm_ns_quirk {
        DESC_NS_TABLE
    } else {
        0
    };
    let ns_page = if config.arm_ns_quirk { DESC_NS } else { 0 };
    let mut writes = [LpaeDescriptorWrite {
        table: LpaeTableLevel::Ttbr0Root,
        index: 0,
        value: 0,
    }; MAX_LPAE_DESCRIPTOR_COUNT];
    writes[0] = LpaeDescriptorWrite {
        table: LpaeTableLevel::Ttbr0Root,
        index: l1_index,
        value: table_pages.level2 | DESC_VALID | DESC_TYPE_TABLE_OR_PAGE | ns_table,
    };
    writes[1] = LpaeDescriptorWrite {
        table: LpaeTableLevel::Level2,
        index: l2_index,
        value: table_pages.level3 | DESC_VALID | DESC_TYPE_TABLE_OR_PAGE | ns_table,
    };
    let attr_index = match config.ring_memory_attribute {
        RingMemoryAttribute::NormalNonCacheable => 0u64,
        RingMemoryAttribute::NormalWriteBack => 1u64,
    };
    for (index, mapping) in mappings.iter().enumerate() {
        let l3_index = ((mapping.iova_page >> 12) & 0x1ff) as u16;
        writes[index + 2] = LpaeDescriptorWrite {
            table: LpaeTableLevel::Level3,
            index: l3_index,
            value: mapping.physical_page
                | DESC_VALID
                | DESC_TYPE_TABLE_OR_PAGE
                | DESC_AF
                | DESC_SH_OUTER_SHAREABLE
                | DESC_AP_UNPRIV_RW
                | DESC_NOT_GLOBAL
                | DESC_XN
                | (attr_index << 2)
                | ns_page,
        };
    }

    let walk_rgn = match config.table_walk_policy {
        TableWalkPolicy::NonCoherent => TCR_RGN_NC,
        TableWalkPolicy::Coherent => TCR_RGN_WBWA,
    };
    let tcr = (u64::from(64 - config.input_address_bits) & TCR_T0SZ_MASK)
        | TCR_EPD1
        | TCR_TG0_4K
        | TCR_SH0_OUTER_SHAREABLE
        | (walk_rgn << TCR_IRGN0_SHIFT)
        | (walk_rgn << TCR_ORGN0_SHIFT)
        | (u64::from(output_encoding) << TCR_IPS_SHIFT);
    let ttbcr2 = ((tcr >> 32) as u32) | TTBCR2_SEP_UPSTREAM | TTBCR2_AS_16BIT;
    let asid = u64::from(config.asid) << TTBR_ASID_SHIFT;
    let context = LpaeContextRegisters {
        tcr,
        ttbcr2,
        ttbr0: table_pages.ttbr0_root | asid,
        ttbr1: table_pages.ttbr1_root | asid,
        mair0: 0xf404_ff44,
        mair1: 0x0000_00e4,
    };

    Ok(LpaeS1Plan {
        table_pages,
        context,
        writes,
        write_count: mappings.len() + 2,
    })
}

/// Build a bounded three-level LPAE plan using one level-2 table and multiple
/// level-3 tables. Mappings may contain IOVA gaps, but must be sorted by IOVA,
/// have distinct physical pages, and remain within one 1-GiB root region.
/// Eight leaf tables bound the plan to 16 MiB and keep table-image storage
/// caller-owned. This supports the multi-megabyte HTT pool profile while
/// preserving the legacy 128-page planner for existing IPA and CE/WMI users.
pub fn plan_lpae_s1_multi<'a>(
    mappings: &'a [LpaePageMapping],
    table_pages: LpaeMultiTablePages<'a>,
    config: LpaeS1Config,
) -> Result<LpaeMultiS1Plan<'a>, LpaePlanError> {
    if mappings.is_empty() {
        return Err(LpaePlanError::DescriptorCountInvariant);
    }
    if mappings.len() > MAX_MULTI_LPAE_PAGE_MAPPINGS {
        return Err(LpaePlanError::TooManyDmaPages);
    }
    if !(31..=SUNFISH_LPAE_MAX_INPUT_BITS).contains(&config.input_address_bits) {
        return Err(LpaePlanError::InvalidInputAddressWidth);
    }
    let output_encoding = output_size_encoding(config.output_address_bits)
        .ok_or(LpaePlanError::UnsupportedOutputAddressWidth)?;
    if config.asid == 0 || config.asid == u16::MAX {
        return Err(LpaePlanError::InvalidAsid);
    }

    let output_limit = 1u64 << config.output_address_bits;
    let iova_limit = 1u64 << config.input_address_bits;
    let mut l1_index = None;
    let mut previous_iova = None;
    let mut previous_l2 = None;
    let mut l3_table_count = 0usize;
    for (index, mapping) in mappings.iter().enumerate() {
        if mapping.iova_page & PAGE_MASK != 0 {
            return Err(LpaePlanError::IovaOutOfRange);
        }
        let iova_end = mapping
            .iova_page
            .checked_add(SMMU_LPAE_PAGE_BYTES)
            .ok_or(LpaePlanError::IovaOutOfRange)?;
        if iova_end > iova_limit {
            return Err(LpaePlanError::IovaOutOfRange);
        }
        if let Some(previous) = previous_iova {
            if mapping.iova_page == previous {
                return Err(LpaePlanError::DuplicateIovaPage);
            }
            if mapping.iova_page < previous {
                return Err(LpaePlanError::IovaPagesNotSorted);
            }
        }
        previous_iova = Some(mapping.iova_page);

        let pa_end = mapping
            .physical_page
            .checked_add(SMMU_LPAE_PAGE_BYTES - 1)
            .ok_or(LpaePlanError::DmaPageOutOfRange)?;
        if mapping.physical_page & PAGE_MASK != 0
            || pa_end >= output_limit
            || pa_end >= (1u64 << 48)
        {
            return Err(LpaePlanError::DmaPageOutOfRange);
        }
        if mappings[..index]
            .iter()
            .any(|old| old.physical_page == mapping.physical_page)
        {
            return Err(LpaePlanError::DuplicateDmaPage);
        }

        let current_l1 = ((mapping.iova_page >> 30) & 0x1ff) as u16;
        if let Some(previous_l1) = l1_index {
            if current_l1 != previous_l1 {
                return Err(LpaePlanError::MappingSpansMultipleL1Entries);
            }
        } else {
            l1_index = Some(current_l1);
        }

        let current_l2 = ((mapping.iova_page >> 21) & 0x1ff) as u16;
        if previous_l2 != Some(current_l2) {
            l3_table_count += 1;
            if l3_table_count > MAX_MULTI_LPAE_LEVEL3_TABLES {
                return Err(LpaePlanError::TooManyLevel3Tables);
            }
            previous_l2 = Some(current_l2);
        }
    }

    if table_pages.level3.len() != l3_table_count {
        return Err(LpaePlanError::Level3TableCountMismatch);
    }

    let fixed_table_pages = [
        table_pages.ttbr0_root,
        table_pages.level2,
        table_pages.ttbr1_root,
    ];
    for (index, page) in fixed_table_pages.iter().copied().enumerate() {
        validate_table_page(page, output_limit)?;
        if fixed_table_pages[..index].contains(&page) || table_pages.level3.contains(&page) {
            return Err(LpaePlanError::DuplicateTablePage);
        }
        if mappings.iter().any(|mapping| mapping.physical_page == page) {
            return Err(LpaePlanError::TablePageAliasesDmaPage);
        }
    }
    for (index, page) in table_pages.level3.iter().copied().enumerate() {
        validate_table_page(page, output_limit)?;
        if table_pages.level3[..index].contains(&page) {
            return Err(LpaePlanError::DuplicateTablePage);
        }
        if mappings.iter().any(|mapping| mapping.physical_page == page) {
            return Err(LpaePlanError::TablePageAliasesDmaPage);
        }
    }

    let l1_index = l1_index.ok_or(LpaePlanError::DescriptorCountInvariant)?;
    let walk_rgn = match config.table_walk_policy {
        TableWalkPolicy::NonCoherent => TCR_RGN_NC,
        TableWalkPolicy::Coherent => TCR_RGN_WBWA,
    };
    let tcr = (u64::from(64 - config.input_address_bits) & TCR_T0SZ_MASK)
        | TCR_EPD1
        | TCR_TG0_4K
        | TCR_SH0_OUTER_SHAREABLE
        | (walk_rgn << TCR_IRGN0_SHIFT)
        | (walk_rgn << TCR_ORGN0_SHIFT)
        | (u64::from(output_encoding) << TCR_IPS_SHIFT);
    let context = LpaeContextRegisters {
        tcr,
        ttbcr2: ((tcr >> 32) as u32) | TTBCR2_SEP_UPSTREAM | TTBCR2_AS_16BIT,
        ttbr0: table_pages.ttbr0_root | (u64::from(config.asid) << TTBR_ASID_SHIFT),
        ttbr1: table_pages.ttbr1_root | (u64::from(config.asid) << TTBR_ASID_SHIFT),
        mair0: 0xf404_ff44,
        mair1: 0x0000_00e4,
    };

    Ok(LpaeMultiS1Plan {
        table_pages,
        mappings,
        context,
        l1_index,
        l3_table_count,
        arm_ns_quirk: config.arm_ns_quirk,
        ring_memory_attribute: config.ring_memory_attribute,
    })
}

/// Bounded IPA shared-memory geometry: two occupied 1-GiB root entries,
/// three occupied 2-MiB leaf regions, plus an empty disabled TTBR1 root.
/// These are caller-owned frame identities, never allocation authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LpaeRootTablePages<const LEAVES: usize = 3> {
    pub ttbr0_root: u64,
    pub level2: [u64; 2],
    pub level3: [u64; LEAVES],
    pub ttbr1_root: u64,
}

impl LpaeRootTablePages {
    pub const fn pages(self) -> [u64; 7] {
        [
            self.ttbr0_root,
            self.level2[0],
            self.level2[1],
            self.level3[0],
            self.level3[1],
            self.level3[2],
            self.ttbr1_root,
        ]
    }
}

impl LpaeRootTablePages<4> {
    pub const fn pages(self) -> [u64; 8] {
        [
            self.ttbr0_root,
            self.level2[0],
            self.level2[1],
            self.level3[0],
            self.level3[1],
            self.level3[2],
            self.level3[3],
            self.ttbr1_root,
        ]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LpaeRootS1Plan<'a, const LEAVES: usize = 3> {
    pages: LpaeRootTablePages<LEAVES>,
    mappings: &'a [LpaePageMapping],
    context: LpaeContextRegisters,
    roots: [u16; 2],
    leaves: [(u16, u16); LEAVES],
    config: LpaeS1Config,
}

impl<const LEAVES: usize> LpaeRootS1Plan<'_, LEAVES> {
    pub const fn table_pages(&self) -> LpaeRootTablePages<LEAVES> {
        self.pages
    }
    pub const fn context(&self) -> LpaeContextRegisters {
        self.context
    }
    pub const fn mappings(&self) -> &[LpaePageMapping] {
        self.mappings
    }
    pub const fn root_indices(&self) -> [u16; 2] {
        self.roots
    }
    pub const fn descriptor_count(&self) -> usize {
        self.mappings.len() + 2 + LEAVES
    }

    /// Materialize one table into caller-owned memory for bounded readback.
    pub fn materialize_table_page(
        &self,
        physical: LpaeRootTablePages<LEAVES>,
        table: usize,
        page: &mut [u64; LPAE_ENTRIES_PER_TABLE],
    ) -> Result<(), LpaePlanError> {
        if physical != self.pages {
            return Err(LpaePlanError::TableBackingIdentityMismatch);
        }
        if table >= LEAVES + 4 {
            return Err(LpaePlanError::DescriptorCountInvariant);
        }
        page.fill(0);
        let ns_table = if self.config.arm_ns_quirk { DESC_NS_TABLE } else { 0 };
        if table == 0 {
            for (i, root) in self.roots.iter().copied().enumerate() {
                page[root as usize] = self.pages.level2[i] | DESC_TYPE_TABLE_OR_PAGE | ns_table;
            }
        } else if table == 1 || table == 2 {
            for (i, (root, leaf)) in self.leaves.iter().copied().enumerate() {
                let root_table = if root == self.roots[0] { 1 } else { 2 };
                if root_table == table {
                    page[leaf as usize] = self.pages.level3[i] | DESC_TYPE_TABLE_OR_PAGE | ns_table;
                }
            }
        } else if table < LEAVES + 3 {
            let attr = match self.config.ring_memory_attribute {
                RingMemoryAttribute::NormalNonCacheable => 0,
                RingMemoryAttribute::NormalWriteBack => 1 << 2,
            };
            let ns_page = if self.config.arm_ns_quirk { DESC_NS } else { 0 };
            for mapping in self.mappings {
                let key = (((mapping.iova_page >> 30) & 511) as u16, ((mapping.iova_page >> 21) & 511) as u16);
                let leaf = self.leaves.iter().position(|candidate| *candidate == key)
                    .ok_or(LpaePlanError::DescriptorCountInvariant)?;
                if table == 3 + leaf {
                    page[((mapping.iova_page >> 12) & 511) as usize] = mapping.physical_page
                        | DESC_TYPE_TABLE_OR_PAGE | DESC_AF | DESC_SH_OUTER_SHAREABLE
                        | DESC_AP_UNPRIV_RW | DESC_NOT_GLOBAL | DESC_XN | attr | ns_page;
                }
            }
        }
        Ok(())
    }

    /// Write only ordinary caller-owned table buffers. No MMIO, cache/TLB
    /// publication, physical ownership or device readiness is established.
    pub fn materialize_table_image<const TABLES: usize>(
        &self,
        physical: LpaeRootTablePages<LEAVES>,
        image: &mut [[u64; LPAE_ENTRIES_PER_TABLE]; TABLES],
    ) -> Result<LpaeTableImageReceipt, LpaePlanError> {
        if physical != self.pages {
            return Err(LpaePlanError::TableBackingIdentityMismatch);
        }
        if TABLES != LEAVES + 4 {
            return Err(LpaePlanError::DescriptorCountInvariant);
        }
        image.iter_mut().for_each(|page| page.fill(0));
        let ns_table = if self.config.arm_ns_quirk {
            DESC_NS_TABLE
        } else {
            0
        };
        for (i, root) in self.roots.iter().copied().enumerate() {
            image[0][root as usize] = self.pages.level2[i] | DESC_TYPE_TABLE_OR_PAGE | ns_table;
        }
        for (i, (root, leaf)) in self.leaves.iter().copied().enumerate() {
            let root_table = if root == self.roots[0] { 1 } else { 2 };
            image[root_table][leaf as usize] =
                self.pages.level3[i] | DESC_TYPE_TABLE_OR_PAGE | ns_table;
        }
        let attr = match self.config.ring_memory_attribute {
            RingMemoryAttribute::NormalNonCacheable => 0,
            RingMemoryAttribute::NormalWriteBack => 1 << 2,
        };
        let ns_page = if self.config.arm_ns_quirk { DESC_NS } else { 0 };
        for mapping in self.mappings {
            let key = (
                ((mapping.iova_page >> 30) & 511) as u16,
                ((mapping.iova_page >> 21) & 511) as u16,
            );
            let leaf = self
                .leaves
                .iter()
                .position(|candidate| *candidate == key)
                .ok_or(LpaePlanError::DescriptorCountInvariant)?;
            image[3 + leaf][((mapping.iova_page >> 12) & 511) as usize] = mapping.physical_page
                | DESC_TYPE_TABLE_OR_PAGE
                | DESC_AF
                | DESC_SH_OUTER_SHAREABLE
                | DESC_AP_UNPRIV_RW
                | DESC_NOT_GLOBAL
                | DESC_XN
                | attr
                | ns_page;
        }
        Ok(LpaeTableImageReceipt {
            written_descriptors: self.descriptor_count() as u16,
        })
    }
}

/// Plan up to 128 distinct pages across exactly two root entries and three
/// leaf regions. Both root and leaf table identities are ordered by IOVA.
/// The older one-root APIs retain their original bounds and behavior.
pub fn plan_lpae_s1_roots<'a>(
    mappings: &'a [LpaePageMapping],
    pages: LpaeRootTablePages,
    config: LpaeS1Config,
) -> Result<LpaeRootS1Plan<'a>, LpaePlanError> {
    plan_lpae_root_family(
        mappings,
        pages,
        config,
        &pages.pages(),
        MAX_LPAE_PAGE_MAPPINGS,
    )
}

/// Full exception-RX extension: 37 established shared/command pages plus
/// 502 receive-data pages, across four occupied 2-MiB regions. The maximum
/// remains separate from the older 128-page, three-leaf API. Neither this
/// plan nor its frame addresses establishes allocation or DMA authority.
pub const MAX_LPAE_RECEIVE_PAGE_MAPPINGS: usize = 37 + 502;

pub fn plan_lpae_s1_receive_roots<'a>(
    mappings: &'a [LpaePageMapping],
    pages: LpaeRootTablePages<4>,
    config: LpaeS1Config,
) -> Result<LpaeRootS1Plan<'a, 4>, LpaePlanError> {
    plan_lpae_root_family(
        mappings,
        pages,
        config,
        &pages.pages(),
        MAX_LPAE_RECEIVE_PAGE_MAPPINGS,
    )
}

// Incremental membership only: the caller still validates each page and
// preserves the first-failure order of the original prefix scan. A saturated
// table falls back to that scan rather than accepting an unchecked page.
#[cfg(feature = "sunfish-smmu-root-unique-fast-check")]
fn root_page_seen(table: &mut [u64; 1024], page: u64, previous: &[LpaePageMapping]) -> bool {
    const EMPTY: u64 = u64::MAX;
    let mut slot = ((page >> 12).wrapping_mul(0x9e37_79b9_7f4a_7c15) as usize) & 1023;
    for _ in 0..table.len() {
        if table[slot] == page { return true; }
        if table[slot] == EMPTY { table[slot] = page; return false; }
        slot = (slot + 1) & 1023;
    }
    previous.iter().any(|old| old.physical_page == page)
}

fn plan_lpae_root_family<'a, const LEAVES: usize>(
    mappings: &'a [LpaePageMapping],
    pages: LpaeRootTablePages<LEAVES>,
    config: LpaeS1Config,
    physical: &[u64],
    maximum: usize,
) -> Result<LpaeRootS1Plan<'a, LEAVES>, LpaePlanError> {
    if mappings.is_empty() {
        return Err(LpaePlanError::DescriptorCountInvariant);
    }
    if mappings.len() > maximum {
        return Err(LpaePlanError::TooManyDmaPages);
    }
    // Reuse the validated context encoding and configuration checks. A
    // single-page pure plan suffices for those values; no table is published.
    let context = plan_lpae_s1(
        &mappings[..1],
        LpaeTablePages {
            ttbr0_root: pages.ttbr0_root,
            level2: pages.level2[0],
            level3: pages.level3[0],
            ttbr1_root: pages.ttbr1_root,
        },
        config,
    )?
    .context();
    for (i, page) in physical.iter().copied().enumerate() {
        validate_table_page(page, 1u64 << config.output_address_bits)?;
        if physical[..i].contains(&page) {
            return Err(LpaePlanError::DuplicateTablePage);
        }
        if mappings.iter().any(|mapping| mapping.physical_page == page) {
            return Err(LpaePlanError::TablePageAliasesDmaPage);
        }
    }
    let mut roots = [0xffff; 2];
    let mut root_count = 0usize;
    let mut leaves = [(0xffff, 0xffff); LEAVES];
    let mut leaf_count = 0usize;
    #[cfg(feature = "sunfish-smmu-root-unique-fast-check")]
    let mut seen_physical = [u64::MAX; 1024];
    for (i, mapping) in mappings.iter().enumerate() {
        let end = mapping
            .iova_page
            .checked_add(SMMU_LPAE_PAGE_BYTES)
            .ok_or(LpaePlanError::IovaOutOfRange)?;
        if mapping.iova_page & PAGE_MASK != 0 || end > 1u64 << config.input_address_bits {
            return Err(LpaePlanError::IovaOutOfRange);
        }
        let pa_end = mapping
            .physical_page
            .checked_add(PAGE_MASK)
            .ok_or(LpaePlanError::DmaPageOutOfRange)?;
        if mapping.physical_page & PAGE_MASK != 0 || pa_end >= 1u64 << config.output_address_bits {
            return Err(LpaePlanError::DmaPageOutOfRange);
        }
        if i != 0 && mapping.iova_page == mappings[i - 1].iova_page {
            return Err(LpaePlanError::DuplicateIovaPage);
        }
        if i != 0 && mapping.iova_page < mappings[i - 1].iova_page {
            return Err(LpaePlanError::IovaPagesNotSorted);
        }
        #[cfg(feature = "sunfish-smmu-root-unique-fast-check")]
        let duplicate = root_page_seen(&mut seen_physical, mapping.physical_page, &mappings[..i]);
        #[cfg(not(feature = "sunfish-smmu-root-unique-fast-check"))]
        let duplicate = mappings[..i].iter().any(|old| old.physical_page == mapping.physical_page);
        if duplicate {
            return Err(LpaePlanError::DuplicateDmaPage);
        }
        let root = ((mapping.iova_page >> 30) & 511) as u16;
        let leaf = ((mapping.iova_page >> 21) & 511) as u16;
        if root_count == 0 || roots[root_count - 1] != root {
            if root_count == roots.len() {
                return Err(LpaePlanError::RootTableCountMismatch);
            }
            roots[root_count] = root;
            root_count += 1;
        }
        if leaf_count == 0 || leaves[leaf_count - 1] != (root, leaf) {
            if leaf_count == leaves.len() {
                return Err(LpaePlanError::Level3TableCountMismatch);
            }
            leaves[leaf_count] = (root, leaf);
            leaf_count += 1;
        }
    }
    if root_count != 2 {
        return Err(LpaePlanError::RootTableCountMismatch);
    }
    if leaf_count != LEAVES {
        return Err(LpaePlanError::Level3TableCountMismatch);
    }
    Ok(LpaeRootS1Plan {
        pages,
        mappings,
        context,
        roots,
        leaves,
        config,
    })
}

fn validate_table_page(page: u64, output_limit: u64) -> Result<(), LpaePlanError> {
    if page == 0 {
        return Err(LpaePlanError::TablePageZero);
    }
    if page & PAGE_MASK != 0 {
        return Err(LpaePlanError::TablePageMisaligned);
    }
    let end = page
        .checked_add(SMMU_LPAE_PAGE_BYTES - 1)
        .ok_or(LpaePlanError::TablePageOutOfRange)?;
    if end >= output_limit || end >= (1u64 << 48) {
        return Err(LpaePlanError::TablePageOutOfRange);
    }
    Ok(())
}

const fn output_size_encoding(bits: u8) -> Option<u8> {
    match bits {
        32 => Some(0),
        36 => Some(1),
        40 => Some(2),
        42 => Some(3),
        44 => Some(4),
        48 => Some(5),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sunfish_ipa_gsi_profile::{
        DmaIovaWindow, DmaPool, LINUX_IPA_AP_IOVA_BYTES, LINUX_IPA_AP_IOVA_START,
        plan_ap_dma_page_mappings, plan_ap_dma_rings,
    };

    #[cfg(feature = "sunfish-smmu-root-unique-fast-check")]
    #[test]
    fn root_unique_lookup_matches_prefix_scan_with_collisions_and_saturation() {
        let mut seed = 0x7135_5e11_u64;
        for run in 0..32 {
            let mut table = [u64::MAX; 1024];
            let mut previous: std::vec::Vec<LpaePageMapping> = std::vec::Vec::new();
            for index in 0..MAX_LPAE_RECEIVE_PAGE_MAPPINGS {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let page = if index > 0 && index % 13 == 0 {
                    previous[(seed as usize) % previous.len()].physical_page
                } else if run == 0 {
                    // Every page hashes to the same initial slot.
                    (index as u64) << 22
                } else {
                    (seed & 0x0fff_ffff) << 12
                };
                let expected = previous.iter().any(|p: &LpaePageMapping| p.physical_page == page);
                assert_eq!(root_page_seen(&mut table, page, &previous), expected, "run {run}, index {index}");
                previous.push(LpaePageMapping { iova_page: index as u64 * 4096, physical_page: page });
            }
        }
        let mut table = [u64::MAX; 1024];
        let mut previous = std::vec::Vec::new();
        for index in 0..1024 {
            let page = (index as u64) << 12;
            assert!(!root_page_seen(&mut table, page, &previous));
            previous.push(LpaePageMapping { iova_page: page, physical_page: page });
        }
        let page = 1024u64 << 12;
        assert!(!root_page_seen(&mut table, page, &previous));
        previous.push(LpaePageMapping { iova_page: page, physical_page: page });
        assert!(root_page_seen(&mut table, page, &previous));
    }

    fn inputs(pool_base: u64) -> (DmaPageMapPlan, LpaeTablePages, LpaeS1Config) {
        let ring_plan = plan_ap_dma_rings(DmaPool {
            base_iova: pool_base,
            bytes: 0xe000,
        })
        .unwrap();
        let physical_pages: [u64; AP_DMA_PAGE_MAPPING_COUNT] =
            core::array::from_fn(|index| 0x8800_0000 + index as u64 * 0x1000);
        let page_map = plan_ap_dma_page_mappings(
            &ring_plan,
            DmaIovaWindow {
                base_iova: LINUX_IPA_AP_IOVA_START,
                bytes: LINUX_IPA_AP_IOVA_BYTES,
            },
            &physical_pages,
            40,
        )
        .unwrap();
        let table_pages = LpaeTablePages {
            ttbr0_root: 0x9000_0000,
            level2: 0x9000_1000,
            level3: 0x9000_2000,
            ttbr1_root: 0x9000_3000,
        };
        let config = LpaeS1Config {
            input_address_bits: 39,
            output_address_bits: 40,
            asid: 2,
            table_walk_policy: TableWalkPolicy::NonCoherent,
            ring_memory_attribute: RingMemoryAttribute::NormalNonCacheable,
            arm_ns_quirk: false,
        };
        (page_map, table_pages, config)
    }

    fn resolve_materialized_page(
        tables: &[[u64; LPAE_ENTRIES_PER_TABLE]; AP_LPAE_TABLE_PAGE_COUNT],
        iova: u64,
    ) -> Option<u64> {
        let l1_index = ((iova >> 30) & 0x1ff) as usize;
        let l1 = tables[0][l1_index];
        if l1 & 0x3 != DESC_VALID | DESC_TYPE_TABLE_OR_PAGE {
            return None;
        }

        let l2_index = ((iova >> 21) & 0x1ff) as usize;
        let l2 = tables[1][l2_index];
        if l2 & 0x3 != DESC_VALID | DESC_TYPE_TABLE_OR_PAGE {
            return None;
        }

        let l3_index = ((iova >> 12) & 0x1ff) as usize;
        let l3 = tables[2][l3_index];
        if l3 & 0x3 != DESC_VALID | DESC_TYPE_TABLE_OR_PAGE {
            return None;
        }
        Some((l3 & SMMU_LPAE_ADDRESS_MASK) | (iova & PAGE_MASK))
    }

    #[test]
    fn emits_source_matched_three_level_s1_registers_and_fourteen_leaf_pages() {
        let (mappings, pages, config) = inputs(LINUX_IPA_AP_IOVA_START);
        let plan = plan_ap_ring_lpae_s1(&mappings, pages, config).unwrap();
        let context = plan.context();
        assert_eq!(context.tcr, (2u64 << 32) | 0x0080_2019);
        assert_eq!(context.ttbcr2, 0x0003_8012);
        assert_eq!(context.ttbr0, pages.ttbr0_root | (2u64 << 48));
        assert_eq!(context.ttbr1, pages.ttbr1_root | (2u64 << 48));
        assert_eq!(context.mair0, 0xf404_ff44);
        assert_eq!(context.mair1, 0xe4);

        let writes = plan.descriptor_writes();
        assert_eq!(writes.len(), 16);
        assert_eq!(writes[0].table, LpaeTableLevel::Ttbr0Root);
        assert_eq!(writes[0].index, 0);
        assert_eq!(writes[0].value, pages.level2 | 3);
        assert_eq!(writes[1].table, LpaeTableLevel::Level2);
        assert_eq!(writes[1].index, 256);
        assert_eq!(writes[1].value, pages.level3 | 3);
        for (index, mapping) in mappings.pages().iter().enumerate() {
            let write = writes[index + 2];
            assert_eq!(write.table, LpaeTableLevel::Level3);
            assert_eq!(write.index, index as u16);
            assert_eq!(write.value & SMMU_LPAE_ADDRESS_MASK, mapping.physical_page);
            assert_eq!(write.value & 0x3, DESC_TYPE_TABLE_OR_PAGE);
            assert_ne!(write.value & DESC_AF, 0);
            assert_ne!(write.value & DESC_AP_UNPRIV_RW, 0);
            assert_eq!(write.value & DESC_NS, 0);
        }
    }

    #[test]
    fn applies_explicit_coherency_cacheability_and_nonsecure_quirk() {
        let (mappings, pages, mut config) = inputs(LINUX_IPA_AP_IOVA_START);
        config.table_walk_policy = TableWalkPolicy::Coherent;
        config.ring_memory_attribute = RingMemoryAttribute::NormalWriteBack;
        config.arm_ns_quirk = true;
        let plan = plan_ap_ring_lpae_s1(&mappings, pages, config).unwrap();
        assert_eq!(plan.context().tcr, (2u64 << 32) | 0x0080_2519);
        assert_eq!(
            plan.descriptor_writes()[0].value & DESC_NS_TABLE,
            DESC_NS_TABLE
        );
        assert_eq!(plan.descriptor_writes()[2].value & DESC_NS, DESC_NS);
        assert_eq!(plan.descriptor_writes()[2].value & (7 << 2), 1 << 2);
    }

    #[test]
    fn materializes_only_the_planned_entries_into_exact_caller_pages() {
        let (mappings, pages, config) = inputs(LINUX_IPA_AP_IOVA_START);
        let plan = plan_ap_ring_lpae_s1(&mappings, pages, config).unwrap();
        let mut storage: [[u64; LPAE_ENTRIES_PER_TABLE]; AP_LPAE_TABLE_PAGE_COUNT] =
            core::array::from_fn(|_| [u64::MAX; LPAE_ENTRIES_PER_TABLE]);
        let receipt;
        {
            let [ttbr0_root, level2, level3, ttbr1_root] = &mut storage;
            let mut buffers = LpaeTableBuffers {
                physical_pages: pages,
                ttbr0_root,
                level2,
                level3,
                ttbr1_root,
            };
            receipt = plan.materialize_table_image(&mut buffers).unwrap();
        }
        assert_eq!(
            receipt,
            LpaeTableImageReceipt {
                written_descriptors: 16
            }
        );
        assert_eq!(storage[0][0], pages.level2 | 3);
        assert_eq!(storage[0][1], 0);
        assert_eq!(storage[1][256], pages.level3 | 3);
        assert_eq!(storage[1][255], 0);
        for (index, mapping) in mappings.pages().iter().enumerate() {
            assert_eq!(
                storage[2][index] & SMMU_LPAE_ADDRESS_MASK,
                mapping.physical_page
            );
        }
        assert!(
            storage[2][AP_DMA_PAGE_MAPPING_COUNT..]
                .iter()
                .all(|entry| *entry == 0)
        );
        assert!(storage[3].iter().all(|entry| *entry == 0));
    }

    #[test]
    fn materialized_walk_resolves_only_the_fourteen_planned_ring_pages() {
        let (mappings, pages, config) = inputs(LINUX_IPA_AP_IOVA_START);
        let plan = plan_ap_ring_lpae_s1(&mappings, pages, config).unwrap();
        let mut storage: [[u64; LPAE_ENTRIES_PER_TABLE]; AP_LPAE_TABLE_PAGE_COUNT] =
            [[0; LPAE_ENTRIES_PER_TABLE]; AP_LPAE_TABLE_PAGE_COUNT];
        {
            let [ttbr0_root, level2, level3, ttbr1_root] = &mut storage;
            let mut buffers = LpaeTableBuffers {
                physical_pages: pages,
                ttbr0_root,
                level2,
                level3,
                ttbr1_root,
            };
            plan.materialize_table_image(&mut buffers).unwrap();
        }

        for mapping in mappings.pages() {
            assert_eq!(
                resolve_materialized_page(&storage, mapping.iova_page),
                Some(mapping.physical_page)
            );
            assert_eq!(
                resolve_materialized_page(&storage, mapping.iova_page + SMMU_LPAE_PAGE_BYTES - 1),
                Some(mapping.physical_page + SMMU_LPAE_PAGE_BYTES - 1)
            );
        }
        assert_eq!(
            resolve_materialized_page(&storage, LINUX_IPA_AP_IOVA_START - SMMU_LPAE_PAGE_BYTES),
            None
        );
        assert_eq!(
            resolve_materialized_page(
                &storage,
                LINUX_IPA_AP_IOVA_START + mappings.pages().len() as u64 * SMMU_LPAE_PAGE_BYTES
            ),
            None
        );
    }

    #[test]
    fn rejects_table_buffer_identity_drift_before_zeroing_any_page() {
        let (mappings, pages, config) = inputs(LINUX_IPA_AP_IOVA_START);
        let plan = plan_ap_ring_lpae_s1(&mappings, pages, config).unwrap();
        let mut storage: [[u64; LPAE_ENTRIES_PER_TABLE]; AP_LPAE_TABLE_PAGE_COUNT] =
            core::array::from_fn(|_| [u64::MAX; LPAE_ENTRIES_PER_TABLE]);
        let mut bad_pages = pages;
        bad_pages.ttbr1_root += SMMU_LPAE_PAGE_BYTES;
        let [ttbr0_root, level2, level3, ttbr1_root] = &mut storage;
        let mut buffers = LpaeTableBuffers {
            physical_pages: bad_pages,
            ttbr0_root,
            level2,
            level3,
            ttbr1_root,
        };
        assert_eq!(
            plan.materialize_table_image(&mut buffers),
            Err(LpaePlanError::TableBackingIdentityMismatch)
        );
        assert!(
            storage
                .iter()
                .all(|page| page.iter().all(|entry| *entry == u64::MAX))
        );
    }

    #[test]
    fn accepts_the_minimum_input_width_that_covers_the_source_ap_iova_window() {
        let (mappings, pages, mut config) = inputs(LINUX_IPA_AP_IOVA_START);
        config.input_address_bits = 31;
        let plan = plan_ap_ring_lpae_s1(&mappings, pages, config).unwrap();
        assert_eq!(plan.context().tcr & TCR_T0SZ_MASK, 33);
        assert_eq!(plan.descriptor_writes()[0].index, 0);
        assert_eq!(plan.descriptor_writes()[1].index, 256);
    }

    #[test]
    fn rejects_untrusted_geometry_and_table_page_aliases() {
        let (mappings, pages, mut config) = inputs(LINUX_IPA_AP_IOVA_START);
        config.input_address_bits = 40;
        assert_eq!(
            plan_ap_ring_lpae_s1(&mappings, pages, config),
            Err(LpaePlanError::InvalidInputAddressWidth)
        );
        config.input_address_bits = 39;
        config.output_address_bits = 35;
        assert_eq!(
            plan_ap_ring_lpae_s1(&mappings, pages, config),
            Err(LpaePlanError::UnsupportedOutputAddressWidth)
        );
        config.output_address_bits = 40;
        config.asid = 0;
        assert_eq!(
            plan_ap_ring_lpae_s1(&mappings, pages, config),
            Err(LpaePlanError::InvalidAsid)
        );

        config.asid = 2;
        let mut bad_pages = pages;
        bad_pages.level2 = 0x9000_0001;
        assert_eq!(
            plan_ap_ring_lpae_s1(&mappings, bad_pages, config),
            Err(LpaePlanError::TablePageMisaligned)
        );
        bad_pages = pages;
        bad_pages.ttbr1_root = bad_pages.level3;
        assert_eq!(
            plan_ap_ring_lpae_s1(&mappings, bad_pages, config),
            Err(LpaePlanError::DuplicateTablePage)
        );
        bad_pages = pages;
        bad_pages.level3 = mappings.pages()[0].physical_page;
        assert_eq!(
            plan_ap_ring_lpae_s1(&mappings, bad_pages, config),
            Err(LpaePlanError::TablePageAliasesDmaPage)
        );
        bad_pages = pages;
        bad_pages.ttbr0_root = 1u64 << 40;
        assert_eq!(
            plan_ap_ring_lpae_s1(&mappings, bad_pages, config),
            Err(LpaePlanError::TablePageOutOfRange)
        );
    }

    #[test]
    fn rejects_ring_runs_that_cross_a_table_boundary() {
        let (mappings, pages, config) = inputs(0x201f_8000);
        assert_eq!(
            plan_ap_ring_lpae_s1(&mappings, pages, config),
            Err(LpaePlanError::MappingSpansMultipleL2Entries)
        );

        let ring_plan = plan_ap_dma_rings(DmaPool {
            base_iova: 0x3fff_8000,
            bytes: 0xe000,
        })
        .unwrap();
        let physical_pages: [u64; AP_DMA_PAGE_MAPPING_COUNT] =
            core::array::from_fn(|index| 0x8800_0000 + index as u64 * 0x1000);
        let mappings = plan_ap_dma_page_mappings(
            &ring_plan,
            DmaIovaWindow {
                base_iova: LINUX_IPA_AP_IOVA_START,
                bytes: LINUX_IPA_AP_IOVA_BYTES,
            },
            &physical_pages,
            40,
        )
        .unwrap();
        let (_, pages, config) = inputs(LINUX_IPA_AP_IOVA_START);
        assert_eq!(
            plan_ap_ring_lpae_s1(&mappings, pages, config),
            Err(LpaePlanError::MappingSpansMultipleL1Entries)
        );
    }
}
