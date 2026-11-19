#!/usr/bin/env python3
"""Cross-typecheck and emit target objects for the upstream HTTP/2 client."""

import subprocess
from pathlib import Path

from prepare_simplexmq_guest_http2 import ROOT, prepare


def main() -> None:
    source = prepare()
    ghc_tree = ROOT / ".cache/ghc-port/ghc-9.6.6"
    compiler = ghc_tree / "_build/stage0/bin/aarch64-unknown-none-ghc"
    target_db = ghc_tree / "_build/stage1/lib/package.conf.d"
    guest_db = ROOT / ".cache/sel4-tls-deps/guest-package.conf.d"
    shims = ROOT / "experiments/ghc-microkit/guest-http2-shims"
    client_shims = ROOT / "experiments/ghc-microkit/guest-client-shims"
    output = ROOT / "experiments/ghc-microkit/build/guest-simplex-http2-check"
    output.mkdir(parents=True, exist_ok=True)
    args = [
        str(compiler), "-fno-code", "-fforce-recomp", "-v0",
        "-no-global-package-db", "-package-env=-",
        "-package-db", str(target_db), "-package-db", str(guest_db),
        "-DSIMPLEX_MICROKIT", f"-i{shims}", f"-i{client_shims}", f"-i{source}",
        "-outputdir", str(output), str(source / "Network/HTTP2/Client.hs"),
    ]
    result = subprocess.run(args, cwd=ghc_tree, capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError("HTTP/2 client target typecheck failed:\n" + result.stderr[:9000])
    print("UPSTREAM HTTP2 CLIENT|TYPECHECK PASS")
    object_dir = output / "objects"
    object_dir.mkdir(parents=True, exist_ok=True)
    compile_args = args.copy()
    compile_args[1:2] = ["--make"]
    compile_args.insert(2, "-no-link")
    compile_args[compile_args.index("-outputdir") + 1] = str(object_dir)
    compiled = subprocess.run(compile_args, cwd=ghc_tree, capture_output=True, text=True)
    if compiled.returncode:
        raise RuntimeError("HTTP/2 client target object build failed:\n" + compiled.stderr[:9000])
    count = len(list(object_dir.rglob("*.o")))
    print(f"UPSTREAM HTTP2 CLIENT|AARCH64 OBJECT BUILD PASS|{count} objects")


if __name__ == "__main__":
    main()
