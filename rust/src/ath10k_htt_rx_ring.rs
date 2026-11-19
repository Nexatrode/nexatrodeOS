//! Bounded WCN3990 ath10k HTT low-latency RX ring preflight.
//!
//! This plans the pinned ath10k ring geometry and encodes its 64-bit
//! `RX_RING_CFG` HTT payload into caller-owned slices. It does not allocate or
//! map DMA pages, perform cache maintenance, submit HTC/HTT traffic, or start
//! WLAN.
#![forbid(unsafe_code)]

use crate::sunfish_smmu_lpae::{LpaePageMapping, SMMU_LPAE_PAGE_BYTES};

pub const WCN3990_RX_RING_SLOTS: u32 = 2048;
pub const WCN3990_RX_RING_FILL_LEVEL: u32 = WCN3990_RX_RING_SLOTS - 1;
pub const WCN3990_RX_BUFFER_BYTES: u32 = 2048;
pub const WCN3990_RX_DESCRIPTOR_BYTES: u32 = 360;
pub const WCN3990_RX_RING_ENTRY_BYTES: u64 = 8;
pub const WCN3990_RX_ALLOC_INDEX_BYTES: u64 = 4;
pub const WCN3990_RX_RING_CONFIG_BYTES: usize = 48;

const HTT_H2T_MSG_TYPE_RX_RING_CFG: u8 = 2;
const RX_RING_FLAGS: u16 = 0xffff;

/// WCN3990's HTT config stores offsets in four-byte words.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RxDescriptorOffsets {
    pub mac80211_header_words: u16,
    pub msdu_payload_words: u16,
    pub ppdu_start_words: u16,
    pub ppdu_end_words: u16,
    pub mpdu_start_words: u16,
    pub mpdu_end_words: u16,
    pub msdu_start_words: u16,
    pub msdu_end_words: u16,
    pub attention_words: u16,
    pub fragment_info_words: u16,
}

pub const WCN3990_RX_DESCRIPTOR_OFFSETS: RxDescriptorOffsets = RxDescriptorOffsets {
    mac80211_header_words: 74,
    msdu_payload_words: 90,
    ppdu_start_words: 27,
    ppdu_end_words: 37,
    mpdu_start_words: 4,
    mpdu_end_words: 26,
    msdu_start_words: 7,
    msdu_end_words: 12,
    attention_words: 1,
    fragment_info_words: 2,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RxRingMemory {
    pub ring_iova: u64,
    pub alloc_index_iova: u64,
    pub buffers_iova: u64,
    /// Input width reported and approved by the caller's SMMU provider.
    pub iova_bits: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidAddressWidth,
    AddressOverflow,
    AddressOutOfRange,
    MisalignedRing,
    MisalignedAllocIndex,
    MisalignedBuffers,
    MemoryOverlap,
    OutputTooSmall,
    MappingsNotSorted,
    MisalignedMappedPage,
    UnmappedDmaPage(u64),
    AliasedPhysicalPage(u64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RxRingPlan {
    memory: RxRingMemory,
}

impl RxRingPlan {
    pub const fn memory(self) -> RxRingMemory {
        self.memory
    }

    pub const fn ring_slots(self) -> u32 {
        WCN3990_RX_RING_SLOTS
    }

    pub const fn initial_fill_level(self) -> u32 {
        WCN3990_RX_RING_FILL_LEVEL
    }

    pub const fn buffer_bytes(self) -> u32 {
        WCN3990_RX_BUFFER_BYTES
    }

    pub const fn ring_storage_bytes(self) -> u64 {
        WCN3990_RX_RING_SLOTS as u64 * WCN3990_RX_RING_ENTRY_BYTES
    }

    pub const fn initial_buffer_pool_bytes(self) -> u64 {
        WCN3990_RX_RING_FILL_LEVEL as u64 * WCN3990_RX_BUFFER_BYTES as u64
    }

    pub const fn first_buffer_payload_capacity(self) -> u32 {
        WCN3990_RX_BUFFER_BYTES - WCN3990_RX_DESCRIPTOR_BYTES
    }

    /// Check that the ring, index, and initial receive-buffer pool each have
    /// page mappings in the supplied sorted map and use distinct physical
    /// pages. This does not prove ownership or device authority.
    pub fn validate_mappings(self, mappings: &[LpaePageMapping]) -> Result<(), Error> {
        if mappings
            .windows(2)
            .any(|pair| pair[0].iova_page >= pair[1].iova_page)
        {
            return Err(Error::MappingsNotSorted);
        }
        if mappings.iter().any(|mapping| {
            mapping.iova_page & (SMMU_LPAE_PAGE_BYTES - 1) != 0
                || mapping.physical_page & (SMMU_LPAE_PAGE_BYTES - 1) != 0
        }) {
            return Err(Error::MisalignedMappedPage);
        }
        self.require_mapped_range(self.memory.ring_iova, self.ring_storage_bytes(), mappings)?;
        self.require_mapped_range(
            self.memory.alloc_index_iova,
            WCN3990_RX_ALLOC_INDEX_BYTES,
            mappings,
        )?;
        self.require_mapped_range(
            self.memory.buffers_iova,
            self.initial_buffer_pool_bytes(),
            mappings,
        )?;
        self.require_unique_physical_pages(mappings)?;
        Ok(())
    }

    /// Write 2048 64-bit little-endian IOVAs. The final slot stays zero, as in
    /// ath10k's 2047-buffer initial fill. Trailing output bytes are untouched.
    pub fn write_initial_address_ring(self, output: &mut [u8]) -> Result<usize, Error> {
        let required =
            usize::try_from(self.ring_storage_bytes()).map_err(|_| Error::AddressOverflow)?;
        if output.len() < required {
            return Err(Error::OutputTooSmall);
        }
        output[..required].fill(0);
        for index in 0..WCN3990_RX_RING_FILL_LEVEL {
            let displacement = u64::from(index)
                .checked_mul(u64::from(WCN3990_RX_BUFFER_BYTES))
                .ok_or(Error::AddressOverflow)?;
            let address = self
                .memory
                .buffers_iova
                .checked_add(displacement)
                .ok_or(Error::AddressOverflow)?;
            let offset = usize::try_from(u64::from(index) * WCN3990_RX_RING_ENTRY_BYTES)
                .map_err(|_| Error::AddressOverflow)?;
            output[offset..offset + 8].copy_from_slice(&address.to_le_bytes());
        }
        Ok(required)
    }

    /// Write the post-fill producer index into the caller's four-byte slot.
    pub fn write_initial_alloc_index(self, output: &mut [u8]) -> Result<usize, Error> {
        if output.len() < WCN3990_RX_ALLOC_INDEX_BYTES as usize {
            return Err(Error::OutputTooSmall);
        }
        output[..4].copy_from_slice(&WCN3990_RX_RING_FILL_LEVEL.to_le_bytes());
        Ok(WCN3990_RX_ALLOC_INDEX_BYTES as usize)
    }

    /// Encode ath10k's 48-byte, one-ring, 64-bit `RX_RING_CFG` HTT payload.
    /// This excludes the outer HTC frame header and submits no transport data.
    pub fn encode_ring_config(self, output: &mut [u8]) -> Result<usize, Error> {
        if output.len() < WCN3990_RX_RING_CONFIG_BYTES {
            return Err(Error::OutputTooSmall);
        }

        let mut config = [0u8; WCN3990_RX_RING_CONFIG_BYTES];
        config[0] = HTT_H2T_MSG_TYPE_RX_RING_CFG;
        config[1] = 1; // one RX ring
        config[4..12].copy_from_slice(&self.memory.alloc_index_iova.to_le_bytes());
        config[12..20].copy_from_slice(&self.memory.ring_iova.to_le_bytes());
        config[20..22].copy_from_slice(&(WCN3990_RX_RING_SLOTS as u16).to_le_bytes());
        config[22..24].copy_from_slice(&(WCN3990_RX_BUFFER_BYTES as u16).to_le_bytes());
        config[24..26].copy_from_slice(&RX_RING_FLAGS.to_le_bytes());
        config[26..28].copy_from_slice(&(WCN3990_RX_RING_FILL_LEVEL as u16).to_le_bytes());

        let offsets = WCN3990_RX_DESCRIPTOR_OFFSETS;
        let offset_words = [
            offsets.mac80211_header_words,
            offsets.msdu_payload_words,
            offsets.ppdu_start_words,
            offsets.ppdu_end_words,
            offsets.mpdu_start_words,
            offsets.mpdu_end_words,
            offsets.msdu_start_words,
            offsets.msdu_end_words,
            offsets.attention_words,
            offsets.fragment_info_words,
        ];
        for (index, word) in offset_words.into_iter().enumerate() {
            let offset = 28 + index * 2;
            config[offset..offset + 2].copy_from_slice(&word.to_le_bytes());
        }

        output[..config.len()].copy_from_slice(&config);
        Ok(config.len())
    }

    fn require_mapped_range(
        self,
        start: u64,
        bytes: u64,
        mappings: &[LpaePageMapping],
    ) -> Result<(), Error> {
        let end = start.checked_add(bytes).ok_or(Error::AddressOverflow)?;
        let first_page = start & !(SMMU_LPAE_PAGE_BYTES - 1);
        let rounded_end = end
            .checked_add(SMMU_LPAE_PAGE_BYTES - 1)
            .ok_or(Error::AddressOverflow)?
            & !(SMMU_LPAE_PAGE_BYTES - 1);
        let mut page = first_page;
        while page < rounded_end {
            if mappings
                .binary_search_by_key(&page, |mapping| mapping.iova_page)
                .is_err()
            {
                return Err(Error::UnmappedDmaPage(page));
            }
            page = page
                .checked_add(SMMU_LPAE_PAGE_BYTES)
                .ok_or(Error::AddressOverflow)?;
        }
        Ok(())
    }

    fn require_unique_physical_pages(self, mappings: &[LpaePageMapping]) -> Result<(), Error> {
        let regions = [
            (self.memory.ring_iova, self.ring_storage_bytes()),
            (self.memory.alloc_index_iova, WCN3990_RX_ALLOC_INDEX_BYTES),
            (self.memory.buffers_iova, self.initial_buffer_pool_bytes()),
        ];
        for (region_index, (start, bytes)) in regions.into_iter().enumerate() {
            let (first_page, end_page) = page_range(start, bytes)?;
            let mut page = first_page;
            while page < end_page {
                let physical = physical_page(mappings, page)?;
                for (prior_region_index, (prior_start, prior_bytes)) in
                    regions[..=region_index].iter().copied().enumerate()
                {
                    let (prior_first, prior_end) = page_range(prior_start, prior_bytes)?;
                    let mut prior_page = prior_first;
                    while prior_page < prior_end {
                        if prior_page != page
                            && (prior_region_index < region_index || prior_page < page)
                            && physical_page(mappings, prior_page)? == physical
                        {
                            return Err(Error::AliasedPhysicalPage(physical));
                        }
                        prior_page = prior_page
                            .checked_add(SMMU_LPAE_PAGE_BYTES)
                            .ok_or(Error::AddressOverflow)?;
                    }
                }
                page = page
                    .checked_add(SMMU_LPAE_PAGE_BYTES)
                    .ok_or(Error::AddressOverflow)?;
            }
        }
        Ok(())
    }
}

fn page_range(start: u64, bytes: u64) -> Result<(u64, u64), Error> {
    let end = start.checked_add(bytes).ok_or(Error::AddressOverflow)?;
    let first_page = start & !(SMMU_LPAE_PAGE_BYTES - 1);
    let rounded_end = end
        .checked_add(SMMU_LPAE_PAGE_BYTES - 1)
        .ok_or(Error::AddressOverflow)?
        & !(SMMU_LPAE_PAGE_BYTES - 1);
    Ok((first_page, rounded_end))
}

fn physical_page(mappings: &[LpaePageMapping], iova_page: u64) -> Result<u64, Error> {
    let index = mappings
        .binary_search_by_key(&iova_page, |mapping| mapping.iova_page)
        .map_err(|_| Error::UnmappedDmaPage(iova_page))?;
    Ok(mappings[index].physical_page)
}

fn validate_span(start: u64, bytes: u64, bits: u8) -> Result<u64, Error> {
    if !(1..=64).contains(&bits) || bytes == 0 {
        return Err(Error::InvalidAddressWidth);
    }
    let last = start.checked_add(bytes - 1).ok_or(Error::AddressOverflow)?;
    if bits < 64 && last >= (1u64 << bits) {
        return Err(Error::AddressOutOfRange);
    }
    last.checked_add(1).ok_or(Error::AddressOverflow)
}

pub fn plan_rx_ring(memory: RxRingMemory) -> Result<RxRingPlan, Error> {
    let ring_end = validate_span(
        memory.ring_iova,
        u64::from(WCN3990_RX_RING_SLOTS) * WCN3990_RX_RING_ENTRY_BYTES,
        memory.iova_bits,
    )?;
    let index_end = validate_span(
        memory.alloc_index_iova,
        WCN3990_RX_ALLOC_INDEX_BYTES,
        memory.iova_bits,
    )?;
    let buffers_end = validate_span(
        memory.buffers_iova,
        u64::from(WCN3990_RX_RING_FILL_LEVEL) * u64::from(WCN3990_RX_BUFFER_BYTES),
        memory.iova_bits,
    )?;

    if memory.ring_iova & (WCN3990_RX_RING_ENTRY_BYTES - 1) != 0 {
        return Err(Error::MisalignedRing);
    }
    if memory.alloc_index_iova & (WCN3990_RX_ALLOC_INDEX_BYTES - 1) != 0 {
        return Err(Error::MisalignedAllocIndex);
    }
    if memory.buffers_iova & 7 != 0 {
        return Err(Error::MisalignedBuffers);
    }

    if memory.ring_iova < index_end && memory.alloc_index_iova < ring_end
        || memory.ring_iova < buffers_end && memory.buffers_iova < ring_end
        || memory.alloc_index_iova < buffers_end && memory.buffers_iova < index_end
    {
        return Err(Error::MemoryOverlap);
    }

    Ok(RxRingPlan { memory })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory() -> RxRingMemory {
        RxRingMemory {
            ring_iova: 0x1000_0000,
            alloc_index_iova: 0x1000_4000,
            buffers_iova: 0x1001_0000,
            iova_bits: 39,
        }
    }

    #[test]
    fn matches_source_ring_geometry_and_posts_2047_of_2048_slots() {
        let plan = plan_rx_ring(memory()).unwrap();
        assert_eq!(plan.ring_slots(), 2048);
        assert_eq!(plan.initial_fill_level(), 2047);
        assert_eq!(plan.buffer_bytes(), 2048);
        assert_eq!(plan.ring_storage_bytes(), 16 * 1024);
        assert_eq!(plan.initial_buffer_pool_bytes(), 4_192_256);
        assert_eq!(plan.first_buffer_payload_capacity(), 1688);
    }

    #[test]
    fn writes_64_bit_addresses_index_and_exact_ring_config() {
        let plan = plan_rx_ring(memory()).unwrap();
        let mut ring = [0xa5; 16 * 1024 + 1];
        assert_eq!(plan.write_initial_address_ring(&mut ring), Ok(16 * 1024));
        assert_eq!(&ring[0..8], &memory().buffers_iova.to_le_bytes());
        assert_eq!(&ring[8..16], &(memory().buffers_iova + 2048).to_le_bytes());
        assert_eq!(
            &ring[2046 * 8..2047 * 8],
            &(memory().buffers_iova + 2046 * 2048).to_le_bytes()
        );
        assert_eq!(&ring[2047 * 8..2048 * 8], &[0; 8]);
        assert_eq!(ring[16 * 1024], 0xa5);

        let mut index = [0; 4];
        assert_eq!(plan.write_initial_alloc_index(&mut index), Ok(4));
        assert_eq!(u32::from_le_bytes(index), 2047);

        let mut config = [0; WCN3990_RX_RING_CONFIG_BYTES];
        assert_eq!(plan.encode_ring_config(&mut config), Ok(48));
        assert_eq!(config[0], 2);
        assert_eq!(config[1..4], [0x01, 0, 0]);
        assert_eq!(
            u64::from_le_bytes(config[4..12].try_into().unwrap()),
            memory().alloc_index_iova
        );
        assert_eq!(
            u64::from_le_bytes(config[12..20].try_into().unwrap()),
            memory().ring_iova
        );
        assert_eq!(u16::from_le_bytes(config[20..22].try_into().unwrap()), 2048);
        assert_eq!(u16::from_le_bytes(config[22..24].try_into().unwrap()), 2048);
        assert_eq!(
            u16::from_le_bytes(config[24..26].try_into().unwrap()),
            0xffff
        );
        assert_eq!(u16::from_le_bytes(config[26..28].try_into().unwrap()), 2047);
        let actual_offsets: Vec<u16> = config[28..48]
            .chunks_exact(2)
            .map(|word| u16::from_le_bytes([word[0], word[1]]))
            .collect();
        assert_eq!(actual_offsets, [74, 90, 27, 37, 4, 26, 7, 12, 1, 2]);
    }

    #[test]
    fn rejects_address_width_alignment_overlap_and_output_errors() {
        assert_eq!(
            plan_rx_ring(RxRingMemory {
                iova_bits: 0,
                ..memory()
            }),
            Err(Error::InvalidAddressWidth)
        );
        assert_eq!(
            plan_rx_ring(RxRingMemory {
                ring_iova: u64::MAX - 7,
                ..memory()
            }),
            Err(Error::AddressOverflow)
        );
        assert_eq!(
            plan_rx_ring(RxRingMemory {
                ring_iova: 0x1000_0001,
                ..memory()
            }),
            Err(Error::MisalignedRing)
        );
        assert_eq!(
            plan_rx_ring(RxRingMemory {
                alloc_index_iova: 0x1000_4001,
                ..memory()
            }),
            Err(Error::MisalignedAllocIndex)
        );
        assert_eq!(
            plan_rx_ring(RxRingMemory {
                buffers_iova: 0x1001_0001,
                ..memory()
            }),
            Err(Error::MisalignedBuffers)
        );
        assert_eq!(
            plan_rx_ring(RxRingMemory {
                alloc_index_iova: 0x1000_0008,
                ..memory()
            }),
            Err(Error::MemoryOverlap)
        );

        let plan = plan_rx_ring(memory()).unwrap();
        let mut short_ring = [0x5a; 16 * 1024 - 1];
        assert_eq!(
            plan.write_initial_address_ring(&mut short_ring),
            Err(Error::OutputTooSmall)
        );
        assert!(short_ring.iter().all(|byte| *byte == 0x5a));
        let mut short_cfg = [0x5a; 47];
        assert_eq!(
            plan.encode_ring_config(&mut short_cfg),
            Err(Error::OutputTooSmall)
        );
        assert!(short_cfg.iter().all(|byte| *byte == 0x5a));
    }

    #[test]
    fn requires_sorted_page_mappings_for_every_dma_region() {
        let plan = plan_rx_ring(memory()).unwrap();
        let mut mappings = Vec::new();
        for iova_page in (0x1000_0000..0x1001_0000).step_by(4096) {
            mappings.push(LpaePageMapping {
                iova_page,
                physical_page: iova_page + 0x4000_0000,
            });
        }
        for iova_page in (0x1001_0000..0x1041_1000).step_by(4096) {
            mappings.push(LpaePageMapping {
                iova_page,
                physical_page: iova_page + 0x4000_0000,
            });
        }
        assert_eq!(plan.validate_mappings(&mappings), Ok(()));

        let missing_page = mappings
            .iter()
            .position(|page| page.iova_page == 0x1040_0000)
            .unwrap();
        mappings.remove(missing_page);
        assert_eq!(
            plan.validate_mappings(&mappings),
            Err(Error::UnmappedDmaPage(0x1040_0000))
        );

        mappings.reverse();
        assert_eq!(
            plan.validate_mappings(&mappings),
            Err(Error::MappingsNotSorted)
        );
    }

    #[test]
    fn rejects_unaligned_or_aliased_physical_dma_pages() {
        let plan = plan_rx_ring(memory()).unwrap();
        let mut mappings = Vec::new();
        for iova_page in (0x1000_0000..0x1001_0000).step_by(4096) {
            mappings.push(LpaePageMapping {
                iova_page,
                physical_page: iova_page + 0x4000_0000,
            });
        }
        for iova_page in (0x1001_0000..0x1041_1000).step_by(4096) {
            mappings.push(LpaePageMapping {
                iova_page,
                physical_page: iova_page + 0x4000_0000,
            });
        }

        let buffer_page = mappings
            .iter_mut()
            .position(|mapping| mapping.iova_page == 0x1001_0000)
            .unwrap();
        mappings[buffer_page].physical_page = 0x5000_0000;
        assert_eq!(
            plan.validate_mappings(&mappings),
            Err(Error::AliasedPhysicalPage(0x5000_0000))
        );

        mappings[buffer_page].physical_page |= 1;
        assert_eq!(
            plan.validate_mappings(&mappings),
            Err(Error::MisalignedMappedPage)
        );
    }
}
