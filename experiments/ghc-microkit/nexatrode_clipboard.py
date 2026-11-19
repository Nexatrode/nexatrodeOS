#!/usr/bin/env python3
"""Copy and paste between the Mac clipboard and a NexatrodeOS UTM VM.

Paste (Mac to guest): Command-V while UTM is the front app types the clipboard
into the virtual machine. The guest puts those keys in the open message or
invitation field.

Copy (guest to Mac): Command-C in the guest prints the active field on its
serial port. This script reads that port and puts the text on the Mac clipboard.

The event tap needs Accessibility permission for the program that launches
this script. If the tap cannot start, run `nexatrode_clipboard.py --paste`
after copying on the Mac, with the NexatrodeOS window in front.
"""

from __future__ import annotations

import argparse
import ctypes
import os
import select
import socket
import subprocess
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BUNDLE = ROOT / "dist" / "NexatrodeOS-Messages.utm"
ALLOW_SERIAL_TCP_FALLBACK = True
UTM_BUNDLE_ID = "com.utmapp.UTM"
SHIFT_FLAG = 0x00020000
SHIFT_KEY = 0x38

# Mac virtual keycodes. Shifted characters use the unshifted key plus Shift.
PLAIN = {
    "a": 0x00, "s": 0x01, "d": 0x02, "f": 0x03, "h": 0x04, "g": 0x05,
    "z": 0x06, "x": 0x07, "c": 0x08, "v": 0x09, "b": 0x0B, "q": 0x0C,
    "w": 0x0D, "e": 0x0E, "r": 0x0F, "y": 0x10, "t": 0x11, "1": 0x12,
    "2": 0x13, "3": 0x14, "4": 0x15, "6": 0x16, "5": 0x17, "=": 0x18,
    "9": 0x19, "7": 0x1A, "-": 0x1B, "8": 0x1C, "0": 0x1D, "]": 0x1E,
    "o": 0x1F, "u": 0x20, "[": 0x21, "i": 0x22, "p": 0x23, "\n": 0x24,
    "\r": 0x24, "l": 0x25, "j": 0x26, "'": 0x27, "k": 0x28, ";": 0x29,
    "\\": 0x2A, ",": 0x2B, "/": 0x2C, "n": 0x2D, "m": 0x2E, ".": 0x2F,
    "`": 0x32, " ": 0x31,
}
SHIFTED = {
    "!": "1", "@": "2", "#": "3", "$": "4", "%": "5", "^": "6", "&": "7",
    "*": "8", "(": "9", ")": "0", "_": "-", "+": "=", "{": "[", "}": "]",
    ":": ";", '"': "'", "~": "`", "|": "\\", "<": ",", ">": ".", "?": "/",
}


def post_key(quartz, keycode: int, down: bool, flags: int) -> None:
    quartz.CGEventCreateKeyboardEvent.restype = ctypes.c_void_p
    quartz.CGEventCreateKeyboardEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint16, ctypes.c_bool]
    quartz.CGEventSetFlags.argtypes = [ctypes.c_void_p, ctypes.c_uint64]
    quartz.CGEventPost.argtypes = [ctypes.c_uint32, ctypes.c_void_p]
    event = quartz.CGEventCreateKeyboardEvent(None, keycode, down)
    quartz.CGEventSetFlags(event, flags)
    quartz.CGEventPost(0, event)
    core = ctypes.cdll.LoadLibrary("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation")
    core.CFRelease.argtypes = [ctypes.c_void_p]
    core.CFRelease(event)


def type_character(quartz, character: str) -> None:
    key = PLAIN.get(character)
    shifted = False
    if key is None and character.isascii() and character.isalpha() and character.isupper():
        key = PLAIN.get(character.lower())
        shifted = True
    if key is None and character in SHIFTED:
        key = PLAIN.get(SHIFTED[character])
        shifted = True
    if key is None:
        return
    flags = SHIFT_FLAG if shifted else 0
    if shifted:
        post_key(quartz, SHIFT_KEY, True, SHIFT_FLAG)
    post_key(quartz, key, True, flags)
    post_key(quartz, key, False, flags)
    if shifted:
        post_key(quartz, SHIFT_KEY, False, 0)
    time.sleep(0.004)


def paste_text(text: str) -> None:
    quartz = ctypes.cdll.LoadLibrary("/System/Library/Frameworks/Quartz.framework/Quartz")
    clipped = text[:2048]
    for character in clipped:
        type_character(quartz, character)
    print(f"pasted {len(clipped)} characters", flush=True)


def mac_clipboard() -> str:
    return subprocess.run(["pbpaste"], check=False, capture_output=True, text=True).stdout


def set_mac_clipboard(text: str) -> None:
    subprocess.run(["pbcopy"], input=text, text=True, check=False)
    print(f"copied {len(text)} characters to the Mac clipboard", flush=True)


def unescape(text: str) -> str:
    out: list[str] = []
    index = 0
    while index < len(text):
        if text[index] == "\\" and index + 1 < len(text):
            if text[index + 1] == "n":
                out.append("\n")
                index += 2
                continue
            if text[index + 1] == "\\":
                out.append("\\")
                index += 2
                continue
        out.append(text[index])
        index += 1
    return "".join(out)


def qemu_pid() -> int | None:
    result = subprocess.run(["pgrep", "-f", "NexatrodeOS-Messages.utm"], capture_output=True, text=True)
    pids = [int(line) for line in result.stdout.split() if line.isdigit()]
    return pids[0] if pids else None


SERIAL_PORT = 44555


def serial_device() -> str | None:
    log = BUNDLE / "Data" / "debug.log"
    device = None
    if log.is_file():
        for line in log.read_text(errors="replace").splitlines():
            marker = "char device redirected to "
            if marker in line:
                device = line.split(marker, 1)[1].split()[0]
    if device and Path(device).exists():
        return device
    return None


class CopyFrames:
    """Bounded line-oriented serial parser; never writes guest text to disk."""
    def __init__(self) -> None:
        self.pending = b""
        self.parts: list[str] = []
        self.active = False
        self.expected = 0

    def feed(self, chunk: bytes) -> list[str]:
        result: list[str] = []
        lines = (self.pending + chunk).split(b"\n")
        self.pending = lines.pop()
        if len(self.pending) > 8192:
            self.pending = b""
            self.parts.clear()
            self.active = False
        for raw in lines:
            line = raw.decode("utf-8", errors="replace").rstrip("\r")
            if line.startswith("NEXATRODE_COPY|BEGIN|"):
                self.parts.clear()
                size = line.removeprefix("NEXATRODE_COPY|BEGIN|")
                self.expected = int(size) if size.isascii() and size.isdigit() and len(size) <= 5 else -1
                self.active = 0 <= self.expected <= 8192
            elif self.active and line.startswith("NEXATRODE_COPY|DATA|"):
                self.parts.append(line.split("|", 2)[2])
                if sum(map(len, self.parts)) > 8192:
                    self.parts.clear()
                    self.active = False
            elif self.active and line == "NEXATRODE_COPY|END":
                joined = "".join(self.parts)
                if len(joined.encode("utf-8")) == self.expected:
                    result.append(unescape(joined))
                self.parts.clear()
                self.active = False
        return result


def watch_serial(stop: threading.Event) -> None:
    frames = CopyFrames()
    opened = None
    handle = None
    while not stop.is_set():
        device = serial_device()
        if handle is not None and device and device != opened:
            os.close(handle)
            handle = None
            opened = None
            frames = CopyFrames()
        if handle is None:
            if device and Path(device).exists():
                try:
                    handle = os.open(device, os.O_RDONLY | os.O_NONBLOCK)
                    opened = device
                    print(f"reading guest serial {device}", flush=True)
                    continue
                except OSError as error:
                    print(f"serial open failed: {error}", flush=True)
                    handle = None
                    opened = None
            if not ALLOW_SERIAL_TCP_FALLBACK:
                time.sleep(1)
                continue
            try:
                sock = socket.create_connection(("127.0.0.1", SERIAL_PORT), timeout=1)
                sock.setblocking(False)
                handle = sock.detach()
                opened = f"tcp:{SERIAL_PORT}"
                print(f"reading guest serial tcp {SERIAL_PORT}", flush=True)
                continue
            except OSError:
                time.sleep(1)
                continue
        ready, _, _ = select.select([handle], [], [], 1)
        if not ready:
            continue
        try:
            chunk = os.read(handle, 4096)
        except OSError:
            os.close(handle)
            handle = None
            opened = None
            frames = CopyFrames()
            continue
        if not chunk:
            os.close(handle)
            handle = None
            opened = None
            frames = CopyFrames()
            continue
        # Serial chunks need not end at a line boundary. Never persist raw
        # guest serial: it can contain private application data.
        for text in frames.feed(chunk):
            set_mac_clipboard(text)


def utm_is_front() -> bool:
    try:
        front = subprocess.run(["lsappinfo", "front"], capture_output=True, text=True, check=False).stdout.strip()
        info = subprocess.run(
            ["lsappinfo", "info", "-only", "bundleid", front],
            capture_output=True, text=True, check=False,
        ).stdout
        return UTM_BUNDLE_ID in info
    except Exception:
        return False


def start_event_tap(stop: threading.Event) -> bool:
    import ctypes

    quartz = ctypes.cdll.LoadLibrary("/System/Library/Frameworks/Quartz.framework/Quartz")
    core = ctypes.cdll.LoadLibrary("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation")
    quartz.CGEventGetFlags.argtypes = [ctypes.c_void_p]
    quartz.CGEventGetFlags.restype = ctypes.c_uint64
    quartz.CGEventGetIntegerValueField.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
    quartz.CGEventGetIntegerValueField.restype = ctypes.c_int64
    quartz.CGEventTapEnable.argtypes = [ctypes.c_void_p, ctypes.c_bool]
    callback_type = ctypes.CFUNCTYPE(
        ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint32, ctypes.c_void_p, ctypes.c_void_p
    )

    def on_key(_proxy, event_type, event, _user):
        if event_type in (0xFFFFFFFE, 0xFFFFFFFF):
            quartz.CGEventTapEnable(tap, True)
            return event
        if event_type != 10:
            return event
        flags = quartz.CGEventGetFlags(event)
        if not flags & 0x00100000:
            return event
        keycode = quartz.CGEventGetIntegerValueField(event, 9)
        if keycode == 9 and utm_is_front():
            threading.Thread(target=lambda: paste_text(mac_clipboard()), daemon=True).start()
            return None
        return event

    holder = callback_type(on_key)
    quartz.CGEventTapCreate.restype = ctypes.c_void_p
    quartz.CGEventTapCreate.argtypes = [
        ctypes.c_uint32, ctypes.c_uint32, ctypes.c_uint32, ctypes.c_uint64,
        callback_type, ctypes.c_void_p,
    ]
    tap = quartz.CGEventTapCreate(0, 0, 0, 1 << 10, holder, None)
    if not tap:
        return False
    core.CFMachPortCreateRunLoopSource.restype = ctypes.c_void_p
    core.CFMachPortCreateRunLoopSource.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_long]
    source = core.CFMachPortCreateRunLoopSource(None, tap, 0)
    core.CFRunLoopGetCurrent.restype = ctypes.c_void_p
    loop = core.CFRunLoopGetCurrent()
    mode = ctypes.c_void_p.in_dll(core, "kCFRunLoopCommonModes")
    core.CFRunLoopAddSource.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p]
    core.CFRunLoopAddSource(loop, source, mode)
    quartz.CGEventTapEnable(tap, True)
    start_event_tap.holder = holder  # keep the callback alive
    core.CFRunLoopRun()
    stop.set()
    return True


def main() -> None:
    global BUNDLE, ALLOW_SERIAL_TCP_FALLBACK
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path, help="UTM bundle whose guest serial supplies copied text")
    parser.add_argument("--paste", action="store_true", help="paste the Mac clipboard once and exit")
    args = parser.parse_args()
    if args.bundle is not None:
        BUNDLE = args.bundle.expanduser().resolve()
        if not BUNDLE.is_dir() or BUNDLE.suffix != ".utm":
            parser.error("--bundle must name an existing .utm bundle")
        ALLOW_SERIAL_TCP_FALLBACK = False
    if args.paste:
        paste_text(mac_clipboard())
        return
    stop = threading.Event()
    threading.Thread(target=watch_serial, args=(stop,), daemon=True).start()
    print("watching the guest serial for copies", flush=True)
    if not start_event_tap(stop):
        print(
            "Command-V paste needs Accessibility permission for this program. "
            "Allow it in System Settings, or run nexatrode_clipboard.py --paste "
            "after copying on the Mac.",
            flush=True,
        )
        while not stop.is_set():
            time.sleep(1)


if __name__ == "__main__":
    main()
