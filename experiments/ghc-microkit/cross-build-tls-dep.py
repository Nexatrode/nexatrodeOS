#!/usr/bin/env python3
"""Build a pinned Hackage TLS dependency for the seL4 GHC target.

The Cabal global database is needed to satisfy Cabal's install step.  The
stage1 and guest databases take precedence, and the post-build audit rejects
dependencies absent from those target databases before registration.
"""

import argparse
import json
import re
import struct
import sys
import subprocess
import tarfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CACHE = ROOT / ".cache"
GHC_TREE = CACHE / "ghc-port/ghc-9.6.6"
TARGET_DB = GHC_TREE / "_build/stage1/lib/package.conf.d"
GUEST_DB = CACHE / "sel4-tls-deps/guest-package.conf.d"
CROSS_GHC = GHC_TREE / "_build/stage0/bin/aarch64-unknown-none-ghc"
CROSS_PKG = GHC_TREE / "_build/stage0/bin/aarch64-unknown-none-ghc-pkg"
CABAL = CACHE / "simplex-toolchain/.ghcup/cabal/3.10.3.0/cabal"
TARGET_CC = ROOT / "experiments/ghc-microkit/target-clang.sh"
TARGET_HEADERS = ROOT / "experiments/ghc-microkit/freestanding-headers"
SIMPLEX_PLAN = CACHE / "simplex-chat-upstream/dist-newstyle/cache/plan.json"
LLVM_AR = Path("/opt/homebrew/opt/llvm/bin/llvm-ar")
PACKAGES = {
    "case-insensitive": "1.2.1.0",
    "http-types": "0.12.4",
    "Only": "0.1",
    "blaze-builder": "0.4.2.3",
    "old-locale": "1.0.0.7",
    "blaze-textual": "0.2.3.1",
    "data-default-class": "0.1.2.0",
    "cereal": "0.5.8.3",
    "basement": "0.0.16",
    "memory": "0.18.0",
    "crypton": "0.34",
    "hourglass": "0.2.12",
    "pem": "0.2.4",
    "asn1-types": "0.3.4",
    "asn1-encoding": "0.9.6",
    "asn1-parse": "0.9.5",
    "crypton-x509": "1.7.6",
    "crypton-x509-store": "1.6.9",
    "crypton-x509-validation": "1.6.12",
    "hashable": "1.4.3.0",
    "async": "2.2.5",
    "tls": "1.9.0",
    "primitive": "0.8.0.0",
    "integer-logarithms": "1.0.3.1",
    "scientific": "0.3.7.0",
    "attoparsec": "0.14.4",
    "network-transport": "0.5.6",
    "unliftio-core": "0.2.1.0",
    "safe-exceptions": "0.1.7.4",
    "vector-stream": "0.1.0.0",
    "vector": "0.13.1.0",
    "unordered-containers": "0.2.19.1",
    "data-fix": "0.3.2",
    "dlist": "1.0",
    "generically": "0.1.1",
    "OneTuple": "0.4.1.1",
    "th-abstraction": "0.6.0.0",
    "integer-conversion": "0.1.0.1",
    "indexed-traversable": "0.1.3",
    "tagged": "0.8.8",
    "base-orphans": "0.9.1",
    "time-compat": "1.9.6.1",
    "text-iso8601": "0.1",
    "text-short": "0.1.5",
    "assoc": "1.1",
    "semialign": "1.3",
    "strict": "0.5",
    "these": "1.2",
    "witherable": "0.4.2",
    "indexed-traversable-instances": "0.1.1.2",
    "distributive": "0.6.2.1",
    "transformers-compat": "0.7.2",
    "comonad": "5.0.8",
    "bifunctors": "5.6.1",
    "StateVar": "1.2.2",
    "contravariant": "1.5.5",
    "semigroupoids": "6.0.0.1",
    "th-compat": "0.1.4",
    "network-uri": "2.6.4.2",
    "parsec": "3.1.16.1",
    "splitmix": "0.1.0.5",
    "random": "1.2.1.1",
    "QuickCheck": "2.14.3",
    "uuid-types": "1.0.5.1",
    "network-byte-order": "0.1.7",
    "psqueues": "0.2.8.0",
    "auto-update": "0.1.6",
    "time-manager": "0.0.1",
    "aeson": "2.2.1.0",
    "base64-bytestring": "1.2.1.0",
    "iso8601-time": "0.1.5",
}


def run(*args, cwd=None):
    subprocess.run([str(arg) for arg in args], cwd=cwd, check=True)


def field(conf: str, name: str) -> str:
    match = re.search(rf"^{re.escape(name)}:\s*(.*?)(?=^\S|\Z)", conf,
                      re.MULTILINE | re.DOTALL)
    if not match:
        raise RuntimeError(f"missing {name} in package registration")
    return " ".join(match.group(1).split())


def available_target_ids() -> set[str]:
    ids = set()
    for db in (TARGET_DB, GUEST_DB):
        for conf_path in db.glob("*.conf"):
            ids.add(field(conf_path.read_text(), "id"))
    return ids


def pinned_host_constraints(package: str) -> list[str]:
    """Keep a target build's dependency closure on the SimpleX lockfile graph."""
    if not SIMPLEX_PLAN.is_file():
        return []
    installed = {}
    for db in (TARGET_DB, GUEST_DB):
        for conf_path in db.glob("*.conf"):
            conf = conf_path.read_text()
            installed[field(conf, "name").lower()] = field(conf, "version")
    plan = json.loads(SIMPLEX_PLAN.read_text())
    by_id = {item.get("id"): item for item in plan.get("install-plan", [])
             if item.get("id")}
    roots = [item for item in by_id.values() if item.get("pkg-name", "").lower() == package.lower()]
    if not roots:
        return []
    pending = list(roots[0].get("depends", []))
    closure = {}
    while pending:
        package_id = pending.pop()
        item = by_id.get(package_id)
        if not item:
            continue
        name, version = item.get("pkg-name"), item.get("pkg-version")
        if not name or not version or name.lower() in closure:
            continue
        closure[name.lower()] = (name, installed.get(name.lower(), version))
        pending.extend(item.get("depends", []))
    return sorted(f"--constraint={name}=={version}"
                  for name, version in closure.values())


def unpack(archive: Path, source_dir: Path, top_name: str) -> None:
    if source_dir.exists():
        return
    source_dir.mkdir(parents=True)
    with tarfile.open(archive, "r:gz") as tar:
        for member in tar.getmembers():
            path = Path(member.name)
            if (not path.parts or path.parts[0] != top_name
                    or path.is_absolute() or ".." in path.parts
                    or member.issym() or member.islnk() or member.isdev()):
                raise RuntimeError(f"unsafe archive member: {member.name}")
            member.name = str(Path(*path.parts[1:]))
            if member.name != ".":
                if sys.version_info >= (3, 12):
                    tar.extract(member, source_dir, filter="data")
                else:
                    # Earlier host Python versions lack tarfile's data filter.
                    # Member paths and link/device types were rejected above.
                    tar.extract(member, source_dir)


def audit_objects(build_dir: Path) -> int:
    def is_aarch64_elf(header: bytes) -> bool:
        return (len(header) >= 20 and header[:6] == b"\x7fELF\x02\x01"
                and struct.unpack_from("<H", header, 18)[0] == 183)

    objects = list(build_dir.rglob("*.o"))
    if not objects:
        raise RuntimeError("no target object files were built")
    for obj in objects:
        header = obj.open("rb").read(20)
        if not is_aarch64_elf(header):
            raise RuntimeError(f"not an AArch64 ELF object: {obj}")
    archives = list(build_dir.glob("libHS*.a"))
    if len(archives) != 1:
        raise RuntimeError(f"expected one static archive, got {archives}")
    archive = archives[0].read_bytes()
    if not archive.startswith(b"!<arch>\n"):
        raise RuntimeError(f"not a standard static archive: {archives[0]}")
    offset, members = 8, 0
    while offset < len(archive):
        header = archive[offset:offset + 60]
        if len(header) != 60 or header[58:60] != b"`\n":
            raise RuntimeError(f"corrupt archive header: {archives[0]}")
        size = int(header[48:58].strip())
        name = header[:16].strip()
        body_start = offset + 60
        body = archive[body_start:body_start + size]
        if len(body) != size:
            raise RuntimeError(f"truncated archive member: {archives[0]}")
        if name not in (b"/", b"//") and not name.startswith(b"__.SYMDEF"):
            if name.startswith(b"#1/"):
                body = body[int(name[3:]):]
            if not is_aarch64_elf(body):
                raise RuntimeError(f"non-AArch64 archive member: {name!r} in {archives[0]}")
            members += 1
        offset = body_start + size + (size & 1)
    if members != len(objects):
        raise RuntimeError(f"archive has {members} members for {len(objects)} objects: "
                           f"{archives[0]}")
    return len(objects)


def adapt_basement(source_dir: Path) -> None:
    """Replace the terminal ioctl probe with a freestanding default size."""
    terminal = source_dir / "Basement/Terminal/Size.hsc"
    replacement = terminal.with_suffix(".hs")
    if (not replacement.exists() and
            (not terminal.exists() or "module Basement.Terminal.Size" not in terminal.read_text())):
        raise RuntimeError("basement terminal source changed upstream")
    target_source = (
        "module Basement.Terminal.Size (getDimensions) where\n"
        "import Prelude\n"
        "import Basement.Types.OffsetSize (CountOf(..))\n"
        "getDimensions :: IO (CountOf Char, CountOf Char)\n"
        "getDimensions = pure (CountOf 80, CountOf 24)\n"
    )
    if not replacement.exists() or replacement.read_text() != target_source:
        replacement.write_text(target_source)
    if terminal.exists():
        terminal.unlink()


def adapt_memory(source_dir: Path) -> None:
    """The private POSIX mmap helper has no Microkit equivalent."""
    manifest = source_dir / "memory.cabal"
    contents = manifest.read_text()
    marker = "    Other-modules:   Data.Memory.MemMap.Posix\n"
    if marker in contents:
        manifest.write_text(contents.replace(marker, ""))
    elif "Data.Memory.MemMap.Posix" in contents:
        raise RuntimeError("memory's POSIX module declaration changed upstream")


def adapt_blaze_textual(source_dir: Path) -> None:
    """Use base formatting while the target lacks integer-gmp internals."""
    module = source_dir / "Blaze/Text/Int.hs"
    original = module.read_text()
    if "module Blaze.Text.Int" not in original:
        raise RuntimeError("blaze-textual integer module changed upstream")
    module.write_text(
        "module Blaze.Text.Int (digit, integral, minus) where\n"
        "import Blaze.ByteString.Builder (Builder)\n"
        "import Blaze.ByteString.Builder.Char8 (fromString)\n"
        "integral :: (Integral a, Show a) => a -> Builder\n"
        "integral = fromString . show\n"
        "digit :: Integral a => a -> Builder\n"
        "digit n = fromString (show (fromIntegral n :: Int))\n"
        "minus :: Builder\n"
        "minus = fromString \"-\"\n"
    )
    manifest = source_dir / "blaze-textual.cabal"
    contents = manifest.read_text()
    integer_flags = (
        "  if flag(integer-simple)\n"
        "    cpp-options: -DINTEGER_SIMPLE\n"
        "    build-depends: integer-simple\n"
        "  else\n"
        "    cpp-options: -DINTEGER_GMP\n"
        "    build-depends: integer-gmp >= 0.2\n"
    )
    if integer_flags in contents:
        contents = contents.replace(integer_flags, "")
    elif ("build-depends: integer-gmp" in contents
          or "build-depends: integer-simple" in contents
          or "cpp-options: -DINTEGER_SIMPLE" in contents):
        raise RuntimeError("blaze-textual integer flags changed upstream")
    manifest.write_text(contents)


def adapt_text_short(source_dir: Path) -> None:
    """Keep one C assertion freestanding without depending on libc labs."""
    source = source_dir / "cbits/cbits.c"
    contents = source.read_text()
    original = "assert(labs(hs_text_short_mutf8_strlen(src0)) == (dst - dst0));"
    replacement = ("HsInt mutf8_len = hs_text_short_mutf8_strlen(src0);\n"
                  "  assert((mutf8_len < 0 ? -mutf8_len : mutf8_len) == (dst - dst0));")
    if replacement in contents:
        return
    if contents.count(original) != 1:
        raise RuntimeError("text-short mutf8 length assertion changed upstream")
    source.write_text(contents.replace(original, replacement))


def adapt_splitmix(source_dir: Path) -> None:
    """Seed QuickCheck's generator from the isolated Microkit entropy service."""
    manifest = source_dir / "splitmix.cabal"
    contents = manifest.read_text()
    old = ("    if impl(ghc)\n"
           "      cpp-options: -DSPLITMIX_INIT_C=1\n\n"
           "      if os(windows)\n"
           "        c-sources: cbits-win/init.c\n\n"
           "      else\n"
           "        c-sources: cbits-unix/init.c\n")
    new = ("    if impl(ghc)\n"
           "      cpp-options: -DSPLITMIX_INIT_MICROKIT=1\n")
    if new not in contents:
        if contents.count(old) != 1:
            raise RuntimeError("splitmix platform seed stanza changed upstream")
        manifest.write_text(contents.replace(old, new))
    source = source_dir / "src/System/Random/SplitMix/Init.hs"
    contents = source.read_text()
    old_import = "import Data.Word (Word64)\n"
    new_import = ("import Data.Word (Word64, Word8)\n"
                 "#if defined(SPLITMIX_INIT_MICROKIT)\n"
                 "import Control.Monad (foldM)\n"
                 "import Data.Bits ((.|.), shiftL)\n"
                 "import Foreign.C.Types (CInt(..))\n"
                 "import Foreign.Marshal.Alloc (allocaBytes)\n"
                 "import Foreign.Ptr (Ptr)\n"
                 "import Foreign.Storable (peekByteOff)\n"
                 "#endif\n")
    if new_import not in contents:
        if contents.count(old_import) != 1:
            raise RuntimeError("splitmix seed imports changed upstream")
        contents = contents.replace(old_import, new_import)
    old_branch = "#if defined(SPLITMIX_INIT_C)\n"
    new_branch = "#if defined(SPLITMIX_INIT_MICROKIT)\n"
    if old_branch in contents:
        contents = contents.replace(old_branch, new_branch)
    old_impl = "initialSeed = initialSeedC\n\nforeign import ccall \"splitmix_init\" initialSeedC :: IO Word64\n"
    new_impl = ("foreign import ccall unsafe \"sel4_haskell_entropy_bytes\"\n"
                "    readEntropy :: Ptr Word8 -> CInt -> IO CInt\n\n"
                "initialSeed = allocaBytes 8 $ \\buffer -> do\n"
                "    count <- readEntropy buffer 8\n"
                "    if count /= 8 then ioError (userError \"Microkit entropy unavailable\")\n"
                "    else foldM (\\seed index -> do\n"
                "        byte <- peekByteOff buffer index :: IO Word8\n"
                "        pure (seed .|. (fromIntegral byte `shiftL` (8 * index)))) 0 [0..7]\n")
    if new_impl not in contents:
        if contents.count(old_impl) != 1:
            raise RuntimeError("splitmix C seed implementation changed upstream")
        contents = contents.replace(old_impl, new_impl)
    source.write_text(contents)


def adapt_time_manager(source_dir: Path) -> None:
    """Use base exception APIs instead of the unliftio re-export."""
    manifest = source_dir / "time-manager.cabal"
    contents = manifest.read_text()
    if ", unliftio\n" in contents:
        contents = contents.replace(", unliftio\n", "\n")
    elif "unliftio" in contents:
        raise RuntimeError("time-manager dependency stanza changed upstream")
    manifest.write_text(contents)
    source = source_dir / "System/TimeManager.hs"
    contents = source.read_text()
    old = "import qualified UnliftIO.Exception as E\n"
    new = "import qualified Control.Exception as E\n"
    if old in contents:
        if contents.count(old) != 1:
            raise RuntimeError("time-manager exception import changed upstream")
        contents = contents.replace(old, new)
    elif new not in contents:
        raise RuntimeError("time-manager exception import changed upstream")
    source.write_text(contents)


def adapt_crypton(source_dir: Path) -> None:
    """Use the target's known AArch64 little-endian byte order."""
    manifest = source_dir / "crypton.cabal"
    contents = manifest.read_text()
    marker = "    cc-options:       -std=gnu99"
    replacement = "    cc-options:       -std=gnu99 -DARGON2_NO_THREADS"
    if replacement in contents:
        pass
    elif marker in contents:
        manifest.write_text(contents.replace(marker, replacement))
    else:
        raise RuntimeError("crypton C options changed upstream")
    header = source_dir / "cbits/crypton_bitfn.h"
    contents = header.read_text()
    marker = "#else\n  # include <endian.h>\n#endif"
    replacement = ("#elif defined(__aarch64__) && defined(__ELF__)\n"
                   "  # define LITTLE_ENDIAN 1234\n"
                   "  # define BYTE_ORDER LITTLE_ENDIAN\n"
                   "#else\n  # include <endian.h>\n#endif")
    if replacement in contents:
        pass
    elif marker in contents:
        header.write_text(contents.replace(marker, replacement))
    else:
        raise RuntimeError("crypton byte-order header changed upstream")
    identify = source_dir / "cbits/ed25519/ed25519-donna-portable-identify.h"
    contents = identify.read_text()
    marker = "\t#include <sys/param.h> /* need this to define BSD */"
    replacement = ("\t#if !defined(__aarch64__) || !defined(__ELF__)\n"
                   + marker + "\n\t#endif")
    if marker in contents and replacement not in contents:
        identify.write_text(contents.replace(marker, replacement))
    elif replacement not in contents:
        raise RuntimeError("crypton platform-identification header changed upstream")
    portable = source_dir / "cbits/ed25519/ed25519-donna-portable.h"
    contents = portable.read_text()
    marker = "\t#include <sys/param.h>\n"
    replacement = ("\t#if !defined(__aarch64__) || !defined(__ELF__)\n"
                   + marker + "\t#endif\n")
    if marker in contents and replacement not in contents:
        portable.write_text(contents.replace(marker, replacement))
    elif replacement not in contents:
        raise RuntimeError("crypton portable header changed upstream")
    ed25519 = source_dir / "cbits/ed25519/ed25519.c"
    contents = ed25519.read_text()
    if "#include <stdlib.h>" not in contents:
        marker = '#include "ed25519-donna.h"'
        if contents.count(marker) != 1:
            raise RuntimeError("crypton Ed25519 source changed upstream")
        ed25519.write_text(contents.replace(marker, "#include <stdlib.h>\n" + marker))

    # Replace Unix device files with the existing isolated Microkit RNG PD.
    # Keep the upstream module name because it is private to crypton.
    unix = source_dir / "Crypto/Random/Entropy/Unix.hs"
    contents = unix.read_text()
    if "sel4_haskell_entropy_bytes" not in contents:
        if 'openBinaryFile filepath ReadMode' not in contents:
            raise RuntimeError("crypton Unix entropy source changed upstream")
        unix.write_text(
            "module Crypto.Random.Entropy.Unix (DevRandom, DevURandom) where\n"
            "import Crypto.Random.Entropy.Source\n"
            "import Data.Word (Word8)\n"
            "import Foreign.C.Types (CInt(..))\n"
            "import Foreign.Ptr (Ptr)\n"
            "newtype DevRandom = DevRandom ()\n"
            "newtype DevURandom = DevURandom ()\n"
            "foreign import ccall unsafe \"sel4_haskell_entropy_bytes\"\n"
            "  microkitEntropy :: Ptr Word8 -> CInt -> IO CInt\n"
            "gather :: Ptr Word8 -> Int -> IO Int\n"
            "gather ptr n | n < 0 || n > fromIntegral (maxBound :: CInt) = pure 0\n"
            "             | otherwise = fromIntegral <$> microkitEntropy ptr (fromIntegral n)\n"
            "instance EntropySource DevRandom where\n"
            "  entropyOpen = pure (Just (DevRandom ()))\n"
            "  entropyGather _ = gather\n"
            "  entropyClose _ = pure ()\n"
            "instance EntropySource DevURandom where\n"
            "  entropyOpen = pure (Just (DevURandom ()))\n"
            "  entropyGather _ = gather\n"
            "  entropyClose _ = pure ()\n"
        )
    backend = source_dir / "Crypto/Random/Entropy/Backend.hs"
    contents = backend.read_text()
    marker = "openBackend (Proxy :: Proxy DevRandom), openBackend (Proxy :: Proxy DevURandom)"
    replacement = "openBackend (Proxy :: Proxy DevURandom)"
    if marker in contents:
        backend.write_text(contents.replace(marker, replacement))
    elif replacement not in contents:
        raise RuntimeError("crypton entropy backend list changed upstream")


def adapt_hourglass(source_dir: Path) -> None:
    """Use the explicit guest wall-clock hook; never invent certificate time."""
    clock = source_dir / "cbits/unix.c"
    contents = clock.read_text()
    marker = "\tclock_gettime(CLOCK_REALTIME, timespec);"
    replacement = (
        "\tuint64_t seconds;\n"
        "\tuint32_t nanoseconds;\n"
        "\tif (!sel4_rts_read_unix_time(&seconds, &nanoseconds) || "
        "nanoseconds >= 1000000000U) abort();\n"
        "\ttimespec->tv_sec = (time_t)seconds;\n"
        "\ttimespec->tv_nsec = (long)nanoseconds;"
    )
    if marker in contents:
        contents = contents.replace(marker, replacement)
    elif replacement not in contents:
        raise RuntimeError("hourglass clock source changed upstream")
    include = "#include <time.h>"
    if "#include <stdbool.h>" not in contents:
        if contents.count(include) != 1:
            raise RuntimeError("hourglass clock includes changed upstream")
        contents = contents.replace(
            include,
            include + "\n#include <stdbool.h>\n#include <stdint.h>\n#include <stdlib.h>\n"
            "extern bool sel4_rts_read_unix_time(uint64_t *, uint32_t *);",
        )
    if clock.read_text() != contents:
        clock.write_text(contents)


def adapt_x509_store(source_dir: Path) -> None:
    """Keep in-memory trust stores and fail closed on filesystem loading."""
    manifest = source_dir / "crypton-x509-store.cabal"
    contents = manifest.read_text()
    marker = "                   , directory\n                   , filepath\n"
    if marker in contents:
        manifest.write_text(contents.replace(marker, ""))
    elif "                   , directory" in contents:
        raise RuntimeError("x509-store manifest changed upstream")
    module = source_dir / "Data/X509/CertificateStore.hs"
    contents = module.read_text()
    boundary = "-- | Create certificate store by reading certificates from file or directory"
    if boundary in contents:
        contents = contents[:contents.index(boundary)] + (
            "-- File loading is unavailable until Microkit has a filesystem service.\n"
            "readCertificateStore :: FilePath -> IO (Maybe CertificateStore)\n"
            "readCertificateStore _ = ioError (userError \"certificate filesystem unavailable\")\n\n"
            "readCertificates :: FilePath -> IO [SignedCertificate]\n"
            "readCertificates _ = ioError (userError \"certificate filesystem unavailable\")\n"
        )
        contents = contents.replace(
            "import System.Directory (getDirectoryContents, doesFileExist, doesDirectoryExist)\n", ""
        ).replace("import System.FilePath ((</>))\n", "")
        module.write_text(contents)
    elif "certificate filesystem unavailable" not in contents:
        raise RuntimeError("x509-store certificate module changed upstream")


def adapt_tls(source_dir: Path) -> None:
    """Route TLS 1.3 ticket age through the guest's checked epoch clock."""
    manifest = source_dir / "tls.cabal"
    contents = manifest.read_text()
    marker = "        unix-time\n"
    if marker in contents:
        manifest.write_text(contents.replace(marker, "        hourglass\n"))
    elif "        hourglass\n" not in contents:
        raise RuntimeError("TLS time dependency changed upstream")
    handshake = source_dir / "Network/TLS/Handshake/Common13.hs"
    contents = handshake.read_text()
    marker = "import Data.UnixTime\nimport Foreign.C.Types (CTime(..))"
    replacement = ("import Time.System (timeCurrentP)\n"
                   "import Time.Types (ElapsedP(..), Elapsed(..), Seconds(..), NanoSeconds(..))")
    if marker in contents:
        contents = contents.replace(marker, replacement)
    elif replacement not in contents:
        raise RuntimeError("TLS ticket clock imports changed upstream")
    marker = (
        "getCurrentTimeFromBase = millisecondsFromBase <$> getUnixTime\n\n"
        "millisecondsFromBase :: UnixTime -> Millisecond\n"
        "millisecondsFromBase (UnixTime (CTime s) us) =\n"
        "    fromIntegral ((s - base) * 1000) + fromIntegral (us `div` 1000)"
    )
    replacement = (
        "getCurrentTimeFromBase = millisecondsFromBase <$> timeCurrentP\n\n"
        "millisecondsFromBase :: ElapsedP -> Millisecond\n"
        "millisecondsFromBase (ElapsedP (Elapsed (Seconds s)) (NanoSeconds ns)) =\n"
        "    fromIntegral ((s - base) * 1000) + fromIntegral (ns `div` 1000000)"
    )
    if marker in contents:
        contents = contents.replace(marker, replacement)
    elif replacement not in contents:
        raise RuntimeError("TLS ticket clock calculation changed upstream")
    if handshake.read_text() != contents:
        handshake.write_text(contents)
    backend = source_dir / "Network/TLS/Backend.hs"
    contents = backend.read_text()
    marker = "safeRecv :: Network.Socket -> Int -> IO ByteString"
    if "#ifdef INCLUDE_NETWORK\n" + marker not in contents:
        if contents.count(marker) != 1:
            raise RuntimeError("TLS socket backend changed upstream")
        contents = contents.replace(marker, "#ifdef INCLUDE_NETWORK\n" + marker)
        marker = "#ifdef INCLUDE_NETWORK\ninstance HasBackend Network.Socket where"
        if contents.count(marker) != 1:
            raise RuntimeError("TLS socket instance changed upstream")
        contents = contents.replace(marker, "#endif\n" + marker)
        backend.write_text(contents)


def relax_cabal_bounds(source_dir: Path, name: str) -> None:
    """Apply the GHC 9.6-compatible bounds already used by the host solve."""
    bounds = {
        "integer-logarithms": (("base      >=4.3 && <4.16", "base      >=4.3 && <4.19"),
                               ("ghc-prim  >=0   && <0.8", "ghc-prim  >=0   && <0.11"),
                               ("ghc-bignum  >=1.0  && <1.1", "ghc-bignum  >=1.0  && <1.4")),
        "scientific": (("base                >=4.5     && <4.16", "base                >=4.5     && <4.19"),
                       ("hashable            >=1.2.7.0 && <1.4", "hashable            >=1.2.7.0 && <1.5"),
                       ("primitive           >=0.7.1.0 && <0.8", "primitive           >=0.7.1.0 && <0.9"),
                       ("template-haskell    >=2.8     && <2.18", "template-haskell    >=2.8     && <2.21"),
                       ("text                >=1.2.3.0 && <1.3", "text                >=1.2.3.0 && <2.1")),
        "attoparsec": (("ghc-prim <0.9", "ghc-prim <0.11"),),
        "network-transport": (("transformers >= 0.2 && < 0.6", "transformers >= 0.2 && < 0.7"),),
        "unliftio-core": (("base >=4.9 && <4.17", "base >=4.9 && <4.19"),),
        "vector-stream": (("base >= 4.9 && < 4.17", "base >= 4.9 && < 4.19"),
                          ("ghc-prim >= 0.2 && < 0.9", "ghc-prim >= 0.2 && < 0.11")),
        "unordered-containers": (("template-haskell < 2.19", "template-haskell < 2.21"),),
        "data-fix": (("base      >=4.4     && <4.16", "base      >=4.4     && <4.19"),
                     ("hashable  >=1.2.7.0 && <1.4", "hashable  >=1.2.7.0 && <1.5")),
        "generically": (("base >=4.9 && <4.18", "base >=4.9 && <4.19"),),
        "time-compat": (("base          >=4.5     && <4.16", "base          >=4.5     && <4.19"),
                         ("base-orphans  >=0.8.4   && <0.9", "base-orphans  >=0.8.4   && <1.0"),
                         ("hashable      >=1.3.2.0 && <1.4", "hashable      >=1.3.2.0 && <1.5")),
        "text-short": (("base        >= 4.7    && < 4.17", "base        >= 4.7    && < 4.19"),
                        ("ghc-prim    >= 0.3.1  && < 0.9", "ghc-prim    >= 0.3.1  && < 0.11"),
                        ("template-haskell >= 2.9.0.0 && <2.19", "template-haskell >= 2.9.0.0 && <2.21")),
        "indexed-traversable-instances": (("base                  >=4.5      && <4.18", "base                  >=4.5      && <4.19"),),
        "distributive": (("transformers        >= 0.3 && < 0.6", "transformers        >= 0.3 && < 0.7"),),
        "comonad": (("transformers        >= 0.3 && < 0.6", "transformers        >= 0.3 && < 0.7"),),
        "bifunctors": (("th-abstraction           >= 0.4.2.0 && < 0.6", "th-abstraction           >= 0.4.2.0 && < 0.7"),),
        "witherable": (("base-orphans                  >=0.8.4    && <0.9", "base-orphans                  >=0.8.4    && <1.0"),
                       ("hashable                      >=1.2.7.0  && <1.4", "hashable                      >=1.2.7.0  && <1.5"),
                       ("transformers                  >=0.5.2.0  && <0.6", "transformers                  >=0.5.2.0  && <0.7"),
                       ("vector                        >=0.12.2.0 && <0.13", "vector                        >=0.12.2.0 && <0.14")),
        "th-compat": (("template-haskell >= 2.5 && < 2.20", "template-haskell >= 2.5 && < 2.21"),),
        "old-locale": (("base >= 4.2 && < 4.9", "base >= 4.2 && < 4.19"),),
    }
    manifest = source_dir / f"{name}.cabal"
    contents = manifest.read_text()
    for old, new in bounds.get(name, ()):
        if new in contents:
            continue
        expected = 2 if name == "th-compat" else 1
        if contents.count(old) != expected:
            raise RuntimeError(f"{name} Cabal bounds changed: {old}")
        contents = contents.replace(old, new, expected)
    if manifest.read_text() != contents:
        manifest.write_text(contents)


def build_unordered_containers(source_dir: Path, work: Path, version: str) -> None:
    """Build this library directly: Cabal resolves an incompatible host GHC heap.

    The pinned library contains only these eight Haskell modules.  The target
    compiler can compile them without Cabal's host-side dependency solver.
    Registration names only audited packages from the target databases.
    """
    modules = (
        "Data/HashMap/Internal.hs", "Data/HashMap/Internal/Array.hs",
        "Data/HashMap/Internal/List.hs", "Data/HashMap/Internal/Strict.hs",
        "Data/HashMap/Lazy.hs", "Data/HashMap/Strict.hs",
        "Data/HashSet.hs", "Data/HashSet/Internal.hs",
    )
    for module in modules:
        if not (source_dir / module).is_file():
            raise RuntimeError(f"missing pinned source: {module}")
    output = work / f"build-unordered-containers-{version}-direct"
    output.mkdir(parents=True, exist_ok=True)
    run(CROSS_GHC, "--make", "-c", "-v0", "-O2", "-fforce-recomp",
        "-no-global-package-db", "-package-env=-", "-hide-all-packages",
        f"-this-unit-id=unordered-containers-{version}-inplace",
        f"-package-db={TARGET_DB}", f"-package-db={GUEST_DB}",
        "-package=base", "-package=deepseq", "-package=hashable",
        "-package=template-haskell", f"-i{source_dir}",
        "-outputdir", output, *(source_dir / module for module in modules),
        cwd=GHC_TREE)
    objects = sorted(output.rglob("*.o"))
    if len(objects) != len(modules):
        raise RuntimeError(f"expected {len(modules)} target objects, got {len(objects)}")
    archive = output / f"libHSunordered-containers-{version}-inplace.a"
    run(LLVM_AR, "crs", archive, *objects)
    count = audit_objects(output)
    dependencies = (
        "base-4.18.2.1", "deepseq-1.4.8.1",
        "hashable-1.4.3.0-inplace", "template-haskell-2.20.0.0",
    )
    unknown = set(dependencies) - available_target_ids()
    if unknown:
        raise RuntimeError(f"dependency not present in target databases: {sorted(unknown)}")
    exposed = " ".join(Path(module).with_suffix("").as_posix().replace("/", ".")
                       for module in modules)
    conf = output / f"unordered-containers-{version}-inplace.conf"
    conf.write_text(
        f"name: unordered-containers\nversion: {version}\n"
        f"id: unordered-containers-{version}-inplace\n"
        f"key: unordered-containers-{version}-inplace\n"
        f"exposed: True\nexposed-modules: {exposed}\n"
        f"import-dirs: {output}\nlibrary-dirs: {output}\n"
        f"hs-libraries: HSunordered-containers-{version}-inplace\n"
        f"depends: {' '.join(dependencies)}\n"
    )
    run(CROSS_PKG, "--no-user-package-db", f"--package-db={TARGET_DB}",
        f"--package-db={GUEST_DB}", "update", conf)
    print(f"Registered {conf.name}: {count} AArch64 objects; target dependencies: "
          + ", ".join(dependencies))


def build_aeson(source_dir: Path, work: Path, version: str) -> None:
    """Compile pinned Aeson directly; Cabal's solver selects GHC 9.14 heap data."""
    manifest = source_dir / "aeson.cabal"
    contents = manifest.read_text()

    def modules_for(field_name: str) -> list[str]:
        match = re.search(rf"(?m)^[ \t]*{re.escape(field_name)}:[ \t]*\n((?:[ \t]+[^\n]+\n)+)", contents)
        if not match:
            raise RuntimeError(f"aeson {field_name} field changed upstream")
        return [line.strip() for line in match.group(1).splitlines()
                if line.strip() and not line.strip().startswith("--")]

    exposed = modules_for("exposed-modules")
    hidden = modules_for("other-modules")
    modules = exposed + hidden
    for module in modules:
        if not (source_dir / "src" / (module.replace(".", "/") + ".hs")).is_file():
            raise RuntimeError(f"aeson source module missing: {module}")
    dependency_names = (
        "base", "bytestring", "containers", "deepseq", "exceptions", "ghc-prim",
        "template-haskell", "text", "time", "data-fix", "dlist", "hashable",
        "indexed-traversable", "integer-conversion", "integer-logarithms",
        "network-uri", "OneTuple", "primitive", "QuickCheck", "scientific",
        "semialign", "strict", "tagged", "text-iso8601", "text-short",
        "th-abstraction", "these", "unordered-containers", "uuid-types", "vector",
        "witherable", "generically", "time-compat",
    )
    installed = {}
    for db in (TARGET_DB, GUEST_DB):
        for conf_path in db.glob("*.conf"):
            registration = conf_path.read_text()
            installed[field(registration, "name").lower()] = (
                field(registration, "version"), field(registration, "id"))
    missing = [name for name in dependency_names if name.lower() not in installed]
    if missing:
        raise RuntimeError(f"aeson target dependencies are not registered: {missing}")

    output = work / f"build-aeson-{version}-direct"
    output.mkdir(parents=True, exist_ok=True)
    module_paths = [source_dir / "src" / (module.replace(".", "/") + ".hs")
                    for module in modules]
    run(CROSS_GHC, "--make", "-c", "-v0", "-O1", "-fforce-recomp", "-cpp",
        "-XHaskell2010",
        "-no-global-package-db", "-package-env=-", "-hide-all-packages",
        f"-this-unit-id=aeson-{version}-inplace", f"-package-db={TARGET_DB}",
        f"-package-db={GUEST_DB}", *(f"-package={name}" for name in dependency_names),
        "-optP-DUSE_ORDEREDMAP=1",
        f"-i{source_dir / 'src'}", "-outputdir", output, *module_paths,
        cwd=GHC_TREE)
    objects = sorted(output.rglob("*.o"))
    if len(objects) != len(modules):
        raise RuntimeError(f"expected {len(modules)} aeson objects, got {len(objects)}")
    archive = output / f"libHSaeson-{version}-inplace.a"
    run(LLVM_AR, "crs", archive, *objects)
    count = audit_objects(output)
    dependencies = [installed[name.lower()][1] for name in dependency_names]
    unknown = set(dependencies) - available_target_ids()
    if unknown:
        raise RuntimeError(f"aeson dependency is not in target databases: {sorted(unknown)}")
    conf = output / f"aeson-{version}-inplace.conf"
    conf.write_text(
        f"name: aeson\nversion: {version}\nid: aeson-{version}-inplace\n"
        f"key: aeson-{version}-inplace\nexposed: True\n"
        f"exposed-modules: {' '.join(exposed)}\nimport-dirs: {output}\n"
        f"library-dirs: {output}\nhs-libraries: HSaeson-{version}-inplace\n"
        f"depends: {' '.join(dependencies)}\n"
    )
    run(CROSS_PKG, "--no-user-package-db", f"--package-db={TARGET_DB}",
        f"--package-db={GUEST_DB}", "update", conf)
    print(f"Registered {conf.name}: {count} AArch64 objects; "
          f"{len(exposed)} exposed modules; target dependencies: {len(dependencies)}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("package", choices=PACKAGES)
    args = parser.parse_args()
    name, version = args.package, PACKAGES[args.package]
    top_name = f"{name}-{version}"
    archive = (CACHE / "simplex-toolchain/cabal/packages/hackage.haskell.org"
               / name / version / f"{top_name}.tar.gz")
    if not archive.is_file():
        archive = (Path.home() / ".cabal/packages/hackage.haskell.org"
                   / name / version / f"{top_name}.tar.gz")
    if name == "aeson" and not archive.is_file():
        archive = (CACHE / "simplex-chat-upstream/dist-newstyle/src"
                   / "aeson-4ebce7837a071208-aeson-2.2.1.0.tar.gz")
    if not archive.is_file():
        raise SystemExit(f"missing pinned Hackage archive: {archive}")
    for tool in (CROSS_GHC, CROSS_PKG, CABAL, LLVM_AR):
        if not tool.is_file():
            raise SystemExit(f"missing tool: {tool}")
    if not TARGET_DB.is_dir():
        raise SystemExit(f"missing stage1 target package database: {TARGET_DB}")

    work = CACHE / "sel4-tls-deps"
    work.mkdir(parents=True, exist_ok=True)
    if not GUEST_DB.exists():
        run(CROSS_PKG, "init", GUEST_DB)
    source_dir = work / top_name
    unpack(archive, source_dir, top_name)
    if name == "basement":
        adapt_basement(source_dir)
    elif name == "memory":
        adapt_memory(source_dir)
    elif name == "text-short":
        adapt_text_short(source_dir)
    elif name == "splitmix":
        adapt_splitmix(source_dir)
    elif name == "crypton":
        adapt_crypton(source_dir)
    elif name == "hourglass":
        adapt_hourglass(source_dir)
    elif name == "crypton-x509-store":
        adapt_x509_store(source_dir)
    elif name == "tls":
        adapt_tls(source_dir)
    elif name == "time-manager":
        adapt_time_manager(source_dir)
    elif name == "blaze-textual":
        adapt_blaze_textual(source_dir)
    elif name in ("integer-logarithms", "scientific", "attoparsec", "network-transport", "unliftio-core", "vector-stream", "unordered-containers", "data-fix", "generically", "time-compat", "text-short", "indexed-traversable-instances", "distributive", "comonad", "bifunctors", "witherable", "th-compat", "old-locale"):
        relax_cabal_bounds(source_dir, name)
    if name == "unordered-containers":
        build_unordered_containers(source_dir, work, version)
        return
    if name == "aeson":
        build_aeson(source_dir, work, version)
        return
    build_root = work / f"build-{top_name}"
    extra = []
    if name == "basement":
        if not TARGET_CC.is_file():
            raise SystemExit(f"missing tool: {TARGET_CC}")
        extra = [f"--with-gcc={TARGET_CC}",
                 f"--extra-include-dirs={TARGET_HEADERS}"]
    elif name in ("crypton", "hourglass", "hashable", "primitive"):
        extra = [f"--with-gcc={TARGET_CC}",
                 f"--extra-include-dirs={TARGET_HEADERS}",
                 *(("--flags=-integer-gmp",) if name == "crypton" else ())]
    elif name == "tls":
        extra = ["--flags=-network -hans -compat"]
    run(CABAL, "v2-build", "--offline", f"--with-compiler={CROSS_GHC}",
        f"--with-hc-pkg={CROSS_PKG}", "--package-db=clear",
        "--package-db=global", f"--package-db={TARGET_DB}",
        f"--package-db={GUEST_DB}", "--constraint=base ==4.18.2.1",
        "--disable-shared", f"--with-ar={LLVM_AR}", f"--builddir={build_root}",
        *pinned_host_constraints(name), *extra, cwd=source_dir)
    conf_dir = build_root / "packagedb/ghc-9.6.6"
    confs = list(conf_dir.glob(f"{top_name}-*.conf"))
    expected = 2 if name == "attoparsec" else 1
    if len(confs) != expected:
        raise RuntimeError(f"expected {expected} registrations for {top_name}, got {confs}")
    if name == "attoparsec":
        confs.sort(key=lambda conf: "attoparsec-internal" not in conf.name)
    for conf in confs:
        registration = conf.read_text()
        dependencies = set(field(registration, "depends").split())
        unknown = dependencies - available_target_ids()
        if unknown:
            raise RuntimeError(f"dependency not present in target databases: {sorted(unknown)}")
        component = (build_root / "build/aarch64-sel4/ghc-9.6.6" / top_name)
        if name == "attoparsec" and "attoparsec-internal" in conf.name:
            component = component / "l/attoparsec-internal/build/attoparsec-internal"
        else:
            component = component / "build"
        count = audit_objects(component)
        run(CROSS_PKG, "--no-user-package-db", f"--package-db={TARGET_DB}",
            f"--package-db={GUEST_DB}", "update", conf)
        print(f"Registered {conf.name}: {count} AArch64 objects; target dependencies: "
              + ", ".join(sorted(dependencies)))


if __name__ == "__main__":
    main()
