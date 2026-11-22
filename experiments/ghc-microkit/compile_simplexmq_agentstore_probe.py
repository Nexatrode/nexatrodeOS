#!/usr/bin/env python3
"""Compile the real SimpleX AgentStore startup probe for Microkit/AArch64."""

import subprocess
import sys
from pathlib import Path

from compile_simplexmq_agent_client import (
    prepare_agent_json,
    prepare_guest_store_codecs,
    prepare_http2_backend,
    prepare_remote_control_discovery,
    prepare_sqlite_guest_source,
    share_agent_file_error_types,
)
from prepare_simplexmq_guest_transport import prepare
from verify_simplexmq_backend_patch import FULL_STAGE, ROOT, ensure_patch_applied, stage_full_source


def main() -> None:
    build_objects = "--objects" in sys.argv[1:]
    ensure_patch_applied()
    stage_full_source()
    prepare(FULL_STAGE)
    share_agent_file_error_types()
    prepare_remote_control_discovery()
    prepare_sqlite_guest_source()
    prepare_agent_json()
    prepare_http2_backend()
    prepare_guest_store_codecs()

    ghc_tree = ROOT / ".cache/ghc-port/ghc-9.6.6"
    compiler = ghc_tree / "_build/stage0/bin/aarch64-unknown-none-ghc"
    target_db = ghc_tree / "_build/stage1/lib/package.conf.d"
    guest_db = ROOT / ".cache/sel4-tls-deps/guest-package.conf.d"
    microkit = ROOT / "experiments/ghc-microkit"
    http2 = ROOT / ".cache/guest-http2-microkit/http2-4.2.2"
    autogen = (
        ROOT / ".cache/simplex-chat-upstream/dist-newstyle/build/aarch64-osx"
        / "ghc-9.6.3/simplexmq-7.0.0.6/build/autogen"
    )
    output = microkit / "build/guest-simplex-agentstore-entry-probe"
    output.mkdir(parents=True, exist_ok=True)
    paths = [
        microkit,
        microkit / "guest-transport-shims",
        microkit / "guest-client-shims",
        microkit / "guest-http2-shims",
        http2,
        FULL_STAGE,
        autogen,
    ]
    mode = ["--make", "-no-link"] if build_objects else ["-fno-code"]
    args = [
        str(compiler),
        *mode,
        "-fforce-recomp",
        "-v0",
        "-cpp",
        "-no-global-package-db",
        "-package-env=-",
        "-package-db",
        str(target_db),
        "-package-db",
        str(guest_db),
        "-DSIMPLEX_MICROKIT",
        "-XStrictData",
        "-XOverloadedStrings",
        "-XDerivingStrategies",
        *[f"-i{path}" for path in paths],
        "-optc-target",
        "-optcaarch64-none-elf",
        f"-optc-I{microkit / 'freestanding-headers'}",
        f"-optc-I{ghc_tree / 'rts/include'}",
        f"-optc-I{ghc_tree / 'rts'}",
        "-outputdir",
        str(output),
        str(microkit / "SimplexAgentStoreProbe.hs"),
    ]
    result = subprocess.run(args, cwd=ghc_tree, capture_output=True, text=True)
    if result.returncode:
        diagnostics = output / ("object-build.stderr" if build_objects else "typecheck.stderr")
        diagnostics.write_text(result.stderr)
        raise RuntimeError(f"AgentStore probe failed; full diagnostics: {diagnostics}\n" + result.stderr[-10000:])
    if build_objects:
        count = len(list(output.rglob("*.o")))
        print(f"SIMPLEX AGENTSTORE ENTRY|AARCH64 OBJECT BUILD PASS|{count} objects")
    else:
        print("SIMPLEX AGENTSTORE ENTRY|TYPECHECK PASS")


if __name__ == "__main__":
    main()
