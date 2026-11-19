"""Exercise the device's paired writer on the original 46 close schedules."""
import ast
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

from replay import HERE, ROOT, flags


def main():
    env = dict(os.environ, RUST_MIN_STACK="16777216")
    cache = ROOT / ".cache/sunfish-native-wifi/private"
    for key, filename in (("SUNFISH_GLK_MEMORY_C", "mpss-glink-memory-linux"),
                          ("SUNFISH_GLK_TRANSPORT_C", "mpss-glink-transport-linux"),
                          ("SUNFISH_LINUX_RMTFS_C", "linux-rmtfs/linux-scm.txt"),
                          ("SUNFISH_RMTFS_WIRE_C", "linux-rmtfs-server/wire.txt")):
        env[key] = str(Path(env.get(key, cache / filename)).resolve(strict=True))
    with tempfile.TemporaryDirectory() as directory:
        work = Path(directory)
        (work / "rust").symlink_to(ROOT / "rust", target_is_directory=True)
        shutil.copytree(ROOT / "tests", work / "tests")
        fake = work / "tests/sunfish_glink_fifo_test.rs"
        hook = "inject_at_registration_write(self, name, &op);"
        source = fake.read_text()
        assert source.count(hook) == 1
        fake.write_text(source.replace(hook, hook + "\n        replay_io(self, &op);"))
        with (work / "tests/sunfish_ipa_close_fifo_cases.rs").open("a") as cases:
            cases.write("\n" + (HERE / "close_schedules.rs").read_text())
        (work / "tests/paired.rs").write_text(
            'include!("sunfish_mpss_linux_transport_test.rs");\n' + (HERE / "probe.rs").read_text())
        binary = work / "paired"
        subprocess.run(["rustc", "--edition=2024", "--test", "-A", "dead_code", "-A", "deprecated",
                        *flags(), "--cfg", 'feature="sunfish-paired-oack-init"', "tests/paired.rs",
                        "--cfg", 'feature="sunfish-grant-ad-writer-trace"', "-o", str(binary)], cwd=work, env=env, check=True)
        raw = subprocess.check_output([str(binary), "--exact",
            "independent_runtime_tests::replay_original_grant", "--nocapture"], text=True, env=env)
        tested = set()
        for row in raw.splitlines():
            if not row.startswith("deadline="):
                continue
            offset = int(row.split()[0].split("=")[1])
            events = ast.literal_eval(row.split("phases=")[1])
            names = [name for name, _ in events]
            times = dict(events)
            assert times["injected"] == times["reg-head"] + offset
            if offset in (22, 23, 24, 25):
                for name in ("reg-head", "ad-head", "ack-head", "oack-head", "init-head", "injected"):
                    assert names.count(name) == 1
                assert names.index("ad-head") < names.index("ack-head") < names.index("oack-head") < names.index("injected")
                assert names.index("oack-head") < names.index("init-head")
                assert (names.index("init-head") < names.index("injected")) == (offset >= 24)
                print(f"offset {offset}: OACK {times['oack-head']}, INIT {times['init-head']}, close {times['injected']}", flush=True)
            tested.add(offset)
        assert tested == set(range(15, 61))
        print("All 46 close placements verified.", flush=True)
        subprocess.run([str(binary), "--test-threads=1", "--skip", "replay_original_grant"], env=env, cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
