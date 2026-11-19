#!/usr/bin/env python3
"""Compare the bounded server's OACK with the pinned tqftpserv C code."""
from pathlib import Path
import hashlib
import subprocess
import tempfile

root = Path(__file__).resolve().parents[2]
source = root / '.cache/sunfish-mobian-comparison/wifi-preparation/source/tqftpserv/tqftpserv.c'
raw = source.read_bytes()
assert hashlib.sha256(raw).hexdigest() == 'e8dc6a2ced3315f2c2e8b544f710b69fb6e8d464a92ce88ed800cf891edf22f1', 'reference C source changed'
text = raw.decode()
oack = text[text.index('static int tftp_send_oack('):text.index('static int tftp_send_error(')]
options = text[text.index('static void parse_options('):text.index('static void handle_rrq(')]
harness = r'''
#include <sys/types.h>
#include <sys/socket.h>
#include <unistd.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#define OP_OACK 6
''' + oack + options + r'''
int main(void) {
    static const char opts[] = "blksize\0" "7680\0" "timeoutms\0" "1000\0" "wsize\0" "10\0";
    size_t block = 512, window = 0, rsize = 0;
    ssize_t tsize = -1;
    unsigned int timeout = 1000;
    off_t seek = 0;
    int s[2]; char result[512];
    parse_options(opts, sizeof(opts) - 1, &block, &tsize, &window, &timeout, &rsize, &seek);
    if (socketpair(AF_UNIX, SOCK_DGRAM, 0, s)) return 1;
    if (tftp_send_oack(s[0], &block, tsize ? (size_t *)&tsize : NULL,
                       window ? &window : NULL, &timeout,
                       rsize ? &rsize : NULL, seek ? &seek : NULL) < 0) return 2;
    ssize_t count = recv(s[1], result, sizeof(result), 0);
    if (count < 0 || fwrite(result, 1, count, stdout) != (size_t)count) return 3;
    return 0;
}
'''
with tempfile.TemporaryDirectory(prefix='sunfish-tftp-oracle-') as temp:
    temp = Path(temp)
    (temp / 'reference.c').write_text(harness)
    subprocess.run(['cc', '-std=c11', str(temp / 'reference.c'), '-o', str(temp / 'reference')], check=True)
    reference = subprocess.check_output([str(temp / 'reference')])
    rust = '#[path = ' + repr(str(root / 'rust/src/qcom_tftp_server_check.rs')).replace("'", '"') + ']\nmod server;\nfn main() { use std::io::Write; std::io::stdout().write_all(server::OACK).unwrap(); }\n'
    (temp / 'native.rs').write_text(rust)
    subprocess.run(['rustc', '--edition=2024', '-Awarnings', str(temp / 'native.rs'), '-o', str(temp / 'native')], check=True)
    native = subprocess.check_output([str(temp / 'native')])
    assert native == reference, f'OACK differs: native={native.hex()} reference={reference.hex()}'
    print(f'PASS: native OACK matches pinned tqftpserv ({len(native)} bytes)')
