//! Source-matched Sunfish `ipa_fws` MDT placement planner.
//!
//! This mirrors the bounded placement rules in the pinned Qualcomm MDT loader
//! for the active Sunfish IPA-FW reservation. It retains no firmware bytes,
//! does not stage segments, and cannot invoke PAS or start the subsystem.

#![forbid(unsafe_code)]

use crate::qcom_mdt::{MDT_MAX_PROGRAM_HEADERS, MdtImage};

pub const PAS_ID: u32 = 0x0f;
pub const REGION_BASE: u64 = 0x98f8_0000;
pub const REGION_BYTES: u64 = 0x1_0000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidImage,
    TooManyFiles,
    DuplicateSegment,
    MissingSegmentFile,
    UnexpectedSegmentFile,
    SegmentFileSize,
    SegmentOutsideRegion,
    RelocationExtent,
}

/// Metadata for one external `ipa_fws.bNN` file. Bytes are never retained.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SegmentFile {
    pub index: u16,
    pub bytes: usize,
}

/// Borrowed contents of one external `ipa_fws.bNN` segment.
pub struct SegmentData<'a> {
    pub index: u16,
    pub bytes: &'a [u8],
}

/// Minimal byte operations required to mirror `qcom_mdt_load()`.
pub trait StagingMemory {
    fn write(&mut self, offset: u32, bytes: &[u8]);
    fn zero(&mut self, offset: u32, bytes: u32);
    fn sync(&mut self);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SegmentPlacement {
    pub index: u16,
    pub offset: u32,
    pub file_bytes: u32,
    pub memory_bytes: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PasMemSetup {
    pub pas_id: u32,
    pub physical_base: u64,
    pub bytes: u32,
}

/// Complete preflight result for Linux's `qcom_mdt_load()` placement rules.
/// The caller must still authenticate an authorized image, stage its segment
/// files, perform the PIL resource/power lifecycle, and obtain PAS approval.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LoadPlan {
    pub pas_id: u32,
    pub physical_base: u64,
    pub region_bytes: u64,
    pub relocatable: bool,
    pub mem_setup: Option<PasMemSetup>,
    placements: [Option<SegmentPlacement>; MDT_MAX_PROGRAM_HEADERS],
    placement_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StageReport {
    pub segments_staged: u16,
    pub file_bytes_staged: u64,
    pub bss_bytes_zeroed: u64,
}

impl LoadPlan {
    pub fn placements(&self) -> &[Option<SegmentPlacement>; MDT_MAX_PROGRAM_HEADERS] {
        &self.placements
    }

    pub const fn placement_count(&self) -> usize {
        self.placement_count
    }
}

/// Preflight the MDT and the exact set of nonempty `.bNN` files before any
/// memory write. For relocatable MDTs Linux sends PAS_MEM_SETUP for the
/// page-rounded image span and places segments relative to the lowest address;
/// fixed-address MDTs place segments relative to the reserved physical base.
pub fn plan(image: &MdtImage, files: &[SegmentFile]) -> Result<LoadPlan, Error> {
    if image.program_header_count == 0
        || image.program_header_count > MDT_MAX_PROGRAM_HEADERS
        || image.loadable_segment_count == 0
        || image.linked_min_address >= image.linked_max_address
        || files.len() > MDT_MAX_PROGRAM_HEADERS
    {
        return Err(if files.len() > MDT_MAX_PROGRAM_HEADERS {
            Error::TooManyFiles
        } else {
            Error::InvalidImage
        });
    }

    let extent = u64::from(image.memory_extent());
    if image.relocatable && (extent == 0 || extent > REGION_BYTES) {
        return Err(Error::RelocationExtent);
    }

    let placement_base = if image.relocatable {
        u64::from(image.linked_min_address)
    } else {
        REGION_BASE
    };
    let placement_limit = if image.relocatable {
        extent
    } else {
        REGION_BYTES
    };
    let mut placements = [None; MDT_MAX_PROGRAM_HEADERS];
    let mut file_used = [false; MDT_MAX_PROGRAM_HEADERS];
    let mut count = 0usize;

    for segment in image.segments.iter().flatten() {
        let index = usize::from(segment.index);
        if index >= image.program_header_count || placements[index].is_some() {
            return Err(Error::DuplicateSegment);
        }
        if segment.memory_size == 0 || segment.file_size > segment.memory_size {
            return Err(Error::InvalidImage);
        }
        let address = u64::from(segment.linked_address);
        let offset = address
            .checked_sub(placement_base)
            .ok_or(Error::SegmentOutsideRegion)?;
        let end = offset
            .checked_add(u64::from(segment.memory_size))
            .ok_or(Error::SegmentOutsideRegion)?;
        if end > placement_limit {
            return Err(Error::SegmentOutsideRegion);
        }

        let file_bytes = if segment.file_size == 0 {
            if files.iter().any(|file| file.index == segment.index) {
                return Err(Error::UnexpectedSegmentFile);
            }
            0
        } else {
            let mut matched = None;
            for (slot, file) in files.iter().enumerate() {
                if file.index == segment.index {
                    if matched.is_some() {
                        return Err(Error::DuplicateSegment);
                    }
                    matched = Some((slot, file.bytes));
                }
            }
            let (slot, bytes) = matched.ok_or(Error::MissingSegmentFile)?;
            if bytes != segment.file_size as usize {
                return Err(Error::SegmentFileSize);
            }
            file_used[slot] = true;
            segment.file_size
        };

        placements[index] = Some(SegmentPlacement {
            index: segment.index,
            offset: u32::try_from(offset).map_err(|_| Error::SegmentOutsideRegion)?,
            file_bytes,
            memory_bytes: segment.memory_size,
        });
        count += 1;
    }

    if count != image.loadable_segment_count {
        return Err(Error::InvalidImage);
    }
    if file_used[..files.len()].iter().any(|used| !used) {
        return Err(Error::UnexpectedSegmentFile);
    }

    Ok(LoadPlan {
        pas_id: PAS_ID,
        physical_base: REGION_BASE,
        region_bytes: REGION_BYTES,
        relocatable: image.relocatable,
        mem_setup: image.relocatable.then_some(PasMemSetup {
            pas_id: PAS_ID,
            physical_base: REGION_BASE,
            bytes: u32::try_from(extent).map_err(|_| Error::RelocationExtent)?,
        }),
        placements,
        placement_count: count,
    })
}

/// Stage an already-preflighted plan after the caller has completed the
/// source-required PAS_INIT_IMAGE and (for relocatable MDTs) PAS_MEM_SETUP.
/// All segment indices and lengths are checked before the first write. This
/// mirrors Linux's program-header order and zeros only each segment's BSS
/// tail; gaps in the reserved pool remain untouched. The caller must authorize
/// the firmware source and must not AUTH_AND_RESET until this completes.
pub fn stage(
    plan: &LoadPlan,
    files: &[SegmentData<'_>],
    memory: &mut impl StagingMemory,
) -> Result<StageReport, Error> {
    if files.len() > MDT_MAX_PROGRAM_HEADERS {
        return Err(Error::TooManyFiles);
    }
    let mut used = [false; MDT_MAX_PROGRAM_HEADERS];
    for placement in plan.placements.iter().flatten() {
        if placement.file_bytes == 0 {
            if files.iter().any(|file| file.index == placement.index) {
                return Err(Error::UnexpectedSegmentFile);
            }
            continue;
        }
        let mut matched = None;
        for (slot, file) in files.iter().enumerate() {
            if file.index == placement.index {
                if matched.is_some() {
                    return Err(Error::DuplicateSegment);
                }
                matched = Some((slot, file.bytes));
            }
        }
        let (slot, bytes) = matched.ok_or(Error::MissingSegmentFile)?;
        if bytes.len() != placement.file_bytes as usize {
            return Err(Error::SegmentFileSize);
        }
        used[slot] = true;
    }
    if used[..files.len()].iter().any(|was_used| !was_used) {
        return Err(Error::UnexpectedSegmentFile);
    }

    let mut report = StageReport {
        segments_staged: 0,
        file_bytes_staged: 0,
        bss_bytes_zeroed: 0,
    };
    for placement in plan.placements.iter().flatten() {
        if placement.file_bytes != 0 {
            let file = files
                .iter()
                .find(|file| file.index == placement.index)
                .expect("file set was preflighted");
            memory.write(placement.offset, file.bytes);
            report.file_bytes_staged += u64::from(placement.file_bytes);
        }
        let bss_bytes = placement.memory_bytes - placement.file_bytes;
        if bss_bytes != 0 {
            memory.zero(placement.offset + placement.file_bytes, bss_bytes);
            report.bss_bytes_zeroed += u64::from(bss_bytes);
        }
        report.segments_staged += 1;
    }
    memory.sync();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qcom_mdt::MdtSegment;

    fn image(segments: &[MdtSegment], min: u32, max: u32, relocatable: bool) -> MdtImage {
        let mut slots = [None; MDT_MAX_PROGRAM_HEADERS];
        for segment in segments {
            slots[usize::from(segment.index)] = Some(*segment);
        }
        MdtImage {
            segments: slots,
            program_header_count: segments
                .iter()
                .map(|segment| usize::from(segment.index) + 1)
                .max()
                .unwrap_or(0),
            loadable_segment_count: segments.len(),
            linked_min_address: min,
            linked_max_address: max,
            relocatable,
        }
    }

    #[test]
    fn relocatable_plan_matches_mdt_mem_setup_and_relative_placement() {
        let image = image(
            &[
                MdtSegment {
                    index: 2,
                    linked_address: 0x1000,
                    file_size: 16,
                    memory_size: 0x2000,
                    flags: 1 << 27,
                },
                MdtSegment {
                    index: 7,
                    linked_address: 0x5000,
                    file_size: 32,
                    memory_size: 0x1000,
                    flags: 1 << 27,
                },
            ],
            0x1000,
            0x6000,
            true,
        );
        let plan = plan(
            &image,
            &[
                SegmentFile {
                    index: 2,
                    bytes: 16,
                },
                SegmentFile {
                    index: 7,
                    bytes: 32,
                },
            ],
        )
        .unwrap();

        assert_eq!(plan.pas_id, 0x0f);
        assert_eq!(plan.physical_base, 0x98f8_0000);
        assert_eq!(
            plan.mem_setup,
            Some(PasMemSetup {
                pas_id: 0x0f,
                physical_base: 0x98f8_0000,
                bytes: 0x5000,
            })
        );
        assert_eq!(plan.placement_count(), 2);
        assert_eq!(plan.placements()[2].unwrap().offset, 0);
        assert_eq!(plan.placements()[7].unwrap().offset, 0x4000);
    }

    #[test]
    fn stage_copies_files_and_zeros_only_bss_in_header_order() {
        #[derive(Default)]
        struct Memory {
            bytes: std::vec::Vec<u8>,
            writes: std::vec::Vec<(u32, usize)>,
            syncs: u8,
        }
        impl StagingMemory for Memory {
            fn write(&mut self, offset: u32, bytes: &[u8]) {
                let start = offset as usize;
                self.bytes[start..start + bytes.len()].copy_from_slice(bytes);
                self.writes.push((offset, bytes.len()));
            }
            fn zero(&mut self, offset: u32, bytes: u32) {
                let start = offset as usize;
                self.bytes[start..start + bytes as usize].fill(0);
            }
            fn sync(&mut self) {
                self.syncs += 1;
            }
        }

        let image = image(
            &[
                MdtSegment {
                    index: 2,
                    linked_address: 0x1000,
                    file_size: 3,
                    memory_size: 6,
                    flags: 1 << 27,
                },
                MdtSegment {
                    index: 7,
                    linked_address: 0x1008,
                    file_size: 2,
                    memory_size: 4,
                    flags: 1 << 27,
                },
            ],
            0x1000,
            0x2000,
            true,
        );
        let plan = plan(
            &image,
            &[
                SegmentFile { index: 2, bytes: 3 },
                SegmentFile { index: 7, bytes: 2 },
            ],
        )
        .unwrap();
        let mut memory = Memory {
            bytes: std::vec![0xaa; REGION_BYTES as usize],
            ..Memory::default()
        };
        let report = stage(
            &plan,
            &[
                SegmentData {
                    index: 7,
                    bytes: &[0xb1, 0xb2],
                },
                SegmentData {
                    index: 2,
                    bytes: &[0xa1, 0xa2, 0xa3],
                },
            ],
            &mut memory,
        )
        .unwrap();

        assert_eq!(report.segments_staged, 2);
        assert_eq!(report.file_bytes_staged, 5);
        assert_eq!(report.bss_bytes_zeroed, 5);
        assert_eq!(memory.writes, [(0, 3), (8, 2)]);
        assert_eq!(&memory.bytes[..6], &[0xa1, 0xa2, 0xa3, 0, 0, 0]);
        assert_eq!(&memory.bytes[6..8], &[0xaa, 0xaa]);
        assert_eq!(&memory.bytes[8..12], &[0xb1, 0xb2, 0, 0]);
        assert_eq!(memory.syncs, 1);
    }

    #[test]
    fn stage_validates_every_file_before_the_first_write() {
        #[derive(Default)]
        struct Memory {
            writes: u8,
        }
        impl StagingMemory for Memory {
            fn write(&mut self, _offset: u32, _bytes: &[u8]) {
                self.writes += 1;
            }
            fn zero(&mut self, _offset: u32, _bytes: u32) {}
            fn sync(&mut self) {}
        }

        let image = image(
            &[
                MdtSegment {
                    index: 0,
                    linked_address: 0x1000,
                    file_size: 2,
                    memory_size: 2,
                    flags: 1 << 27,
                },
                MdtSegment {
                    index: 1,
                    linked_address: 0x2000,
                    file_size: 2,
                    memory_size: 2,
                    flags: 1 << 27,
                },
            ],
            0x1000,
            0x3000,
            true,
        );
        let plan = plan(
            &image,
            &[
                SegmentFile { index: 0, bytes: 2 },
                SegmentFile { index: 1, bytes: 2 },
            ],
        )
        .unwrap();
        let mut memory = Memory::default();
        assert_eq!(
            stage(
                &plan,
                &[SegmentData {
                    index: 0,
                    bytes: &[1, 2],
                }],
                &mut memory,
            ),
            Err(Error::MissingSegmentFile)
        );
        assert_eq!(memory.writes, 0);
    }

    #[test]
    fn fixed_address_plan_is_relative_to_reserved_physical_base_without_mem_setup() {
        let image = image(
            &[MdtSegment {
                index: 1,
                linked_address: 0x98f8_2000,
                file_size: 8,
                memory_size: 0x1000,
                flags: 0,
            }],
            0x98f8_2000,
            0x98f8_3000,
            false,
        );
        let plan = plan(&image, &[SegmentFile { index: 1, bytes: 8 }]).unwrap();
        assert_eq!(plan.mem_setup, None);
        assert_eq!(plan.placements()[1].unwrap().offset, 0x2000);
    }

    #[test]
    fn preflight_rejects_missing_duplicate_wrong_sized_and_unexpected_files() {
        let image = image(
            &[
                MdtSegment {
                    index: 0,
                    linked_address: 0x1000,
                    file_size: 4,
                    memory_size: 0x1000,
                    flags: 1 << 27,
                },
                MdtSegment {
                    index: 1,
                    linked_address: 0x2000,
                    file_size: 0,
                    memory_size: 0x1000,
                    flags: 1 << 27,
                },
            ],
            0x1000,
            0x3000,
            true,
        );
        assert_eq!(plan(&image, &[]), Err(Error::MissingSegmentFile));
        assert_eq!(
            plan(&image, &[SegmentFile { index: 0, bytes: 3 }]),
            Err(Error::SegmentFileSize)
        );
        assert_eq!(
            plan(
                &image,
                &[
                    SegmentFile { index: 0, bytes: 4 },
                    SegmentFile { index: 0, bytes: 4 },
                ]
            ),
            Err(Error::DuplicateSegment)
        );
        assert_eq!(
            plan(
                &image,
                &[
                    SegmentFile { index: 0, bytes: 4 },
                    SegmentFile { index: 1, bytes: 1 },
                ]
            ),
            Err(Error::UnexpectedSegmentFile)
        );
    }

    #[test]
    fn preflight_rejects_an_image_larger_than_the_reserved_ipa_region() {
        let image = image(
            &[MdtSegment {
                index: 0,
                linked_address: 0x1000,
                file_size: 1,
                memory_size: 0x1_0001,
                flags: 1 << 27,
            }],
            0x1000,
            0x12_000,
            true,
        );
        assert_eq!(
            plan(&image, &[SegmentFile { index: 0, bytes: 1 }]),
            Err(Error::RelocationExtent)
        );
    }

    #[test]
    fn preflight_rejects_invalid_segment_file_and_memory_extents() {
        let image = image(
            &[MdtSegment {
                index: 0,
                linked_address: 0x1000,
                file_size: 8,
                memory_size: 4,
                flags: 1 << 27,
            }],
            0x1000,
            0x2000,
            true,
        );
        assert_eq!(
            plan(&image, &[SegmentFile { index: 0, bytes: 8 }]),
            Err(Error::InvalidImage)
        );
    }
}
