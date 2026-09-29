use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
    process,
};

use personal_os_rust_server::vault_crypto::{DevelopmentCryptoProvider, VaultCryptoProvider};
use personal_os_rust_server::{vault_backup, vault_blob_record, vault_record};

const SECTOR_SIZE: usize = 512;
const GPT_HEADER_LBA: usize = 1;
const GPT_MIN_HEADER_SIZE: usize = 92;
const VAULT_PARTITION_START_LBA: u64 = 2048;
const VAULT_PARTITION_START: usize = VAULT_PARTITION_START_LBA as usize * SECTOR_SIZE;
const VAULT_PARTITION_BLOCKS: usize = 8;
const BLOCK_SIZE: usize = 4096;
const SLOT_SIZE: usize = vault_record::SLOT_SIZE;
const SLOT_COUNT: usize = 2;
const NAMESPACE_FIRST_BLOCK: [usize; vault_backup::NAMESPACE_COUNT] = [0, 4, 6];

fn main() {
    if let Err(error) = run() {
        eprintln!("vault-backup: {error}");
        process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args_os().skip(1);
    match args
        .next()
        .and_then(|arg| arg.into_string().ok())
        .as_deref()
    {
        Some("export") => {
            let disk_path = next_path(&mut args, "disk image")?;
            let archive_path = next_path(&mut args, "archive output")?;
            reject_extra_args(args)?;
            let disk = read_disk(&disk_path)?;
            validate_gpt(&disk)?;
            let crypto = development_crypto();
            let mut records = [[0; BLOCK_SIZE]; vault_backup::NAMESPACE_COUNT];
            for (namespace, record) in records.iter_mut().enumerate() {
                *record = active_full_record(&disk, namespace, &crypto)?;
            }
            let archive = vault_backup::create_v2(&crypto, &records)
                .map_err(|error| format!("could not authenticate selected records: {error:?}"))?;
            write_new_file(&archive_path, &archive)?;
            print_development_key_notice();
            println!(
                "Exported authenticated vault records to {}",
                archive_path.display()
            );
            Ok(())
        }
        Some("restore") => {
            let archive_path = next_path(&mut args, "archive input")?;
            let source_disk_path = next_path(&mut args, "source disk image")?;
            let output_disk_path = next_path(&mut args, "new disk image output")?;
            let allow_rollback = match args.next() {
                None => false,
                Some(flag) if flag == "--allow-rollback" => true,
                Some(_) => return Err(usage()),
            };
            reject_extra_args(args)?;
            let archive = fs::read(&archive_path)
                .map_err(|error| format!("cannot read {}: {error}", archive_path.display()))?;
            let crypto = development_crypto();
            let records = open_archive(&crypto, &archive)?;
            let mut disk = read_disk(&source_disk_path)?;
            validate_gpt(&disk)?;
            let current_generations = current_authenticated_generations(&disk, &crypto)?;
            let rollback =
                vault_backup::rollback_full_records(&crypto, &current_generations, &records)
                    .map_err(|error| format!("backup rollback check failed: {error:?}"))?;
            if rollback.iter().any(|is_rollback| *is_rollback) {
                if !allow_rollback {
                    let names = ["system", "Notes", "Contacts"];
                    let older = rollback
                        .iter()
                        .enumerate()
                        .filter_map(|(namespace, is_rollback)| {
                            is_rollback.then_some(names[namespace])
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(format!(
                        "backup would roll back {older} relative to the source disk; pass --allow-rollback to confirm"
                    ));
                }
                eprintln!(
                    "warning: explicitly restoring older vault generations; this offline source-disk check is not hardware-backed anti-rollback protection"
                );
            }
            for (namespace, record) in records.iter().enumerate() {
                for slot_index in 0..SLOT_COUNT {
                    let block = NAMESPACE_FIRST_BLOCK[namespace] + slot_index;
                    let start = vault_block_offset(block)?;
                    disk[start..start + BLOCK_SIZE].fill(0);
                }
                let active_start = vault_block_offset(NAMESPACE_FIRST_BLOCK[namespace])?;
                disk[active_start..active_start + BLOCK_SIZE].copy_from_slice(record);
            }
            write_new_file(&output_disk_path, &disk)?;
            print_development_key_notice();
            println!(
                "Restored the backup into a new disk image at {}",
                output_disk_path.display()
            );
            println!("The source disk image was left unchanged.");
            Ok(())
        }
        _ => Err(usage()),
    }
}

fn next_path(
    args: &mut impl Iterator<Item = std::ffi::OsString>,
    name: &str,
) -> Result<PathBuf, String> {
    args.next()
        .map(PathBuf::from)
        .ok_or_else(|| format!("missing {name}\n{}", usage()))
}

fn reject_extra_args(mut args: impl Iterator<Item = std::ffi::OsString>) -> Result<(), String> {
    if args.next().is_some() {
        Err(usage())
    } else {
        Ok(())
    }
}

fn usage() -> String {
    "usage:\n  vault-backup export <raw-disk-image> <new-archive-file>\n  vault-backup restore <archive-file> <source-raw-disk-image> <new-disk-image> [--allow-rollback]".into()
}

fn read_disk(path: &PathBuf) -> Result<Vec<u8>, String> {
    let disk =
        fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if disk.len() % SECTOR_SIZE != 0 {
        return Err(format!(
            "{} is not a raw 512-byte-sector disk image",
            path.display()
        ));
    }
    Ok(disk)
}

fn validate_gpt(disk: &[u8]) -> Result<(), String> {
    let header_start = GPT_HEADER_LBA * SECTOR_SIZE;
    let header_end = header_start + SECTOR_SIZE;
    let header = disk
        .get(header_start..header_end)
        .ok_or_else(|| "disk image is too small to contain a GPT header".to_string())?;
    if &header[..8] != b"EFI PART" {
        return Err("disk image has no GPT header at LBA 1".into());
    }
    let header_size = read_u32(header, 12)? as usize;
    if !(GPT_MIN_HEADER_SIZE..=SECTOR_SIZE).contains(&header_size) {
        return Err("disk image has an invalid GPT header size".into());
    }
    let entries_lba = read_u64(header, 72)?;
    let entry_count = read_u32(header, 80)? as usize;
    let entry_size = read_u32(header, 84)? as usize;
    if entry_count == 0 || entry_size < 128 {
        return Err("disk image has an invalid GPT partition table".into());
    }
    let entries_start = usize::try_from(entries_lba)
        .ok()
        .and_then(|lba| lba.checked_mul(SECTOR_SIZE))
        .ok_or_else(|| "GPT partition table offset overflows".to_string())?;
    let entries_bytes = entry_count
        .checked_mul(entry_size)
        .ok_or_else(|| "GPT partition table size overflows".to_string())?;
    let entries_end = entries_start
        .checked_add(entries_bytes)
        .filter(|end| *end <= disk.len())
        .ok_or_else(|| "GPT partition table extends beyond the disk image".to_string())?;

    let required_end_lba =
        VAULT_PARTITION_START_LBA + (VAULT_PARTITION_BLOCKS * BLOCK_SIZE / SECTOR_SIZE) as u64 - 1;
    let mut matching_partition = false;
    for entry in disk[entries_start..entries_end].chunks_exact(entry_size) {
        if entry[..16].iter().all(|byte| *byte == 0) {
            continue;
        }
        let start_lba = read_u64(entry, 32)?;
        let end_lba = read_u64(entry, 40)?;
        if start_lba == VAULT_PARTITION_START_LBA {
            if end_lba < required_end_lba || end_lba >= (disk.len() / SECTOR_SIZE) as u64 {
                return Err("GPT vault partition is too small or extends beyond the image".into());
            }
            if matching_partition {
                return Err("GPT has multiple partitions at the vault offset".into());
            }
            matching_partition = true;
        }
    }
    if !matching_partition {
        return Err("GPT has no partition at the vault's expected LBA 2048".into());
    }
    Ok(())
}

fn development_crypto() -> DevelopmentCryptoProvider {
    DevelopmentCryptoProvider::new()
}

fn active_full_record(
    disk: &[u8],
    namespace: usize,
    crypto: &impl VaultCryptoProvider,
) -> Result<[u8; BLOCK_SIZE], String> {
    let mut selected: Option<(u64, [u8; BLOCK_SIZE], Option<vault_record::DecodedRecord>)> = None;
    for slot_index in 0..SLOT_COUNT {
        let block = NAMESPACE_FIRST_BLOCK[namespace] + slot_index;
        let start = vault_block_offset(block)?;
        let record: [u8; BLOCK_SIZE] = disk[start..start + BLOCK_SIZE]
            .try_into()
            .map_err(|_| format!("namespace {namespace} slot {slot_index} is truncated"))?;
        if record[..8] == vault_blob_record::MAGIC {
            let mut checked = record;
            if let Some(decoded) = vault_blob_record::decode(crypto, namespace, &mut checked)
                .map_err(|_| "development crypto provider failed".to_string())?
            {
                if selected
                    .as_ref()
                    .is_none_or(|(current, _, _)| decoded.generation > *current)
                {
                    selected = Some((decoded.generation, record, None));
                }
            }
            continue;
        }
        if record[SLOT_SIZE..].iter().any(|byte| *byte != 0) {
            continue;
        }
        let small: [u8; SLOT_SIZE] = record[..SLOT_SIZE].try_into().unwrap();
        let decoded = vault_record::decode(crypto, namespace, &small)
            .map_err(|_| "development crypto provider failed".to_string())?;
        if let Some(decoded) = decoded {
            if selected
                .as_ref()
                .is_none_or(|(current, _, _)| decoded.generation > *current)
            {
                selected = Some((decoded.generation, record, Some(decoded)));
            }
        }
    }
    match selected {
        Some((_, record, None)) => Ok(record),
        Some((_, record, Some(decoded)))
            if decoded.format == vault_record::RecordFormat::Encrypted =>
        {
            Ok(record)
        }
        Some((_, _, Some(decoded))) => {
            eprintln!(
                "warning: namespace {namespace} is legacy plaintext; preserving its decoded value, but its old checksum does not authenticate it"
            );
            let generation = decoded.generation.checked_add(1).ok_or_else(|| {
                format!("legacy namespace {namespace} generation cannot be advanced")
            })?;
            let encrypted =
                vault_record::encode(crypto, namespace, generation, decoded.value, true)
                    .map_err(|_| "development crypto provider failed".to_string())?
                    .ok_or_else(|| format!("namespace {namespace} cannot be encoded"))?;
            let mut block = [0; BLOCK_SIZE];
            block[..SLOT_SIZE].copy_from_slice(&encrypted);
            Ok(block)
        }
        None => Err(format!(
            "namespace {namespace} has no committed authenticated record to export"
        )),
    }
}

fn current_authenticated_generations(
    disk: &[u8],
    crypto: &impl VaultCryptoProvider,
) -> Result<[Option<u64>; vault_backup::NAMESPACE_COUNT], String> {
    let mut generations = [None; vault_backup::NAMESPACE_COUNT];
    for (namespace, generation) in generations.iter_mut().enumerate() {
        for slot_index in 0..SLOT_COUNT {
            let block = NAMESPACE_FIRST_BLOCK[namespace] + slot_index;
            let start = vault_block_offset(block)?;
            let record: [u8; BLOCK_SIZE] = disk[start..start + BLOCK_SIZE]
                .try_into()
                .map_err(|_| format!("namespace {namespace} slot {slot_index} is truncated"))?;
            let record_generation = if record[..8] == vault_blob_record::MAGIC {
                let mut checked = record;
                vault_blob_record::decode(crypto, namespace, &mut checked)
                    .map_err(|_| "development crypto provider failed".to_string())?
                    .map(|decoded| decoded.generation)
            } else if record[SLOT_SIZE..].iter().all(|byte| *byte == 0) {
                let small: [u8; SLOT_SIZE] = record[..SLOT_SIZE].try_into().unwrap();
                vault_record::decode(crypto, namespace, &small)
                    .map_err(|_| "development crypto provider failed".to_string())?
                    .filter(|decoded| decoded.format == vault_record::RecordFormat::Encrypted)
                    .map(|decoded| decoded.generation)
            } else {
                None
            };
            if let Some(record_generation) = record_generation {
                if generation.is_none_or(|current| record_generation > current) {
                    *generation = Some(record_generation);
                }
            }
        }
    }
    Ok(generations)
}

fn open_archive(
    crypto: &impl VaultCryptoProvider,
    bytes: &[u8],
) -> Result<[[u8; BLOCK_SIZE]; vault_backup::NAMESPACE_COUNT], String> {
    if bytes.len() == vault_backup::BACKUP_V2_SIZE {
        return vault_backup::open_v2(crypto, bytes)
            .map_err(|error| format!("backup validation failed: {error:?}"));
    }
    let small_records = vault_backup::open(crypto, bytes)
        .map_err(|error| format!("backup validation failed: {error:?}"))?;
    let mut records = [[0; BLOCK_SIZE]; vault_backup::NAMESPACE_COUNT];
    for (record, small) in records.iter_mut().zip(small_records) {
        record[..SLOT_SIZE].copy_from_slice(&small);
    }
    Ok(records)
}

fn vault_block_offset(block: usize) -> Result<usize, String> {
    let relative = block
        .checked_mul(BLOCK_SIZE)
        .ok_or_else(|| "vault block offset overflows".to_string())?;
    let end = relative
        .checked_add(BLOCK_SIZE)
        .ok_or_else(|| "vault block end overflows".to_string())?;
    if end > VAULT_PARTITION_BLOCKS * BLOCK_SIZE {
        return Err(format!("vault block {block} is outside the data partition"));
    }
    VAULT_PARTITION_START
        .checked_add(relative)
        .ok_or_else(|| "raw disk offset overflows".to_string())
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let field = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| "truncated GPT field".to_string())?;
    Ok(u32::from_le_bytes(field.try_into().unwrap()))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, String> {
    let field = bytes
        .get(offset..offset + 8)
        .ok_or_else(|| "truncated GPT field".to_string())?;
    Ok(u64::from_le_bytes(field.try_into().unwrap()))
}

fn write_new_file(path: &PathBuf, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| {
            format!(
                "cannot create {} (it may already exist): {error}",
                path.display()
            )
        })?;
    if let Err(error) = (|| -> std::io::Result<()> {
        file.write_all(bytes)?;
        file.sync_all()
    })() {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(format!("cannot finish writing {}: {error}", path.display()));
    }
    Ok(())
}

fn print_development_key_notice() {
    eprintln!(
        "warning: this prototype uses the development key embedded in the image; the archive is not confidential from someone with the source or boot image"
    );
}
