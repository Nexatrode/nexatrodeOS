//! Full exception-RX extension of the established shared/command IOVA plan.
//! Pure geometry only: no allocator, mapping installation or Ready authority.
#![forbid(unsafe_code)]

use crate::sunfish_ipa_linux_receive as rx;
use crate::sunfish_smmu_lpae::{
    self as lpae, LpaePageMapping, LpaeRootS1Plan, LpaeRootTablePages, LpaeS1Config,
    RingMemoryAttribute, TableWalkPolicy,
};

pub const SHARED_PAGES: usize = 37;
pub const RESERVED_PAGES: usize = rx::DATA_RESERVED_BYTES / 4096;
pub const MAPPED_PAGES: usize = SHARED_PAGES + rx::DATA_USED_PAGES;
pub const TABLE_FRAMES: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Geometry,
    Shared,
    Physical,
    Alias,
    Tables(lpae::LpaePlanError),
}
pub struct Mapping {
    pages: [LpaePageMapping; MAPPED_PAGES],
    receive_physical: [u64; RESERVED_PAGES],
}
fn valid_private(p: u64) -> bool {
    (0x80000000..1u64 << 40).contains(&p) && p % 4096 == 0
}
impl Mapping {
    /// Input addresses are metadata only. The actual retained shared owner,
    /// original command pool and receive allocation must be checked again by
    /// the native adapter; a caller-supplied list establishes no ownership.
    pub fn new(
        shared: &[LpaePageMapping; SHARED_PAGES],
        receive_physical: &[u64],
    ) -> Result<Self, Error> {
        Self::new_with_progress(shared, receive_physical, &mut |_| {})
    }
    #[inline(never)]
    pub(crate) fn new_with_progress(
        shared: &[LpaePageMapping; SHARED_PAGES],
        receive_physical: &[u64],
        progress: &mut impl FnMut(usize),
    ) -> Result<Self, Error> {
        progress(12);
        if MAPPED_PAGES != lpae::MAX_LPAE_RECEIVE_PAGE_MAPPINGS
            || rx::DATA_USED_PAGES != 502
            || RESERVED_PAGES != 512
            || receive_physical.len() != RESERVED_PAGES
        {
            return Err(Error::Geometry);
        }
        progress(13);
        for (n, p) in shared.iter().enumerate() {
            let expected = if n < 2 {
                0x146a8000 + n as u64 * 4096
            } else if n < 34 {
                0x80000000 + (n - 2) as u64 * 4096
            } else {
                0x8611d000 + (n - 34) as u64 * 4096
            };
            if p.iova_page != expected
                || ((n < 2 || n >= 34) && p.physical_page != expected)
                || ((2..34).contains(&n)
                    && (!valid_private(p.physical_page)
                        || p.physical_page != shared[2].physical_page + (n - 2) as u64 * 4096))
            {
                return Err(Error::Shared);
            }
            if shared[..n]
                .iter()
                .any(|old| old.physical_page == p.physical_page)
            {
                return Err(Error::Alias);
            }
        }
        progress(14);
        let mut physical = [0; RESERVED_PAGES];
        for (n, &page) in receive_physical.iter().enumerate() {
            if !valid_private(page)
                || page
                    != receive_physical[0]
                        .checked_add(n as u64 * 4096)
                        .ok_or(Error::Physical)?
            {
                return Err(Error::Physical);
            }
            if shared.iter().any(|old| old.physical_page == page) {
                return Err(Error::Alias);
            }
            physical[n] = page;
        }
        progress(15);
        let pages = core::array::from_fn(|n| {
            if n < 34 {
                shared[n]
            } else if n < 34 + rx::DATA_USED_PAGES {
                let index = n - 34;
                LpaePageMapping {
                    iova_page: rx::DATA_IOVA + index as u64 * 4096,
                    physical_page: physical[index],
                }
            } else {
                shared[n - rx::DATA_USED_PAGES]
            }
        });
        progress(16);
        Ok(Self {
            pages,
            receive_physical: physical,
        })
    }
    pub fn mappings(&self) -> &[LpaePageMapping; MAPPED_PAGES] {
        &self.pages
    }
    /// Includes CPU-only tail pages, for an actual resource owner's alias
    /// guard. This list never implies that a page is allocated or mapped.
    pub fn receive_allocation(&self) -> &[u64; RESERVED_PAGES] {
        &self.receive_physical
    }
    pub fn plan(&self, tables: LpaeRootTablePages<4>) -> Result<LpaeRootS1Plan<'_, 4>, Error> {
        let frames = tables.pages();
        // The LPAE encoder checks all mapped pages; also exclude the ten
        // CPU-only canary pages from page-table storage and aliases.
        if frames
            .iter()
            .any(|page| self.receive_physical.contains(page))
        {
            return Err(Error::Alias);
        }
        lpae::plan_lpae_s1_receive_roots(
            &self.pages,
            tables,
            LpaeS1Config {
                input_address_bits: 39,
                output_address_bits: 40,
                asid: 4,
                table_walk_policy: TableWalkPolicy::NonCoherent,
                ring_memory_attribute: RingMemoryAttribute::NormalNonCacheable,
                arm_ns_quirk: false,
            },
        )
        .map_err(Error::Tables)
    }
}
