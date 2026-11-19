"""Narrow compatibility rule for unchanged historical proofs after one additive Cargo feature."""
from pathlib import Path
import hashlib

ROOT = Path(__file__).resolve().parents[2]
BASE_CARGO_SHA256 = "6423da4490bc5b5f93a58ebb78dba128f4cbad86ff865561205674b62bd4291c"
ADDED_FEATURE = (
    b'sunfish-native-startup-mpss-runtime-gate-diagnostic = '
    b'["sunfish-native-startup-mpss-runtime"]\n'
)
BASE_GLINK_SHA256 = "c11b11c6ca074cccc0d7bb23cd36951b4ad7656865bec2dcbb7255f6885ed3e8"
RUNTIME_GATED_GLINK_SHA256 = "a4de0d0e8e903f11458a0c57d15fe5a0a45e9e067125d4c5bdb7a57bcd175930"
BASE_GLINK = ROOT / ".cache/sunfish-native-wifi/private/linux-rmtfs-server/source/rust/src/qcom_glink_qmi.rs"
DIAGNOSTIC_ONLY_SOURCE_UPDATES = {
    "rust/src/sunfish_native_startup_preflight.rs": (
        "815f33311a81d2e5551940a2e53ee2cee048620cdee46027087d4ca9ed54e9b4",
        "d7699d708624fa46890fca225de7be5f63082231eab375b24c687c94b20e54b2",
    ),
    "rust/src/pixel_smmu_install.rs": (
        "1b817f7dc7421b41b5c0b924b152db1dda833524346ed252a9971e3d3cb0b9cf",
        "bf2b3476caa563d2eaa9ec31f5c50aaf3374c03eecd728ac6320144e923c366e",
    ),
    "rust/src/pixel_linux_rmtfs.rs": (
        "525db6e87b3d03e3e9623da9b34a1a2e3a7b3902c5d478f7070dcee56cd32a5d",
        "cb961d0c33ae4a290365b303fb88847fe69796befb5457305aa46948b632e5ed",
    ),
    "rust/src/pixel_mpss_auth.rs": (
        "41f331152ba20995ffac4c5baebd771bd58aabda1e1409a9a0c2e987bd54f487",
        "21fd8fbcaa1069bf613337f37b14611a4c96a5d576d808f79ca568c2f2c00bf1",
    ),
}
ANCHOR = b'sunfish-native-startup-mpss-runtime = '
BASELINE = ROOT / ".cache/sunfish-native-wifi/private/linux-rmtfs-server/source/rust/Cargo.toml"


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def source_matches(relative: str, expected: str) -> bool:
    """Accept exact proof inputs, plus only the recorded one-line feature addition."""
    path = ROOT / relative
    if not path.is_file():
        return False
    if sha256(path) == expected:
        return True
    if relative != "rust/Cargo.toml" or expected != BASE_CARGO_SHA256 or not BASELINE.is_file():
        return False
    baseline = BASELINE.read_bytes()
    if hashlib.sha256(baseline).hexdigest() != BASE_CARGO_SHA256:
        return False
    current = path.read_bytes()
    anchor_lines = [line for line in baseline.splitlines(keepends=True)
                    if line.startswith(ANCHOR)]
    if len(anchor_lines) != 1 or ADDED_FEATURE in baseline:
        return False
    anchor = anchor_lines[0]
    expected_current = baseline.replace(anchor, anchor + ADDED_FEATURE, 1)
    return current == expected_current


def source_matches_compatible(relative: str, expected: str) -> bool:
    if source_matches(relative, expected):
        return True
    if relative == "rust/src/qcom_glink_qmi.rs" and expected == BASE_GLINK_SHA256:
        return (BASE_GLINK.is_file()
                and sha256(BASE_GLINK) == BASE_GLINK_SHA256
                and sha256(ROOT / relative) == RUNTIME_GATED_GLINK_SHA256)
    allowed = DIAGNOSTIC_ONLY_SOURCE_UPDATES.get(relative)
    return bool(allowed and expected == allowed[0]
                and sha256(ROOT / relative) == allowed[1])
