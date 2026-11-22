#!/usr/bin/env python3
"""Build the upstream SimpleX chat lockfile's Zstandard decoder for AArch64."""

import hashlib
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile

HERE = Path(__file__).resolve().parent
ARCHIVE = HERE.parents[1] / ".cache/simplex-toolchain/cabal/packages/hackage.haskell.org/zstd/0.1.3.0/zstd-0.1.3.0.tar.gz"
SHA256 = "a0c2b5235e66b2696257cdee11a419ca17bff389f78989b5381eb3ce10a1f06d"


def build_decoder(output: Path) -> None:
    if hashlib.sha256(ARCHIVE.read_bytes()).hexdigest() != SHA256:
        raise ValueError(f"unexpected zstd source archive: {ARCHIVE}")
    output.mkdir(parents=True, exist_ok=True)
    dest = output / "SimplexDirectZstd.o"
    with tempfile.TemporaryDirectory(dir=output.parent) as work_dir:
        work = Path(work_dir)
        with tarfile.open(ARCHIVE) as archive:
            archive.extractall(work, filter="data")
        lib = work / "zstd-0.1.3.0/zstd/lib"
        # The decoder needs no compressor, worker pool, assembly or legacy codecs.
        sources = sorted((lib / "decompress").glob("*.c"))
        sources += [lib / "common" / f"{name}.c" for name in (
            "debug", "entropy_common", "error_private", "fse_decompress", "xxhash", "zstd_common"
        )]
        objects = []
        for source in sources:
            obj = work / source.with_suffix(".o").name
            subprocess.run([
                "/usr/bin/clang", "-target", "aarch64-none-elf", "-mcpu=cortex-a53",
                "-mstrict-align", "-ffreestanding", "-fno-stack-protector", "-O2",
                "-DNDEBUG", "-DZSTD_LEGACY_SUPPORT=0", "-DZSTD_DISABLE_ASM", "-DZSTD_TRACE=0",
                f"-I{HERE / 'freestanding-headers'}", "-c", source, "-o", obj,
            ], check=True)
            objects.append(obj)
        linked = work / dest.name
        subprocess.run(["/opt/homebrew/bin/ld.lld", "-r", *objects, "-o", linked], check=True)
        linked.replace(dest)


if __name__ == "__main__":
    build_decoder(Path(sys.argv[1]).resolve())
