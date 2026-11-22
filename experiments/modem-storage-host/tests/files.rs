use modem_storage_host::{
    Error, Files, open_verified,
    qcom_modem_handlers::{Disk, Fault},
    qcom_modem_services::Partition as Part,
    qcom_modem_storage::{self as s, Io},
};
use sha2::{Digest as _, Sha256};
use std::{
    fs::{self, DirBuilder, OpenOptions},
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(1);
fn hash(b: &[u8]) -> [u8; 32] {
    Sha256::digest(b).into()
}
fn data(i: usize) -> Vec<u8> {
    (0..16384).map(|at| (at * 17 + i * 31) as u8).collect()
}
fn binding(i: usize) -> s::Binding {
    s::Binding {
        partition: s::PARTITIONS[i],
        object: i as u64 + 1,
        source: hash(&data(i)),
        disk: Disk {
            bytes: 16384,
            writable: i != 2,
            durable: true,
        },
    }
}
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "modem-private-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        DirBuilder::new().mode(0o700).create(&p).unwrap();
        for i in 0..4 {
            let mut f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(p.join(i.to_string()))
                .unwrap();
            f.write_all(&data(i)).unwrap();
            f.sync_all().unwrap();
        }
        Self(p)
    }
    fn path(&self, i: usize) -> PathBuf {
        self.0.join(i.to_string())
    }
    fn files(&self) -> Files {
        files(&self.0, false)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn files(root: &Path, changed: bool) -> Files {
    let f = std::array::from_fn(|i| {
        let mut current = data(i);
        if changed && i == 1 {
            current[4096..4608].fill(0x91);
        }
        open_verified(&root.join(i.to_string()), binding(i), hash(&current)).unwrap()
    });
    Files::attach(1, [9; 32], f)
}
fn store(f: Files) -> s::Storage<Files> {
    let p = s::Profile::validate(f.configuration()).unwrap();
    let x = s::Storage::attach(p, f);
    assert_eq!(x.status().error, None);
    x
}

#[test]
fn verified_files_read_the_four_distinct_source_images() {
    let t = Temp::new();
    let mut f = t.files();
    assert_eq!(f.error(), None);
    for (i, p) in s::PARTITIONS.iter().enumerate() {
        let mut b = [0; 4096];
        assert_eq!(f.read(*p, 512, &mut b).unwrap(), 4096);
        assert_eq!(&b, &data(i)[512..4608]);
    }
}
#[test]
fn file_data_is_synced_before_success_and_matches_after_reopen() {
    let t = Temp::new();
    {
        let mut x = store(t.files());
        x.write(Part::ModemFs2, 4096, &[0x91; 512]).unwrap();
        assert!(x.status().dirty[1]);
        x.flush(Part::ModemFs2).unwrap();
        assert!(!x.status().dirty[1]);
    }
    let mut x = store(files(&t.0, true));
    let mut b = [0; 512];
    x.read(Part::ModemFs2, 4096, &mut b).unwrap();
    assert_eq!(b, [0x91; 512]);
    for i in [0, 2, 3] {
        assert_eq!(fs::read(t.path(i)).unwrap(), data(i));
    }
}
#[test]
fn wrong_checkpoint_does_not_fall_back_to_blank_or_original_data() {
    let t = Temp::new();
    let r = open_verified(&t.path(0), binding(0), [7; 32])
        .err()
        .unwrap();
    assert_eq!(r.error(), Error::Hash);
    assert_eq!(fs::read(t.path(0)).unwrap(), data(0));
}
#[test]
fn locks_remain_held_in_both_verified_and_rejected_resources() {
    let t = Temp::new();
    let a = open_verified(&t.path(0), binding(0), hash(&data(0))).unwrap();
    let r = open_verified(&t.path(0), binding(0), hash(&data(0)))
        .err()
        .unwrap();
    assert_eq!(r.error(), Error::Lock);
    drop(r);
    drop(a);
    let failed = open_verified(&t.path(0), binding(0), [6; 32])
        .err()
        .unwrap();
    let r = open_verified(&t.path(0), binding(0), hash(&data(0)))
        .err()
        .unwrap();
    assert_eq!(r.error(), Error::Lock);
    drop(r);
    drop(failed);
    assert!(open_verified(&t.path(0), binding(0), hash(&data(0))).is_ok());
}
#[test]
fn rejects_symlinks_hardlinks_directories_missing_files_and_wrong_lengths() {
    let t = Temp::new();
    let link = t.0.join("link");
    symlink(t.path(0), &link).unwrap();
    assert_eq!(
        open_verified(&link, binding(0), hash(&data(0)))
            .err()
            .unwrap()
            .error(),
        Error::Shape
    );
    fs::remove_file(&link).unwrap();
    fs::hard_link(t.path(0), &link).unwrap();
    assert_eq!(
        open_verified(&t.path(0), binding(0), hash(&data(0)))
            .err()
            .unwrap()
            .error(),
        Error::Shape
    );
    fs::remove_file(&link).unwrap();
    assert!(open_verified(&t.0, binding(0), hash(&data(0))).is_err());
    assert!(open_verified(&t.0.join("missing"), binding(0), hash(&data(0))).is_err());
    let mut bad = binding(0);
    bad.disk.bytes += 512;
    assert!(open_verified(&t.path(0), bad, hash(&data(0))).is_err());
}
#[test]
fn readonly_and_out_of_range_direct_io_never_changes_files() {
    let t = Temp::new();
    let mut f = t.files();
    assert_eq!(f.write(Part::ModemFsg, 0, &[8; 512]), Err(Fault::Failed));
    for (at, n) in [
        (0, 0),
        (1, 512),
        (0, 513),
        (0, 4608),
        (16384, 512),
        (u64::MAX, 512),
    ] {
        assert!(f.write(Part::ModemFs1, at, &vec![0; n]).is_err());
    }
    for i in 0..4 {
        assert_eq!(fs::read(t.path(i)).unwrap(), data(i));
    }
}
#[test]
fn file_replacement_unlink_resize_or_mode_change_invalidates_all_io() {
    for case in 0..3 {
        let t = Temp::new();
        let mut f = t.files();
        match case {
            0 => {
                fs::remove_file(t.path(0)).unwrap();
                fs::write(t.path(0), data(0)).unwrap();
            }
            1 => OpenOptions::new()
                .write(true)
                .open(t.path(1))
                .unwrap()
                .set_len(8192)
                .unwrap(),
            _ => fs::set_permissions(t.path(2), fs::Permissions::from_mode(0o400)).unwrap(),
        };
        assert!(f.observe().is_err());
        assert!(f.write(Part::ModemFs1, 0, &[0; 512]).is_err());
        assert!(f.flush(Part::ModemFs1).is_err());
    }
}
#[test]
fn duplicate_object_order_device_and_generation_configuration_rejected() {
    for n in 0..4 {
        let t = Temp::new();
        let f = std::array::from_fn(|i| {
            let mut b = binding(i);
            if n == 0 && i == 1 {
                b.object = 1;
            }
            if n == 1 {
                b.partition = s::PARTITIONS[(i + 1) % 4];
            }
            open_verified(&t.path(i), b, hash(&data(i))).unwrap()
        });
        let mut x = Files::attach(
            if n == 3 { 0 } else { 1 },
            if n == 2 { [0; 32] } else { [9; 32] },
            f,
        );
        assert!(x.error().is_some());
        assert!(x.observe().is_err());
    }
}
#[test]
fn mutable_file_requires_explicit_new_checkpoint_for_verified_restart() {
    let t = Temp::new();
    {
        let mut f = t.files();
        f.write(Part::ModemFs2, 4096, &[0x91; 512]).unwrap();
        f.flush(Part::ModemFs2).unwrap();
    }
    assert_eq!(
        open_verified(&t.path(1), binding(1), hash(&data(1)))
            .err()
            .unwrap()
            .error(),
        Error::Hash
    );
    assert_eq!(files(&t.0, true).error(), None);
}
#[test]
fn persistent_data_reopens_in_a_fresh_process() {
    let t = Temp::new();
    for mode in ["write", "read"] {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "reopen_child", "--nocapture"])
            .env("MODEM_STORAGE_PRIVATE_TEST_DIR", &t.0)
            .env("MODEM_STORAGE_PRIVATE_TEST_MODE", mode)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("PRIVATE_FILE_REOPEN_PASS"));
    }
}
#[test]
#[ignore = "invoked only by the disposable-file subprocess test"]
fn reopen_child() {
    let root = PathBuf::from(std::env::var_os("MODEM_STORAGE_PRIVATE_TEST_DIR").unwrap());
    let read = std::env::var("MODEM_STORAGE_PRIVATE_TEST_MODE").unwrap() == "read";
    let mut x = store(files(&root, read));
    if read {
        let mut b = [0; 512];
        x.read(Part::ModemFs2, 4096, &mut b).unwrap();
        assert_eq!(b, [0x91; 512]);
    } else {
        x.write(Part::ModemFs2, 4096, &[0x91; 512]).unwrap();
        x.flush(Part::ModemFs2).unwrap();
    }
    println!("PRIVATE_FILE_REOPEN_PASS");
}
