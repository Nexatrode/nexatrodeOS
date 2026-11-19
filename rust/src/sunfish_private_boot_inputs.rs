//! Private, immutable Sunfish reference firmware and volatile modem storage.
//!
//! The platform selects device-bound inputs using the privately generated
//! module, and independently verifies the connected phone at temporary boot.
//! This code checks the selected binding and every payload hash before any
//! shadow changes. It neither detects hardware identity nor authenticates
//! firmware signatures; PAS remains responsible for firmware authentication.
//! It grants no SCM, DMA, modem-start or production WLAN readiness token.
#![forbid(unsafe_code)]

use crate::qcom_modem_ram_storage::{self, RamShadow};
use crate::qcom_modem_storage::{Configuration, Digest, Storage};
use crate::qcom_qrtr_tftp::AuthorizedWlanImage;
use crate::sunfish_lpd::Sha256;
use crate::sunfish_vendor_wlan_source::{
    PHONE_OBSERVED_WLAN_MDSP_LEN, PHONE_OBSERVED_WLAN_MDSP_SHA256,
};

pub struct Inputs<'a> {
    pub configuration: Configuration,
    pub efs: [&'a [u8]; 4],
    pub wlan: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    DeviceBinding,
    FirmwareLength,
    FirmwareDigest,
    Shadow(qcom_modem_ram_storage::Error),
    Storage,
}

/// CPU-side providers only. Native SCM pool ownership and transport services
/// must be constructed separately before the modem may be started.
pub struct Prepared<'a> {
    pub storage: Storage<RamShadow<'a>>,
    pub wlan: AuthorizedWlanImage<'a>,
}

impl<'a> Inputs<'a> {
    /// For an authorized private Sunfish experiment using the exact WLAN
    /// bytes from the successful Linux reference. `observed_device` must come
    /// from the platform's separately checked boot/device binding, not a peer
    /// request or merely a second read of this input configuration.
    ///
    /// All source slices must remain immutable; safe Rust borrows enforce
    /// exclusivity of the caller's four shadow buffers. Native callers must
    /// uphold that contract when deriving slices from mapped frames.
    pub fn prepare<'shadow>(
        self,
        observed_device: Digest,
        shadows: [&'shadow mut [u8]; 4],
    ) -> Result<Prepared<'shadow>, Error>
    where
        'a: 'shadow,
    {
        self.prepare_identity(
            observed_device,
            shadows,
            PHONE_OBSERVED_WLAN_MDSP_LEN,
            PHONE_OBSERVED_WLAN_MDSP_SHA256,
        )
    }

    // The public path always supplies the exact measured reference identity.
    // Separate expected values here allow synthetic, non-distributed test data.
    fn prepare_identity<'shadow>(
        self,
        observed_device: Digest,
        shadows: [&'shadow mut [u8]; 4],
        wlan_len: u64,
        wlan_digest: Digest,
    ) -> Result<Prepared<'shadow>, Error>
    where
        'a: 'shadow,
    {
        if observed_device == [0; 32] || observed_device != self.configuration.device {
            return Err(Error::DeviceBinding);
        }
        if self.wlan.len() as u64 != wlan_len {
            return Err(Error::FirmwareLength);
        }
        let mut hash = Sha256::new();
        hash.update(self.wlan);
        if hash.finish() != wlan_digest {
            return Err(Error::FirmwareDigest);
        }
        // RamShadow verifies ALL four EFS sources and geometry before copying
        // any bytes. Firmware checks above also precede the first shadow copy.
        let ram = RamShadow::new(self.configuration, self.efs, shadows).map_err(Error::Shadow)?;
        let mut storage = ram.into_storage().map_err(|_| Error::Storage)?;
        storage.observe().map_err(|_| Error::Storage)?;
        Ok(Prepared {
            storage,
            wlan: AuthorizedWlanImage::private_sunfish_reference(self.wlan),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qcom_modem_handlers::{Disk, Persistence};
    use crate::qcom_modem_services::Partition;
    use crate::qcom_modem_storage::{Binding, PARTITIONS};
    use crate::qcom_qrtr_tftp::Server;

    fn digest(bytes: &[u8]) -> Digest {
        let mut hash = Sha256::new();
        hash.update(bytes);
        hash.finish()
    }

    fn inputs<'a>(efs: &'a [[u8; 8192]; 4], wlan: &'a [u8]) -> Inputs<'a> {
        Inputs {
            configuration: Configuration {
                generation: 1,
                device: [0x51; 32],
                bindings: core::array::from_fn(|i| Binding {
                    partition: PARTITIONS[i],
                    object: i as u64 + 1,
                    source: digest(&efs[i]),
                    disk: Disk {
                        bytes: 8192,
                        writable: true,
                        durable: false,
                    },
                }),
            },
            efs: efs.each_ref().map(|s| &s[..]),
            wlan,
        }
    }

    #[test]
    fn private_inputs_make_usable_volatile_storage_and_retained_tftp_source() {
        let efs = [[0x31; 8192]; 4];
        let wlan = [0x77; 1024];
        let mut shadows = [[0xa5; 8192]; 4];
        let mut prepared = inputs(&efs, &wlan)
            .prepare_identity(
                [0x51; 32],
                shadows.each_mut().map(|s| &mut s[..]),
                wlan.len() as u64,
                digest(&wlan),
            )
            .unwrap();
        assert_eq!(prepared.storage.persistence(), Persistence::RamShadow);
        prepared
            .storage
            .write(Partition::ModemFs1, 512, &[0x99; 512])
            .unwrap();
        prepared.storage.flush(Partition::ModemFs1).unwrap();
        let mut read = [0; 512];
        prepared
            .storage
            .read(Partition::ModemFs1, 512, &mut read)
            .unwrap();
        assert_eq!(read, [0x99; 512]);
        assert_eq!(efs, [[0x31; 8192]; 4]);
        let server = Server::new(1, 0, 0x5000, 0x5100, &[], prepared.wlan).unwrap();
        assert!(server.image_available());
    }

    #[test]
    fn private_inputs_reject_wrong_phone_or_wlan_before_copying_any_efs_shadow() {
        let efs = [[0x31; 8192]; 4];
        let wlan = [0x77; 1024];
        for (device, length, expected, error) in [
            ([0; 32], 1024, digest(&wlan), Error::DeviceBinding),
            ([0x52; 32], 1024, digest(&wlan), Error::DeviceBinding),
            ([0x51; 32], 1023, digest(&wlan), Error::FirmwareLength),
            ([0x51; 32], 1024, [0; 32], Error::FirmwareDigest),
        ] {
            let mut shadows = [[0xa5; 8192]; 4];
            assert!(matches!(inputs(&efs, &wlan).prepare_identity(
                device, shadows.each_mut().map(|s| &mut s[..]), length, expected,
            ), Err(actual) if actual == error));
            assert_eq!(shadows, [[0xa5; 8192]; 4]);
        }
    }

    #[test]
    fn private_inputs_reject_last_efs_digest_before_copying_first_shadow() {
        let efs = [[0x31; 8192]; 4];
        let wlan = [0x77; 1024];
        let mut input = inputs(&efs, &wlan);
        input.configuration.bindings[3].source[0] ^= 1;
        let mut shadows = [[0xa5; 8192]; 4];
        assert!(matches!(
            input.prepare_identity(
                [0x51; 32],
                shadows.each_mut().map(|s| &mut s[..]),
                wlan.len() as u64,
                digest(&wlan),
            ),
            Err(Error::Shadow(qcom_modem_ram_storage::Error::SourceDigest(
                Partition::ModemFsc
            )))
        ));
        assert_eq!(shadows, [[0xa5; 8192]; 4]);
    }

    #[test]
    fn public_private_input_path_does_not_accept_synthetic_firmware() {
        let efs = [[0x31; 8192]; 4];
        let mut shadows = [[0xa5; 8192]; 4];
        assert!(matches!(
            inputs(&efs, &[0; 1024]).prepare([0x51; 32], shadows.each_mut().map(|s| &mut s[..]),),
            Err(Error::FirmwareLength)
        ));
        assert_eq!(shadows, [[0xa5; 8192]; 4]);
        let wrong_firmware = std::vec![0; PHONE_OBSERVED_WLAN_MDSP_LEN as usize];
        assert!(matches!(
            inputs(&efs, &wrong_firmware)
                .prepare([0x51; 32], shadows.each_mut().map(|s| &mut s[..]),),
            Err(Error::FirmwareDigest)
        ));
        assert_eq!(shadows, [[0xa5; 8192]; 4]);
    }
}
