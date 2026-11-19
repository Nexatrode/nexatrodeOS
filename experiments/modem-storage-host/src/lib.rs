//! Offline regular-file adapter. No device, partition, SCM or native pool IO.
#![forbid(unsafe_code)]
#[allow(dead_code)] // The shared QMI module names the lifecycle permit type.
#[path = "../../../rust/src/qcom_wlan_power.rs"]
pub mod qcom_wlan_power;
#[path = "../../../rust/src/qcom_glink_fifo.rs"]
pub mod qcom_glink_fifo;
#[path = "../../../rust/src/qcom_glink_qmi.rs"]
pub mod qcom_glink_qmi;
#[path = "../../../rust/src/qcom_ipa.rs"]
pub mod qcom_ipa;
#[path = "../../../rust/src/qcom_ipa_client.rs"]
pub mod qcom_ipa_client;
#[path = "../../../rust/src/qcom_ipa_service.rs"]
pub mod qcom_ipa_service;
#[path = "../../../rust/src/qcom_modem_backend.rs"]
pub mod qcom_modem_backend;
#[path = "../../../rust/src/qcom_modem_handlers.rs"]
pub mod qcom_modem_handlers;
#[path = "../../../rust/src/qcom_modem_owner.rs"]
pub mod qcom_modem_owner;
#[path = "../../../rust/src/qcom_modem_pools.rs"]
pub mod qcom_modem_pools;
#[path = "../../../rust/src/qcom_modem_services.rs"]
pub mod qcom_modem_services;
#[path = "../../../rust/src/qcom_modem_storage.rs"]
pub mod qcom_modem_storage;
#[path = "../../../rust/src/qcom_msa.rs"]
pub mod qcom_msa;
#[path = "../../../rust/src/qcom_qmi.rs"]
pub mod qcom_qmi;
#[path = "../../../rust/src/qcom_qmi_router.rs"]
pub mod qcom_qmi_router;
#[path = "../../../rust/src/qcom_qrtr_control.rs"]
pub mod qcom_qrtr_control;
#[path = "../../../rust/src/qcom_qrtr_tftp.rs"]
pub mod qcom_qrtr_tftp;
#[path = "../../../rust/src/qcom_servreg.rs"]
pub mod qcom_servreg;
#[path = "../../../rust/src/qcom_servreg_notif.rs"]
pub mod qcom_servreg_notif;
#[path = "../../../rust/src/qcom_servreg_notif_client.rs"]
pub mod qcom_servreg_notif_client;
#[path = "../../../rust/src/qcom_tftp.rs"]
pub mod qcom_tftp;
#[path = "../../../rust/src/qcom_ufs_gpt.rs"]
pub mod qcom_ufs_gpt;
#[path = "../../../rust/src/qcom_ufs_host.rs"]
pub mod qcom_ufs_host;
#[path = "../../../rust/src/qcom_ufs_irq.rs"]
pub mod qcom_ufs_irq;
#[path = "../../../rust/src/qcom_ufs_modem_storage.rs"]
pub mod qcom_ufs_modem_storage;
#[path = "../../../rust/src/qcom_ufs_platform.rs"]
pub mod qcom_ufs_platform;
#[path = "../../../rust/src/qcom_ufs_protocol.rs"]
pub mod qcom_ufs_protocol;
#[path = "../../../rust/src/qcom_ufs_scsi_block_io.rs"]
pub mod qcom_ufs_scsi_block_io;
#[path = "../../../rust/src/sunfish_lpd.rs"]
pub mod sunfish_lpd;
#[path = "../../../rust/src/sunfish_smmu.rs"]
pub mod sunfish_smmu;
#[path = "../../../rust/src/sunfish_vendor_ext4.rs"]
pub mod sunfish_vendor_ext4;
#[path = "../../../rust/src/sunfish_vendor_wlan_source.rs"]
pub mod sunfish_vendor_wlan_source;
#[path = "../../../rust/src/qcom_wlfw_bdf.rs"]
pub mod qcom_wlfw_bdf;
#[path = "../../../rust/src/qcom_wlfw_client.rs"]
pub mod qcom_wlfw_client;
#[path = "../../../rust/src/sunfish_ipa_profile.rs"]
pub mod sunfish_ipa_profile;
#[path = "../../../rust/src/wlfw_protocol.rs"]
pub mod wlfw_protocol;

use qcom_modem_handlers::{Fault, IO_BYTES};
use qcom_modem_services::Partition;
use qcom_modem_storage::{
    index, Binding, Configuration, Digest, Io, Observation, Profile, MAX_PARTITION_BYTES,
};
use sha2::{Digest as _, Sha256};
use std::{
    fs::{self, File, Metadata, OpenOptions},
    os::unix::fs::{FileExt, MetadataExt},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT_FILES: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Open,
    Metadata,
    Shape,
    Lock,
    Hash,
    Alias,
    Configuration,
    Stopped,
    Write,
    Flush,
}
/// Reject without silently discarding an opened capability. Dropping this
/// explicit error closes its optional file; it never acknowledges persistence.
#[derive(Debug)]
pub struct Rejected {
    error: Error,
    _file: Option<File>,
}
impl Rejected {
    pub fn error(&self) -> Error {
        self.error
    }
}

pub struct VerifiedFile {
    file: File,
    binding: Binding,
    device: u64,
    inode: u64,
    mode: u32,
}
fn same(a: &Metadata, b: &Metadata) -> bool {
    a.dev() == b.dev()
        && a.ino() == b.ino()
        && a.len() == b.len()
        && a.mode() == b.mode()
        && a.mtime() == b.mtime()
        && a.mtime_nsec() == b.mtime_nsec()
        && a.ctime() == b.ctime()
        && a.ctime_nsec() == b.ctime_nsec()
        && b.nlink() == 1
        && b.is_file()
}
/// Local trusted setup only, never a modem pathname. Opens an existing regular
/// file without create/truncate/append; rejects symlinks/hardlinks and TOCTOU
/// inode changes, then holds an exclusive advisory lock and verifies a trusted
/// current checkpoint digest. origin/source remains immutable across writes.
/// Device identity and expected checkpoint authenticity come from the caller.
/// Locking is cooperative; no other descriptor/process may alter these files.
/// Full hashing is bounded by64MiB/file and happens before callback attachment.
/// No phone data is created or collected by this API.
pub fn open_verified(
    path: &Path,
    binding: Binding,
    expected_current: Digest,
) -> Result<VerifiedFile, Rejected> {
    let before = fs::symlink_metadata(path).map_err(|_| Rejected {
        error: Error::Open,
        _file: None,
    })?;
    if !before.is_file()
        || before.file_type().is_symlink()
        || before.nlink() != 1
        || before.len() == 0
        || before.len() > MAX_PARTITION_BYTES
        || before.len() != binding.disk.bytes
        || before.len() % 512 != 0
        || expected_current == [0; 32]
        || binding.source == [0; 32]
    {
        return Err(Rejected {
            error: Error::Shape,
            _file: None,
        });
    }
    let file = OpenOptions::new()
        .read(true)
        .write(binding.disk.writable)
        .open(path)
        .map_err(|_| Rejected {
            error: Error::Open,
            _file: None,
        })?;
    let verified = (|| {
        let after = file.metadata().map_err(|_| Error::Metadata)?;
        if !same(&before, &after)
            || (binding.disk.writable && after.permissions().readonly())
            || (binding.disk.writable && !binding.disk.durable)
        {
            return Err(Error::Shape);
        }
        file.try_lock().map_err(|_| Error::Lock)?;
        let mut hash = Sha256::new();
        let mut buffer = [0; IO_BYTES];
        let mut at = 0;
        while at < binding.disk.bytes {
            let n = (binding.disk.bytes - at).min(IO_BYTES as u64) as usize;
            if file
                .read_at(&mut buffer[..n], at)
                .map_err(|_| Error::Hash)?
                != n
            {
                return Err(Error::Hash);
            }
            hash.update(&buffer[..n]);
            at += n as u64;
        }
        let actual: Digest = hash.finalize().into();
        if actual != expected_current
            || !same(&after, &file.metadata().map_err(|_| Error::Metadata)?)
        {
            return Err(Error::Hash);
        }
        Ok((after.dev(), after.ino(), after.mode()))
    })();
    match verified {
        Ok((device, inode, mode)) => Ok(VerifiedFile {
            file,
            binding,
            device,
            inode,
            mode,
        }),
        Err(error) => Err(Rejected {
            error,
            _file: Some(file),
        }),
    }
}

pub struct Files {
    files: [VerifiedFile; 4],
    configuration: Configuration,
    identity: u64,
    error: Option<Error>,
}
impl Files {
    /// Retains all four verified descriptors even on invalid pairing/profile.
    pub fn attach(generation: u64, device: Digest, files: [VerifiedFile; 4]) -> Self {
        let configuration = Configuration {
            generation,
            device,
            bindings: files.each_ref().map(|f| f.binding),
        };
        let identity = NEXT_FILES
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .unwrap_or(0);
        let mut s = Self {
            files,
            configuration,
            identity,
            error: None,
        };
        if identity == 0 || Profile::validate(configuration).is_err() {
            s.error = Some(Error::Configuration);
        }
        for i in 0..4 {
            if s.files[..i]
                .iter()
                .any(|a| a.device == s.files[i].device && a.inode == s.files[i].inode)
            {
                s.error = Some(Error::Alias);
            }
        }
        s
    }
    pub fn configuration(&self) -> Configuration {
        self.configuration
    }
    pub fn error(&self) -> Option<Error> {
        self.error
    }
    fn stop(&mut self, error: Error) -> Fault {
        self.error = Some(error);
        Fault::Uncertain
    }
    fn check(&mut self) -> Result<(), Fault> {
        if self.error.is_some() {
            return Err(Fault::Uncertain);
        }
        for f in &self.files {
            let m = f.file.metadata().map_err(|_| Fault::Uncertain);
            let valid = m.is_ok_and(|m| {
                m.is_file()
                    && m.nlink() == 1
                    && m.dev() == f.device
                    && m.ino() == f.inode
                    && m.len() == f.binding.disk.bytes
                    && m.mode() == f.mode
            });
            if !valid {
                return Err(self.stop(Error::Metadata));
            }
        }
        Ok(())
    }
    fn bounds(&self, p: Partition, at: u64, n: usize) -> bool {
        n != 0
            && n <= IO_BYTES
            && n % 512 == 0
            && at % 512 == 0
            && at
                .checked_add(n as u64)
                .is_some_and(|e| e <= self.configuration.bindings[index(p)].disk.bytes)
    }
}
impl Io for Files {
    fn observe(&mut self) -> Result<Observation, Fault> {
        self.check()?;
        Ok(Observation {
            configuration: self.configuration,
            identity: self.identity,
            ready: true,
        })
    }
    fn read(&mut self, p: Partition, at: u64, out: &mut [u8]) -> Result<usize, Fault> {
        self.check()?;
        if !self.bounds(p, at, out.len()) {
            return Err(Fault::Failed);
        }
        let r = self.files[index(p)]
            .file
            .read_at(out, at)
            .map_err(|_| Fault::Failed);
        self.check()?;
        r
    }
    fn write(&mut self, p: Partition, at: u64, bytes: &[u8]) -> Result<usize, Fault> {
        self.check()?;
        if !self.bounds(p, at, bytes.len()) || !self.configuration.bindings[index(p)].disk.writable
        {
            return Err(Fault::Failed);
        }
        match self.files[index(p)].file.write_at(bytes, at) {
            Ok(n) if n == bytes.len() => {
                self.check()?;
                Ok(n)
            }
            _ => Err(self.stop(Error::Write)),
        }
    }
    fn flush(&mut self, p: Partition) -> Result<(), Fault> {
        self.check()?;
        if !self.configuration.bindings[index(p)].disk.durable {
            return Err(Fault::Failed);
        }
        if self.files[index(p)].file.sync_all().is_err() {
            return Err(self.stop(Error::Flush));
        }
        self.check()
    }
}
