//! Verified, exclusively borrowed EFS shadows for native cold-start experiments.
//!
//! This implements the storage behavior exercised by Linux `rmtfs -r`: reads
//! and writes use private RAM, and sync acknowledges that retained RAM only.
//! Original snapshots remain immutable. No UFS device, raw partition, SCM or
//! DMA capability is present in this provider. The platform must independently
//! bind the snapshot manifest to this phone and the intended cold boot.
#![forbid(unsafe_code)]

use crate::qcom_modem_handlers::{Fault, Persistence};
use crate::qcom_modem_storage::RAM_IO_BYTES as IO_BYTES;
use crate::qcom_modem_services::Partition;
use crate::qcom_modem_storage::{
    self as storage, Configuration, Io, Observation, PARTITIONS, Profile, SECTOR_BYTES, Storage,
};
use crate::sunfish_lpd::Sha256;
use core::sync::atomic::{AtomicU64, Ordering};

static NEXT_SHADOW: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Profile,
    Geometry(Partition),
    SourceDigest(Partition),
    IdentityExhausted,
}

/// Safe Rust borrows keep all four source slices immutable and every shadow
/// exclusive. A native caller constructing those slices from mapped frames
/// must uphold the same exclusivity and lifetime, with no writable aliases.
pub struct RamShadow<'a> {
    configuration: Configuration,
    identity: u64,
    sources: [&'a [u8]; 4],
    shadows: [&'a mut [u8]; 4],
    retired: bool,
}

impl<'a> RamShadow<'a> {
    /// Verify all geometry and complete source digests before modifying any
    /// shadow byte. Expected digests must come from the selected private
    /// manifest; a matching digest alone is not device/freshness evidence.
    pub fn new(
        configuration: Configuration,
        sources: [&'a [u8]; 4],
        shadows: [&'a mut [u8]; 4],
    ) -> Result<Self, Error> {
        Profile::validate_ram_shadow(configuration).map_err(|_| Error::Profile)?;
        for (index, partition) in PARTITIONS.into_iter().enumerate() {
            let binding = configuration.bindings[index];
            if sources[index].len() as u64 != binding.disk.bytes
                || shadows[index].len() != sources[index].len()
            {
                return Err(Error::Geometry(partition));
            }
            let mut hash = Sha256::new();
            hash.update(sources[index]);
            if hash.finish() != binding.source {
                return Err(Error::SourceDigest(partition));
            }
        }
        let identity = NEXT_SHADOW
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| Error::IdentityExhausted)?;
        for index in 0..4 {
            shadows[index].copy_from_slice(sources[index]);
        }
        Ok(Self {
            configuration,
            identity,
            sources,
            shadows,
            retired: false,
        })
    }

    pub fn into_storage(self) -> Result<Storage<Self>, Fault> {
        let profile = Profile::validate_ram_shadow(self.configuration)?;
        Ok(Storage::attach(profile, self))
    }

    /// Keep buffers retained for diagnosis while refusing every later I/O.
    /// There is deliberately no reset/reinitialize or persistence operation.
    pub fn retire(&mut self) {
        self.retired = true;
    }

    pub fn source(&self, partition: Partition) -> &[u8] {
        self.sources[storage::index(partition)]
    }

    fn span(&self, partition: Partition, offset: u64, count: usize) -> Result<usize, Fault> {
        if self.retired
            || count == 0
            || count > IO_BYTES
            || count as u64 % SECTOR_BYTES != 0
            || offset % SECTOR_BYTES != 0
            || offset.checked_add(count as u64).is_none_or(|end| {
                end > self.configuration.bindings[storage::index(partition)]
                    .disk
                    .bytes
            })
        {
            return Err(Fault::Failed);
        }
        usize::try_from(offset).map_err(|_| Fault::Failed)
    }
}

impl Io for RamShadow<'_> {
    fn persistence(&self) -> Persistence {
        Persistence::RamShadow
    }

    fn observe(&mut self) -> Result<Observation, Fault> {
        Ok(Observation {
            configuration: self.configuration,
            identity: self.identity,
            ready: !self.retired,
        })
    }

    fn read(&mut self, partition: Partition, offset: u64, out: &mut [u8]) -> Result<usize, Fault> {
        let start = self.span(partition, offset, out.len())?;
        out.copy_from_slice(&self.shadows[storage::index(partition)][start..start + out.len()]);
        Ok(out.len())
    }

    fn write(&mut self, partition: Partition, offset: u64, bytes: &[u8]) -> Result<usize, Fault> {
        let start = self.span(partition, offset, bytes.len())?;
        self.shadows[storage::index(partition)][start..start + bytes.len()].copy_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self, _partition: Partition) -> Result<(), Fault> {
        if self.retired {
            Err(Fault::Failed)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qcom_modem_handlers::Disk;
    use crate::qcom_modem_storage::Binding;

    fn configuration(sources: &[[u8; 8192]; 4]) -> Configuration {
        Configuration {
            generation: 7,
            device: [0x51; 32],
            bindings: core::array::from_fn(|index| {
                let mut hash = Sha256::new();
                hash.update(&sources[index]);
                Binding {
                    partition: PARTITIONS[index],
                    object: index as u64 + 1,
                    source: hash.finish(),
                    disk: Disk {
                        bytes: 8192,
                        writable: true,
                        durable: false,
                    },
                }
            }),
        }
    }

    #[test]
    fn ram_shadow_writes_and_sync_preserve_all_original_snapshots() {
        let sources = core::array::from_fn(|i| [i as u8 + 1; 8192]);
        let configuration = configuration(&sources);
        assert!(Profile::validate(configuration).is_err());
        let mut shadows = [[0xa5; 8192]; 4];
        let mut io = RamShadow::new(
            configuration,
            sources.each_ref().map(|s| &s[..]),
            shadows.each_mut().map(|s| &mut s[..]),
        )
        .unwrap();
        assert_eq!(io.persistence(), Persistence::RamShadow);
        let mut buffer = [0; 4096];
        for (i, partition) in PARTITIONS.into_iter().enumerate() {
            io.read(partition, 0, &mut buffer).unwrap();
            assert_eq!(buffer, [i as u8 + 1; 4096]);
            io.write(partition, 4096, &[0x77; 4096]).unwrap();
            io.flush(partition).unwrap();
            io.read(partition, 4096, &mut buffer).unwrap();
            assert_eq!(buffer, [0x77; 4096]);
            assert_eq!(io.source(partition), &[i as u8 + 1; 8192]);
        }
        drop(io);
        assert_eq!(sources, core::array::from_fn(|i| [i as u8 + 1; 8192]));
        assert!(shadows.iter().all(|s| s[4096..] == [0x77; 4096]));
    }

    #[test]
    fn ram_shadow_rejects_corrupt_last_source_before_touching_any_destination() {
        let sources = [[0x31; 8192]; 4];
        let mut configuration = configuration(&sources);
        configuration.bindings[3].source[31] ^= 1;
        let mut shadows = [[0xa5; 8192]; 4];
        let result = RamShadow::new(
            configuration,
            sources.each_ref().map(|s| &s[..]),
            shadows.each_mut().map(|s| &mut s[..]),
        );
        assert!(matches!(
            result,
            Err(Error::SourceDigest(Partition::ModemFsc))
        ));
        assert_eq!(shadows, [[0xa5; 8192]; 4]);
    }

    #[test]
    fn ram_shadow_storage_reports_volatility_and_clears_only_its_ram_dirty_state() {
        let sources = [[0x31; 8192]; 4];
        let mut shadows = [[0; 8192]; 4];
        let mut storage = RamShadow::new(
            configuration(&sources),
            sources.each_ref().map(|s| &s[..]),
            shadows.each_mut().map(|s| &mut s[..]),
        )
        .unwrap()
        .into_storage()
        .unwrap();
        assert_eq!(storage.persistence(), Persistence::RamShadow);
        assert!(!storage.disk(Partition::ModemFs1).unwrap().durable);
        storage
            .write(Partition::ModemFs1, 512, &[0x99; 512])
            .unwrap();
        assert_eq!(storage.status().dirty, [true, false, false, false]);
        storage.flush(Partition::ModemFs1).unwrap();
        assert_eq!(storage.status().dirty, [false; 4]);
        let mut buffer = [0; 512];
        storage.read(Partition::ModemFs1, 512, &mut buffer).unwrap();
        assert_eq!(buffer, [0x99; 512]);
        assert_eq!(sources, [[0x31; 8192]; 4]);
    }

    #[test]
    fn ram_shadow_rejects_cross_boundary_unaligned_and_retired_io() {
        let sources = [[0x31; 8192]; 4];
        let mut shadows = [[0; 8192]; 4];
        let mut io = RamShadow::new(
            configuration(&sources),
            sources.each_ref().map(|s| &s[..]),
            shadows.each_mut().map(|s| &mut s[..]),
        )
        .unwrap();
        for (offset, count) in [
            (0, 0),
            (1, 512),
            (0, 513),
            (8192, 512),
            (u64::MAX - 511, 512),
            (0, 131072 + 512),
        ] {
            assert_eq!(
                io.write(Partition::ModemFs1, offset, &std::vec![1; count]),
                Err(Fault::Failed)
            );
        }
        assert_eq!(io.source(Partition::ModemFs1), &[0x31; 8192]);
        io.retire();
        assert!(!io.observe().unwrap().ready);
        assert_eq!(
            io.write(Partition::ModemFs1, 0, &[0; 512]),
            Err(Fault::Failed)
        );
        assert_eq!(io.flush(Partition::ModemFs1), Err(Fault::Failed));
    }
}
