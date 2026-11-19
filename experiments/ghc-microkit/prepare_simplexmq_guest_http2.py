#!/usr/bin/env python3
"""Stage the upstream HTTP/2 source with its POSIX convenience layer fenced out."""

import re
import shutil
import tarfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
ARCHIVE = (ROOT / ".cache/simplex-toolchain/cabal/packages/hackage.haskell.org"
          / "http2/4.2.2/http2-4.2.2.tar.gz")
STAGE = ROOT / ".cache/guest-http2-microkit/http2-4.2.2"


def prepare() -> Path:
    if not ARCHIVE.is_file():
        raise FileNotFoundError(f"missing pinned HTTP/2 archive: {ARCHIVE}")
    if STAGE.exists():
        shutil.rmtree(STAGE)
    STAGE.parent.mkdir(parents=True, exist_ok=True)
    with tarfile.open(ARCHIVE, "r:gz") as archive:
        members = archive.getmembers()
        for member in members:
            target = (STAGE.parent / member.name).resolve()
            if not target.is_relative_to(STAGE.parent.resolve()):
                raise RuntimeError(f"unsafe path in HTTP/2 source archive: {member.name}")
            if not (member.isdir() or member.isfile()):
                raise RuntimeError(f"unsupported entry in HTTP/2 source archive: {member.name}")
        archive.extractall(STAGE.parent, members=members)
    source = STAGE / "Network/HTTP2/Arch/Config.hs"
    text = source.read_text()
    text = text.replace("module Network.HTTP2.Arch.Config where\n",
                        "{-# LANGUAGE CPP #-}\nmodule Network.HTTP2.Arch.Config where\n", 1)
    text = text.replace("import Network.Socket\nimport Network.Socket.ByteString (sendAll)\n",
                        "import Network.Socket (SockAddr)\n", 1)
    pattern = re.compile(
        r"(?ms)^-- \| Making simple configuration.*?^-- \| Deallocating the resource of the simple configuration\."
    )
    match = pattern.search(text)
    if not match:
        raise RuntimeError("upstream HTTP/2 socket helper block changed")
    block = match.group(0)
    marker = "-- | Deallocating the resource of the simple configuration."
    host, rest = block.split(marker, 1)
    text = text[:match.start()] + "#if !defined(SIMPLEX_MICROKIT)\n" + host \
        + "#endif\n\n" + marker + rest + text[match.end():]
    source.write_text(text)
    client = STAGE / "Network/HTTP2/Client.hs"
    client_text = client.read_text()
    client_text = client_text.replace(
        "{-# LANGUAGE OverloadedStrings #-}\n",
        "{-# LANGUAGE CPP #-}\n{-# LANGUAGE OverloadedStrings #-}\n", 1)
    for symbol in ("allocSimpleConfig", "defaultReadN"):
        line = f"  , {symbol}\n"
        if client_text.count(line) != 1:
            raise RuntimeError(f"upstream HTTP/2 client export changed: {symbol}")
        client_text = client_text.replace(
            line, f"#if !defined(SIMPLEX_MICROKIT)\n{line}#endif\n", 1)
    client.write_text(client_text)
    return STAGE


if __name__ == "__main__":
    print(prepare())
