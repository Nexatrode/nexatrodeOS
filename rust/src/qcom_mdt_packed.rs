//! Borrowed monolithic Qualcomm ELF32/MBN input for the Linux-reference loader.
//!
//! Follows qcom_mdt_read_metadata()/qcom_mdt_load_no_init() in Linux
//! 2267e248204e7e051e0f7355298949a6440de579. All nonempty file ranges are
//! validated before exposing any load or copying metadata. BSS-only program
//! headers may have offsets beyond EOF. This is structural validation, not
//! firmware identity/signature authentication, PAS, target staging or DMA.
//! Split .mdt/.bNN files must use the existing split-image path.
#![forbid(unsafe_code)]

use crate::qcom_mdt::{MDT_MAX_PROGRAM_HEADERS, MdtError, MdtImage, MdtSegment, parse_mdt};
const PH_BYTES: usize = 32;
const TYPE_MASK: u32 = 7 << 24;
const TYPE_HASH: u32 = 2 << 24;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Mdt(MdtError),
    SectionHeaders,
    MetadataHeader,
    MissingHash,
    AmbiguousHash,
    SplitOrTruncated,
    Overlap,
    OutputTooSmall,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Segment<'a> {
    pub descriptor: MdtSegment,
    /// Empty for a BSS-only segment; its file offset is never dereferenced.
    pub bytes: &'a [u8],
}

/// An immutable, fully range-checked monolithic view. It owns no allocation
/// and grants no authority to load firmware or transfer memory ownership.
pub struct Packed<'a> {
    image: MdtImage,
    header: &'a [u8],
    hash: &'a [u8],
    metadata_len: usize,
    segments: [Option<Segment<'a>>; MDT_MAX_PROGRAM_HEADERS],
}
fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())
}
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn range(bytes: &[u8], offset: u32, len: u32) -> Result<&[u8], Error> {
    let start = offset as usize;
    let end = start
        .checked_add(len as usize)
        .ok_or(Error::SplitOrTruncated)?;
    bytes.get(start..end).ok_or(Error::SplitOrTruncated)
}
impl<'a> Packed<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        let image = parse_mdt(bytes).map_err(Error::Mdt)?;
        // parse_mdt established the ELF32 identity and complete bounded PHDR
        // table, so all fixed header fields and each PHDR below are in range.
        let phoff = u32_at(bytes, 28) as usize;
        let count = image.program_header_count;
        let table_end = phoff + count * PH_BYTES;
        let section_offset = u32_at(bytes, 32) as usize;
        let section_size = u16_at(bytes, 46) as usize;
        let section_count = u16_at(bytes, 48) as usize;
        if section_size != 0 || section_count != 0 {
            let end = section_count
                .checked_mul(40)
                .and_then(|n| section_offset.checked_add(n));
            if section_size != 40 || end.is_none_or(|n| n > bytes.len()) {
                return Err(Error::SectionHeaders);
            }
        }
        if count < 2 || u32_at(bytes, phoff) == 1 {
            return Err(Error::MetadataHeader);
        }
        let header_len = u32_at(bytes, phoff + 16) as usize;
        // Stricter than the reference: the metadata's first part must retain
        // the whole ELF and PHDR table, not silently truncate its own headers.
        if header_len < table_end || header_len > bytes.len() {
            return Err(Error::MetadataHeader);
        }
        let mut hash_index = None;
        // Linux's split detector checks *every* nonempty PHDR, not only PT_LOAD.
        // A zero-sized BSS PHDR beyond EOF must not turn a packed image into
        // split firmware, or provoke an out-of-bounds zero-length slice.
        for index in 0..count {
            let at = phoff + index * PH_BYTES;
            let size = u32_at(bytes, at + 16);
            if size != 0 {
                range(bytes, u32_at(bytes, at + 4), size)?;
            }
            if index != 0 && u32_at(bytes, at + 24) & TYPE_MASK == TYPE_HASH {
                if hash_index.replace(index).is_some() {
                    return Err(Error::AmbiguousHash);
                }
            }
        }
        let hash_at = phoff + hash_index.ok_or(Error::MissingHash)? * PH_BYTES;
        let hash_size = u32_at(bytes, hash_at + 16);
        if hash_size == 0 {
            return Err(Error::MissingHash);
        }
        let metadata_len = header_len
            .checked_add(hash_size as usize)
            .ok_or(Error::MetadataHeader)?;
        // Preserve the reference's compact-metadata shortcut when the entire
        // input is exactly header+hash (possible for a BSS-only image). The
        // PHDR file ranges above are still required to be in this input; this
        // adapter never resolves an external segment file.
        let hash_offset = if metadata_len == bytes.len() {
            header_len as u32
        } else {
            u32_at(bytes, hash_at + 4)
        };
        let hash = range(bytes, hash_offset, hash_size)?;
        let mut segments = [None; MDT_MAX_PROGRAM_HEADERS];
        for (i, descriptor) in image.segments.iter().flatten().copied().enumerate() {
            let file = if descriptor.file_size == 0 {
                &[]
            } else {
                let at = phoff + descriptor.index as usize * PH_BYTES;
                range(bytes, u32_at(bytes, at + 4), descriptor.file_size)?
            };
            // Reject overlapping target ranges before a caller can stage any
            // bytes. The existing parser already checked address arithmetic.
            for earlier in image.segments.iter().flatten().take(i) {
                if descriptor.linked_address < earlier.linked_address + earlier.memory_size
                    && earlier.linked_address < descriptor.linked_address + descriptor.memory_size
                {
                    return Err(Error::Overlap);
                }
            }
            segments[i] = Some(Segment {
                descriptor,
                bytes: file,
            });
        }
        Ok(Self {
            image,
            header: &bytes[..header_len],
            hash,
            metadata_len,
            segments,
        })
    }
    pub const fn image(&self) -> &MdtImage {
        &self.image
    }
    pub const fn metadata_len(&self) -> usize {
        self.metadata_len
    }
    /// Compact ELF-header/PHDR bytes immediately followed by hash bytes.
    /// The ELF's hash p_offset continues to describe the *original* file;
    /// this compact output is a PAS payload, not another packed firmware file.
    pub const fn metadata_parts(&self) -> (&'a [u8], &'a [u8]) {
        (self.header, self.hash)
    }
    /// CPU-owned output only. Checks length before changing any byte, and
    /// leaves output beyond metadata_len untouched. No physical/DMA contract.
    pub fn copy_metadata(&self, output: &mut [u8]) -> Result<usize, Error> {
        if output.len() < self.metadata_len {
            return Err(Error::OutputTooSmall);
        }
        output[..self.header.len()].copy_from_slice(self.header);
        output[self.header.len()..self.metadata_len].copy_from_slice(self.hash);
        Ok(self.metadata_len)
    }
    pub fn segments(&self) -> impl Iterator<Item = Segment<'a>> + '_ {
        self.segments.iter().flatten().copied()
    }
}
