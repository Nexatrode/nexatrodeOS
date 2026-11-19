#!/usr/bin/env python3
"""Apply and typecheck the pinned SimpleXMQ TLS-backend source patch.

The existing Cabal source-repository package is immutable to Cabal's build
planner. Check the transport modules against the built host package, then
compile the upstream client and its source dependency closure together so
their shared types have one identity.
"""

import re
import shutil
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SOURCE = (ROOT / ".cache/simplex-chat-upstream/dist-newstyle/src"
          / "simplexmq-f058c38b0eecb6c0")
REVISION = "efaad8e73436d60f5052f07dda6b71151ad5039b"
PATCH = ROOT / "patches/simplexmq-microkit-tls-backend.patch"
BUILD = (ROOT / ".cache/simplex-chat-upstream/dist-newstyle/build/aarch64-osx"
         / "ghc-9.6.3/simplexmq-7.0.0.6/build/autogen")
CONF = (ROOT / ".cache/simplex-chat-upstream/dist-newstyle/packagedb"
        / "ghc-9.6.3/simplexmq-7.0.0.6-inplace.conf")
STORE_DB = ROOT / ".cache/simplex-toolchain/cabal/store/ghc-9.6.3/package.db"
PROJECT_DB = CONF.parent
GHC = ROOT / ".cache/simplex-toolchain/.ghcup/ghc/9.6.3/bin/ghc"
STAGE = ROOT / ".cache/simplexmq-backend-check/src"
FULL_STAGE = ROOT / ".cache/simplexmq-backend-check/full-src"
MODULES = ("Simplex/Messaging/Transport.hs",
           "Simplex/Messaging/Transport/Client.hs")


def run(*args: object, **kwargs: object) -> subprocess.CompletedProcess:
    return subprocess.run([str(arg) for arg in args], check=True, **kwargs)


def conf_field(contents: str, name: str) -> str:
    match = re.search(rf"^{re.escape(name)}:\s*(.*?)(?=^\S|\Z)",
                      contents, re.MULTILINE | re.DOTALL)
    if not match:
        raise RuntimeError(f"missing {name} in {CONF}")
    return " ".join(match.group(1).split())


def ensure_patch_applied() -> None:
    revision = subprocess.check_output(
        ["git", "-C", str(SOURCE), "rev-parse", "HEAD"], text=True).strip()
    if revision != REVISION:
        raise RuntimeError(f"unexpected SimpleXMQ revision: {revision}")
    reverse = subprocess.run(["git", "-C", str(SOURCE), "apply", "--reverse",
                              "--check", str(PATCH)], capture_output=True)
    if reverse.returncode != 0:
        run("git", "-C", SOURCE, "apply", "--check", PATCH)
        run("git", "-C", SOURCE, "apply", PATCH)


def stage_full_source() -> None:
    # The pinned checkout predates two rank-2 GHC 9.6 compatibility fixes in
    # Crypto.hs. Apply them only to the throwaway verification copy; they are
    # unrelated to the transport patch and are never written into SOURCE.
    shutil.rmtree(FULL_STAGE, ignore_errors=True)
    shutil.copytree(SOURCE / "src", FULL_STAGE)
    crypto = FULL_STAGE / "Simplex/Messaging/Crypto.hs"
    contents = crypto.read_text()
    fixes = (("  toPubKey = id", "  toPubKey f k = f k"),
             ("  toPrivKey = id", "  toPrivKey f k = f k"))
    for old, new in fixes:
        if contents.count(old) != 1:
            raise RuntimeError(f"expected one compatibility site: {old}")
        contents = contents.replace(old, new)
    crypto.write_text(contents)


def main() -> None:
    ensure_patch_applied()
    for name in MODULES:
        target = STAGE / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(SOURCE / "src" / name, target)
    contents = CONF.read_text()
    dependencies = conf_field(contents, "depends").split()
    unit = conf_field(contents, "id")
    args = [GHC, "-fno-code", "-fforce-recomp", "-v0",
            "-hide-all-packages", "-no-user-package-db",
            "-package-db", STORE_DB, "-package-db", PROJECT_DB,
            f"-i{STAGE}", f"-i{BUILD}", "-XStrictData"]
    for dep in (*dependencies, unit):
        args.extend(("-package-id", dep))
    args.append(STAGE / MODULES[1])
    run(*args)
    print(f"Pinned SimpleXMQ transport modules typecheck ({len(dependencies)} dependencies)")

    stage_full_source()
    full_args = [GHC, "-fno-code", "-fforce-recomp", "-v0",
                 "-hide-all-packages", "-no-user-package-db",
                 "-package-db", STORE_DB, "-package-db", PROJECT_DB,
                 f"-i{FULL_STAGE}", f"-i{BUILD}", "-XStrictData"]
    for dep in dependencies:
        full_args.extend(("-package-id", dep))
    full_args.append(FULL_STAGE / "Simplex/Messaging/Client.hs")
    run(*full_args)
    print("Pinned SimpleXMQ Client.hs and source dependency closure typecheck")
    guest_args = full_args.copy()
    guest_shims = ROOT / "experiments/ghc-microkit/guest-client-shims"
    guest_args.insert(guest_args.index(f"-i{FULL_STAGE}"), f"-i{guest_shims}")
    run(*guest_args)
    print("Pinned SimpleXMQ Client.hs typechecks with guest concurrency adapters")


if __name__ == "__main__":
    main()
