#!/usr/bin/env python3
"""Export and import an official SimpleX archive for the NexatrodeOS guest.

The zip matches the official app: simplex_v1_chat.db and simplex_v1_agent.db.
The passphrase is the SQLCipher key. It is written next to the zip, not inside
it, because the official importer copies those two files and then asks for the
key.

export only reads the guest disk. import writes a new disk image and refuses
the live Messages disk and the saved profile copy.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import tempfile
import zipfile
from datetime import datetime, timezone
from pathlib import Path

from cryptography.hazmat.primitives.ciphers.aead import AESGCMSIV

ROOT = Path(__file__).resolve().parents[2]
LIVE_DISK = ROOT / "dist" / "NexatrodeOS-Messages.utm" / "Data" / "personal-os-disk.img"
SAVED_DISK = Path.home() / "simplexdatabases" / "nexatrodeos" / "personal-os-disk.img"
ARCHIVE_DIR = SAVED_DISK.parent
SIMPLEX_CHAT = (
    ROOT
    / ".cache/simplex-chat-upstream/dist-newstyle/build/aarch64-osx/ghc-9.6.3"
    / "simplex-chat-7.0.0.12/x/simplex-chat/build/simplex-chat/simplex-chat"
)
PASSPHRASE = "qemu-agentstore-test-key-v1"

SECTOR = 512
BLOCK = 4096
PARTITION_START = 2048 * SECTOR
AGENT_METADATA_BLOCKS = (10, 11)
AGENT_FIRST_BLOCK = 14
JOURNAL_METADATA_BLOCKS = (12, 13)
METADATA_NAMESPACE = 0x20
JOURNAL_NAMESPACE = 0x21
MESSAGES_NAMESPACE = 4
MESSAGES_BLOCKS = (8, 9)
OWN_NAME = "NexatrodeOS"
FILE_BLOCKS = 1024
COMMIT_SALT = 0x3C6EF372FE94F82B
DEVELOPMENT_KEY = bytes([
    0x91, 0x2E, 0x7A, 0x04, 0xC3, 0x58, 0xD1, 0x6F, 0x88, 0x33, 0xB7, 0x2A, 0x45, 0xE0, 0x19, 0xAD,
    0x6C, 0x52, 0xF8, 0x0B, 0x97, 0x31, 0x44, 0xAA, 0x0E, 0xD5, 0x63, 0x7F, 0x28, 0xB1, 0x9C, 0xE2,
])
CHAT_DB_NAME = "simplex_v1_chat.db"
AGENT_DB_NAME = "simplex_v1_agent.db"
PROTECTED_DISKS = {LIVE_DISK.resolve(), SAVED_DISK.resolve()}


def block(disk: bytes, number: int) -> bytes:
    start = PARTITION_START + number * BLOCK
    end = start + BLOCK
    if end > len(disk):
        raise ValueError(f"vault block {number} extends beyond the disk image")
    return disk[start:end]


def decrypt_metadata(record: bytes, namespace: int) -> tuple[int, bytes] | None:
    if record[:8] != b"PVLTBLB1" or int.from_bytes(record[8:12], "little") != 1:
        return None
    generation = int.from_bytes(record[12:20], "little")
    length = int.from_bytes(record[20:24], "little")
    if length > BLOCK - 36 - 16 - 8:
        return None
    tag_end = 36 + length + 16
    marker = int.from_bytes(record[tag_end:tag_end + 8], "little")
    if marker != generation ^ COMMIT_SALT or any(record[tag_end + 8:]):
        return None
    nonce = b"PB1" + bytes([namespace]) + generation.to_bytes(8, "little")
    if record[24:36] != nonce:
        return None
    aad = (
        b"PVLTBL1A"
        + (1).to_bytes(4, "little")
        + namespace.to_bytes(8, "little")
        + generation.to_bytes(8, "little")
        + length.to_bytes(4, "little")
    )
    try:
        payload = AESGCMSIV(DEVELOPMENT_KEY).decrypt(nonce, record[36:tag_end], aad)
    except Exception:
        return None
    return generation, payload


def file_metadata(disk: bytes, slots: tuple[int, int], namespace: int, file_id: int) -> tuple[int, int, bool, int]:
    selected: tuple[int, int, bool, int] | None = None
    for slot, number in enumerate(slots):
        opened = decrypt_metadata(block(disk, number), namespace)
        if opened is None:
            continue
        generation, payload = opened
        if len(payload) != 16 or payload[:8] != b"PVLTFM1\0":
            continue
        if int.from_bytes(payload[8:12], "little") != file_id:
            continue
        encoded = int.from_bytes(payload[12:16], "little")
        length = encoded & 0x7FFFFFFF
        exists = encoded & 0x80000000 != 0
        if length > FILE_BLOCKS * BLOCK:
            continue
        candidate = (generation, length, exists, slot)
        if selected is None or candidate[0] > selected[0]:
            selected = candidate
    if selected is None:
        raise ValueError("file metadata has no committed record")
    return selected


def read_extent(disk: bytes, first_block: int, length: int) -> bytes:
    output = bytearray(length)
    copied = 0
    while copied < length:
        logical = copied // BLOCK
        within = copied % BLOCK
        if logical >= FILE_BLOCKS:
            raise ValueError("file extends past its fixed extent")
        data = block(disk, first_block + logical)
        count = min(BLOCK - within, length - copied)
        output[copied:copied + count] = data[within:within + count]
        copied += count
    return bytes(output)


def extract_agent_database(disk: bytes) -> bytes:
    _generation, length, exists, _slot = file_metadata(disk, AGENT_METADATA_BLOCKS, METADATA_NAMESPACE, 0)
    if not exists or length < 16:
        raise ValueError("agent database is missing on this disk")
    database = read_extent(disk, AGENT_FIRST_BLOCK, length)
    if database.startswith(b"SQLite format 3"):
        raise ValueError("agent database is not encrypted")
    return database


def encode_metadata(namespace: int, file_id: int, generation: int, length: int, exists: bool) -> bytes:
    payload = bytearray(16)
    payload[:8] = b"PVLTFM1\0"
    payload[8:12] = file_id.to_bytes(4, "little")
    encoded = length | (0x80000000 if exists else 0)
    payload[12:16] = encoded.to_bytes(4, "little")
    nonce = b"PB1" + bytes([namespace]) + generation.to_bytes(8, "little")
    aad = (
        b"PVLTBL1A"
        + (1).to_bytes(4, "little")
        + namespace.to_bytes(8, "little")
        + generation.to_bytes(8, "little")
        + (16).to_bytes(4, "little")
    )
    sealed = AESGCMSIV(DEVELOPMENT_KEY).encrypt(nonce, bytes(payload), aad)
    record = bytearray(BLOCK)
    record[:8] = b"PVLTBLB1"
    record[8:12] = (1).to_bytes(4, "little")
    record[12:20] = generation.to_bytes(8, "little")
    record[20:24] = (16).to_bytes(4, "little")
    record[24:36] = nonce
    record[36:36 + len(sealed)] = sealed
    marker_at = 36 + len(sealed)
    record[marker_at:marker_at + 8] = (generation ^ COMMIT_SALT).to_bytes(8, "little")
    return bytes(record)


def write_extent(disk: bytearray, first_block: int, payload: bytes) -> None:
    if len(payload) > FILE_BLOCKS * BLOCK:
        raise ValueError("file is larger than the guest extent")
    padded = payload + bytes((-len(payload)) % BLOCK)
    # Drop stale bytes past the new end by writing one extra zero block when it fits.
    blocks = [padded[index:index + BLOCK] for index in range(0, len(padded), BLOCK)]
    if len(payload) % BLOCK == 0 and len(blocks) < FILE_BLOCKS:
        blocks.append(bytes(BLOCK))
    for logical, data in enumerate(blocks):
        start = PARTITION_START + (first_block + logical) * BLOCK
        disk[start:start + BLOCK] = data


def install_file(
    disk: bytearray,
    slots: tuple[int, int],
    namespace: int,
    file_id: int,
    first_block: int,
    payload: bytes,
    exists: bool,
) -> None:
    generation, _length, _exists, active = file_metadata(disk, slots, namespace, file_id)
    target = 1 - active
    record = encode_metadata(namespace, file_id, generation + 1, len(payload) if exists else 0, exists)
    start = PARTITION_START + slots[target] * BLOCK
    disk[start:start + BLOCK] = record
    if exists:
        write_extent(disk, first_block, payload)


def protected(path: Path) -> bool:
    try:
        return path.resolve() in PROTECTED_DISKS
    except OSError:
        return False


def read_disk(path: Path) -> bytes:
    disk = path.read_bytes()
    if len(disk) % SECTOR != 0 or disk[512:520] != b"EFI PART":
        raise ValueError(f"{path} is not a raw GPT disk image")
    return disk


def chat_command(prefix: Path, command: str, key: str = PASSPHRASE) -> str:
    result = subprocess.run(
        [
            str(SIMPLEX_CHAT),
            "-d", str(prefix),
            "-k", key,
            "-y",
            "--maintenance",
            "-t", "0",
            "-e", command,
        ],
        check=False,
        capture_output=True,
        text=True,
        timeout=60,
    )
    output = result.stdout + result.stderr
    if result.returncode != 0 or "error:" in output.lower() or "db error" in output.lower():
        raise RuntimeError(output)
    return result.stdout


def extract_archive(archive_path: Path, work: Path) -> Path:
    with zipfile.ZipFile(archive_path) as archive:
        names = set(archive.namelist())
        if CHAT_DB_NAME not in names or AGENT_DB_NAME not in names:
            raise ValueError("archive is missing simplex_v1_chat.db or simplex_v1_agent.db")
        archive.extract(CHAT_DB_NAME, work)
        archive.extract(AGENT_DB_NAME, work)
    prefix = work / "simplex_v1"
    chat_db = Path(str(prefix) + "_chat.db")
    agent_db = Path(str(prefix) + "_agent.db")
    chat_db.write_bytes((work / CHAT_DB_NAME).read_bytes())
    agent_db.write_bytes((work / AGENT_DB_NAME).read_bytes())
    return prefix


def write_archive(archive_path: Path, chat_db: bytes, agent_db: bytes) -> None:
    if archive_path.exists():
        raise ValueError(f"{archive_path} already exists")
    with zipfile.ZipFile(archive_path, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        archive.writestr(CHAT_DB_NAME, chat_db)
        archive.writestr(AGENT_DB_NAME, agent_db)
    os.chmod(archive_path, 0o600)


def unlock_archive(archive_path: Path, passphrase: str) -> str:
    """Open an official zip with the passphrase; return the /users listing."""
    with tempfile.TemporaryDirectory(prefix="nexatrode-simplex-unlock-") as temporary:
        prefix = extract_archive(archive_path, Path(temporary))
        listed = chat_command(prefix, "/users", key=passphrase)
        if OWN_NAME not in listed and "Current user:" not in listed:
            raise RuntimeError("passphrase opened the archive but no chat user was listed")
        print(listed.strip())
        print(f"unlocked: {archive_path}")
        return listed


def rekey_archive(
    archive_path: Path,
    current: str,
    new: str,
    output_dir: Path,
) -> Path:
    """Change the SQLCipher passphrase on both DBs like mobile `/db key`."""
    if not current or not new:
        raise ValueError("current and new passphrases are required")
    if current == new:
        raise ValueError("new passphrase must differ from the current passphrase")
    if protected(output_dir):
        raise ValueError("refusing to write the archive over a disk image")
    output_dir.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H%M%SZ")
    out_zip = output_dir / f"simplex-chat.{stamp}.zip"
    out_pass = output_dir / f"simplex-chat.{stamp}.passphrase"
    with tempfile.TemporaryDirectory(prefix="nexatrode-simplex-rekey-") as temporary:
        work = Path(temporary)
        prefix = extract_archive(archive_path, work)
        # Prove the current key first so a wrong unlock does not leave half-rekeyed files.
        unlock_listing = chat_command(prefix, "/users", key=current)
        if OWN_NAME not in unlock_listing:
            raise RuntimeError("current passphrase did not unlock the NexatrodeOS profile")
        changed = chat_command(prefix, f"/db key {current} {new}", key=current)
        if "ok" not in changed.lower():
            raise RuntimeError(changed)
        # Old key must fail; new key must list the same user.
        old = subprocess.run(
            [
                str(SIMPLEX_CHAT),
                "-d", str(prefix),
                "-k", current,
                "-y",
                "--maintenance",
                "-t", "0",
                "-e", "/users",
            ],
            check=False,
            capture_output=True,
            text=True,
            timeout=60,
        )
        old_out = old.stdout + old.stderr
        if old.returncode == 0 and OWN_NAME in old_out and "not a database" not in old_out.lower():
            raise RuntimeError("old passphrase still opens the databases after rekey")
        listed = chat_command(prefix, "/users", key=new)
        if OWN_NAME not in listed:
            raise RuntimeError("new passphrase did not keep the NexatrodeOS user")
        write_archive(
            out_zip,
            Path(str(prefix) + "_chat.db").read_bytes(),
            Path(str(prefix) + "_agent.db").read_bytes(),
        )
    out_pass.write_text(new + "\n")
    os.chmod(out_pass, 0o600)
    print(f"archive: {out_zip}")
    print(f"passphrase file: {out_pass}")
    print("rekey: ok")
    print("The source archive was left unchanged.")
    return out_zip


def sql_rows(prefix: Path, side: str, statement: str) -> list[list[str]]:
    output = chat_command(prefix, f"/sql {side} {statement}")
    lines = [line for line in output.splitlines() if line and not line.startswith("Current user:")]
    if not lines:
        return []
    return [line.split("|") for line in lines[1:]]


def sql_quote(value: str) -> str:
    return "'" + value.replace("'", "''") + "'"


def add_named_contacts(prefix: Path) -> list[str]:
    """Point the chat database at duplex connections the guest has named."""
    peers = sql_rows(
        prefix,
        "agent",
        "SELECT hex(p.conn_id), p.display_name FROM guest_peer_names p "
        "WHERE EXISTS (SELECT 1 FROM snd_queues s WHERE s.conn_id = p.conn_id)",
    )
    added: list[str] = []
    for conn_hex, name in peers:
        if not re.fullmatch(r"[0-9A-Fa-f]+", conn_hex) or not name or any(ord(ch) < 32 for ch in name):
            continue
        taken = {row[0] for row in sql_rows(prefix, "chat", "SELECT local_display_name FROM display_names")}
        local_name = name
        suffix = 2
        while local_name in taken:
            local_name = f"{name} {suffix}"
            suffix += 1
        profile_id = 1 + max(
            (int(row[0]) for row in sql_rows(prefix, "chat", "SELECT contact_profile_id FROM contact_profiles")),
            default=0,
        )
        stamp = "datetime('now')"
        quoted = sql_quote(local_name)
        chat_command(
            prefix,
            "/sql chat INSERT INTO contact_profiles "
            "(contact_profile_id, display_name, full_name, properties, user_id, created_at, updated_at) "
            f"VALUES ({profile_id}, {quoted}, '', '{{}}', 1, {stamp}, {stamp})",
        )
        chat_command(
            prefix,
            "/sql chat INSERT INTO display_names "
            "(user_id, local_display_name, ldn_base, ldn_suffix, created_at, updated_at) "
            f"VALUES (1, {quoted}, {quoted}, 0, {stamp}, {stamp})",
        )
        chat_command(
            prefix,
            "/sql chat INSERT INTO contacts "
            "(contact_profile_id, user_id, local_display_name, is_user, contact_used, created_at, updated_at, chat_ts) "
            f"VALUES ({profile_id}, 1, {quoted}, 0, 1, {stamp}, {stamp}, {stamp})",
        )
        contact_id = sql_rows(
            prefix,
            "chat",
            f"SELECT contact_id FROM contacts WHERE local_display_name = {quoted}",
        )[0][0]
        chat_command(
            prefix,
            "/sql chat INSERT INTO connections "
            "(agent_conn_id, conn_status, conn_type, contact_id, user_id, created_at, updated_at) "
            f"VALUES (X'{conn_hex}', 'ready', 'contact', {contact_id}, 1, {stamp}, {stamp})",
        )
        added.append(local_name)
    return added


def messages_payload(disk: bytes) -> bytes | None:
    selected: tuple[int, bytes] | None = None
    for number in MESSAGES_BLOCKS:
        opened = decrypt_metadata(block(disk, number), MESSAGES_NAMESPACE)
        if opened is None:
            continue
        if selected is None or opened[0] > selected[0]:
            selected = opened
    return None if selected is None else selected[1]


# Matches rust/src/messages.rs ChatTranscripts / decode_archive.
CHAT_SLOT_COUNT = 16
TRANSCRIPT_LIMIT = 700
MESSAGE_CLOCK_MARK = "\x1f"


def transcript_slots(payload: bytes) -> list[str]:
    """Decode PMS3 chat transcripts; ignore the trailing archived bitmask."""
    if len(payload) >= 4 and payload[:4] == b"PMS2":
        return []
    if len(payload) < 6 or payload[:4] != b"PMS3":
        return []
    inner = int.from_bytes(payload[4:6], "little")
    if 6 + inner > len(payload):
        return []
    cursor = 6 + inner
    slots: list[str] = []
    for _slot in range(CHAT_SLOT_COUNT):
        if cursor == len(payload):
            break
        if cursor + 2 > len(payload):
            return []
        length = int.from_bytes(payload[cursor:cursor + 2], "little")
        cursor += 2
        if length > TRANSCRIPT_LIMIT or cursor + length > len(payload):
            return []
        slots.append(payload[cursor:cursor + length].decode("utf-8", "replace"))
        cursor += length
    # Optional 2-byte archived mask written after the 16 slots.
    if cursor + 2 == len(payload):
        cursor += 2
    return slots if cursor == len(payload) else []


def strip_message_time(body: str) -> str:
    """Drop the guest HH:MM + unit-separator clock prefix from a stored body."""
    if (
        len(body) >= 6
        and body[2] == ":"
        and body[5] == MESSAGE_CLOCK_MARK
        and body[0:2].isdigit()
        and body[3:5].isdigit()
    ):
        return body[6:]
    return body


def transcript_lines(text: str) -> list[tuple[str, str]]:
    rows: list[tuple[str, str]] = []
    for line in text.splitlines():
        speaker, separator, body = line.partition(": ")
        body = strip_message_time(body)
        if body.endswith(" \u2713"):
            body = body[:-2]
        if separator and speaker and body:
            rows.append((speaker, body))
    return rows


def message_json(sent: bool, body: str) -> tuple[str, str, str]:
    tag = "sndMsgContent" if sent else "rcvMsgContent"
    content = {tag: {"msgContent": {"type": "text", "text": body}}}
    status = "snd_sent complete" if sent else "rcv_read"
    return tag, json.dumps(content, ensure_ascii=False, separators=(",", ":")), status


def add_transcripts(prefix: Path, disk: bytes) -> int:
    """Copy the guest's saved thread text into the chat database.

    Each slot is the latest 700 characters of one chat. A line is stored with
    the contact named in that slot. Lines that name nobody we exported are left
    out. The guest does not store the original clock time, so the rows are
    ordered one second apart.
    """
    payload = messages_payload(disk)
    if payload is None:
        return 0
    stored = 0
    second = 0
    for text in transcript_slots(payload):
        lines = transcript_lines(text)
        if not lines:
            continue
        others = {speaker for speaker, _body in lines if speaker != OWN_NAME}
        # 1:1 slots name exactly one peer. Group slots may name several; keep the
        # first peer we already exported as a contact so history is not dropped.
        contact = None
        for speaker in others:
            found = sql_rows(
                prefix,
                "chat",
                "SELECT contact_id FROM contacts WHERE local_display_name = "
                + sql_quote(speaker),
            )
            if len(found) == 1:
                contact = speaker
                contact_id = found[0][0]
                break
        if contact is None:
            continue
        last_stamp = ""
        for speaker, body in lines:
            sent = speaker == OWN_NAME
            tag, content, status = message_json(sent, body)
            stamp = f"2026-11-03 {18 + second // 3600:02d}:{(second // 60) % 60:02d}:{second % 60:02d}"
            second += 1
            last_stamp = stamp
            chat_command(
                prefix,
                "/sql chat INSERT INTO chat_items "
                "(user_id, contact_id, item_sent, item_ts, item_deleted, item_content, item_text, "
                "item_content_tag, item_status, created_at, updated_at, include_in_history) "
                f"VALUES (1, {contact_id}, {int(sent)}, {sql_quote(stamp)}, 0, {sql_quote(content)}, "
                f"{sql_quote(body)}, {sql_quote(tag)}, {sql_quote(status)}, {sql_quote(stamp)}, {sql_quote(stamp)}, 1)",
            )
            stored += 1
        if last_stamp:
            chat_command(
                prefix,
                "/sql chat UPDATE contacts SET chat_ts = "
                + sql_quote(last_stamp)
                + " WHERE contact_id = "
                + contact_id,
            )
    return stored


def create_chat_database(work: Path, agent_db: bytes, disk: bytes) -> tuple[bytes, list[str], int]:
    """Build a chat database the same SimpleX version can open with this key.

    The temporary profile is an empty local user. Chat is not started against
    the guest agent database, so the running VM keeps its queues.
    """
    if not SIMPLEX_CHAT.is_file():
        raise RuntimeError(f"simplex-chat is not built at {SIMPLEX_CHAT}")
    prefix = work / "schema" / "simplex_v1"
    prefix.parent.mkdir()
    created = subprocess.run(
        [
            str(SIMPLEX_CHAT),
            "-d", str(prefix),
            "-k", PASSPHRASE,
            "-y",
            "--user-display-name", "NexatrodeOS",
            "--smp-proxy", "never",
            "-t", "0",
            "-e", "/users",
        ],
        check=False,
        capture_output=True,
        text=True,
        timeout=30,
    )
    chat_db = Path(str(prefix) + "_chat.db")
    if not chat_db.is_file():
        raise RuntimeError(created.stdout + created.stderr + "\nSimpleX did not create a chat database")
    # Confirm the chat user exists by opening it beside a copy of the guest agent
    # database in maintenance mode, which does not subscribe to the relays.
    inspect_dir = work / "inspect"
    inspect_dir.mkdir()
    inspect_prefix = inspect_dir / "simplex_v1"
    (Path(str(inspect_prefix) + "_chat.db")).write_bytes(chat_db.read_bytes())
    (Path(str(inspect_prefix) + "_agent.db")).write_bytes(agent_db)
    listed = subprocess.run(
        [
            str(SIMPLEX_CHAT),
            "-d", str(inspect_prefix),
            "-k", PASSPHRASE,
            "-y",
            "--maintenance",
            "-t", "0",
            "-e", "/users",
        ],
        check=False,
        capture_output=True,
        text=True,
        timeout=60,
    )
    if listed.returncode != 0:
        raise RuntimeError(listed.stdout + listed.stderr)
    if "NexatrodeOS" not in listed.stdout:
        raise RuntimeError("chat database did not keep the NexatrodeOS user")
    names = add_named_contacts(inspect_prefix)
    message_count = add_transcripts(inspect_prefix, disk)
    return (Path(str(inspect_prefix) + "_chat.db")).read_bytes(), names, message_count


def export_archive(disk_path: Path, output_dir: Path) -> Path:
    if protected(output_dir):
        raise ValueError("refusing to write the archive over a disk image")
    disk = read_disk(disk_path)
    agent_db = extract_agent_database(disk)
    output_dir.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H%M%SZ")
    archive_path = output_dir / f"simplex-chat.{stamp}.zip"
    passphrase_path = output_dir / f"simplex-chat.{stamp}.passphrase"
    with tempfile.TemporaryDirectory(prefix="nexatrode-simplex-archive-") as temporary:
        chat_db, contacts, messages = create_chat_database(Path(temporary), agent_db, disk)
        with zipfile.ZipFile(archive_path, "w", compression=zipfile.ZIP_DEFLATED) as archive:
            archive.writestr(CHAT_DB_NAME, chat_db)
            archive.writestr(AGENT_DB_NAME, agent_db)
    os.chmod(archive_path, 0o600)
    passphrase_path.write_text(PASSPHRASE + "\n")
    os.chmod(passphrase_path, 0o600)
    print(f"archive: {archive_path}")
    print(f"passphrase file: {passphrase_path}")
    print(f"agent database: {len(agent_db)} bytes")
    print("contacts: " + (", ".join(contacts) if contacts else "none"))
    print(f"messages: {messages}")
    print("The guest disk was not modified.")
    return archive_path


def import_archive(
    archive_path: Path,
    source_disk: Path,
    output_disk: Path,
    passphrase: str | None = None,
) -> None:
    if protected(output_disk):
        raise ValueError(f"refusing to write {output_disk}: that disk holds the current profile")
    if output_disk.exists():
        raise ValueError(f"{output_disk} already exists")
    with zipfile.ZipFile(archive_path) as archive:
        names = set(archive.namelist())
        if CHAT_DB_NAME not in names or AGENT_DB_NAME not in names:
            raise ValueError("archive is missing simplex_v1_chat.db or simplex_v1_agent.db")
        agent_db = archive.read(AGENT_DB_NAME)
    if len(agent_db) < 16 or agent_db.startswith(b"SQLite format 3"):
        raise ValueError("archive agent database is not encrypted")
    if passphrase is not None:
        unlock_archive(archive_path, passphrase)
    disk = bytearray(read_disk(source_disk))
    install_file(disk, AGENT_METADATA_BLOCKS, METADATA_NAMESPACE, 0, AGENT_FIRST_BLOCK, agent_db, True)
    install_file(disk, JOURNAL_METADATA_BLOCKS, JOURNAL_NAMESPACE, 1, AGENT_FIRST_BLOCK + FILE_BLOCKS, b"", False)
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    descriptor = os.open(output_disk, flags, 0o600)
    try:
        written = os.write(descriptor, disk)
        if written != len(disk):
            raise ValueError("short write while creating the imported disk")
        os.fsync(descriptor)
    except Exception:
        os.close(descriptor)
        output_disk.unlink(missing_ok=True)
        raise
    os.close(descriptor)
    restored = extract_agent_database(output_disk.read_bytes())
    if restored != agent_db:
        output_disk.unlink()
        raise ValueError("imported disk did not read back the archive agent database")
    print(f"imported disk: {output_disk}")
    print("The source disk was left unchanged.")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    export = commands.add_parser("export")
    export.add_argument("--disk", type=Path, default=LIVE_DISK)
    export.add_argument("--output-dir", type=Path, default=ARCHIVE_DIR)
    restore = commands.add_parser("import")
    restore.add_argument("archive", type=Path)
    restore.add_argument("source_disk", type=Path)
    restore.add_argument("output_disk", type=Path)
    restore.add_argument(
        "--passphrase",
        default=None,
        help="unlock the archive with this passphrase before writing the new disk",
    )
    unlock = commands.add_parser("unlock", help="open an official zip with its passphrase")
    unlock.add_argument("archive", type=Path)
    unlock.add_argument("--passphrase", default=PASSPHRASE)
    rekey = commands.add_parser("rekey", help="change the archive passphrase like mobile /db key")
    rekey.add_argument("archive", type=Path)
    rekey.add_argument("--current", default=PASSPHRASE)
    rekey.add_argument("--new", required=True)
    rekey.add_argument("--output-dir", type=Path, default=ARCHIVE_DIR)
    args = parser.parse_args()
    try:
        if args.command == "export":
            export_archive(args.disk, args.output_dir)
        elif args.command == "import":
            import_archive(args.archive, args.source_disk, args.output_disk, args.passphrase)
        elif args.command == "unlock":
            unlock_archive(args.archive, args.passphrase)
        else:
            rekey_archive(args.archive, args.current, args.new, args.output_dir)
    except (OSError, ValueError, RuntimeError) as error:
        raise SystemExit(str(error))


if __name__ == "__main__":
    main()
