#!/usr/bin/env python3
"""Link the upstream SimpleX agent runtime lifecycle probe for QEMU."""

from __future__ import annotations

import subprocess
import os
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
MICROKIT = ROOT / "experiments/ghc-microkit"
OUTPUT = Path(os.environ.get(
    "SIMPLEX_AGENT_RUNTIME_OUTPUT",
    str(MICROKIT / "build/guest-simplex-agent-runtime-probe"),
)).resolve()
GHC_TREE = ROOT / ".cache/ghc-port/ghc-9.6.6"


def run(command: list[str], log: Path | None = None, cwd: Path | None = None) -> None:
    result = subprocess.run(
        command, cwd=cwd, text=True, stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    if log is not None:
        log.write_text(result.stdout)
    if result.returncode:
        detail = f"; see {log}" if log else ""
        raise SystemExit(f"command failed ({result.returncode}){detail}\n{result.stdout[-12000:]}")


def main() -> None:
    OUTPUT.mkdir(parents=True, exist_ok=True)
    compiler = GHC_TREE / "_build/stage0/bin/aarch64-unknown-none-ghc"
    ghc_pkg = GHC_TREE / "_build/stage0/bin/aarch64-unknown-none-ghc-pkg"
    target_db = GHC_TREE / "_build/stage1/lib/package.conf.d"
    guest_db = ROOT / ".cache/sel4-tls-deps/guest-package.conf.d"
    c_object_names = {
        "sel4_libc_agent_runtime.o",
        "agentstore_qemu_test_time.o",
        "simplex_sqlite_minimal_compat.o",
        "simplex_sntrup_sha512_ltc.o",
        "haskell_entropy_bridge.o",
        "haskell_tcp_bridge.o",
        "simplex_agent_ipc_bridge.o",
        "simplex_agent_rpc_client.o",
        "qemu_wallclock.o",
    }
    # Ignore packaged images and generated build trees. Older public-image
    # builds placed network and vault targets here; their unrelated objects
    # must never be fed into the Haskell agent link.
    skip_parts = {"slint-target", "messages-target", "qemu-image",
                  "net-qemu-dns", "installation-vault-target"}
    object_paths = sorted(
        path
        for path in OUTPUT.rglob("*.o")
        if path.name not in c_object_names
        and skip_parts.isdisjoint(path.parts)
    )
    if not object_paths:
        raise SystemExit("runtime probe object build is missing; run compile_simplexmq_agent_runtime_probe.py")

    extra_c = [
        (OUTPUT / "sel4_libc_agent_runtime.o", MICROKIT / "sel4_libc.c"),
        (OUTPUT / "agentstore_qemu_test_time.o", MICROKIT / "agentstore_qemu_test_time.c"),
        (OUTPUT / "simplex_sqlite_minimal_compat.o", MICROKIT / "simplex_sqlite_minimal_compat.c"),
        (OUTPUT / "simplex_sntrup_sha512_ltc.o", MICROKIT / "simplex_sntrup_sha512_ltc.c"),
        (OUTPUT / "haskell_entropy_bridge.o", MICROKIT / "haskell_entropy_bridge.c"),
        (OUTPUT / "haskell_tcp_bridge.o", MICROKIT / "haskell_tcp_bridge.c"),
        (OUTPUT / "simplex_agent_ipc_bridge.o", MICROKIT / "simplex_agent_ipc_bridge.c"),
        (OUTPUT / "qemu_wallclock.o", MICROKIT / "tls_qemu_wallclock.c"),
    ]
    cflags = [
        "-target", "aarch64-none-elf", "-mcpu=cortex-a53", "-mstrict-align",
        "-ffreestanding", "-fno-builtin", "-fno-stack-protector", "-std=gnu11",
        "-O2", "-g", "-Wall", "-Wextra", "-Werror",
        f"-I{MICROKIT / 'freestanding-headers'}",
        f"-I{ROOT / '.cache/microkit-sdk-2.3.1/board/qemu_virt_aarch64/debug/include'}",
        f"-I{ROOT / 'tests/net/include'}",
        f"-I{ROOT / 'tests'}",
        f"-I{ROOT / '.cache/simplex-chat-upstream/dist-newstyle/src/direct-sq_-fd066e285bc22458/cbits'}",
        f"-I{ROOT / '.cache/simplex-crypto/libtomcrypt/src/headers'}",
    ]
    for obj, source in extra_c:
        flags = []
        if obj.name == "sel4_libc_agent_runtime.o":
            # The 1 MiB default libc arena is suitable for small probes but
            # cannot hold the upstream agent's SQLite/SQLCipher workload.
            heap = os.environ.get("SEL4_LIBC_HEAP_SIZE", "67108864u")
            flags.append(f"-DSEL4_LIBC_HEAP_SIZE={heap}")
        elif obj.name == "haskell_entropy_bridge.o":
            flags.append("-DENTROPY_CHANNEL=3")
        elif obj.name == "haskell_tcp_bridge.o":
            flags.append("-DHASKELL_TCP_CLIENT_CHANNEL=1" if
                         os.environ.get("SIMPLEX_AGENT_DEDICATED_PD") == "1" else
                         "-DHASKELL_TCP_CLIENT_CHANNEL=5")
        elif obj.name == "qemu_wallclock.o":
            flags.extend(("-DCLOCK_CHANNEL=2", "-DCLOCK_REQUEST=0x205u",
                          "-DCLOCK_REPLY=0x100u"))
        run(["/usr/bin/clang", *cflags, *flags, "-c", str(source), "-o", str(obj)])

    objects = [
        *[str(path) for path in object_paths],
        str(OUTPUT / "sel4_libc_agent_runtime.o"),
        str(OUTPUT / "qemu_wallclock.o"),
        str(MICROKIT / "build/guest-simplex-sqlite-core/calendar_utc.o"),
        str(OUTPUT / "haskell_entropy_bridge.o"),
        str(OUTPUT / "haskell_tcp_bridge.o"),
        str(OUTPUT / "simplex_agent_ipc_bridge.o"),
        str(MICROKIT / "build/rts-adapter/sddf_time_bridge.o"),
        str(MICROKIT / "build/guest-simplex-sqlcipher-core/sqlite3.o"),
        str(MICROKIT / "build/guest-simplex-sqlcipher-core/sqlite_os_other.o"),
        str(MICROKIT / "build/guest-simplex-sqlcipher-core/sqlcipher_freestanding.o"),
        str(MICROKIT / "sqlite_vfs_probe/build/microkit/vfs.o"),
        str(MICROKIT / "sqlite_vfs_probe/build/microkit/vault_storage.o"),
        str(MICROKIT / "sqlite_vfs_probe/build/microkit/vault_storage_microkit.o"),
        str(MICROKIT / "sqlite_vfs_probe/build/microkit/simplex_sqlite_init_microkit.o"),
        str(OUTPUT / "agentstore_qemu_test_time.o"),
        str(OUTPUT / "simplex_sqlite_minimal_compat.o"),
        str(MICROKIT / "build/guest-simplex-agentstore-entry-probe/sntrup761.o"),
        str(OUTPUT / "simplex_sntrup_sha512_ltc.o"),
        str(MICROKIT / "build/guest-simplex-sqlcipher-crypto/source/libtomcrypt.a"),
        str(ROOT / ".cache/ghc-port/openlibm/libopenlibm.a"),
        str(MICROKIT / "build/compiler-rt-quad/libquad-builtins.a"),
        str(MICROKIT / "build/haskell-hello/haskell_microkit_entry.o"),
        str(ROOT / ".cache/microkit-sdk-2.3.1/board/qemu_virt_aarch64/debug/lib/libmicrokit.a"),
    ]
    missing = [path for path in objects if not Path(path).exists()]
    if missing:
        raise SystemExit("missing prerequisites:\n" + "\n".join(missing))

    image = OUTPUT / "agent-runtime.elf"
    package_dump = subprocess.run(
        [
            str(ghc_pkg),
            f"--package-db={target_db}",
            f"--package-db={guest_db}",
            "dump",
        ],
        check=True,
        text=True,
        stdout=subprocess.PIPE,
    ).stdout
    packages = [
        line.split(":", 1)[1].strip()
        for line in package_dump.splitlines()
        if line.startswith("id:")
        and not line.split(":", 1)[1].strip().startswith("system-cxx-std-lib-")
    ]
    args = [
        str(compiler), "-v0", "-no-hs-main", "-no-global-package-db",
        "-package-env=-", "-package-db", str(target_db), "-package-db", str(guest_db),
        *[arg for package in packages for arg in ("-package-id", package)],
        *objects, "-pgml", str(MICROKIT / "ghc-ld-lld.py"),
        "-optc-target", "-optcaarch64-none-elf",
        f"-optc-I{MICROKIT / 'freestanding-headers'}",
        f"-optc-I{GHC_TREE / 'rts/include'}", f"-optc-I{GHC_TREE / 'rts'}",
        "-optc-Dssize_t=long", "-optc-ffreestanding", "-optc-fuse-ld=lld",
        "-optc-nostdlib", "-optc-Wl,-e,init", "-o", str(image),
    ]
    run(args, OUTPUT / "link.log", GHC_TREE)
    nm = "/opt/homebrew/opt/llvm/bin/llvm-nm"
    undefined = subprocess.run(
        [nm, "-u", str(image)], check=True, text=True, stdout=subprocess.PIPE,
    ).stdout.strip()
    if undefined:
        raise SystemExit("linked agent runtime has undefined symbols:\n" + undefined)
    print("SIMPLEX AGENT RUNTIME LINK|PASS|0 undefined symbols")


if __name__ == "__main__":
    main()
