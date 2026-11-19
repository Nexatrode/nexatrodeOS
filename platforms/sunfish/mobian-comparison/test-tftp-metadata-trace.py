#!/usr/bin/env python3
"""Build and exercise metadata-only socket tracing in an offline arm64 container."""
from pathlib import Path
import hashlib
import json
import os
import shutil
import subprocess
import tempfile

root = Path(__file__).resolve().parents[3]
source = root / 'platforms/sunfish/mobian-comparison/tftp-metadata-trace.c'
out = root / '.cache/sunfish-native-wifi/private/tftp-metadata-trace-prepared-v4'
reference = root / '.cache/sunfish-mobian-comparison/wifi-preparation/source/tqftpserv/tqftpserv-debian-arm64'
assert hashlib.sha256(reference.read_bytes()).hexdigest() == '01e063925e3e86b0c33af5f87227633cf0e9305cdd35d1b1552262d489bddf56'
image = 'qxz-devnet:local'
inspect = json.loads(subprocess.check_output(['docker', 'image', 'inspect', image], text=True))[0]
assert inspect['Os'] == 'linux' and inspect['Architecture'] == 'arm64'
harness = r'''
#include <arpa/inet.h>
#include <errno.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>
int main(void) {
    int rx = socket(AF_INET, SOCK_DGRAM, 0), tx = socket(AF_INET, SOCK_DGRAM, 0);
    if (rx < 0 || tx < 0) return 1;
    struct sockaddr_in address = { .sin_family = AF_INET, .sin_addr.s_addr = htonl(INADDR_LOOPBACK) };
    if (bind(rx, (void *)&address, sizeof(address))) return 2;
    socklen_t length = sizeof(address);
    if (getsockname(rx, (void *)&address, &length) || connect(tx, (void *)&address, length)) return 3;
    const unsigned char packet[] = {0, 6, 'S','E','C','R','E','T','_','P','A','Y','L','O','A','D'};
    if (send(tx, packet, sizeof(packet), 0) != sizeof(packet)) return 4;
    unsigned char received[sizeof(packet)] = {0}; struct sockaddr_storage from; length = sizeof(from);
    if (recvfrom(rx, received, sizeof(received), 0, (void *)&from, &length) != sizeof(packet)) return 5;
    if (memcmp(packet, received, sizeof(packet))) return 6;
    errno = 0; if (send(-1, packet, sizeof(packet), 0) != -1 || errno != EBADF) return 7;
    errno = 0; if (connect(-1, (void *)1, 12) != -1 || errno != EBADF) return 8;
    errno = 0; if (recvfrom(-1, received, sizeof(received), 0, (void *)0x1000, (void *)0x2000) != -1 || errno != EBADF) return 9;
    errno = 0; if (connect(tx, (void *)0x1000, 12) != -1 || errno != EFAULT) return 10;
    if (send(tx, packet, sizeof(packet), 0) != sizeof(packet)) return 11;
    errno = 0; if (recvfrom(rx, received, sizeof(received), MSG_DONTWAIT, (void *)0x1000, (void *)0x2000) != -1 || errno != EFAULT) return 12;
    close(rx); close(tx); puts("HARNESS PASS"); return 0;
}
'''
with tempfile.TemporaryDirectory(prefix='sunfish-tftp-trace-') as temporary:
    tmp = Path(temporary)
    shutil.copy2(source, tmp / 'trace.c')
    shutil.copy2(reference, tmp / 'reference-tqftpserv')
    (tmp / 'harness.c').write_text(harness)
    script = r'''set -eu
cc -std=c11 -O2 -Wall -Wextra -Werror -fPIC -shared -fno-strict-aliasing /work/trace.c -o /work/trace.so -ldl
cc -std=c11 -O2 -Wall -Wextra -Werror -fPIC -shared -fno-strict-aliasing -DTRACE_SOCKET_FAMILY=AF_INET /work/trace.c -o /work/trace-test.so -ldl
cc -std=c11 -O2 -Wall -Wextra -Werror /work/harness.c -o /work/harness
LD_PRELOAD=/work/trace.so /work/harness > /work/production.stdout 2>/work/production.stderr
LD_PRELOAD=/work/trace-test.so /work/harness > /work/test.stdout 2>/work/test.stderr
LD_PRELOAD=/work/trace-test.so stdbuf -oL -eL /work/harness > /work/stdbuf.stdout 2>/work/stdbuf.stderr
readelf -h /work/trace.so > /work/elf-header.txt
readelf -d /work/trace.so > /work/elf-dynamic.txt
readelf --version-info /work/trace.so > /work/elf-versions.txt
readelf --wide --dyn-syms /work/reference-tqftpserv > /work/reference-symbols.txt
'''
    subprocess.run(['docker', 'run', '--rm', '--network', 'none', '--entrypoint', '/bin/sh',
                    '--mount', f'type=bind,src={tmp},dst=/work', image, '-c', script], check=True)
    assert (tmp / 'production.stdout').read_text().strip() == 'HARNESS PASS'
    assert (tmp / 'test.stdout').read_text().strip() == 'HARNESS PASS'
    assert (tmp / 'stdbuf.stdout').read_text().strip() == 'HARNESS PASS'
    assert (tmp / 'production.stderr').read_bytes() == b'', 'non-QRTR socket should not be logged'
    logged = (tmp / 'test.stderr').read_text()
    for label in ('event=CONNECT', 'event=SEND', 'event=RECV', 'opcode=6'):
        assert label in logged, label
    assert 'SECRET_PAYLOAD' not in logged, 'payload appeared in trace'
    stdbuf_log = (tmp / 'stdbuf.stderr').read_text()
    for label in ('event=CONNECT', 'event=SEND', 'event=RECV', 'opcode=6'):
        assert label in stdbuf_log, 'stdbuf: ' + label
    assert 'SECRET_PAYLOAD' not in stdbuf_log
    symbols = (tmp / 'reference-symbols.txt').read_text()
    for label in ('connect@GLIBC_', 'send@GLIBC_', 'recvfrom@GLIBC_'):
        assert label in symbols, 'reference binary: ' + label
    assert 'AArch64' in (tmp / 'elf-header.txt').read_text()
    assert not out.exists(), f'preserve existing output: {out}'
    out.mkdir(parents=True, mode=0o700)
    for name in ['trace.so', 'elf-header.txt', 'elf-dynamic.txt', 'elf-versions.txt', 'reference-symbols.txt']:
        shutil.copy2(tmp / name, out / name)
    digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    manifest = {
        'source_sha256': digest(source), 'test_sha256': digest(Path(__file__)),
        'reference_tqftpserv_sha256': digest(reference),
        'tracer_sha256': digest(out / 'trace.so'), 'build_image_id': inspect['Id'],
        'scope': 'Preparation only; metadata-only QRTR socket interposition, no device action.',
        'limitations': 'Synthetic Linux loopback test; QRTR socket interposition and Linux reference startup remain untested on the device. Logging changes timing. No payload bytes or paths are logged.',
        'device_actions': [],
    }
    (out / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print('PASS: arm64 Linux tracer built; synthetic socket and privacy checks passed; no device action.')
    print('SHA-256:', manifest['tracer_sha256'])
