#!/usr/bin/env python3
"""Report the current seL4 target import boundary for upstream Client.hs."""

import re
import subprocess
from prepare_simplexmq_guest_transport import prepare
from verify_simplexmq_backend_patch import FULL_STAGE, ROOT, ensure_patch_applied, stage_full_source


def main() -> None:
    ensure_patch_applied()
    stage_full_source()
    prepare(FULL_STAGE)
    ghc_tree = ROOT / ".cache/ghc-port/ghc-9.6.6"
    compiler = ghc_tree / "_build/stage0/bin/aarch64-unknown-none-ghc"
    target_db = ghc_tree / "_build/stage1/lib/package.conf.d"
    guest_db = ROOT / ".cache/sel4-tls-deps/guest-package.conf.d"
    autogen = (ROOT / ".cache/simplex-chat-upstream/dist-newstyle/build/aarch64-osx"
               / "ghc-9.6.3/simplexmq-7.0.0.6/build/autogen")
    shims = ROOT / "experiments/ghc-microkit/guest-client-shims"
    transport_shims = ROOT / "experiments/ghc-microkit/guest-transport-shims"
    output = ROOT / "experiments/ghc-microkit/build/guest-simplex-client-check"
    output.mkdir(parents=True, exist_ok=True)
    args = [compiler, "-fno-code", "-fforce-recomp", "-v0",
            "-no-global-package-db", "-package-env=-",
            "-package-db", target_db, "-package-db", guest_db,
            "-DSIMPLEX_MICROKIT", f"-i{transport_shims}", f"-i{shims}",
            f"-i{FULL_STAGE}", f"-i{autogen}",
            "-outputdir", output,
            FULL_STAGE / "Simplex/Messaging/Transport/Client.hs"]
    result = subprocess.run([str(arg) for arg in args], cwd=ghc_tree,
                            capture_output=True, text=True)
    if result.returncode == 0:
        print("TARGET TRANSPORT CLIENT|TYPECHECK PASS")
        compile_args = [arg for arg in args if str(arg) != "-fno-code"]
        compile_args.insert(1, "--make")
        compile_args.insert(2, "-no-link")
        compiled = subprocess.run([str(arg) for arg in compile_args], cwd=ghc_tree,
                                  capture_output=True, text=True)
        if compiled.returncode != 0:
            raise RuntimeError("target transport object build failed:\n"
                               + compiled.stderr[:6000])
        auxiliary_dirs = {output / "protocol-client", output / "protocol-client-objects",
                          output / "agent-client"}
        objects = [obj for obj in output.rglob("*.o")
                   if not any(parent in obj.parents for parent in auxiliary_dirs)]
        print(f"TARGET TRANSPORT CLIENT|OBJECT BUILD PASS|{len(objects)} objects")
        client_args = [arg for arg in args if str(arg) != "-fno-code"]
        client_args.insert(1, "-fno-code")
        client_args[client_args.index("-outputdir") + 1] = output / "protocol-client"
        (output / "protocol-client").mkdir(parents=True, exist_ok=True)
        client_args[-1] = FULL_STAGE / "Simplex/Messaging/Client.hs"
        client = subprocess.run([str(arg) for arg in client_args], cwd=ghc_tree,
                                capture_output=True, text=True)
        if client.returncode == 0:
            print("TARGET PROTOCOL CLIENT|TYPECHECK PASS")
            object_dir = output / "protocol-client-objects"
            object_dir.mkdir(parents=True, exist_ok=True)
            object_args = [arg for arg in client_args if str(arg) != "-fno-code"]
            object_args[1:1] = ["--make", "-no-link"]
            object_args[object_args.index("-outputdir") + 1] = object_dir
            compiled_client = subprocess.run([str(arg) for arg in object_args],
                                             cwd=ghc_tree, capture_output=True,
                                             text=True)
            if compiled_client.returncode != 0:
                print("TARGET PROTOCOL CLIENT|OBJECT BUILD BLOCKED")
                print(compiled_client.stderr[:7000])
            else:
                object_count = len(list(object_dir.rglob("*.o")))
                print(f"TARGET PROTOCOL CLIENT|OBJECT BUILD PASS|{object_count} objects")
                agent_args = [arg for arg in args if str(arg) != "-fno-code"]
                agent_args.insert(1, "-fno-code")
                agent_args[agent_args.index("-outputdir") + 1] = output / "agent-client"
                http2_source = ROOT / ".cache/guest-http2-microkit/http2-4.2.2"
                http2_shims = ROOT / "experiments/ghc-microkit/guest-http2-shims"
                agent_args[agent_args.index("-outputdir"):agent_args.index("-outputdir")] = [
                    f"-i{http2_shims}", f"-i{http2_source}"]
                (output / "agent-client").mkdir(parents=True, exist_ok=True)
                agent_args[-1] = FULL_STAGE / "Simplex/Messaging/Agent/Client.hs"
                agent = subprocess.run([str(arg) for arg in agent_args], cwd=ghc_tree,
                                       capture_output=True, text=True)
                if agent.returncode == 0:
                    print("TARGET AGENT CLIENT|TYPECHECK PASS")
                else:
                    missing_agent = list(dict.fromkeys(re.findall(
                        r"Could not find module [‘']([^’']+)[’']", agent.stderr)))
                    if missing_agent:
                        print("TARGET AGENT CLIENT|CURRENT MISSING IMPORTS")
                        for module in missing_agent:
                            print(f"  {module}")
                        print(f"TARGET AGENT CLIENT|INCOMPLETE|{len(missing_agent)} current import errors")
                    else:
                        print("TARGET AGENT CLIENT|TYPECHECK BLOCKED")
                        print(agent.stderr[:7000])
        else:
            missing_client = list(dict.fromkeys(re.findall(
                r"Could not find module [‘']([^’']+)[’']", client.stderr)))
            if missing_client:
                print("TARGET PROTOCOL CLIENT|CURRENT MISSING IMPORTS")
                for module in missing_client:
                    print(f"  {module}")
                print(f"TARGET PROTOCOL CLIENT|INCOMPLETE|{len(missing_client)} current import errors")
            else:
                print("TARGET PROTOCOL CLIENT|TYPECHECK BLOCKED")
                print(client.stderr[:6000])
        return
    missing = list(dict.fromkeys(re.findall(r"Could not find module [‘']([^’']+)[’']",
                                                result.stderr)))
    if not missing:
        raise RuntimeError("target transport compilation failed outside import resolution:\n"
                           + result.stderr[:6000])
    print("TARGET TRANSPORT CLIENT|CURRENT MISSING IMPORTS")
    for module in missing:
        print(f"  {module}")
    print(f"TARGET TRANSPORT CLIENT|INCOMPLETE|{len(missing)} current import errors")


if __name__ == "__main__":
    main()
