"""USB-only host helpers for the temporary Sunfish Linux comparison."""

from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[3]
CACHE = ROOT / ".cache/sunfish-mobian-comparison"


def interface():
    current = None
    for line in subprocess.check_output(["ifconfig", "-a"], text=True).splitlines():
        if line and not line[0].isspace():
            current = line.split(":", 1)[0]
        elif "ether 02:00:00:42:01:02" in line:
            if not re.fullmatch(r"[A-Za-z0-9]+", current or ""):
                raise RuntimeError("Unexpected comparison USB interface")
            return current
    raise RuntimeError("Comparison USB network is not present")


def run(script, *, data=None, timeout=30, check=True, known_hosts=None, first_contact=False):
    hosts = known_hosts or CACHE / "comparison-fixed-known-hosts"
    command = [
        "ssh", "-F", "/dev/null", "-6", "-i", str(CACHE / "comparison-key"),
        "-o", "IdentitiesOnly=yes", "-o", "BatchMode=yes", "-o", "ConnectTimeout=8",
        "-o", f"StrictHostKeyChecking={'accept-new' if first_contact else 'yes'}",
        "-o", f"UserKnownHostsFile={hosts}",
        f"root@fe80::42:1%{interface()}", script,
    ]
    return subprocess.run(command, input=data, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE, timeout=timeout, check=check)


def verify(**ssh_options):
    result = run("uname -r; id -u; tr '\\000' '\\n' </proc/device-tree/compatible", **ssh_options)
    lines = result.stdout.decode().splitlines()
    if lines[:2] != ["7.1.0-sm7150+", "0"] or "google,sunfish" not in lines:
        raise RuntimeError("USB peer is not the expected rooted Sunfish reference kernel")
    return lines
