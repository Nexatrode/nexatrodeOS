# SQLite VFS capability probe

This probe validates the SQLite file boundary used by the SimpleX agent store.
The VFS routes the database and rollback journal through bounded storage
operations rather than a POSIX filesystem. In the Microkit build, those
operations call the Messages app's private vault capability.

The implementation supports one connection and rollback-journal mode. Its
host test uses an in-memory backing store and deterministic test entropy; the
separate Microkit adapter uses the vault, clock, and entropy capabilities.
The VFS registers as SQLite's default so default-VFS randomness and time
lookups resolve through those capabilities. The pinned SQLCipher amalgamation
compiles for AArch64 with its LibTomCrypt provider, and Fortuna seed bytes come
directly from the default VFS; provider activation fails if the callback
returns fewer bytes than requested. Host and QEMU tests create an encrypted
database, check that its SQLite plaintext header is absent, reject a wrong key,
and reopen with the correct key. In QEMU, Fortuna requests entropy through the
Microkit virtio RNG service, while database and journal bytes remain in an
in-memory fixture. This proves the target entropy IPC and codec path. A separate
`test-sqlcipher-vault` run now exercises the same codec and VFS over the
disk-backed Messages vault and entropy PD: it creates an encrypted database on
one QEMU boot, reads the row after a second boot, and rejects a wrong key. This
uses a QEMU-only development key, not production key management, and does not
yet run SimpleX's Haskell store migrations or agent. WAL and migration rename
are also not yet supported.

Run the host tests and target adapter compile from the repository root:

```sh
make -C experiments/ghc-microkit/sqlite_vfs_probe test
make -C experiments/ghc-microkit/sqlite_vfs_probe microkit-compile
make -C experiments/ghc-microkit guest-simplex-sqlcipher-host-test
make -C experiments/ghc-microkit guest-simplex-sqlcipher-core-compile
make -C experiments/ghc-microkit guest-simplex-sqlcipher-qemu-smoke
./dev test-sqlcipher-vault --timeout 60
```

The regular QEMU vault storage scenario also tests file write, reboot, readback,
and journal deletion alongside the scalar/blob crash-interruption checks. The
SQLCipher vault scenario adds an encrypted database reboot round trip. See
`./dev test-storage` and `./dev test-sqlcipher-vault`.
