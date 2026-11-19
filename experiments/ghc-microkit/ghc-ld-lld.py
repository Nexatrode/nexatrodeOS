#!/usr/bin/env python3
"""Adapt GHC's compiler-style linker arguments to the direct LLD executable."""

from __future__ import annotations

import os
import shlex
import shutil
import sys


def expand_response_files(arguments: list[str]) -> list[str]:
    expanded: list[str] = []
    for argument in arguments:
        if argument.startswith("@"):
            with open(argument[1:], encoding="utf-8") as response_file:
                expanded.extend(shlex.split(response_file.read()))
        else:
            expanded.append(argument)
    return expanded


def main() -> None:
    linker = os.environ.get("LLD") or shutil.which("ld.lld")
    if not linker:
        raise SystemExit("set LLD or put ld.lld on PATH")

    arguments = expand_response_files(sys.argv[1:])
    direct_arguments: list[str] = []
    for argument in arguments:
        if argument.startswith("-Wl,"):
            direct_arguments.extend(argument[4:].split(","))
        else:
            direct_arguments.append(argument)

    # Keep linker-specific diagnostics configurable without teaching GHC's
    # compiler driver about direct-LLD-only options such as --why-extract.
    direct_arguments.extend(shlex.split(os.environ.get("GHC_LLD_EXTRA_ARGS", "")))

    # This diagnostic-only mode produces an ELF with unresolved symbols so the
    # full missing-import set can be collected before the target C runtime is
    # implemented. Such an image is not runnable.
    if os.environ.get("GHC_LLD_ALLOW_UNRESOLVED") == "1":
        direct_arguments.insert(0, "--noinhibit-exec")

    os.execv(linker, [linker, *direct_arguments])


if __name__ == "__main__":
    main()
