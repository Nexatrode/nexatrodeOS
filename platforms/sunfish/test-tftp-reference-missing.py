#!/usr/bin/env python3
"""Compare the pinned Linux missing-file ERROR packet with the Rust encoder."""
from pathlib import Path
import hashlib
import subprocess
import tempfile

root = Path(__file__).resolve().parents[2]
source = root / '.cache/sunfish-mobian-comparison/wifi-preparation/source/tqftpserv/tqftpserv.c'
raw = source.read_bytes()
assert hashlib.sha256(raw).hexdigest() == 'e8dc6a2ced3315f2c2e8b544f710b69fb6e8d464a92ce88ed800cf891edf22f1'
text = raw.decode()
send_error = text[text.index('static int tftp_send_error('):text.index('static void parse_options(')]
assert 'tftp_send_error(sock, 1, "file not found")' in text
harness = r'''
#include <arpa/inet.h>
#include <stdint.h>
#include <sys/socket.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#define OP_ERROR 5
''' + send_error + r'''
int main(void) {
    int s[2]; char result[128];
    if (socketpair(AF_UNIX, SOCK_DGRAM, 0, s)) return 1;
    if (tftp_send_error(s[0], 1, "file not found") < 0) return 2;
    ssize_t n = recv(s[1], result, sizeof(result), 0);
    return n < 0 || fwrite(result, 1, n, stdout) != (size_t)n;
}
'''
with tempfile.TemporaryDirectory(prefix='sunfish-tftp-missing-') as temp:
    temp = Path(temp)
    (temp / 'reference.c').write_text(harness)
    subprocess.run(['cc', '-std=c11', str(temp / 'reference.c'), '-o', str(temp / 'reference')], check=True)
    reference = subprocess.check_output([str(temp / 'reference')])
    rust = '#[path = "' + str(root / 'rust/src/qcom_tftp.rs') + '"]\nmod qcom_tftp;\n'
    rust += 'fn main() { use std::io::Write; let mut b=[0;128]; let n=qcom_tftp::encode_error(1,b"file not found",&mut b).unwrap(); std::io::stdout().write_all(&b[..n]).unwrap(); }\n'
    (temp / 'native.rs').write_text(rust)
    subprocess.run(['rustc', '--edition=2024', '-Awarnings', str(temp / 'native.rs'), '-o', str(temp / 'native')], check=True)
    native = subprocess.check_output([str(temp / 'native')])
    assert native == reference, (native.hex(), reference.hex())
    restricted = '#[path = "' + str(root / 'rust/src/qcom_tftp_server_check.rs') + '"]\nmod server;\n'
    restricted += 'fn main() { use std::io::Write; std::io::stdout().write_all(server::MISSING_REPLY).unwrap(); }\n'
    (temp / 'restricted.rs').write_text(restricted)
    subprocess.run(['rustc', '--edition=2024', '-Awarnings', '--cfg', 'feature="sunfish-tftp-missing-temp-replies"', str(temp / 'restricted.rs'), '-o', str(temp / 'restricted')], check=True)
    assert subprocess.check_output([str(temp / 'restricted')]) == reference
    print(f'PASS: native missing-file ERROR matches pinned tqftpserv ({len(native)} bytes)')
