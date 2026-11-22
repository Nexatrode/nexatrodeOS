"""Boot an ephemeral HVF affinity probe. Verify shared progress and tagged registers on each vCPU."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import socket
import subprocess
import sys
import tempfile
import time


def qmp(stream, command, **arguments):
    request = {"execute": command, "arguments": arguments}
    stream.write((json.dumps(request) + "\n").encode())
    while True:
        data = stream.readline()
        if not data:
            raise RuntimeError("QMP closed")
        response = json.loads(data)
        if "error" in response:
            raise RuntimeError(response["error"])
        if "return" in response:
            return response["return"]


def await_progress(serial, cores, deadline, process):
    # Only accept a complete, newline-terminated report with a positive delta on every secondary.
    while process.poll() is None and time.monotonic() < deadline:
        if serial.exists():
            log = serial.read_text(errors="replace")
            ready = re.search(rf"^SMP\|READY cores={cores} ([^\n]*)\n", log, re.M)
            if ready:
                deltas = {int(i): int(t) for i, t in re.findall(r"\bdelta(\d+)=(\d+)", ready[1])}
                frequency = re.search(r"\bfrequency=(\d+)", ready[1])
                return deltas, int(frequency[1]) if frequency else None
            if "not found" in log and "GICR base" in log:
                break
        time.sleep(0.05)
    return {}, None


def register_sample(stream, out, core):
    registers = qmp(stream, "human-monitor-command", **{"command-line": "info registers", "cpu-index": core})
    (out / f"cpu{core}.regs").write_text(registers)
    return {reg: int(re.search(rf"\b{reg}=\s*([a-f0-9]+)", registers, re.I)[1], 16)
            for reg in ("PC", "X25", "X26")}


def run(args, out):
    serial = out / "serial.log"
    image = args.image.resolve()
    result = {"cores": args.cores, "irqchip": args.irqchip, "accel": "hvf",
              "qemu_version": subprocess.check_output([args.qemu, "--version"], text=True).splitlines()[0],
              "image_sha256": hashlib.sha256(image.read_bytes()).hexdigest()}
    with tempfile.TemporaryDirectory(prefix="hvfsmp-") as tmp, (out / "qemu.log").open("wb") as errors:
        sock = Path(tmp) / "qmp"
        cmd = [args.qemu, "-machine", f"virt,virtualization=off,secure=off,gic-version=3,kernel-irqchip={args.irqchip}",
               "-accel", "hvf", "-cpu", "host", "-smp", f"{args.cores},sockets=1,cores={args.cores},threads=1",
               "-m", "2048", "-nodefaults", "-display", "none", "-monitor", "none", "-nic", "none",
               "-serial", f"file:{serial}", "-qmp", f"unix:{sock},server=on,wait=off",
               "-device", f"loader,file={image},addr=0x70000000,cpu-num=0"]
        result["command"] = cmd
        process = subprocess.Popen(cmd, stdout=errors, stderr=errors)
        try:
            deadline = time.monotonic() + args.timeout
            while not sock.exists() and process.poll() is None and time.monotonic() < deadline:
                time.sleep(0.02)
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
                client.settimeout(3)
                client.connect(str(sock))
                with client.makefile("rwb", buffering=0) as stream:
                    greeting = json.loads(stream.readline())
                    if "QMP" not in greeting:
                        raise RuntimeError("Missing QMP greeting")
                    qmp(stream, "qmp_capabilities")
                    deltas, frequency = await_progress(serial, args.cores, deadline, process)
                    result["progress_ticks"] = deltas
                    result["timer_hz"] = frequency
                    ids = sorted(cpu["cpu-index"] for cpu in qmp(stream, "query-cpus-fast"))
                    result["cpu_indices"] = ids
                    progress = all(deltas.get(i, 0) > 0 for i in range(1, args.cores))
                    tagged = False
                    for _ in range(12 if progress else 1):
                        time.sleep(0.01)
                        qmp(stream, "stop")
                        samples = {i: register_sample(stream, out, i) for i in ids}
                        result["samples"] = samples
                        tagged = ids == list(range(args.cores)) and all(
                            reg["X25"] == 0x534d5000 + i for i, reg in samples.items())
                        if tagged or not progress:
                            break
                        qmp(stream, "cont")
                    result["pass"] = progress and frequency == 24_000_000 and tagged
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
            (out / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    return result["pass"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("cores", type=int, choices=(2, 4))
    parser.add_argument("image", type=Path)
    parser.add_argument("--irqchip", choices=("on", "off"), default="on")
    parser.add_argument("--timeout", type=float, default=15)
    parser.add_argument("--qemu", default="qemu-system-aarch64")
    parser.add_argument("--output", type=Path, required=True, help="new directory for serial, QMP and JSON")
    args = parser.parse_args()
    out = args.output.resolve()
    if out.exists():
        parser.error(f"Output already exists at {out}. Supply a new directory.")
    out.mkdir(parents=True)
    passed = run(args, out)
    print(f"{'PASS' if passed else 'FAIL'}  {out / 'result.json'}")
    sys.exit(0 if passed else 1)


if __name__ == "__main__":
    main()
