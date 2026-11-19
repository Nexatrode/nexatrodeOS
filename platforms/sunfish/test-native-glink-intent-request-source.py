#!/usr/bin/env python3
"""Pin the Linux request/ack contract used by the opt-in host experiment."""
import hashlib, tarfile
from pathlib import Path
root = Path(__file__).resolve().parents[2]
archive = root / ".cache/sunfish-mobian-comparison/downloads/linux-2267e248.tar.gz"
with tarfile.open(archive) as tar:
    member = next(m for m in tar if m.name.endswith("/drivers/rpmsg/qcom_glink_native.c"))
    source = tar.extractfile(member).read()
assert hashlib.sha256(source).hexdigest() == "14a1b55234934beba5d9166c6113fd7064df45d14c91ac084c06cc4ca0ffc34e"
s = source.decode()
assert "#define GLINK_CMD_RX_INTENT_REQ\t\t7" in s
assert "#define GLINK_CMD_RX_INTENT_REQ_ACK\t8" in s
start = s.index("static int qcom_glink_request_intent(")
end = s.index("static int __qcom_glink_send", start)
request = s[start:end]
for part in ["cmd.id = GLINK_CMD_RX_INTENT_REQ", "cmd.cid = channel->lcid", "cmd.size = size", "intent_req_result", "intent_received", "10 * HZ"]:
    assert part in request, part
print("PASS: pinned Linux request, acknowledgement and buffer-wait contract")
