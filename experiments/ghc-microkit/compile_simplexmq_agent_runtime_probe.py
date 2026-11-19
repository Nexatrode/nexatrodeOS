#!/usr/bin/env python3
"""Compile the upstream SimpleX agent lifecycle probe for Microkit/AArch64."""

from __future__ import annotations

import subprocess
import os
from pathlib import Path

from compile_simplexmq_agent_client import (
    prepare_agent_json,
    prepare_guest_store_codecs,
    prepare_http2_backend,
    prepare_remote_control_discovery,
    prepare_sqlite_guest_source,
    prepare_xftp_decrypt_ttl_skew,
    prepare_xftp_http2_backend,
    prepare_xftp_snd_worker_logs,
    share_agent_file_error_types,
)
from prepare_simplexmq_guest_transport import prepare
from verify_simplexmq_backend_patch import ROOT, ensure_patch_applied, stage_full_source


def main() -> None:
    ensure_patch_applied()
    stage_full_source()
    full_stage = ROOT / ".cache/simplexmq-backend-check/full-src"
    prepare(full_stage)
    share_agent_file_error_types()
    prepare_remote_control_discovery()
    prepare_sqlite_guest_source()
    prepare_agent_json()
    prepare_http2_backend()
    prepare_xftp_http2_backend()
    prepare_xftp_decrypt_ttl_skew()
    prepare_xftp_snd_worker_logs()
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
    output = Path(os.environ.get(
        "SIMPLEX_AGENT_RUNTIME_OUTPUT",
        str(microkit / "build/guest-simplex-agent-runtime-probe"),
    )).resolve()
    source = Path(os.environ.get(
        "SIMPLEX_AGENT_RUNTIME_SOURCE",
        str(microkit / "SimplexAgentRuntimeProbe.hs"),
    )).resolve()
    output.mkdir(parents=True, exist_ok=True)
    paths = [
        microkit,
        microkit / "guest-transport-shims",
        microkit / "guest-client-shims",
        microkit / "guest-http2-shims",
        http2,
        full_stage,
        autogen,
    ]
    mode = ["--make", "-no-link"]
    if os.environ.get("AGENT_RUNTIME_FORCE_REBUILD") == "1":
        mode.append("-fforce-recomp")
    args = [
        str(compiler), *mode, "-v0", "-cpp",
        "-no-global-package-db", "-package-env=-", "-package-db", str(target_db),
        "-package-db", str(guest_db), "-DSIMPLEX_MICROKIT", "-XStrictData",
        "-XOverloadedStrings", "-XDerivingStrategies",
        *[f"-i{path}" for path in paths], "-optc-target", "-optcaarch64-none-elf",
        f"-optc-I{microkit / 'freestanding-headers'}",
        f"-optc-I{ghc_tree / 'rts/include'}", f"-optc-I{ghc_tree / 'rts'}",
        "-outputdir", str(output), str(microkit / "NativeNetwork.hs"),
        str(microkit / "NativeSQLiteVFS.hs"),
        str(output / "SMPFixtureCA.hs"),
        str(source),
    ]
    if os.environ.get("SIMPLEX_AGENT_EXTERNAL_PEER") == "1":
        args.insert(args.index("-XStrictData"), "-DSIMPLEX_AGENT_EXTERNAL_PEER")
    if os.environ.get("SIMPLEX_AGENT_EXTERNAL_BIDIRECTIONAL") == "1":
        args.insert(args.index("-XStrictData"), "-DSIMPLEX_AGENT_EXTERNAL_BIDIRECTIONAL")
    if os.environ.get("SIMPLEX_AGENT_SERVICE_MODE") == "1":
        args.insert(args.index("-XStrictData"), "-DSIMPLEX_AGENT_SERVICE_MODE")
    if os.environ.get("SIMPLEX_ATTACHMENT_SELF_TEST") == "1":
        args.insert(args.index("-XStrictData"), "-DSIMPLEX_ATTACHMENT_SELF_TEST")
    if os.environ.get("SIMPLEX_PUBLIC_SMP") == "1":
        args.insert(args.index("-XStrictData"), "-DSIMPLEX_PUBLIC_SMP")
    result = subprocess.run(args, cwd=ghc_tree, capture_output=True, text=True)
    if result.returncode:
        diagnostics = output / "object-build.stderr"
        diagnostics.write_text(result.stderr)
        raise SystemExit(
            f"agent runtime probe failed; full diagnostics: {diagnostics}\n"
            + result.stderr[-12000:]
        )
    count = len(list(output.rglob("*.o")))
    print(f"SIMPLEX AGENT RUNTIME|AARCH64 OBJECT BUILD PASS|{count} objects")


if __name__ == "__main__":
    main()
