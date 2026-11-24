#!/usr/bin/env python3
"""Relink an experimental SimpleX guest using a private copy of cached objects.

No VM disk or installation key is read. Cached inputs are read-only; all
compiled objects and the linked executable are written below --output.
"""
import argparse
import hashlib
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
OBJCOPY = Path("/opt/homebrew/opt/llvm/bin/llvm-objcopy")
EXTRA_OBJECTS = {"sel4_libc_agent_runtime.o", "agentstore_qemu_test_time.o",
                 "simplex_sqlite_minimal_compat.o", "simplex_sntrup_sha512_ltc.o",
                 "haskell_entropy_bridge.o", "haskell_tcp_bridge.o",
                 "simplex_agent_ipc_bridge.o", "simplex_agent_rpc_client.o", "qemu_wallclock.o"}
SKIP_PARTS = {"slint-target", "messages-target", "qemu-image", "net-qemu-dns", "installation-vault-target"}


def run(*args):
    subprocess.run([str(x) for x in args], check=True, cwd=ROOT)


def text_digest(binary, tmp, name):
    image = tmp / name
    run(OBJCOPY, "--dump-section", f".text={image}", binary)
    with image.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").digest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--cached-checkout", required=True, type=Path)
    parser.add_argument("--objects", required=True, type=Path, help="cached agent runtime object tree")
    parser.add_argument("--inputs", required=True, type=Path, help="prepared accelerated UTM inputs")
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    cached, objects, inputs, output = (x.resolve() for x in (args.cached_checkout, args.objects, args.inputs, args.output))
    if output.is_relative_to(cached) or output.is_relative_to(inputs) or any(x.suffix == ".utm" for x in (output, *output.parents)):
        raise ValueError("output must be a private worktree staging directory")
    output.mkdir(parents=True, exist_ok=True)
    agent_image = inputs / "pds/agentstore_probe.elf"
    old_runtime = objects / "agent-runtime.elf"
    with tempfile.TemporaryDirectory(prefix="simplex-gate-", dir=output) as name:
        temp = Path(name)
        if text_digest(agent_image, temp, "image-text") != text_digest(old_runtime, temp, "runtime-text"):
            raise RuntimeError("cached object tree does not match the prepared agent's code")
        local_objects = temp / "objects"
        local_objects.mkdir()
        for item in objects.rglob("*.o"):
            rel = item.relative_to(objects)
            if item.name in EXTRA_OBJECTS or not SKIP_PARTS.isdisjoint(rel.parts): continue
            dest = local_objects / rel
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(item, dest)
        if not (local_objects / "Main.o").is_file(): raise RuntimeError("cached agent Main.o missing")

        sdk = inputs / "sdk/board/qemu_virt_aarch64_el1/smp-debug/include"
        ghc = cached / ".cache/ghc-port/ghc-9.6.6"
        generated = cached / "experiments/ghc-microkit/build/generated-rts-headers"
        headers = ROOT / "experiments/ghc-microkit/freestanding-headers"
        cflags = ["-target", "aarch64-none-elf", "-mcpu=cortex-a53", "-mstrict-align",
                  "-ffreestanding", "-fno-builtin", "-fno-stack-protector", "-std=gnu11",
                  "-O2", "-g", "-Wall", "-Wextra", "-Werror", f"-I{sdk}",
                  f"-I{generated}", f"-I{headers}", f"-I{ghc / 'rts/include'}", f"-I{ghc / 'rts'}"]
        entry, gate = temp / "entry.o", temp / "gate.o"
        run("clang", *cflags, "-DSIMPLEX_WAIT_FOR_WRAPPED_VAULT", "-c",
            ROOT / "experiments/ghc-microkit/haskell_microkit_entry.c", "-o", entry)
        run("clang", *cflags, "-c", ROOT / "experiments/ghc-microkit/simplex_vault_gate.c", "-o", gate)

        spec = importlib.util.spec_from_file_location("simplex_link", ROOT / "experiments/ghc-microkit/link_simplexmq_agent_runtime_probe.py")
        linker = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(linker)
        linker.ROOT = cached
        linker.MICROKIT = cached / "experiments/ghc-microkit"
        linker.OUTPUT = local_objects
        linker.GHC_TREE = ghc
        os.environ.update(SIMPLEX_AGENT_DEDICATED_PD="1", SIMPLEX_AGENT_ENTRY_OBJECT=str(entry),
                          SIMPLEX_AGENT_GATE_OBJECT=str(gate), SIMPLEX_AGENT_LINK_CWD=str(temp),
                          SIMPLEX_AGENT_MICROKIT_INCLUDE=str(sdk),
                          SIMPLEX_AGENT_MICROKIT_LIB=str(sdk.parent / "lib/libmicrokit.a"))
        linker.main()
        timer = temp / "timer-config"
        run(OBJCOPY, "--dump-section", f".timer_client_config={timer}", agent_image)
        run(OBJCOPY, "--update-section", f".timer_client_config={timer}",
            local_objects / "agent-runtime.elf", output / "agentstore_probe.elf")
    print("Built experimental gated SimpleX agent in", output)


if __name__ == "__main__":
    main()
