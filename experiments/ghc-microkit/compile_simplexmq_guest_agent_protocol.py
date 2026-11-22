#!/usr/bin/env python3
"""Cross-compile the staged SimpleX Agent.Protocol source closure."""

import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
MICROKIT = ROOT / "experiments/ghc-microkit"
sys.path.insert(0, str(MICROKIT))
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
    build_root = (ROOT / ".cache/simplex-chat-upstream/dist-newstyle/build/aarch64-osx"
                  / "ghc-9.6.3/simplexmq-7.0.0.6/build")
    output = MICROKIT / "build/guest-simplex-agent-protocol"
    output.mkdir(parents=True, exist_ok=True)
    source = FULL_STAGE / "Simplex/Messaging/Agent/Protocol.hs"
    args = [
        compiler, "--make", "-no-link", "-fforce-recomp", "-v0",
        "-no-global-package-db", "-package-env=-",
        "-package-db", target_db, "-package-db", guest_db,
        "-DSIMPLEX_MICROKIT", "-XStrictData", "-XOverloadedStrings",
        f"-i{MICROKIT / 'guest-client-shims'}",
        f"-i{MICROKIT / 'guest-transport-shims'}",
        f"-i{FULL_STAGE}", f"-i{build_root}", f"-i{build_root / 'autogen'}",
        "-optc-target", "-optcaarch64-none-elf",
        f"-optc-I{MICROKIT / 'freestanding-headers'}",
        f"-optc-I{ghc_tree / 'rts/include'}", f"-optc-I{ghc_tree / 'rts'}",
        "-outputdir", output, source,
    ]
    result = subprocess.run([str(arg) for arg in args], cwd=ghc_tree,
                            capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError("guest Agent.Protocol object build failed:\n"
                           + result.stderr[-12000:])
    objects = list(output.rglob("*.o"))
    print(f"UPSTREAM AGENT.PROTOCOL|AARCH64 OBJECT BUILD PASS|{len(objects)} objects")
    print("GUEST FILE DESCRIPTION CODEC|JSON/YAML SUBSET ONLY")


if __name__ == "__main__":
    main()
