#!/usr/bin/env python3
"""Offline GSI protocol tests and a compiled oracle from the pinned Linux C.

No USB/hardware commands. Source files are verified against the exact kernel
archive. Register descriptors/encoders and the context programming functions
are compiled unchanged, replacing only kernel facilities and MMIO with host
fixtures. Rust vectors must match every captured write. The fixture models
the already prepared private pool, not hardware DMA or startup authority.
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
from datetime import datetime, timezone
import re
import shutil
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[2]
PIN = "2267e248204e7e051e0f7355298949a6440de579"
ARCHIVE_HASH = "d55c453cea03bc7aeb3381df2538da5423fb926ce6b88dd33eadecbc12a29cb2"
REFERENCE = ROOT / ".cache/sunfish-mobian-comparison/source-review/kernel"
ARCHIVE = ROOT / ".cache/sunfish-mobian-comparison/downloads/linux-2267e248.tar.gz"
OUT = ROOT / ".cache/sunfish-native-wifi/private/ipa-linux-gsi"
IPA = "drivers/net/ipa/"
INPUTS = [IPA + name for name in (
    "gsi.c", "gsi.h", "gsi_reg.h", "gsi_private.h", "reg.h", "ipa_version.h",
    "reg/gsi_reg-v4.0.c", "data/ipa_data-v4.2.c", "ipa_endpoint.h",
    "ipa_uc.h", "ipa_modem.c", "ipa_main.c",
)] + ["arch/arm64/boot/dts/qcom/sm7150.dtsi"]


def sha(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def run(args: list[str], name: str, env=None, expected=0) -> subprocess.CompletedProcess:
    p = subprocess.run(args, cwd=ROOT, env=env, text=True, capture_output=True)
    (OUT / (name + ".log")).write_text(p.stdout + p.stderr)
    if p.returncode != expected:
        raise SystemExit(f"{name}: exit {p.returncode}; see {OUT / (name + '.log')}")
    return p


def strip_includes(text: str) -> str:
    return re.sub(r"^#include[^\n]*\n", "", text, flags=re.M)


def function(text: str, name: str) -> str:
    match = re.search(r"^(?:static\s+)?(?:void|int|u32|bool|enum\s+\w+)\s+" + re.escape(name) + r"\(", text, re.M)
    assert match, name
    brace = text.index("{", match.start())
    depth = 1
    end = brace + 1
    while depth:
        if text[end] == "{": depth += 1
        elif text[end] == "}": depth -= 1
        end += 1
    return text[match.start():end] + "\n"


def c_oracle(source: dict[str, str]) -> str:
    gsi = source[IPA + "gsi.c"]
    scratch = gsi[gsi.index("struct gsi_channel_scratch_gpi {"):gsi.index("/* Check things that can be validated at build time.")]
    prefix = r'''
#include <stdint.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
typedef uint8_t u8;
typedef uint16_t u16;
typedef uint32_t u32;
typedef uint64_t u64;
#define BIT(n) (UINT32_C(1) << (n))
#define GENMASK(h,l) ((UINT32_MAX >> (31 - (h))) & (UINT32_MAX << (l)))
#define ARRAY_SIZE(a) (sizeof(a) / sizeof((a)[0]))
#define __ffs(v) __builtin_ctz(v)
#define __fls(v) (31 - __builtin_clz(v))
#define __iomem
#define msecs_to_jiffies(n) (n)
#define hweight32(v) __builtin_popcount(v)
#define lower_32_bits(v) ((u32)(v))
#define upper_32_bits(v) ((u32)((u64)(v) >> 32))
#define GSI_RING_ELEMENT_SIZE 16
#define GSI_EVT_RING_INT_MODT 32
#define GSI_CMD_TIMEOUT 50
#define GSI_CHANNEL_COUNT_MAX 28
#define GSI_EVT_RING_COUNT_MAX 28
#define dev_err(dev,...) ((void)(dev))
#define dev_warn(dev,...) ((void)(dev))
static bool warn_on(bool condition) { if (condition) abort(); return false; }
#define WARN_ON(c) warn_on(c)
#define WARN(c,...) warn_on(c)
static bool is_power_of_2(u32 v) { return v && !(v & (v - 1)); }
'''
    structs = r'''
struct napi_struct { int unused; };
struct completion { int count; };
struct device;
struct gsi_ring { u64 addr; u32 count; u32 index; };
struct gsi_channel { struct gsi *gsi; struct gsi_ring tre_ring;
    bool toward_ipa; bool command; u8 evt_ring_id; u8 trans_tre_max; struct napi_struct napi; };
struct gsi_evt_ring { struct gsi_ring ring; };
struct gsi { enum ipa_version version; void *virt; struct device *dev;
    u32 irq; u32 type_enabled_bitmap; int result; struct completion completion; void *dummy_dev;
    u32 channel_count; u32 evt_ring_count; u32 modem_channel_bitmap; int mutex;
    struct gsi_channel channel[31]; struct gsi_evt_ring evt_ring[31]; };
const struct reg *gsi_reg(struct gsi *gsi, enum gsi_reg_id id) {
    (void)gsi; return reg(&gsi_regs_v4_0, id);
}
static u8 mmio[0x2c000];
static u32 scratch_input;
static u32 channel_index;
static bool setup_mode;
static u32 setup_result;
static u32 registers[0x2c000 / 4];
static u32 pending_command;
static struct gsi *active_gsi;
static void iowrite32(u32 value, void *address) {
    size_t offset = (u8 *)address - mmio;
    if (!setup_mode) {
        printf("%08x %u %05lx %08x\n", scratch_input, channel_index,
               (unsigned long)offset, value);
        return;
    }
    printf("setup %u %05lx %08x\n", setup_result, (unsigned long)offset, value);
    if (offset == 0x1f0a0) registers[0x1f090 / 4] &= ~value;
    else if (offset == 0x1f0a4) registers[0x1f094 / 4] &= ~value;
    else if (offset == 0x1f110) registers[0x1f100 / 4] &= ~value;
    else if (offset == 0x1f008 || offset == 0x1f010) {
        u32 id = value & 0xff;
        if (value >> 24) abort(); /* ALLOCATE only */
        pending_command = offset;
        registers[(offset == 0x1f008 ? 0x1c000 : 0x1d000) / 4 + id * 0x80 / 4] = 1 << 20;
        registers[(offset == 0x1f008 ? 0x1f090 : 0x1f094) / 4] = 1 << id;
    } else if (offset == 0x1f018) {
        if ((value & 31) != 2) abort(); /* modem ALLOCATE only */
        pending_command = offset;
        registers[0x1f100 / 4] = 2;
        registers[0x1f400 / 4] = (registers[0x1f400 / 4] & ~(7 << 5)) | (setup_result << 5);
    } else if (offset >= 0x1c000 && offset < 0x1d200 && offset % 0x80 == 0)
        registers[offset / 4] = (value & ~(15 << 20)) | (registers[offset / 4] & (15 << 20));
    else registers[offset / 4] = value;
}
static u32 ioread32(void *address) {
    size_t offset = (u8 *)address - mmio;
    if (setup_mode) return registers[offset / 4];
    if (offset != 0x1c06c + channel_index * 0x80) abort();
    return scratch_input;
}
static void reinit_completion(struct completion *c) { c->count = 0; }
static void complete(struct completion *c) { c->count++; }
static unsigned long wait_for_completion_timeout(struct completion *c, unsigned long timeout);
static void gsi_isr(void) { abort(); }
static int request_irq(u32 irq, void (*handler)(void), u32 flags, const char *name, void *data) {
    (void)handler; (void)flags; (void)name; (void)data;
    if (irq != 464 || registers[0x1f088 / 4] || registers[0x1f098 / 4] || registers[0x1f09c / 4]) abort();
    return 0;
}
static int gsi_channel_poll(struct napi_struct *napi, int budget) { (void)napi; return budget; }
#define netif_napi_add(dev,napi,poll) ((void)(dev), (void)(napi), (void)(poll))
#define netif_napi_add_tx(dev,napi,poll) netif_napi_add(dev,napi,poll)
static void gsi_evt_ring_de_alloc_command(struct gsi *gsi, u32 id) { (void)gsi; (void)id; abort(); }
static void gsi_isr_glob_err(struct gsi *gsi) { (void)gsi; abort(); }
static void mutex_lock(int *mutex) { (void)mutex; }
static void mutex_unlock(int *mutex) { (void)mutex; }
static void gsi_modem_channel_halt(struct gsi *gsi, u32 id) { (void)gsi; (void)id; abort(); }
static void gsi_channel_teardown_one(struct gsi *gsi, u32 id) { (void)gsi; (void)id; abort(); }
static void gsi_irq_disable(struct gsi *gsi) { (void)gsi; abort(); }
static void gsi_irq_teardown(struct gsi *gsi) { (void)gsi; abort(); }
'''
    # The four pool addresses are test inputs. The C encoders and program
    # functions below, including scratch preservation and doorbell arithmetic,
    # are the actual source, not a second hand translation of the Rust plan.
    main = r'''
int main(void) {
    static const u32 scratches[] = {0, 0xffffffff, 0xabcd1234, 0xffff, 0xffff0000};
    static const u8 event_ids[] = {2, 0, 1, 3};
    static const u32 counts[] = {512, 256, 256, 256};
    static const u8 tlvs[] = {8, 20, 6, 6};
    static const u64 tre[] = {0x8000c000, 0x80004000, 0x80009000, 0x8000f000};
    static const u64 event[] = {0x8000a000, 0x80003000, 0x80008000, 0x8000e000};
    struct gsi g = {.version = IPA_VERSION_4_2, .virt = mmio};
    if (reg_offset(gsi_reg(&g, GSI_STATUS)) != 0x1f000 ||
        reg_decode(gsi_reg(&g, HW_PARAM_2), NUM_CH_PER_EE, 0xe4046b59) != 11 ||
        reg_decode(gsi_reg(&g, HW_PARAM_2), NUM_EV_PER_EE, 0xe4046b59) != 11)
        abort();
    for (size_t s = 0; s < ARRAY_SIZE(scratches); s++) {
        scratch_input = scratches[s];
        for (u32 i = 0; i < 4; i++) {
            channel_index = i;
            struct gsi_channel *c = &g.channel[i];
            c->gsi = &g; c->toward_ipa = i < 2; c->command = i == 1;
            c->evt_ring_id = event_ids[i]; c->trans_tre_max = tlvs[i];
            c->tre_ring = (struct gsi_ring){.addr = tre[i], .count = counts[i]};
            g.evt_ring[event_ids[i]].ring = (struct gsi_ring){.addr = event[i], .count = counts[i]};
            gsi_evt_ring_program(&g, event_ids[i]);
            gsi_channel_program(c, true);
        }
    }
    setup_mode = true;
    for (setup_result = 1; setup_result <= 2; setup_result++) {
        memset(registers, 0, sizeof(registers));
        g.type_enabled_bitmap = 0; g.irq = 464; active_gsi = &g;
        g.modem_channel_bitmap = 15;
        registers[0x1f000 / 4] = 1;
        registers[0x1f040 / 4] = 0xe4046b59;
        /* The real gsi_setup(), not a hand-written log initialization,
         * must accept and initialize a nonzero pre-initialization log. */
        registers[0x1f200 / 4] = setup_result == 1 ? 0xe71132f0 : 0xffffffff;
        for (u32 i = 0; i < 4; i++) g.evt_ring[event_ids[i]].ring.index = 0;
        if (gsi_setup(&g)) abort();
        if (registers[0x1f200 / 4] || g.channel_count != 11 || g.evt_ring_count != 11) abort();
    }
}
'''
    names = ["gsi_channel_id", "gsi_channel_initialized", "ch_c_cntxt_0_type_encode", "gsi_ring_addr",
             "gsi_evt_ring_doorbell", "gsi_evt_ring_program", "gsi_channel_program",
             "gsi_irq_type_update", "gsi_irq_type_enable", "gsi_irq_type_disable",
             "gsi_irq_ev_ctrl_enable", "gsi_irq_ev_ctrl_disable", "gsi_irq_ch_ctrl_enable", "gsi_irq_ch_ctrl_disable",
             "gsi_irq_enable", "gsi_irq_setup", "gsi_isr_chan_ctrl", "gsi_isr_evt_ctrl", "gsi_isr_gp_int1", "gsi_isr_glob_ee",
             "gsi_command", "gsi_evt_ring_state", "gsi_evt_ring_command", "gsi_evt_ring_alloc_command",
             "gsi_channel_state", "gsi_channel_command", "gsi_channel_alloc_command",
             "gsi_generic_command", "gsi_modem_channel_alloc", "gsi_channel_setup_one",
             "gsi_channel_setup", "gsi_ring_setup", "gsi_setup"]
    states = "\n".join(re.search(r"enum " + name + r"\s*\{.*?\};", source[IPA+"gsi.h"], re.S).group()
                         for name in ("gsi_channel_state", "gsi_evt_ring_state"))
    wait = r'''
static unsigned long wait_for_completion_timeout(struct completion *c, unsigned long timeout) {
    if (!timeout) abort();
    if (pending_command == 0x1f008) gsi_isr_chan_ctrl(active_gsi);
    else if (pending_command == 0x1f010) gsi_isr_evt_ctrl(active_gsi);
    else if (pending_command == 0x1f018) gsi_isr_glob_ee(active_gsi);
    else abort();
    pending_command = 0;
    return c->count;
}
'''
    return "\n".join([prefix, strip_includes(source[IPA + "ipa_version.h"]),
        strip_includes(source[IPA + "gsi_reg.h"]), strip_includes(source[IPA + "reg.h"]),
        strip_includes(source[IPA + "reg/gsi_reg-v4.0.c"]), states, structs, scratch,
        *(function(gsi, name) for name in names), wait, main])


def main():
    OUT.mkdir(parents=True, exist_ok=True, mode=0o700)
    assert sha(ARCHIVE) == ARCHIVE_HASH, "kernel archive identity changed"
    snapshots = OUT / "linux-source"
    source = {}
    with tarfile.open(ARCHIVE) as archive:
        prefix = archive.getnames()[0].split("/")[0] + "/"
        for rel in INPUTS:
            body = archive.extractfile(prefix + rel).read()
            assert (REFERENCE / rel).read_bytes() == body, rel
            p = snapshots / rel
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(body)
            source[rel] = body.decode()
    dts = source[INPUTS[-1]]
    block = dts[dts.index("ipa: ipa@1e40000"):]
    assert re.search(r"GIC_SPI 311 IRQ_TYPE_EDGE_RISING.*?GIC_SPI 432 IRQ_TYPE_LEVEL_HIGH", block, re.S)
    # Bind the order/meaning of fixture inputs to source declarations; the
    # actual C context functions independently encode those inputs.
    endpoints = source[IPA + "data/ipa_data-v4.2.c"]
    for name, channel, endpoint, count, tlvs in [
        ("AP_COMMAND_TX",1,6,256,20),("AP_LAN_RX",2,8,256,6),
        ("AP_MODEM_TX",0,1,512,8),("AP_MODEM_RX",3,9,256,6)]:
        block = endpoints.split("[IPA_ENDPOINT_" + name + "] = {",1)[1].split("\n\t},",1)[0]
        for field,value in [("channel_id",channel),("endpoint_id",endpoint),
                            ("tre_count",count),("event_count",count),("tlv_count",tlvs)]:
            assert re.search(r"\."+field+r"\s*=\s*"+str(value)+r"\b",block), (name,field)
    cpath = OUT / "linux-context-oracle.c"
    cpath.write_text(c_oracle(source))
    cc = shutil.which("clang") or shutil.which("cc")
    assert cc, "C compiler required"
    run([cc,"-std=gnu11","-Wall","-Wextra","-Werror",str(cpath),"-o",str(OUT/"linux-context-oracle")],"c-build")
    oracle = run([str(OUT/"linux-context-oracle")],"c-oracle").stdout
    contexts = [line for line in oracle.splitlines(True) if not line.startswith("setup ")]
    setup = [line for line in oracle.splitlines(True) if line.startswith("setup ")]
    assert len(contexts) == 400 and len(setup) == 340
    golden = OUT / "linux-context-writes.txt"
    golden.write_text("".join(contexts))
    setup_golden = OUT / "linux-setup-writes.txt"
    setup_golden.write_text("".join(setup))
    rustc = shutil.which("rustc")
    assert rustc, "rustc required"
    test = OUT / "gsi-test"
    run([rustc,"--edition=2024","-D","warnings","--test",str(ROOT/"tests/sunfish_ipa_linux_gsi_test.rs"),"-o",str(test)],"rust-build")
    env = dict(os.environ,SUNFISH_GSI_C_REFERENCE=str(golden),SUNFISH_GSI_C_SETUP_REFERENCE=str(setup_golden))
    tested = run([str(test)],"rust-tests",env)
    groups = int(re.search(r"test result: ok\. (\d+) passed;", tested.stdout).group(1))
    wrapper = OUT / "core.rs"
    wrapper.write_text('#![no_std]\n' + "\n".join(
        f'#[path = "{ROOT / "rust/src" / (name + ".rs")}"]\npub mod {name};'
        for name in ("sunfish_ipa_linux_buffers","sunfish_ipa_linux_gsi")))
    run([rustc,"--edition=2024","-D","warnings","--crate-type=lib","--target=aarch64-unknown-none",str(wrapper),"-o",str(OUT/"gsi-aarch64.rlib")],"aarch64-core")
    mutations = {
        "wrong-event-index":"u32::from(ch.event) << 14|u32::from(ch.id) << 14",
        "reversed-scratch-half":"scratch3 & 0xffff|scratch3 & 0xffff0000",
        "wrong-doorbell":"u64::from(ch.event_bytes) - 16|u64::from(ch.event_bytes)",
        "wrong-direction":"(ch.toward_ipa as u32) << 3|(!ch.toward_ipa as u32) << 3",
    }
    original = (ROOT/"rust/src/sunfish_ipa_linux_gsi.rs").read_text()
    harness = (ROOT/"tests/sunfish_ipa_linux_gsi_test.rs").read_text()
    for name, replacements in mutations.items():
        old,new = replacements.split("|")
        assert original.count(old) == 1, (name,old)
        mutated = OUT/(name+".rs")
        mutated.write_text(original.replace(old,new))
        fixture = OUT/(name+"-test.rs")
        fixture.write_text(harness.replace("../rust/src/sunfish_ipa_linux_buffers.rs",str(ROOT/"rust/src/sunfish_ipa_linux_buffers.rs")).replace("../rust/src/sunfish_ipa_linux_gsi.rs",str(mutated)))
        binary = OUT/(name+"-test")
        run([rustc,"--edition=2024","-D","warnings","--test","--crate-name","gsi_mutation",str(fixture),"-o",str(binary)],name+"-build")
        run([str(binary),"--exact","context_words_match_compiled_pinned_c_functions"],name,env,expected=101)
    selected_sources = ["rust/src/sunfish_ipa_linux_gsi.rs", "rust/src/sunfish_ipa_linux_buffers.rs", "rust/src/lib.rs",
        "tests/sunfish_ipa_linux_gsi_test.rs", "platforms/sunfish/test-native-ipa-linux-gsi.py", "platforms/sunfish/test-modem-qmi.py"]
    for rel in selected_sources:
        dst = OUT / "source" / rel
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / rel, dst)
    manifest = {"created_utc":datetime.now(timezone.utc).isoformat(), "linux_commit":PIN,"linux_archive_sha256":ARCHIVE_HASH,
        "reference_sha256":{rel:sha(snapshots/rel) for rel in INPUTS},
        "source_sha256":{rel:sha(ROOT/rel) for rel in selected_sources},
        "c_write_comparisons":740,"encoder_mutations_rejected":len(mutations),
        "rust_behavioral_groups":groups,"aarch64_no_std":True,"hardware_commands":[],
        "limits":"Offline AP/modem allocation/context core only; no native GSI backend or authority, channel start, endpoint setup or functional DMA yet."}
    manifest["artifacts"] = {p.name:{"bytes":p.stat().st_size,"sha256":sha(p)}
        for p in OUT.iterdir() if p.is_file() and p.name != "manifest.json"}
    (OUT/"manifest.json").write_text(json.dumps(manifest,indent=2)+"\n")
    print(f"PASS: {groups} Rust groups, 740 pinned-C writes, {len(mutations)} rejected encoder mutations, AArch64 no_std; {OUT}")


if __name__ == "__main__":
    main()
