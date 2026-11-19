#!/bin/sh
set -eu
udc=
attempt=0
while [ "$attempt" -lt 30 ]; do
    udc=$(find /sys/class/udc -mindepth 1 -maxdepth 1 -printf '%f\n' | sort | head -n 1)
    [ -z "$udc" ] || break
    if ! tr '\000' '\n' < /proc/device-tree/compatible | grep -qx google,sunfish; then break; fi
    attempt=$((attempt + 1))
    sleep 1
done
if [ -z "$udc" ]; then
    echo 'COMPARISON USB: no device controller (expected on QEMU virt)'
    exit 0
fi
mountpoint -q /sys/kernel/config || mount -t configfs configfs /sys/kernel/config
g=/sys/kernel/config/usb_gadget/sunfish-reference
mkdir -p "$g"
printf '0x1d6b' > "$g/idVendor"
printf '0x0104' > "$g/idProduct"
mkdir -p "$g/strings/0x409" "$g/configs/c.1/strings/0x409"
printf 'Sunfish Linux comparison' > "$g/strings/0x409/product"
printf 'Reference build' > "$g/strings/0x409/manufacturer"
printf 'sunfish-reference' > "$g/strings/0x409/serialnumber"
printf 'ECM and serial' > "$g/configs/c.1/strings/0x409/configuration"
mkdir -p "$g/functions/ecm.usb0" "$g/functions/acm.usb0"
printf '02:00:00:42:01:01' > "$g/functions/ecm.usb0/dev_addr"
printf '02:00:00:42:01:02' > "$g/functions/ecm.usb0/host_addr"
ln -s "$g/functions/ecm.usb0" "$g/configs/c.1/ecm.usb0"
ln -s "$g/functions/acm.usb0" "$g/configs/c.1/acm.usb0"
printf '%s' "$udc" > "$g/UDC"
ip address add 172.16.42.1/24 dev usb0
ip -6 address add fe80::42:1/64 dev usb0 nodad
ip link set usb0 up
echo 'COMPARISON USB: ECM 172.16.42.1 / fe80::42:1 and ACM configured'
