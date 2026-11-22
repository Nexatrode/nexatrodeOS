#!/usr/bin/env python3
"""Prepare an isolated, throwaway upstream SMP server under ignored .cache."""

import os
from pathlib import Path
import re
import subprocess
import sys


def main() -> None:
    server_bin = Path(sys.argv[1]).resolve()
    root = Path(sys.argv[2]).resolve()
    etc = root / "etc"
    var = root / "var"
    env = os.environ.copy()
    env["SMP_SERVER_CFG_PATH"] = str(etc)
    env["SMP_SERVER_LOG_PATH"] = str(var)
    ini = etc / "smp-server.ini"
    if not ini.exists():
        subprocess.run(
            [str(server_bin), "init", "--no-password", "--ip", "127.0.0.1",
             "--disable-web", "--disable-store-log", "--yes"],
            env=env, check=True, stdout=subprocess.DEVNULL,
        )
    text = ini.read_text()
    text, host_count = re.subn(r"(?m)^host = .*?$", "host = 127.0.0.1", text)
    text, port_count = re.subn(r"(?m)^port = .*?$", "port = 65224", text)
    if host_count != 1 or port_count != 1:
        raise SystemExit("unexpected SMP fixture configuration")
    ini.write_text(text)
    subprocess.run(
        ["openssl", "x509", "-in", str(etc / "ca.crt"), "-outform", "DER",
         "-out", str(root / "ca.der")], check=True,
    )
    print("Isolated QEMU SMP fixture ready")


if __name__ == "__main__":
    main()
