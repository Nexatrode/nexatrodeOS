#!/usr/bin/env python3
"""Disposable macOS host experiment; not a general application sandbox.

Two test processes use the same calculator model. Seatbelt explicitly denies
each process access to its sibling's fixture directory. This targeted check
does not deny other host access, and proves no seL4 or production security.
"""
import json
from pathlib import Path
import selectors
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def reply(process, command=None):
    if command is not None:
        process.stdin.write(command + "\n")
        process.stdin.flush()
    with selectors.DefaultSelector() as selector:
        selector.register(process.stdout, selectors.EVENT_READ)
        if not selector.select(timeout=10):
            raise RuntimeError("fixture did not respond")
    result = process.stdout.readline().strip()
    if not result:
        raise RuntimeError("fixture exited before replying")
    return result


def expect(actual, expected):
    if actual != expected:
        raise AssertionError(f"expected {expected!r}, got {actual!r}")


def main():
    if sys.platform != "darwin" or not Path("/usr/bin/sandbox-exec").is_file():
        raise SystemExit("This host-only test requires macOS sandbox-exec")
    with tempfile.TemporaryDirectory(prefix="nexatrode-desktop-fixture-") as name:
        root = Path(name).resolve()
        one, two = root / "one", root / "two"
        one.mkdir(); two.mkdir()
        (one / "private.txt").write_text("ONE", encoding="utf-8")
        (two / "private.txt").write_text("TWO", encoding="utf-8")
        binary = root / "calculator-fixture"
        subprocess.run(["rustc", "--edition", "2024", str(ROOT / "tests/desktop_calculator_fixture.rs"),
                        "-o", str(binary)], check=True, timeout=120)

        # A control proves the test account can read both files without a policy.
        control = subprocess.run([str(binary)], input=f"READ {two / 'private.txt'}\nEXIT\n",
                                 text=True, capture_output=True, check=True, timeout=10)
        if "DATA TWO\n" not in control.stdout:
            raise RuntimeError("unrestricted control could not read fixture data")

        processes = []
        try:
            for denied in (two, one):
                profile = f"(version 1) (allow default) (deny file-read* file-write* (subpath {json.dumps(str(denied))}))"
                process = subprocess.Popen(["/usr/bin/sandbox-exec", "-p", profile, str(binary)],
                                           stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                           stderr=subprocess.PIPE, text=True, bufsize=1)
                processes.append(process)
                expect(reply(process), "READY")
            first, second = processes
            for process, own, other, value in ((first, one, two, "ONE"), (second, two, one, "TWO")):
                expect(reply(process, f"READ {own / 'private.txt'}"), f"DATA {value}")
                expect(reply(process, f"READ {other / 'private.txt'}"), "READ_ERROR PermissionDenied")
            for command, result in (("7", "7"), ("+", "7"), ("5", "5"), ("=", "12")):
                expect(reply(first, command), f"DISPLAY {result}")
            expect(reply(second, "2"), "DISPLAY 2")
            expect(reply(first, "?"), "DISPLAY 12")
            for process in processes:
                expect(reply(process, "EXIT"), "BYE")
                expect(process.wait(timeout=10), 0)
            print("PASS: independent calculator processes, targeted sibling-file denial, and clean exit")
        finally:
            for process in processes:
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=10)
                for stream in (process.stdin, process.stdout, process.stderr):
                    if stream is not None: stream.close()


if __name__ == "__main__":
    main()
