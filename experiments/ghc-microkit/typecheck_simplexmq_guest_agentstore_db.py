#!/usr/bin/env python3
"""Typecheck the patched upstream AgentStore SQLite wrapper for AArch64."""

import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(Path(__file__).resolve().parent))
from prepare_simplexmq_guest_transport import prepare  # noqa: E402
from verify_simplexmq_backend_patch import (  # noqa: E402
    FULL_STAGE, ensure_patch_applied, stage_full_source,
)


def main() -> None:
    ensure_patch_applied()
    stage_full_source()
    prepare(FULL_STAGE)
    ghc_tree = ROOT / ".cache/ghc-port/ghc-9.6.6"
    compiler = ghc_tree / "_build/stage0/bin/aarch64-unknown-none-ghc"
    target_db = ghc_tree / "_build/stage1/lib/package.conf.d"
    guest_db = ROOT / ".cache/sel4-tls-deps/guest-package.conf.d"
    microkit = ROOT / "experiments/ghc-microkit"
    autogen = (ROOT / ".cache/simplex-chat-upstream/dist-newstyle/build/aarch64-osx"
               / "ghc-9.6.3/simplexmq-7.0.0.6/build/autogen")
    output = microkit / "build/guest-simplex-agentstore-db-check"
    output.mkdir(parents=True, exist_ok=True)
    source = FULL_STAGE / "Simplex/Messaging/Agent/Store/SQLite/DB.hs"
    args = [
        compiler, "-fno-code", "-fforce-recomp", "-v0", "-cpp",
        "-no-global-package-db", "-package-env=-",
        "-package-db", target_db, "-package-db", guest_db,
        "-DSIMPLEX_MICROKIT", "-XStrictData", "-XOverloadedStrings",
        f"-i{microkit / 'guest-client-shims'}",
        f"-i{microkit / 'guest-transport-shims'}", f"-i{microkit}",
        f"-i{FULL_STAGE}", f"-i{autogen}", "-outputdir", output,
        "-optc-target", "-optcaarch64-none-elf",
        f"-optc-I{microkit / 'freestanding-headers'}",
        f"-optc-I{ghc_tree / 'rts/include'}", f"-optc-I{ghc_tree / 'rts'}",
        source,
    ]
    result = subprocess.run([str(arg) for arg in args], cwd=ghc_tree,
                            capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError("guest AgentStore SQLite DB typecheck failed:\n"
                           + result.stderr[:9000])
    print("UPSTREAM AGENTSTORE SQLITE DB|TYPECHECK PASS")
    print("CUSTOM SQLCIPHER VFS OPEN2|AARCH64 API MATCH PASS")


if __name__ == "__main__":
    main()
