# Disposable passphrase-wrapped key experiment

The optional `utm-wrapped-vault-experiment` build connects this experiment to
the UTM device-lock screen and a separate vault bootstrap journal. It is off by
default, refuses to compile with the installation-key feature, and rejects
enrollment on a nonblank partition. It does not migrate or change the key or
protection of existing wallets, messages, or other data. Do not use this build
or its envelope format to protect real data: it is an unaudited prototype.

The experiment wraps a separate 32-byte vault key with a key derived from a
passphrase. Its fixed version-one parameters are PBKDF2-HMAC-SHA256 with 600,000
rounds and AES-256-GCM-SIV. The format authenticates the algorithm identifiers,
parameters, salt, and nonce along with the ciphertext; unsupported parameters
are rejected before derivation. The caller must obtain the vault key, a fresh
salt, and a fresh nonce from a cryptographically secure random source. The
fixture inputs in the tests are public and must never be used for real data.

PBKDF2 was chosen here because the project already contains its implementation,
not as a final KDF choice. It is not memory-hard. Evaluate Argon2id and measure
its memory and latency requirements on actual target hardware before choosing
production parameters. The derived key and returned plaintext key use
`Zeroizing`; this does not establish that all copies, registers, caches, DMA
buffers, or cryptographic internal state are cleared on lock.

The experimental UTM path stores two bootstrap records using the existing blob
journal's staged/committed writes. The outer bootstrap framing uses a public
development key; it is not protection against an attacker. Only the wrapped
vault key depends on the passphrase. The key is provided to the vault after
unlock and cleared from the vault provider on lock, and the vault denies its
app endpoints while locked. Other application processes may retain plaintext
or copies in memory: this experiment does not establish a secure memory wipe.

The prototype has no real-data migration, hardware-backed key binding, verified
boot, trusted retry counter, rollback
protection, or secure memory lifecycle. The existing device-lock verifier is
separate from this envelope and must not be treated as a wrapping key. Anyone
who copies an envelope can attempt offline password guesses; software UI retry
delays do not prevent those guesses. Existing storage interruption tests cover
the current vault protocol, not this new envelope's migration or lifecycle.

The disposable full-system build gates the SimpleX agent's startup on the
vault reporting that its key is unlocked. A dedicated notification wakes the
agent; it checks the vault status again after every wake. This is experimental
startup integration, not a complete lifecycle or memory-clearing design. Do not
use this build as a regular VM. Disabling the experimental lock is intentionally
unsupported.

The separate [dynamic-app lock fixture](dynamic-app-lock.md#wrapped-key-integration)
now connects a disposable wrapped key to process destruction and recreation.
Its manager owns the vault eviction/commit endpoint, cancels stale epochs and
expires abandoned preparations. This is separate from the full-UI agent gate;
it does not migrate or change the saved wallet/chat installation key.

Before any real-data integration, design and independently review enrollment,
credential changes, authenticated storage and recovery, safe migration without
discarding the old key, lock/auto-lock key eviction, and the behavior of running
apps. Test power loss at each write boundary on disposable disks. Verify the
actual entropy service, passphrase handling, memory exposure, and boot trust.

## Reproducing the tests

From the repository root, host tests use the existing locked dependencies:

```sh
cargo +nightly-2026-09-25 test --manifest-path rust/Cargo.toml --locked --offline --release --lib vault_key_prototype
```

The four-core accelerated UTM crypto probe is a separate Microkit system using only
fixed disposable inputs. Build it with the prepared UTM SDK (`UTM_INPUTS` below
is the path to that SDK and seL4 installation):

```sh
export UTM_INPUTS=/path/to/utm-accelerated
export SEL4_PREFIX="$UTM_INPUTS/sel4-install"
export SEL4_INCLUDE_DIRS="$UTM_INPUTS/sdk/board/qemu_virt_aarch64_el1/smp-debug/include"
export CARGO_TARGET_DIR="$PWD/build/utm-accelerated-ui-cargo"
cargo +nightly-2026-09-25 build --locked --offline --release -Zjson-target-spec \
  --config rust/.cargo/config.toml --manifest-path rust/Cargo.toml \
  --target rust/targets/aarch64-sel4-microkit.json \
  --features vault-key-prototype --bin rust_vault_key_prototype_probe
mkdir -p build/vault-key-prototype
"$UTM_INPUTS/sdk/bin/microkit" tests/utm_vault_key_probe.system \
  --board qemu_virt_aarch64_el1 --config smp-debug \
  --search-path "$CARGO_TARGET_DIR/aarch64-sel4-microkit/release" \
  -o build/vault-key-prototype/loader.img -r build/vault-key-prototype/report.txt
python3 tests/utm_vault_key_probe.py --template /path/to/regular-vm.utm/config.plist \
  --loader build/vault-key-prototype/loader.img
```

The runner reads the template settings, substitutes a blank temporary disk and
standalone probe loader, restricts guest networking, and deletes the temporary
VM after the test. It does not boot the normal shell, use the real vault disk,
or test persistence, lock screen behavior, or Pixel hardware.

## Disposable-disk recovery test

`tests/utm_vault_key_storage.py` uses a separate storage test system, the
existing test-mode vault and block driver, and a fresh 16 MiB GPT disk per case.
It saves an actual wrapped test key in a two-slot encrypted blob, rewraps the
same key under a different test passphrase, and checks the recovered envelope
with both the correct and wrong passphrases. The normal case checks replacement,
guest resets, and a forced VM stop followed by a fresh start. Four more cases
pause the test-mode vault before/after the stage flush and before/after the
commit flush, issue a QMP guest reset, then validate the appropriate old or new
committed envelope after recovery and again after a VM stop/start.

On 2026-11-22, the four-vCPU accelerated UTM run passed the normal case and all
four interruption points. Before and after the stage flush, the old committed
envelope survived; after the commit flush, the new envelope survived. Before
the commit flush, either result is acceptable, and that run recovered the new
envelope. All recovered records reopened after a separate VM stop/start.

These are resets against UTM's emulated storage, not hardware power cuts. The
test uses a fixed development outer vault key and public fixture inputs. It
does not exercise a real passphrase, real vault key, enrollment, migration,
lock-screen integration, secure memory clearing, or resistance to rollback.

Build the fixture and run it from the repository root, using the prepared UTM
inputs, sDDF tree and its Python environment:

```sh
python3 tests/build_utm_vault_key_storage.py --inputs /path/to/utm-accelerated \
  --sddf /path/to/sddf-personal-os --sddf-python /path/to/sddf-venv/bin/python \
  --output build/utm-vault-key-storage
python3 tests/utm_vault_key_storage.py --template /path/to/regular-vm.utm/config.plist \
  --artifacts build/utm-vault-key-storage --sdk /path/to/utm-accelerated/sdk
```

Only the regular VM's configuration is read as a template. The runner replaces
its loader and disk, restricts networking, and removes its disposable VMs and
disks when finished. No wallet or chat disk is mounted.

## Full UI experiment

The separate full-UI test loader and runner are built and invoked as follows:

```sh
python3 tests/build_simplex_vault_gate.py --cached-checkout /path/to/cached-checkout \
  --objects /path/to/cached-agent-objects --inputs /path/to/utm-accelerated \
  --output build/simplex-vault-gate
python3 tests/build_utm_wrapped_vault.py --inputs /path/to/utm-accelerated \
  --sddf /path/to/sddf-personal-os --gated-agent build/simplex-vault-gate/agentstore_probe.elf \
  --output build/utm-wrapped-lock
python3 tests/utm_wrapped_lock_e2e.py --template /path/to/regular-vm.utm/config.plist \
  --loader build/utm-wrapped-lock/loader.img --work build/utm-wrapped-lock --require-agent
```

Add `--check-nonblank` to test that enrollment refuses a disposable partition
containing a sentinel. The fixture uses a freshly created disk, test credentials
and restricted networking; its loader cannot use the real installation key.
The SimpleX relinker requires a cached agent object tree whose executable code
matches the prepared UTM agent; it checks the code hash and writes its output
in the worktree. No existing wallet or chat disk is read.

On 2026-11-22 the disposable UTM UI run enrolled a passphrase, rejected a wrong
passphrase, unlocked with the right one, saved and exactly read back a test note,
and recovered the note after a full VM stop/start. The runner verified that the
test note was not present as plaintext in the disk image. It then changed the
newest bootstrap slot's commit marker on the stopped disposable disk to simulate
an incomplete record; the older slot unlocked and recovered the note. Manual
lock and the 60-second auto-lock both caused the vault provider to clear its
active key. A separate disposable disk with a nonblank sentinel was rejected
without changing the sentinel or writing a bootstrap record.

With `--require-agent`, the same test also checks that SimpleX stays at its
startup gate while the vault is locked or given an incorrect passphrase, then
starts and reads the empty disposable chat list after unlock. It checks that
database reads and receive polling resume after a manual re-lock and unlock,
and checks the startup gate again on reboot and bootstrap-slot recovery. Guest
networking is restricted, so this is not a message-delivery test.

The bootstrap test simulates an incomplete slot by editing a stopped test disk;
it does not inject power loss at every bootstrap write boundary. Earlier tests
in this document reset the separate blob-writing fixture at its flush
boundaries. Neither test establishes hardware power-loss behavior, anti-rollback
protection, or that all plaintext has been removed from other processes.
