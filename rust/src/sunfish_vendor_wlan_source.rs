//! Read-only composition from a Sunfish `super` source to the exact WLAN file.
//!
//! This joins the LP metadata parser for the caller-selected active slot with
//! the ext4 parser for the exact WLAN MBN or board-data file. Opening reads
//! metadata only; file payload is read only when requested through
//! `ReadSourceIo`. The chain does not establish source authenticity, firmware
//! authorization, or a production UFS/SMMU provider.
#![forbid(unsafe_code)]

use crate::qcom_tftp::ReadSourceIo;
use crate::sunfish_lpd::Sha256;
use crate::sunfish_lpd::{self, VendorPartition};
use crate::sunfish_vendor_ext4::{self, VendorWlanFirmware};

const IDENTITY_CHUNK_BYTES: usize = 4096;
pub const PHONE_OBSERVED_WLAN_MDSP_LEN: u64 = 3_751_004;
pub const PHONE_OBSERVED_WLAN_MDSP_SHA256: [u8; 32] = [
    0xa2, 0xee, 0x6c, 0x86, 0xb8, 0x2b, 0x42, 0x14, 0x0e, 0x39, 0x96, 0x49, 0x00, 0xcb, 0x9c, 0xc1,
    0x0c, 0xa2, 0x63, 0x08, 0x5a, 0x71, 0x25, 0x00, 0x2f, 0xdb, 0x9c, 0x44, 0x8d, 0xf9, 0x4f, 0x2e,
];
pub const PHONE_OBSERVED_BOARD_DATA_LEN: u64 = 26_328;
pub const PHONE_OBSERVED_BOARD_DATA_SHA256: [u8; 32] = [
    0xb5, 0xab, 0xa8, 0xc3, 0x4d, 0xb9, 0x7f, 0x25, 0xa8, 0xd8, 0xe9, 0xf9, 0x36, 0x1b, 0x5b, 0x53,
    0x65, 0xe7, 0x4b, 0x58, 0xe0, 0xf4, 0xad, 0xdc, 0x7b, 0x92, 0x2b, 0xfe, 0x72, 0x26, 0xb1, 0x88,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    LogicalPartition(sunfish_lpd::Error),
    VendorFilesystem(sunfish_vendor_ext4::Error),
    IdentityLength,
    IdentityRead,
    IdentityDigest,
}

/// Exact `/vendor/firmware/wlanmdsp.mbn` file over a caller-provided `super`
/// byte source. `slot_number` must come from separately trusted boot-slot
/// evidence; this parser does not infer the active slot from `super` metadata.
pub struct SunfishVendorWlanSource<S: ReadSourceIo> {
    file: VendorWlanFirmware<VendorPartition<S>>,
}

impl<S: ReadSourceIo> SunfishVendorWlanSource<S> {
    pub fn open(super_source: S, slot_number: u32) -> Result<Self, Error> {
        let vendor =
            VendorPartition::open(super_source, slot_number).map_err(Error::LogicalPartition)?;
        let file = VendorWlanFirmware::open(vendor).map_err(Error::VendorFilesystem)?;
        Ok(Self { file })
    }

    pub const fn file_len(&self) -> u64 {
        self.file.file_len()
    }

    /// Verify against the exact WLAN MBN identity previously observed on the
    /// user's Sunfish LineageOS vendor partition. This match is not an
    /// authenticity or firmware-use authorization decision.
    pub fn verify_phone_observed_identity(self) -> Result<VerifiedSunfishWlanSource<S>, Error> {
        self.verify_identity(
            PHONE_OBSERVED_WLAN_MDSP_LEN,
            PHONE_OBSERVED_WLAN_MDSP_SHA256,
        )
    }
}

impl<S: ReadSourceIo> ReadSourceIo for SunfishVendorWlanSource<S> {
    fn len(&mut self) -> Result<u64, ()> {
        self.file.len()
    }

    fn read_exact_at(&mut self, offset: u64, out: &mut [u8]) -> Result<(), ()> {
        self.file.read_exact_at(offset, out)
    }
}

/// Exact `/vendor/firmware/bdwlan-sunfish.bin` source over a caller-provided
/// read-only `super` source. This identifies board-file bytes only; it does
/// not tie them to an ath10k WLFW board ID or authorize their use.
pub struct SunfishVendorBoardSource<S: ReadSourceIo> {
    file: VendorWlanFirmware<VendorPartition<S>>,
}

impl<S: ReadSourceIo> SunfishVendorBoardSource<S> {
    pub fn open(super_source: S, slot_number: u32) -> Result<Self, Error> {
        let vendor =
            VendorPartition::open(super_source, slot_number).map_err(Error::LogicalPartition)?;
        let file = VendorWlanFirmware::open_board_data(vendor).map_err(Error::VendorFilesystem)?;
        Ok(Self { file })
    }

    pub const fn file_len(&self) -> u64 {
        self.file.file_len()
    }

    pub fn verify_phone_observed_identity(self) -> Result<VerifiedSunfishBoardSource<S>, Error> {
        self.verify_identity(
            PHONE_OBSERVED_BOARD_DATA_LEN,
            PHONE_OBSERVED_BOARD_DATA_SHA256,
        )
    }

    pub fn verify_identity(
        self,
        expected_len: u64,
        expected_sha256: [u8; 32],
    ) -> Result<VerifiedSunfishBoardSource<S>, Error> {
        VerifiedSunfishWlanAsset::verify(self, expected_len, expected_sha256)
    }
}

impl<S: ReadSourceIo> ReadSourceIo for SunfishVendorBoardSource<S> {
    fn len(&mut self) -> Result<u64, ()> {
        self.file.len()
    }

    fn read_exact_at(&mut self, offset: u64, out: &mut [u8]) -> Result<(), ()> {
        self.file.read_exact_at(offset, out)
    }
}

/// A WLAN asset whose complete contents matched a caller-supplied identity.
///
/// This establishes byte identity only. It does not authenticate the vendor
/// partition, grant firmware-use rights, or convert into
/// `AuthorizedWlanImage`; callers still need an independent trusted
/// authorization decision before serving firmware.
pub type VerifiedSunfishWlanSource<S> = VerifiedSunfishWlanAsset<SunfishVendorWlanSource<S>>;
pub type VerifiedSunfishBoardSource<S> = VerifiedSunfishWlanAsset<SunfishVendorBoardSource<S>>;

pub struct VerifiedSunfishWlanAsset<F: ReadSourceIo> {
    file: F,
    file_len: u64,
    sha256: [u8; 32],
}

impl<F: ReadSourceIo> VerifiedSunfishWlanAsset<F> {
    fn verify(mut file: F, expected_len: u64, expected_sha256: [u8; 32]) -> Result<Self, Error> {
        let file_len = file.len().map_err(|_| Error::IdentityRead)?;
        if file_len != expected_len {
            return Err(Error::IdentityLength);
        }

        let mut hash = Sha256::new();
        let mut scratch = [0u8; IDENTITY_CHUNK_BYTES];
        let mut offset = 0u64;
        while offset < file_len {
            let count = usize::try_from((file_len - offset).min(scratch.len() as u64))
                .map_err(|_| Error::IdentityRead)?;
            file.read_exact_at(offset, &mut scratch[..count])
                .map_err(|_| Error::IdentityRead)?;
            hash.update(&scratch[..count]);
            offset = offset
                .checked_add(count as u64)
                .ok_or(Error::IdentityRead)?;
        }
        if file.len().map_err(|_| Error::IdentityRead)? != file_len {
            return Err(Error::IdentityRead);
        }
        let actual_sha256 = hash.finish();
        if actual_sha256 != expected_sha256 {
            return Err(Error::IdentityDigest);
        }

        Ok(Self {
            file,
            file_len,
            sha256: actual_sha256,
        })
    }

    pub const fn file_len(&self) -> u64 {
        self.file_len
    }

    pub const fn sha256(&self) -> [u8; 32] {
        self.sha256
    }
}

impl<F: ReadSourceIo> ReadSourceIo for VerifiedSunfishWlanAsset<F> {
    fn len(&mut self) -> Result<u64, ()> {
        if self.file.len()? != self.file_len {
            return Err(());
        }
        Ok(self.file_len)
    }

    fn read_exact_at(&mut self, offset: u64, out: &mut [u8]) -> Result<(), ()> {
        if self.file.len()? != self.file_len {
            return Err(());
        }
        self.file.read_exact_at(offset, out)?;
        if self.file.len()? != self.file_len {
            return Err(());
        }
        Ok(())
    }
}

impl<S: ReadSourceIo> SunfishVendorWlanSource<S> {
    /// Read and verify the complete exact-path image using a bounded scratch
    /// buffer. The source's length is checked before and after hashing, and
    /// every payload read is delegated through the existing read-only source
    /// chain.
    pub fn verify_identity(
        self,
        expected_len: u64,
        expected_sha256: [u8; 32],
    ) -> Result<VerifiedSunfishWlanSource<S>, Error> {
        VerifiedSunfishWlanAsset::verify(self, expected_len, expected_sha256)
    }
}
