#!/bin/sh
set -eu
touch_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
output_dir="$touch_dir/../../../build/iphone7plus/touch"
mkdir -p "$output_dir"
${CC:-cc} -std=c11 -O2 -Wall -Wextra -Werror \
    "$touch_dir/report.c" "$touch_dir/report_test.c" \
    -o "$output_dir/report-test"
"$output_dir/report-test"
${CC:-cc} -std=c11 -O2 -Wall -Wextra -Werror \
    "$touch_dir/packet.c" "$touch_dir/report.c" "$touch_dir/packet_test.c" \
    -o "$output_dir/packet-test"
"$output_dir/packet-test"
${CC:-cc} -std=c11 -O2 -Wall -Wextra -Werror \
    "$touch_dir/boot_stream.c" "$touch_dir/boot_stream_test.c" \
    -o "$output_dir/boot-stream-test"
"$output_dir/boot-stream-test"
${CC:-cc} -std=c11 -O2 -Wall -Wextra -Werror \
    "$touch_dir/boot_stream.c" "$touch_dir/boot_execute.c" \
    "$touch_dir/boot_execute_test.c" -o "$output_dir/boot-execute-test"
"$output_dir/boot-execute-test"
${CC:-cc} -std=c11 -O2 -Wall -Wextra -Werror \
    "$touch_dir/spi2_transport.c" "$touch_dir/spi2_transport_test.c" \
    -o "$output_dir/spi2-transport-test"
"$output_dir/spi2-transport-test"
${CC:-cc} -std=c11 -O2 -Wall -Wextra -Werror \
    "$touch_dir/hw_control.c" "$touch_dir/hw_control_test.c" \
    -o "$output_dir/hw-control-test"
"$output_dir/hw-control-test"
${CC:-cc} -std=c11 -O2 -Wall -Wextra -Werror \
    "$touch_dir/i2c_pmu.c" "$touch_dir/i2c_pmu_test.c" \
    -o "$output_dir/i2c-pmu-test"
"$output_dir/i2c-pmu-test"
${CC:-cc} -std=c11 -O2 -Wall -Wextra -Werror \
    "$touch_dir/z2_report.c" "$touch_dir/packet.c" "$touch_dir/z2_report_test.c" \
    -o "$output_dir/z2-report-test"
"$output_dir/z2-report-test"
${CC:-cc} -std=c11 -O2 -Wall -Wextra -Werror \
    "$touch_dir/z2_report.c" "$touch_dir/packet.c" "$touch_dir/report.c" \
    "$touch_dir/z2_event_test.c" -o "$output_dir/z2-event-test"
"$output_dir/z2-event-test"
python3 "$touch_dir/boot_stream_test.py"
echo 'iPhone touch packet and report decoders: PASS'
