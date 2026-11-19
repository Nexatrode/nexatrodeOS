#!/bin/sh
set -eu

command -v adb >/dev/null 2>&1 || {
    printf '%s\n' 'adb is required.' >&2
    exit 2
}

adb_serial=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --serial)
            shift
            if [ "$#" -eq 0 ] || [ -z "$1" ]; then
                printf '%s\n' '--serial requires an ADB serial.' >&2
                exit 2
            fi
            adb_serial=$1
            ;;
        -h|--help)
            printf '%s\n' 'Usage: capture-lineage-ipa-state.sh [--serial ADB_SERIAL]'
            exit 0
            ;;
        *)
            printf 'Unknown argument: %s\n' "$1" >&2
            exit 2
            ;;
    esac
    shift
done

adb_cmd() {
    if [ -n "$adb_serial" ]; then
        adb -s "$adb_serial" "$@"
    else
        adb "$@"
    fi
}

if [ -n "$adb_serial" ]; then
    device_count=$(adb devices | awk -v serial="$adb_serial" 'NR > 1 && $1 == serial && $2 == "device" { count++ } END { print count + 0 }')
    if [ "$device_count" -ne 1 ]; then
        printf 'Expected authorized ADB device %s; found %s matching devices.\n' "$adb_serial" "$device_count" >&2
        exit 2
    fi
else
    device_count=$(adb devices | awk 'NR > 1 && $2 == "device" { count++ } END { print count + 0 }')
    if [ "$device_count" -ne 1 ]; then
        printf 'Expected exactly one authorized ADB device; found %s. Use --serial to select one.\n' "$device_count" >&2
        exit 2
    fi
fi

product=$(adb_cmd shell getprop ro.product.device | tr -d '\r')
lineage_version=$(adb_cmd shell getprop ro.lineage.version | tr -d '\r')
baseband=$(adb_cmd shell getprop ro.baseband | tr -d '\r')
mdm_helper_state=$(adb_cmd shell getprop init.svc.vendor.mdm_helper | tr -d '\r')
mdm_helper_pid=$(adb_cmd shell pidof mdm_helper 2>/dev/null | tr -d '\r' || true)
mdm_helper_script_status=present
if ! adb_cmd shell test -r /vendor/bin/init.mdm.sh; then
    mdm_helper_script_status=absent_or_unreadable
fi
mdm_helper_binary_status=executable
if ! adb_cmd shell test -x /vendor/bin/mdm_helper; then
    mdm_helper_binary_status=absent_or_not_executable
fi
if [ "$product" != sunfish ] || [ -z "$lineage_version" ]; then
    printf '%s\n' 'Refusing capture: the connected device is not identified as Sunfish running LineageOS.' >&2
    exit 2
fi

debugfs_status=available
filesystems=$(adb_cmd shell cat /proc/filesystems 2>/dev/null | tr -d '\r' || true)
if ! printf '%s\n' "$filesystems" | awk '$NF == "debugfs" { found=1 } END { exit !found }'; then
    debugfs_status=unsupported_by_running_kernel
elif ! adb_cmd shell test -d /sys/kernel/debug; then
    debugfs_status=not_mounted
fi

hw_type_status=read
if [ "$debugfs_status" = unsupported_by_running_kernel ]; then
    hw_type_status=unavailable_debugfs_unsupported
elif ! hw_type=$(adb_cmd shell cat /sys/kernel/debug/ipa/hw_type 2>&1); then
    hw_type_status=unavailable_without_permission_changes
    hw_type=
fi
gen_reg_status=read
if [ "$debugfs_status" = unsupported_by_running_kernel ]; then
    gen_reg_status=unavailable_debugfs_unsupported
elif ! gen_reg=$(adb_cmd shell cat /sys/kernel/debug/ipa/gen_reg 2>&1); then
    gen_reg_status=unavailable_without_permission_changes
    gen_reg=
fi

# Read only the recent kernel log buffer through Android's existing logcat
# interface. Do not enable dynamic debug or change device logging settings.
if kernel_log=$(adb_cmd shell logcat -b kernel -d -t 2500 2>&1); then
    ipa_log=$(printf '%s\n' "$kernel_log" | tr -d '\r' | grep -E \
        'subsys-pil-tz .*ipa_fws: ipa_fws: (loading from|Brought out of reset)|PIL FW loading process (initiated|is complete) sub_sys=ipa_fws|IPA attach to smmu failed|IPA FW loaded successfully|IPA FW loading process has failed|IPA post init failed|IPA HW initialization sequence completed|error initializing HW|IPA gsi is registered|IPA GPI pipes were connected|IPA driver initialization was successful|Sending QMI_IPA_INIT_MODEM_DRIVER_REQ_V01|QMI_IPA_INIT_MODEM_DRIVER_REQ_V01 response received|QMI send Req 33 failed|Error sending qmi request id 33|Timeout for qmi request id 33|Got bad response .*request id 33|IPA_INIT_MODEM_SNAPSHOT (request|response)|ipa-wan .* (platform_type|hdr_tbl_info|v4_route_tbl_info|v6_route_tbl_info|v4_filter_tbl_start_addr|v6_filter_tbl_start_addr|modem_mem_info|ctrl_comm_dest_end_pt|is_ssr_bootup|v4_hash_route_tbl_info|v6_hash_route_tbl_info|v4_hash_filter_tbl_start_addr|v6_hash_filter_tbl_start_addr|Received .*successfully)' || true)
    if [ -n "$ipa_log" ]; then
        printf '%s\n' 'kernel_ipa_log=matching_entries'
        printf '%s\n' "$ipa_log"
    else
        printf '%s\n' 'kernel_ipa_log=no_matching_entries_in_last_2500_records'
    fi
else
    printf '%s\n' 'kernel_ipa_log=unavailable_without_permission_changes'
fi

# Capture only the relevant boot-order/readiness lines from Android's existing
# logcat buffers. This remains a passive snapshot: no service is restarted,
# no debug setting is changed, and unrelated log contents are not emitted.
if boot_log=$(adb_cmd logcat -b all -d -v epoch 2>&1); then
    order_log=$(printf '%s\n' "$boot_log" | tr -d '\r' | grep -Ei \
        "starting service '(vendor\\.qrtr-ns|vendor\\.per_mgr|per_proxy|vendor\\.mdm_helper|pd_mapper|vendor\\.ipacm|tftp_server|vendor\\.tftp_server|vendor\\.rmt_storage)'|started service '(vendor\\.qrtr-ns|vendor\\.per_mgr|per_proxy|vendor\\.mdm_helper|pd_mapper|vendor\\.ipacm|tftp_server|vendor\\.tftp_server|vendor\\.rmt_storage)' has pid|servloc: (service_locator_new_server: Connection established with the Service locator|init_service_locator: Service locator initialized)|subsys-restart: Changing subsys fw_name to modem|firmware: loading 'modem\\.(mdt|b[0-9][0-9])'|subsys-pil-tz .*modem: (Power/Clock ready interrupt received|Brought out of reset)|qrtr: Modem QMI Readiness|rmt_storage:.*(Done with init|Open Request|Read iovec request|Calling Read|Done Read)|tftp_server:.*|Sending QMI_IPA_INIT_MODEM_DRIVER_REQ_V01|QMI_IPA_INIT_MODEM_DRIVER_REQ_V01 response received|IPA_INIT_MODEM_SNAPSHOT (request|response)|service_notifier_new_server: Connection established between QMI handle and 180 service|root_service_service_ind_cb: Indication received from msm/modem/wlan_pd|send_ind_ack: Indication ACKed .*service msm/modem/wlan_pd|icnss_qmi: QMI Server Connected|icnss: WLAN FW is ready|wma_wait_for_ready_event: .*FW ready event received|wakeup_source: .*tftp_server_wakelock" || true)
    if [ -n "$order_log" ]; then
        printf '%s\n' 'lineage_modem_service_order=matching_entries'
        printf '%s\n' "$order_log"
    else
        printf '%s\n' 'lineage_modem_service_order=no_matching_entries_in_available_buffers'
    fi

    # Preserve only already-recorded Peripheral Manager server/client/proxy
    # and CNSS lifecycle lines needed to correlate modem voting with domain-up
    # and WLFW readiness. No logging configuration is changed.
    pm_log=$(printf '%s\n' "$boot_log" | tr -d '\r' | grep -Ei \
        'PerMgrSrv:.*(Peripheral Manager service start|modem state|voter/listener count|power on count|power off count)|PerMgrLib:.*(modem|register|vote|connect)|PerMgrProxy:.*(Starting pm-proxy service|Found internal modem|Voting for internal modem|Casting proxy vote|vote successful)|mdm_helper:.*(modem|wlan|wlfw|tftp|qmi|firmware|pdr)|cnss-daemon:.*(starting pm|failed to register pm client|failed to vote|event .*received on pm|vote for modem|release vote for modem|wlps service connected|wlfw service connected|failed to start wlfw service|failed to request wlan service)|pm_client_(register|connect|event_acknowledge)|pm_event_notifier|wlfw_start' || true)
    if [ -n "$pm_log" ]; then
        printf '%s\n' 'lineage_peripheral_manager=matching_entries'
        printf '%s\n' "$pm_log"
    else
        printf '%s\n' 'lineage_peripheral_manager=no_matching_entries_in_available_buffers'
    fi
else
    printf '%s\n' 'lineage_modem_service_order=unavailable_without_permission_changes'
fi

printf 'device=%s\nlineage=%s\n' "$product" "$lineage_version"
printf 'baseband=%s\nmdm_helper_service=%s\nmdm_helper_pid=%s\n' \
    "$baseband" "${mdm_helper_state:-unset}" "${mdm_helper_pid:-none}"
printf 'mdm_helper_script=%s\nmdm_helper_binary=%s\n' \
    "$mdm_helper_script_status" "$mdm_helper_binary_status"
printf 'debugfs_status=%s\n' "$debugfs_status"
printf 'hw_type_status=%s\n' "$hw_type_status"
if [ "$hw_type_status" = read ]; then
    printf 'hw_type=%s\n' "$(printf '%s' "$hw_type" | tr -d '\r')"
fi
printf 'gen_reg_status=%s\n' "$gen_reg_status"
if [ "$gen_reg_status" = read ]; then
    printf '%s\n' "$(printf '%s' "$gen_reg" | tr -d '\r')"
fi
