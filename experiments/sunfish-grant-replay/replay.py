"""Replay IMG_9638's three grant schedules against the Rust host FIFO and IRQ owner."""
import ast
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import tomllib

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
POLICIES = ("baseline", "at-registration", "advertisement-only")


def flags():
    graph = tomllib.loads((ROOT / "rust/Cargo.toml").read_text())["features"]
    chosen = set((HERE / "features.txt").read_text().splitlines())
    todo = list(chosen)
    while todo:
        for dependency in graph[todo.pop()]:
            if dependency not in chosen:
                chosen.add(dependency)
                todo.append(dependency)
    return [arg for name in sorted(chosen) for arg in ("--cfg", f'feature="{name}"')]


def replay(work, env, policy):
    # Each policy is a small delta on the preceding policy. Apply only to
    # the disposable sources; the firmware build never sees this instrumentation.
    subprocess.run(["git", "apply", str(HERE / f"{policy}.patch")], cwd=work, check=True)
    executable = work / "owner-test"
    subprocess.run(["rustc", "--edition=2024", "--test", "-A", "dead_code",
                    "-A", "deprecated", *flags(), "tests/grant_replay.rs",
                    "-o", str(executable)], cwd=work, env=env, check=True)
    result = subprocess.run([str(executable), "--exact",
                             "independent_runtime_tests::replay_original_grant", "--nocapture"],
                            env=env, cwd=work, stdout=subprocess.PIPE, text=True)
    if result.returncode:
        raise RuntimeError(result.stdout)
    rows = {}
    for line in result.stdout.splitlines():
        if not line.startswith("deadline="):
            continue
        match = re.match(r"deadline=(\d+) success=(true|false) outcome=(\d+) mask=Some\((\d+)\)", line)
        events = ast.literal_eval(line.split("phases=")[1])
        close_index = next(i for i, event in enumerate(events) if event[0] == "injected")
        before = dict(events[:close_index])
        times = dict(events)
        assert times["wrq-accepted"] < times["oack-queued"] < times["grant-reserved"]
        assert times["grant-reserved"] < times["reg-head"] < times["eligible-at-reg"]
        assert ("oack-head" in before) == (match[2] == "true")
        rows[int(match[1])] = (before, int(match[3]), int(match[4]))
    assert sorted(rows) == list(range(15, 61))
    return rows


def main():
    env = dict(os.environ, RUST_MIN_STACK="16777216")
    cache = ROOT / ".cache/sunfish-native-wifi/private"
    for key, relative in (("SUNFISH_LINUX_RMTFS_C", "linux-rmtfs/linux-scm.txt"),
                          ("SUNFISH_RMTFS_WIRE_C", "linux-rmtfs-server/wire.txt")):
        source = Path(env.get(key, cache / relative)).resolve(strict=True)
        env[key] = str(source)
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp)
        shutil.copytree(ROOT / "rust/src", work / "rust/src")
        shutil.copytree(ROOT / "tests", work / "tests")
        harness = work / "tests/sunfish_glink_fifo_test.rs"
        source = harness.read_text()
        hook = "inject_at_registration_write(self, name, &op);"
        assert source.count(hook) == 1
        harness.write_text(source.replace(hook, hook + "\n        replay_io(self, &op);"))
        shutil.copyfile(HERE / "close_schedules.rs", work / "tests/sunfish_ipa_close_fifo_cases.rs")
        (work / "tests/grant_replay.rs").write_text(
            'include!("sunfish_mpss_linux_transport_test.rs");\n' + (HERE / "probe.rs").read_text())
        results = {name: replay(work, env, name) for name in POLICIES}
    matching = [n for n, (events, outcome, mask) in results["baseline"].items()
                if outcome == 2 and mask == 4 and "ad-head" in events
                and "init-head" in events and "ack-head" not in events]
    assert matching == [22, 23, 24, 25], matching
    expected = [(0, 0, 4), (4, 4, 0), (4, 3, 0)]
    for policy, want in zip(POLICIES, expected, strict=True):
        counts = tuple(sum(event in results[policy][n][0] for n in matching)
                       for event in ("ack-head", "oack-head", "init-head"))
        assert counts == want, (policy, counts)
        print(f"{policy}: grant ACK {counts[0]}/4, OACK {counts[1]}/4, INIT {counts[2]}/4")


if __name__ == "__main__":
    main()
