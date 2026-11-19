#!/usr/bin/env python3
"""Cross-compile upstream certificate-chain handling and guest logging."""

import struct
import subprocess

from verify_simplexmq_backend_patch import ROOT, SOURCE, ensure_patch_applied


def main() -> None:
    ensure_patch_applied()
    ghc_tree = ROOT / ".cache/ghc-port/ghc-9.6.6"
    compiler = ghc_tree / "_build/stage0/bin/aarch64-unknown-none-ghc"
    target_db = ghc_tree / "_build/stage1/lib/package.conf.d"
    guest_db = ROOT / ".cache/sel4-tls-deps/guest-package.conf.d"
    shims = ROOT / "experiments/ghc-microkit/guest-client-shims"
    output = ROOT / "experiments/ghc-microkit/build/guest-simplex-shared"
    output.mkdir(parents=True, exist_ok=True)
    args = [compiler, "--make", "-c", "-fforce-recomp", "-v0",
            "-no-global-package-db", "-package-env=-",
            "-package-db", target_db, "-package-db", guest_db,
            f"-i{shims}", f"-i{SOURCE / 'src'}", "-outputdir", output,
            SOURCE / "src/Simplex/Messaging/Transport/Shared.hs",
            shims / "UnliftIO.hs", shims / "UnliftIO/Exception.hs",
            shims / "UnliftIO/STM.hs"]
    subprocess.run([str(arg) for arg in args], cwd=ghc_tree, check=True)
    for name in ("Simplex/Messaging/Transport/Shared.o",
                 "Control/Logger/Simple.o", "UnliftIO.o",
                 "UnliftIO/Exception.o", "UnliftIO/STM.o"):
        obj = output / name
        header = obj.read_bytes()[:20]
        if (len(header) < 20 or header[:6] != b"\x7fELF\x02\x01"
                or struct.unpack_from("<H", header, 18)[0] != 183):
            raise RuntimeError(f"not an AArch64 ELF object: {obj}")
    print("UPSTREAM SHARED+GUEST SHIMS|AARCH64 OBJECTS PASS")


if __name__ == "__main__":
    main()
