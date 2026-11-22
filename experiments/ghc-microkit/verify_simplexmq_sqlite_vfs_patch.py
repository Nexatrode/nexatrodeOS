#!/usr/bin/env python3
"""Apply and typecheck the pinned SimpleXMQ SQLite VFS integration seam."""

import subprocess
import sys
from pathlib import Path

from verify_simplexmq_backend_patch import (
    BUILD,
    CONF,
    FULL_STAGE,
    GHC,
    PROJECT_DB,
    REVISION,
    SOURCE,
    STORE_DB,
    conf_field,
    ensure_patch_applied,
    run,
    stage_full_source,
)


ROOT = Path(__file__).resolve().parents[2]
PATCH = ROOT / "patches/simplexmq-microkit-sqlite-vfs.patch"


def ensure_store_patch_applied() -> None:
    revision = subprocess.check_output(
        ["git", "-C", str(SOURCE), "rev-parse", "HEAD"], text=True
    ).strip()
    if revision != REVISION:
        raise RuntimeError(f"unexpected SimpleXMQ revision: {revision}")
    reverse = subprocess.run(
        ["git", "-C", str(SOURCE), "apply", "--reverse", "--check", str(PATCH)],
        capture_output=True,
    )
    if reverse.returncode != 0:
        run("git", "-C", SOURCE, "apply", "--check", PATCH)
        run("git", "-C", SOURCE, "apply", PATCH)


def typecheck_store(target: bool) -> None:
    dependencies = conf_field(CONF.read_text(), "depends").split()
    unit = conf_field(CONF.read_text(), "id")
    args = [
        GHC,
        "-fno-code",
        "-fforce-recomp",
        "-v0",
        "-hide-all-packages",
        "-no-user-package-db",
        "-package-db",
        STORE_DB,
        "-package-db",
        PROJECT_DB,
        f"-i{FULL_STAGE}",
        f"-i{BUILD.parent}",
        f"-i{BUILD}",
        "-XStrictData",
    ]
    if target:
        args.append(f"-i{ROOT / 'experiments/ghc-microkit'}")
    for dependency in (*dependencies, unit):
        args.extend(("-package-id", dependency))
    if target:
        args.append("-DSIMPLEX_MICROKIT")
    args.append(FULL_STAGE / "Simplex/Messaging/Agent/Store/SQLite.hs")
    run(*args)


def main() -> None:
    ensure_patch_applied()
    ensure_store_patch_applied()
    stage_full_source()
    typecheck_store(target=False)
    print("Pinned SimpleXMQ SQLite store typechecks with its host filesystem path")
    typecheck_store(target=True)
    print("Pinned SimpleXMQ SQLite store typechecks with the named Microkit VFS path")


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        sys.exit(error.returncode)
