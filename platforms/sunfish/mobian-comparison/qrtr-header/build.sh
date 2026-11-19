#!/bin/sh
set -eu
cd /capture
for index in 0 1 2 3 4 5 6 7; do
    test -b "/dev/loop$index" || mknod "/dev/loop$index" b 7 "$index"
done
mkdir -p /build
mount -o loop /capture/build.ext4 /build
trap 'umount /build' EXIT
test "$(cat /build/source/.reference-commit)" = 2267e248204e7e051e0f7355298949a6440de579
cmp /build/linux-out/.config kernel.config
for index in 0 1 2 3 4 5 6 7; do
    test -b "/dev/loop$index" || mknod "/dev/loop$index" b 7 "$index"
done
mkdir -p /build/sunfish-qrtr-header
cp observe.h sunfish_qrtr_trace.h /build/sunfish-qrtr-header/
python3 - <<'PY'
from pathlib import Path
s = Path('/build/source/net/qrtr/smd.c').read_text()
for before, after in [
    ('#include "qrtr.h"', '#include "qrtr.h"\n#include "observe.h"'),
    ('\trc = qrtr_endpoint_post', '\tif (len > 0)\n\t\tsunfish_qrtr_observe(false, data, len);\n\trc = qrtr_endpoint_post'),
    ('\trc = rpmsg_send', '\tsunfish_qrtr_observe(true, skb->data, skb->len);\n\trc = rpmsg_send'),
]:
    assert s.count(before) == 1, before
    s = s.replace(before, after)
d = Path('/build/sunfish-qrtr-header')
(d/'smd.c').write_text(s)
(d/'Makefile').write_text('obj-m := qrtr-smd.o\nqrtr-smd-y := smd.o\nccflags-y += -I$(src) -I$(srctree)/net/qrtr\n')
PY
make -C /build/source O=/build/linux-out M=/build/sunfish-qrtr-header ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- LOCALVERSION=+ modules
cp /build/sunfish-qrtr-header/qrtr-smd.ko /capture/
aarch64-linux-gnu-readelf -p .modinfo qrtr-smd.ko
