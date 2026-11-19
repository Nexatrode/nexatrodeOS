#!/usr/bin/env python3
"""Link the upstream AgentStore probe with the QEMU-only clock fixture."""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
MICROKIT = ROOT / "experiments/ghc-microkit"
OUTPUT = MICROKIT / "build/guest-simplex-agentstore-entry-probe"
GHC_TREE = ROOT / ".cache/ghc-port/ghc-9.6.6"


def run(command: list[str], *, log: Path | None = None, cwd: Path | None = None) -> None:
    if log is None:
        subprocess.run(command, check=True, cwd=cwd)
        return
    result = subprocess.run(
        command, check=False, cwd=cwd, text=True, stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    log.write_text(result.stdout)
    if result.returncode:
        raise SystemExit(f"command failed ({result.returncode}); see {log}\n{result.stdout[-10000:]}")


def main() -> None:
    output = OUTPUT
    output.mkdir(parents=True, exist_ok=True)
    wrong_key = "--wrong-key" in sys.argv[1:]
    compiler = GHC_TREE / "_build/stage0/bin/aarch64-unknown-none-ghc"
    target_db = GHC_TREE / "_build/stage1/lib/package.conf.d"
    guest_db = ROOT / ".cache/sel4-tls-deps/guest-package.conf.d"
    http2 = ROOT / ".cache/guest-http2-microkit/http2-4.2.2"
    full_stage = ROOT / ".cache/simplexmq-backend-check/full-src"
    autogen = (
        ROOT / ".cache/simplex-chat-upstream/dist-newstyle/build/aarch64-osx"
        / "ghc-9.6.3/simplexmq-7.0.0.6/build/autogen"
    )
    paths = [
        MICROKIT,
        MICROKIT / "guest-transport-shims",
        MICROKIT / "guest-client-shims",
        MICROKIT / "guest-http2-shims",
        http2,
        full_stage,
        autogen,
        output,
    ]
    objects = [
        output / "haskell_microkit_entry.o",
        MICROKIT / "build/haskell-hello/sel4_libc.o",
        output / "agentstore_qemu_test_time.o",
        MICROKIT / "build/haskell-hello/wallclock_unavailable.o",
        MICROKIT / "build/guest-simplex-sqlite-core/calendar_utc.o",
        MICROKIT / "build/haskell-hello/haskell_entropy_bridge.o",
        MICROKIT / "build/haskell-hello/haskell_tcp_bridge.o",
        MICROKIT / "build/rts-adapter/sddf_time_bridge.o",
        MICROKIT / "build/guest-simplex-sqlcipher-core/sqlite3.o",
        MICROKIT / "build/guest-simplex-sqlcipher-core/sqlite_os_other.o",
        MICROKIT / "build/guest-simplex-sqlcipher-core/sqlcipher_freestanding.o",
        MICROKIT / "sqlite_vfs_probe/build/microkit/vfs.o",
        MICROKIT / "sqlite_vfs_probe/build/microkit/vault_storage.o",
        MICROKIT / "sqlite_vfs_probe/build/microkit/vault_storage_microkit.o",
        MICROKIT / "sqlite_vfs_probe/build/microkit/simplex_sqlite_init_microkit.o",
        output / "simplex_sqlite_minimal_compat.o",
        output / "sntrup761.o",
        output / "simplex_sntrup_sha512_ltc.o",
        MICROKIT / "build/guest-simplex-sqlcipher-crypto/source/libtomcrypt.a",
        ROOT / ".cache/ghc-port/openlibm/libopenlibm.a",
        MICROKIT / "build/compiler-rt-quad/libquad-builtins.a",
        ROOT / ".cache/microkit-sdk-2.3.1/board/qemu_virt_aarch64/debug/lib/libmicrokit.a",
    ]
    missing = [
        str(path)
        for path in [compiler, target_db, guest_db, *(p for p in objects if p.name != "agentstore_qemu_test_time.o")]
        if not path.exists()
    ]
    if missing:
        raise SystemExit("missing prerequisite(s):\n" + "\n".join(missing))

    cc = "/usr/bin/clang"
    run(
        [
            cc, "-target", "aarch64-none-elf", "-mcpu=cortex-a53", "-mstrict-align",
            "-ffreestanding", "-fno-builtin", "-fno-stack-protector", "-std=gnu11",
            "-O2", "-g", "-Wall", "-Wextra", "-Werror",
            f"-I{MICROKIT / 'freestanding-headers'}", "-c",
            str(MICROKIT / "agentstore_qemu_test_time.c"), "-o",
            str(output / "agentstore_qemu_test_time.o"),
        ]
    )

    args = [
        str(compiler), "-v0", "-no-hs-main", "-cpp", "-no-global-package-db",
        "-package-env=-", "-package-db", str(target_db), "-package-db", str(guest_db),
        "-DSIMPLEX_MICROKIT", "-XStrictData", "-XOverloadedStrings",
        "-XDerivingStrategies", *[f"-i{path}" for path in paths],
        "-outputdir", str(output),
        str(MICROKIT / "NativeNetwork.hs"),
        str(MICROKIT / "NativeSQLiteVFS.hs"),
        str(
            MICROKIT
            / ("SimplexAgentStoreWrongKeyProbe.hs" if wrong_key else "SimplexAgentStoreProbe.hs")
        ),
        *[str(path) for path in objects],
        "-package", "tls", "-package", "crypton", "-package", "hourglass",
        "-package", "crypton-x509", "-package", "crypton-x509-store",
        "-package", "crypton-x509-validation", "-package", "direct-sqlcipher",
        "-pgml", str(MICROKIT / "ghc-ld-lld.py"),
        "-optc-target", "-optcaarch64-none-elf",
        f"-optc-I{MICROKIT / 'freestanding-headers'}",
        f"-optc-I{GHC_TREE / 'rts/include'}", f"-optc-I{GHC_TREE / 'rts'}",
        "-optc-Dssize_t=long", "-optc-ffreestanding", "-optc-fuse-ld=lld",
        "-optc-nostdlib", "-optc-Wl,-e,init", "-o",
        str(output / ("probe.wrong-key.testclock.elf" if wrong_key else "probe.testclock.elf")),
    ]
    link_log = output / ("link-wrong-key-testclock.log" if wrong_key else "link-testclock.log")
    run(args, log=link_log, cwd=GHC_TREE)

    nm = "/opt/homebrew/opt/llvm/bin/llvm-nm"
    undefined = subprocess.run(
        [
            nm,
            "-u",
            str(output / ("probe.wrong-key.testclock.elf" if wrong_key else "probe.testclock.elf")),
        ],
        check=True, text=True, stdout=subprocess.PIPE,
    ).stdout.strip()
    if undefined:
        raise SystemExit("linked AgentStore image has undefined symbols:\n" + undefined)
    print("SIMPLEX AGENTSTORE WRONG-KEY LINK|PASS|0 undefined symbols" if wrong_key else
          "SIMPLEX AGENTSTORE LINK|PASS|0 undefined symbols")


if __name__ == "__main__":
    main()
