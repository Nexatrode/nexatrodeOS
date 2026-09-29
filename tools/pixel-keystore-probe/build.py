#!/usr/bin/env python3
"""Build and optionally run the temporary Android StrongBox probe."""

import argparse
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time
import uuid
import zipfile


ROOT = Path(__file__).resolve().parent
PACKAGE = "org.personal.os.pixelprobe"
ACTIVITY = PACKAGE + "/.MainActivity"
ANDROID_HOME_DEFAULT = Path.home() / "Library" / "Android" / "sdk"


def run(args):
    subprocess.run([str(arg) for arg in args], check=True)


def sdk_root():
    configured = os.environ.get("ANDROID_SDK_ROOT") or os.environ.get("ANDROID_HOME")
    path = Path(configured).expanduser() if configured else ANDROID_HOME_DEFAULT
    if not path.is_dir():
        raise RuntimeError(f"Android SDK not found at {path}")
    return path


def numeric_versions(paths):
    versions = []
    for path in paths:
        match = re.fullmatch(r"android-(\d+)", path.name)
        if match:
            versions.append((int(match.group(1)), path))
    return sorted(versions)


def build_apk():
    sdk = sdk_root()
    platforms = numeric_versions((sdk / "platforms").glob("android-*"))
    build_tools = sorted(
        (path for path in (sdk / "build-tools").iterdir() if path.is_dir()),
        key=lambda path: tuple(int(part) for part in path.name.split(".") if part.isdigit()),
    )
    if not platforms or not build_tools:
        raise RuntimeError("Android SDK platform and build-tools packages are required")

    android_jar = platforms[-1][1] / "android.jar"
    tools = build_tools[-1]
    required = ["aapt2", "d8", "zipalign", "apksigner"]
    missing = [name for name in required if not (tools / name).exists()]
    if missing:
        raise RuntimeError(f"Missing Android build tools: {', '.join(missing)}")

    output = ROOT / "build"
    classes = output / "classes"
    dex = output / "dex"
    output.mkdir(parents=True, exist_ok=True)
    classes.mkdir(exist_ok=True)
    dex.mkdir(exist_ok=True)

    java_files = sorted((ROOT / "src").rglob("*.java"))
    run([
        "javac", "--release", "11", "-classpath", android_jar,
        "-d", classes, *java_files,
    ])
    program_jar = output / "classes.jar"
    with zipfile.ZipFile(program_jar, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for class_file in classes.rglob("*.class"):
            archive.write(class_file, class_file.relative_to(classes))
    run([
        tools / "d8", "--lib", android_jar, "--min-api", "28",
        "--output", dex, program_jar,
    ])

    unsigned = output / "probe-unsigned.apk"
    run([
        tools / "aapt2", "link", "-o", unsigned, "-I", android_jar,
        "--manifest", ROOT / "AndroidManifest.xml", "--min-sdk-version", "28",
        "--target-sdk-version", "35", "--version-code", "1", "--version-name", "1.0",
    ])
    with zipfile.ZipFile(unsigned, "a", compression=zipfile.ZIP_DEFLATED) as apk:
        apk.write(dex / "classes.dex", "classes.dex")

    aligned = output / "probe-aligned.apk"
    run([tools / "zipalign", "-f", "-p", "4", unsigned, aligned])
    keystore = output / "debug.keystore"
    if not keystore.exists():
        run([
            "keytool", "-genkeypair", "-keystore", keystore, "-storepass", "android",
            "-keypass", "android", "-alias", "pixel-probe", "-keyalg", "RSA",
            "-keysize", "2048", "-validity", "10000", "-dname",
            "CN=Personal OS Pixel Probe",
        ])
    apk_path = output / "pixel-keystore-probe.apk"
    run([
        tools / "apksigner", "sign", "--ks", keystore, "--ks-pass", "pass:android",
        "--key-pass", "pass:android", "--out", apk_path, aligned,
    ])
    run([tools / "apksigner", "verify", apk_path])
    return apk_path


def connected_device_count():
    output = subprocess.check_output(["adb", "devices"], text=True)
    devices = [line.split() for line in output.splitlines()[1:] if line.strip()]
    if len(devices) != 1 or devices[0][1] != "device":
        raise RuntimeError("Connect exactly one authorized Android device over ADB")
    return 1


def run_probe(apk_path):
    connected_device_count()
    run(["adb", "install", "-r", apk_path])
    run_id = "probe-" + uuid.uuid4().hex[:12]
    run(["adb", "shell", "am", "start", "-n", ACTIVITY, "--es", "run_id", run_id])

    deadline = time.monotonic() + 45
    while time.monotonic() < deadline:
        log = subprocess.check_output(
            ["adb", "logcat", "-d", "-s", "PixelKeyProbe:I"], text=True,
            errors="replace",
        )
        results = [line for line in log.splitlines() if run_id in line and "RESULT " in line]
        if results:
            print(results[-1].split("RESULT ", 1)[1])
            return
        time.sleep(1)
    raise RuntimeError("Probe did not report within 45 seconds; inspect the app on the phone")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run", action="store_true", help="install, run, and remove the probe app")
    args = parser.parse_args()
    apk_path = build_apk()
    print(f"Built {apk_path}")
    if args.run:
        try:
            run_probe(apk_path)
        finally:
            cleanup = subprocess.run(["adb", "uninstall", PACKAGE], check=False)
            if cleanup.returncode == 0:
                print("Removed the temporary probe app; its Android Keystore alias is app-scoped.")
            else:
                print("Could not remove the probe app; uninstall it from Android settings.")


if __name__ == "__main__":
    try:
        main()
    except (OSError, subprocess.CalledProcessError, RuntimeError) as error:
        print(f"pixel-key-probe: {error}", file=sys.stderr)
        sys.exit(1)
