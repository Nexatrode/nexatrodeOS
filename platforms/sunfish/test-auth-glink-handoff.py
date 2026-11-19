#!/usr/bin/env python3
"""Host-only regression for the auth badge split to the real runtime router."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
POLICY = ROOT / "rust/src/pixel_mpss_auth_irq.rs"
HARNESS = ROOT / "tests/pixel_mpss_runtime_irq_test.rs"


def runtime_policy(text):
    runtime = '#[cfg(feature = "sunfish-native-startup-mpss-runtime")]\n'
    legacy = '#[cfg(not(feature = "sunfish-native-startup-mpss-runtime"))]\npub(super) const OWNED: u64 = SMP | WDOG;\n'
    if text.count(runtime) != 2 or text.count(legacy) != 1:
        raise ValueError("auth feature selection changed; review this host harness")
    return text.replace(runtime, "").replace(legacy, "")


CASE = r'''
#[allow(dead_code)]
pub mod sunfish_mpss_linux_smp2p {
    #[derive(PartialEq)] pub enum Poll { Observed }
    #[derive(Clone, Copy)] pub struct Peer { pub slave_kernel: Option<u32> }
    pub struct Report { pub ready_seen: bool, pub peer: Option<Peer> }
}
#[allow(dead_code)]
#[path = "POLICY_PATH"] mod auth_irq_policy;
#[test]
fn combined_early_glink_handoff() {
    use std::sync::atomic::{AtomicU64, Ordering};
    use pixel_mpss_runtime_irq::{Kernel, Router};
    struct K;
    impl Kernel for K {
        fn acknowledge(&mut self, channel:u8)->Result<(),()> {
            assert_eq!(channel,4); Ok(())
        }
    }
    let (owned, ipa) = auth_irq_policy::split(0x38);
    assert_eq!(ipa, 8);
    let pending = AtomicU64::new(owned);
    pending.fetch_and(!(1<<5), Ordering::AcqRel);
    let mut router = Router::new(K, 1);
    router.capture(pending.swap(0, Ordering::AcqRel)).unwrap();
    assert_eq!(router.report().pending_modem, 16);
    router.acknowledge_boot_glink().unwrap();
    assert_eq!(router.report().pending_modem, 0);
    assert_eq!(router.report().acknowledgements, 1);
    assert!(router.acknowledge_boot_glink().is_err());
}
'''


def run_case(directory, text, expect_success):
    policy = directory / "policy.rs"
    policy.write_text(text)
    source = HARNESS.read_text().split("\n", 1)[1]
    source = source.replace('"sunfish_modem_owner_test.rs"', f'"{ROOT}/tests/sunfish_modem_owner_test.rs"')
    source = source.replace('"../rust/src/pixel_mpss_runtime_irq.rs"', f'"{ROOT}/rust/src/pixel_mpss_runtime_irq.rs"')
    source += CASE.replace("POLICY_PATH", str(policy))
    harness = directory / "handoff.rs"
    harness.write_text(source)
    binary = directory / "handoff-test"
    subprocess.run(["rustc", "--edition=2024", "--test", "-D", "warnings", str(harness), "-o", str(binary)], check=True)
    result = subprocess.run([str(binary), "combined_early_glink_handoff"], capture_output=True, text=True)
    if (result.returncode == 0) != expect_success:
        raise RuntimeError(f"unexpected test result: {result.stdout}\n{result.stderr}")
    if expect_success and "1 passed; 0 failed" not in result.stdout:
        raise RuntimeError("combined handoff test did not run")
    if not expect_success and "assertion `left == right` failed" not in result.stdout:
        raise RuntimeError("old routing did not fail at the expected assertion")


def main():
    text = runtime_policy(POLICY.read_text())
    old, new = "pub(super) const OWNED: u64 = SMP | WDOG | GLINK;", "pub(super) const OWNED: u64 = SMP | WDOG;"
    if text.count(old) != 1:
        raise ValueError("runtime routing anchor changed")
    with tempfile.TemporaryDirectory(prefix="sunfish-auth-glink-") as tmp:
        directory = Path(tmp)
        run_case(directory, text, True)
        run_case(directory, text.replace(old, new), False)
    print("PASS: auth split to real router, single GLINK rearm; legacy routing rejected. Host only.")


if __name__ == "__main__":
    main()
