# Rich app data on the current vault

Status: PVLTBLB1 codec, app/vault transfer pages, first-read app migration,
PVLTBAK2 backup codec, a bounded multi-line Notes editor, and a bounded
multi-contact Contacts app are implemented.

## Why this is needed

Notes now stores up to four independent notes, each with a 21-character title
and a body of up to 128 uppercase ASCII letters, digits, spaces, and line
breaks, in its encrypted blob record. `PNT2` stores each title and body
separately. Older `PNT1` collections and raw single-note records remain
readable; migration takes each body's first line as its title, then saves the
new format. The list shows titles, and its touch controls open, add, and
confirmed-delete notes. In the editor, TITLE and BODY switch fields; Older and
Newer scroll the three-row body view. Contacts stores up to four entries with a
12-character uppercase name and a 16-digit phone number. Enter starts and
advances through a new contact, Up/Down selects entries, and Delete removes the
selected entry. The screen's read-only render snapshot is separate from the
persistent record.

## Storage direction

The vault partition already gives Notes and Contacts two dedicated 4 KiB
blocks each, one block per recovery slot. Keep that block layout and two-slot
stage/flush/commit recovery. Preserve the current 72-byte PVLTREC1 records for
system data and old app data. Rich app records use the separate PVLTBLB1 codec
in `rust/src/vault_blob_record.rs`: a bounded payload is encrypted in place,
its length and namespace/generation are authenticated, and a commit marker plus
zero padding makes partial or malformed slots reject cleanly. The codec uses
the current crypto-provider interface and AES-256-GCM-SIV implementation; it
does not enable the separate proposed AES-GCM PVLTREC2 format.

Each app endpoint supports bounded blob reads and writes through a dedicated
4 KiB transfer page shared only by that app and the vault. Endpoint identity
selects the fixed Notes or Contacts namespace; no namespace selector comes from
app request data. The vault validates payload lengths during synchronous calls,
checks the expected generation, and uses the same two durable flushes as scalar
writes.

On first access, the app should decode a valid old 64-bit field into the new
app model and save the equivalent blob into the inactive slot. The old decoder
and PVLTBAK1 restore path remain available; PVLTBAK2 carries full encrypted
blocks without rewriting their ciphertext.

## First app models

The Contacts model uses a versioned `PCT2` blob with bounded, length-prefixed
name, phone, and optional hex key-fingerprint fields. `PCT1` blobs still decode
with an empty fingerprint and rewrite as `PCT2` on the next persist. The decoder
validates the full payload before exposing it to the UI; malformed lengths,
invalid characters, excess records, and trailing data are rejected. Legacy
single-name records migrate to a one-entry Contacts list on first open. Keep
future display snapshots bounded and copy only the visible page of app data into
them.

The current interactive pass can create/edit note titles and bodies, insert a
line break with physical or on-screen Enter, delete text, and preserve both
fields, the collection, and last opened note across cold boots. Its input router
receives only a shared Notes mode word for list-row hit-testing; note contents
stay inside the app display snapshot. Contacts can create, select, and delete
entries and survives a QEMU cold boot. Phone digits and an optional 8-character
hex key fingerprint can be entered through the on-screen keyboard or a physical
keyboard. These are app controls and do not belong in the vault protocol.

## Acceptance checks

- Old PVLTREC1 disks and PVLTBAK1 archives still open without rewriting the
  source image.
- PVLTBLB1 authenticates namespace, generation, declared length, ciphertext,
  tag, commit state, and canonical zero padding; staged records remain
  unreadable.
- QEMU tests cover empty, multi-line, and maximum-size app payloads, corrupted
  records, provider outage, both flush boundaries, and reset recovery.
- Notes and Contacts use separate endpoint-bound transfer pages; malicious
  app requests cannot select or expose the sibling app's payload.
- The new backup export/restore path preserves opaque encrypted blocks and
  enforces the existing rollback check.
- QEMU input integration adds and opens two Notes, confirms deletion, restores
  the selected note and both records after a cold boot, and continues to restore
  the two-entry Contacts list.
- Manual confirmation of the refreshed UTM profile is still pending.
