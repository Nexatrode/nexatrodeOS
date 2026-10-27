#!/usr/bin/env python3
"""Test the loader source guard's diagnostic-only branches and display."""
from __future__ import annotations

import importlib.util
import inspect
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PORT = ROOT / "platforms" / "sunfish"
ANCHOR = "    /* May not have been loaded in the right location."
ENTRY = ANCHOR + "\n    bl     relocate\n    ret\n"


def load_prepare():
    spec = importlib.util.spec_from_file_location(
        "prepare_image_loader_sdk", PORT / "prepare-image-loader-sdk.py"
    )
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def load_boot_audit():
    spec = importlib.util.spec_from_file_location(
        "validate_native_mpss_auth_boot", PORT / "validate-native-mpss-auth-boot.py"
    )
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def load_runtime_layout_audit():
    spec = importlib.util.spec_from_file_location(
        "audit_native_mpss_runtime_layout", PORT / "audit-native-mpss-runtime-layout.py"
    )
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


class NativeSourceGuardDiagnosticTests(unittest.TestCase):
    def test_hex_formatter_renders_all_sixteen_uppercase_digits_on_value_line(self):
        header = PORT / "loader-boot-checkpoints.h"
        self.assertIn("void loader_checkpoint_hex(const char *title, uint64_t value)",
                      header.read_text())
        source = r'''
#include <assert.h>
#include <stdint.h>
#include <stdlib.h>
static volatile uint32_t *test_fb;
#define CHECKPOINT_FB test_fb
#include "loader-boot-checkpoints.h"

int main(void) {
    const unsigned width = 1080, height = 2340;
    const size_t pixels = (size_t)width * height;
    uint32_t *storage = calloc(2 * pixels, sizeof(uint32_t));
    assert(storage);
    test_fb = storage;
    loader_checkpoint_hex("SOURCE ADDRESS", UINT64_C(0x0123456789abcdef));
    loader_checkpoint_hex("SOURCE EXTENT", UINT64_C(0xfedcba9876543210));
    const char *expected[] = {"0123456789ABCDEF", "FEDCBA9876543210"};
    for (unsigned row = 0; row < 2; row++) {
        for (unsigned column = 0; column < 16; column++) {
            unsigned c = (unsigned char)expected[row][column];
            unsigned glyph = c >= 'A' ? c - 'A' : 26 + c - '0';
            unsigned x = 48 + column * 6 * 7;
            for (unsigned gy = 0; gy < 7; gy++) {
                for (unsigned gx = 0; gx < 5; gx++) {
                    uint32_t color = (checkpoint_font[glyph][gy] & (1u << (4 - gx)))
                        ? 0x0000e5e5 : 0x00122b52;
                    for (unsigned dy = 0; dy < 7; dy++)
                        for (unsigned dx = 0; dx < 7; dx++)
                            assert(storage[(230 + row * 120 + gy * 7 + dy) * width + x + gx * 7 + dx] == color);
                }
            }
        }
    }
    free(storage);
    return 0;
}
'''
        with tempfile.TemporaryDirectory() as temporary:
            c_path = Path(temporary) / "hex-check.c"
            binary = Path(temporary) / "hex-check"
            c_path.write_text(source)
            subprocess.run(
                ["cc", "-std=c11", "-Wall", "-Wextra", "-Werror", "-I", str(PORT),
                 str(c_path), "-o", str(binary)], check=True, capture_output=True, text=True
            )
            subprocess.run([str(binary)], check=True, capture_output=True, text=True)

    def test_guard_distinguishes_each_failed_check_and_preserves_values(self):
        prepare = load_prepare()
        self.assertIn("diagnostic_only", inspect.signature(prepare.native_startup_entry).parameters)
        code = prepare.native_startup_entry(ENTRY, diagnostic_only=True)
        for branch in (
            "b.ne .Lnative_source_address_fail",
            "b.hi .Lnative_source_prefix_fail",
            "b.cs .Lnative_source_overflow_fail",
            "b.hi .Lnative_source_extent_fail",
        ):
            with self.subTest(branch=branch):
                self.assertIn(branch, code)
        for title, register in (
            ("SOURCE ADDRESS", "x0"),
            ("SOURCE PREFIX SIZE", "x3"),
            ("SOURCE SIZE OVERFLOW", "x3"),
            ("SOURCE EXTENT", "x3"),
        ):
            with self.subTest(title=title):
                label = ".Lnative_source_" + {
                    "SOURCE ADDRESS": "address",
                    "SOURCE PREFIX SIZE": "prefix",
                    "SOURCE SIZE OVERFLOW": "overflow",
                    "SOURCE EXTENT": "extent",
                }[title] + "_fail:"
                self.assertRegex(
                    code,
                    re.escape(label) + r"\n\s+mov x1, " + register
                    + r"\n\s+adr x0, \.Lnative_source_[a-z_]+_text\n\s+bl loader_checkpoint_hex",
                )
                self.assertIn(f'.asciz "{title}"', code)

    def test_source_guard_acceptance_addresses_and_bounds_are_unchanged(self):
        prepare = load_prepare()
        self.assertEqual(prepare.LINK, 0xC0000000)
        self.assertEqual(prepare.RUNTIME_SOURCE, 0xB1281000)
        self.assertIn("diagnostic_only", inspect.signature(prepare.native_startup_entry).parameters)
        for diagnostic_only in (False, True):
            with self.subTest(diagnostic_only=diagnostic_only):
                code = prepare.native_startup_entry(ENTRY, diagnostic_only=diagnostic_only)
                guard = code.split(ANCHOR, 1)[0]
                self.assertIn("ldr x1, =LINK_ADDRESS", guard)
                self.assertIn("ldr x1, =0xb1281000", guard)
                self.assertNotIn("0xb0081000", guard)
                self.assertNotIn("0xb1282000", guard)
                self.assertRegex(
                    guard,
                    r"ldr x1, =0xb1281000\n\s+cmp x0, x1\n\s+b\.ne \.Lnative_source_(?:address_fail|fail)",
                )
                self.assertIn("ldr x1, =0x20000", guard)
                self.assertIn("b.hi .Lnative_source_", guard)
                self.assertIn("ldr x1, =0x6000000", guard)
                self.assertIn("adds x3, x3, x4", guard)
                self.assertIn("b.cs .Lnative_source_", guard)
                self.assertLess(guard.index("ldr x1, =0x20000"), code.index("    bl     relocate"))
                self.assertLess(guard.index("ldr x1, =0x6000000"), code.index("    bl     relocate"))

    def test_runtime_layout_audit_derives_source_bounds_from_prepare_constant(self):
        prepare = load_prepare()
        audit = load_runtime_layout_audit()
        self.assertEqual(
            audit.runtime_source_bounds(prepare),
            (prepare.RUNTIME_SOURCE - 0x1000, prepare.RUNTIME_SOURCE),
        )
        self.assertEqual(audit.runtime_source_bounds(prepare), (0xB1280000, 0xB1281000))

    def test_diagnostic_mode_stops_before_relocation_on_fail_and_pass(self):
        prepare = load_prepare()
        self.assertIn("diagnostic_only", inspect.signature(prepare.native_startup_entry).parameters)
        code = prepare.native_startup_entry(ENTRY, diagnostic_only=True)
        guard = code.split(ANCHOR, 1)[0]
        self.assertIn(".Lnative_source_diagnostic_stop:", guard)
        self.assertIn("b .Lnative_source_diagnostic_stop", guard)
        self.assertIn('adr x0, .Lnative_source_address_text', guard)
        self.assertIn('adr x0, .Lnative_source_extent_text', guard)
        self.assertLess(guard.index(".Lnative_source_ok:"), guard.index(".Lnative_source_diagnostic_stop:"))
        self.assertGreaterEqual(guard.count("bl loader_checkpoint_hex"), 6)
        self.assertNotIn("bl     relocate", guard)

    def test_normal_auth_path_keeps_its_continue_behavior(self):
        prepare = load_prepare()
        code = prepare.native_startup_entry(ENTRY)
        guard = code.split(ANCHOR, 1)[0]
        self.assertIn(".Lnative_source_ok:", guard)
        self.assertNotIn(".Lnative_source_diagnostic_stop:", guard)
        self.assertIn("bl loader_checkpoint", code)
        self.assertIn("bl     relocate", code)

    def test_compiled_guard_cfg_rejects_escape_and_indirect_transfer(self):
        audit = load_boot_audit()
        rows = {
            0: ("cmp", "", None, None, "_text"),
            4: ("b.eq", "0xc", 12, None, "_text"),
            8: ("nop", "", None, None, "_text"),
            12: ("bl", "0x40 <loader_checkpoint_hex>", 64, "loader_checkpoint_hex", "_text"),
            16: ("b", "0x1c", 28, None, "_text"),
            28: ("b", "0x1c", 28, None, "_text"),
        }
        result = audit.audit_guard_control_flow(
            rows, entry=0, relocation_call=32, stop=28,
            allowed_helpers={"loader_checkpoint_hex"})
        self.assertTrue(result["all_paths_stop_before_relocation"])
        self.assertEqual(result["stop_loop_address"], "0x1c")
        for address, bad_row in (
            (8, ("b", "0x20", 32, None, "_text")),
            (8, ("br", "x0", None, None, "_text")),
            (8, ("b", "0x14", 20, None, "_text")),
        ):
            with self.subTest(bad_row=bad_row):
                mutant = dict(rows)
                mutant[address] = bad_row
                with self.assertRaises(ValueError):
                    audit.audit_guard_control_flow(
                        mutant, entry=0, relocation_call=32, stop=28,
                        allowed_helpers={"loader_checkpoint_hex"})

    def test_closed_display_helper_cfg_accepts_local_call_and_lr_restore(self):
        audit = load_boot_audit()
        symbols = {
            "loader_checkpoint": 0x100,
            "loader_checkpoint_hex": 0x200,
            "loader_checkpoint_draw_line": 0x300,
            "next_symbol": 0x400,
        }
        instructions = {
            0x100: ("sub", "sp, sp, #0x20", None, None, "loader_checkpoint"),
            0x104: ("stp", "x29, x30, [sp, #0x10]", None, None, "loader_checkpoint"),
            0x108: ("add", "x30, x20, x27", None, None, "loader_checkpoint"),
            0x10C: ("bl", "0x300 <loader_checkpoint_draw_line>", 0x300,
                    "loader_checkpoint_draw_line", "loader_checkpoint"),
            0x110: ("ldp", "x29, x30, [sp, #0x10]", None, None, "loader_checkpoint"),
            0x114: ("add", "sp, sp, #0x20", None, None, "loader_checkpoint"),
            0x118: ("ret", "", None, None, "loader_checkpoint"),
            0x200: ("ret", "", None, None, "loader_checkpoint_hex"),
            0x300: ("ret", "", None, None, "loader_checkpoint_draw_line"),
        }
        result = audit.audit_closed_display_helpers(symbols, instructions,
                                                     (0xc0007000, 0xc0008000))
        self.assertEqual(result["helpers_checked"], 3)

    def test_closed_display_helper_cfg_rejects_alternate_return_register(self):
        audit = load_boot_audit()
        symbols = {
            "loader_checkpoint": 0x100,
            "loader_checkpoint_hex": 0x200,
            "loader_checkpoint_draw_line": 0x300,
            "next_symbol": 0x400,
        }
        instructions = {
            0x100: ("ret", "x0", None, None, "loader_checkpoint"),
            0x200: ("ret", "", None, None, "loader_checkpoint_hex"),
            0x300: ("ret", "", None, None, "loader_checkpoint_draw_line"),
        }
        with self.assertRaisesRegex(ValueError, "normal LR return"):
            audit.audit_closed_display_helpers(symbols, instructions,
                                                (0xc0007000, 0xc0008000))

    def test_closed_display_helper_cfg_rejects_unpreserved_lr_scratch(self):
        audit = load_boot_audit()
        symbols = {
            "loader_checkpoint": 0x100,
            "loader_checkpoint_hex": 0x200,
            "loader_checkpoint_draw_line": 0x300,
            "next_symbol": 0x400,
        }
        instructions = {
            0x100: ("add", "x30, x20, x27", None, None, "loader_checkpoint"),
            0x104: ("ret", "", None, None, "loader_checkpoint"),
            0x200: ("ret", "", None, None, "loader_checkpoint_hex"),
            0x300: ("ret", "", None, None, "loader_checkpoint_draw_line"),
        }
        with self.assertRaisesRegex(ValueError, "preserves LR before reuse"):
            audit.audit_closed_display_helpers(symbols, instructions,
                                                (0xc0007000, 0xc0008000))

    def test_closed_display_helper_cfg_rejects_mismatched_or_duplicate_lr_restore(self):
        audit = load_boot_audit()
        symbols = {
            "loader_checkpoint": 0x100,
            "loader_checkpoint_hex": 0x200,
            "loader_checkpoint_draw_line": 0x300,
            "next_symbol": 0x400,
        }
        tails = {
            0x200: ("ret", "", None, None, "loader_checkpoint_hex"),
            0x300: ("ret", "", None, None, "loader_checkpoint_draw_line"),
        }
        prefix = {
            0x100: ("stp", "x29, x30, [sp, #-0x10]!", None, None, "loader_checkpoint"),
        }
        mutants = (
            {
                0x104: ("ldp", "x29, x30, [sp, #0x8]", None, None, "loader_checkpoint"),
                0x108: ("ret", "", None, None, "loader_checkpoint"),
            },
            {
                0x104: ("ldp", "x29, x30, [sp], #0x10", None, None, "loader_checkpoint"),
                0x108: ("ldp", "x29, x30, [sp], #0x10", None, None, "loader_checkpoint"),
                0x10C: ("ret", "", None, None, "loader_checkpoint"),
            },
        )
        for mutant in mutants:
            with self.subTest(mutant=mutant):
                with self.assertRaises(ValueError):
                    audit.audit_closed_display_helpers(
                        symbols, {**tails, **prefix, **mutant},
                        (0xc0007000, 0xc0008000))

    def test_closed_display_helper_cfg_rejects_store_over_saved_lr_word(self):
        audit = load_boot_audit()
        symbols = {
            "loader_checkpoint": 0x100,
            "loader_checkpoint_hex": 0x200,
            "loader_checkpoint_draw_line": 0x300,
            "next_symbol": 0x400,
        }
        tails = {
            0x200: ("ret", "", None, None, "loader_checkpoint_hex"),
            0x300: ("ret", "", None, None, "loader_checkpoint_draw_line"),
        }
        prefix = {
            0x100: ("stp", "x29, x30, [sp, #-0x10]!", None, None, "loader_checkpoint"),
        }
        mutants = (
            (("str", "x0, [sp, #0x8]"),),
            (("stp", "x0, x1, [sp]"),),
            (("stp", "xzr, x0, [sp]"),),
            (("str", "x0, [sp, x2]"),),
            (("cas", "x0, x1, [sp, #0x8]"),),
            (("caspal", "x0, x1, x2, x3, [sp]"),),
            (("stgp", "x0, x1, [sp]"),),
            (("mov", "x29, sp"), ("str", "x0, [x29, #0x8]")),
            (("mov", "x29, sp"), ("str", "x0, [x29, x2]")),
            (("mov", "x29, sp"), ("movz", "x10, #4"),
             ("add", "x9, x29, x10, lsl #1"), ("strb", "w0, [x9]")),
        )
        for steps in mutants:
            with self.subTest(steps=steps):
                instructions = {**tails, **prefix}
                address = 0x104
                for mnemonic, operands in steps:
                    instructions[address] = (mnemonic, operands, None, None, "loader_checkpoint")
                    address += 4
                instructions[address] = ("ldp", "x29, x30, [sp], #0x10", None,
                                         None, "loader_checkpoint")
                address += 4
                instructions[address] = ("ret", "", None, None, "loader_checkpoint")
                with self.assertRaises(ValueError):
                    audit.audit_closed_display_helpers(
                        symbols, instructions, (0xc0007000, 0xc0008000))

    def test_closed_display_helper_cfg_accounts_for_two_granule_tag_stores(self):
        audit = load_boot_audit()
        symbols = {
            "loader_checkpoint": 0x100,
            "loader_checkpoint_hex": 0x200,
            "loader_checkpoint_draw_line": 0x300,
            "next_symbol": 0x400,
        }
        tails = {
            0x200: ("ret", "", None, None, "loader_checkpoint_hex"),
            0x300: ("ret", "", None, None, "loader_checkpoint_draw_line"),
        }
        for mnemonic in ("st2g", "stz2g"):
            with self.subTest(mnemonic=mnemonic):
                instructions = {
                    **tails,
                    0x100: ("stp", "x29, x30, [sp, #-0x20]!", None,
                            None, "loader_checkpoint"),
                    0x104: (mnemonic, "x0, [sp, #-0x10]", None,
                            None, "loader_checkpoint"),
                    0x108: ("ldp", "x29, x30, [sp], #0x20", None,
                            None, "loader_checkpoint"),
                    0x10C: ("ret", "", None, None, "loader_checkpoint"),
                }
                with self.assertRaisesRegex(ValueError, "does not overwrite saved LR"):
                    audit.audit_closed_display_helpers(
                        symbols, instructions, (0xc0007000, 0xc0008000))

    def test_closed_display_helper_cfg_rejects_unproven_absolute_store_bases(self):
        audit = load_boot_audit()
        symbols = {
            "loader_checkpoint": 0x100,
            "loader_checkpoint_hex": 0x200,
            "loader_checkpoint_draw_line": 0x300,
            "next_symbol": 0x400,
        }
        tails = {
            0x200: ("ret", "", None, None, "loader_checkpoint_hex"),
            0x300: ("ret", "", None, None, "loader_checkpoint_draw_line"),
        }
        prefix = {
            0x100: ("stp", "x29, x30, [sp, #-0x10]!", None, None, "loader_checkpoint"),
        }
        mutants = (
            ((('adr', 'x9, 0xc0007ff8 <_stack_alias>', 0xc0007ff8,
               '_stack_alias'),), ('str', 'x0, [x9]')),
            ((('adrp', 'x9, 0xc0007000 <_stack_page>', 0xc0007000,
               '_stack_page'),
              ('add', 'x9, x9, #0xff8', None, None)), ('str', 'x0, [x9]')),
            ((('movz', 'x9, #0x7ff8', None, None),
              ('movk', 'x9, #0xc000, lsl #16', None, None)), ('str', 'x0, [x9]')),
        )
        for address_instructions, store in mutants:
            with self.subTest(address_instructions=address_instructions):
                instructions = {**tails, **prefix}
                address = 0x104
                for mnemonic, operands, target, target_name in address_instructions:
                    instructions[address] = (mnemonic, operands, target,
                                             target_name, "loader_checkpoint")
                    address += 4
                instructions[address] = (store[0], store[1], None, None,
                                         "loader_checkpoint")
                address += 4
                instructions[address] = ("ldp", "x29, x30, [sp], #0x10", None,
                                         None, "loader_checkpoint")
                address += 4
                instructions[address] = ("ret", "", None, None, "loader_checkpoint")
                with self.assertRaisesRegex(ValueError, "write base may alias saved LR"):
                    audit.audit_closed_display_helpers(
                        symbols, instructions, (0xc0007000, 0xc0008000))

    def test_closed_display_helper_cfg_accepts_only_declared_framebuffer_addresses(self):
        audit = load_boot_audit()
        symbols = {
            "loader_checkpoint": 0x100,
            "loader_checkpoint_hex": 0x200,
            "loader_checkpoint_draw_line": 0x300,
            "next_symbol": 0x400,
        }
        instructions = {
            0x100: ("stp", "x29, x30, [sp, #-0x10]!", None, None, "loader_checkpoint"),
            0x104: ("movz", "x9, #0x0000", None, None, "loader_checkpoint"),
            0x108: ("movk", "x9, #0x9c00, lsl #16", None, None, "loader_checkpoint"),
            0x10C: ("movk", "x9, #0x9ca4, lsl #16", None, None, "loader_checkpoint"),
            0x110: ("str", "x0, [x9]", None, None, "loader_checkpoint"),
            0x114: ("ldp", "x29, x30, [sp], #0x10", None, None, "loader_checkpoint"),
            0x118: ("ret", "", None, None, "loader_checkpoint"),
            0x200: ("ret", "", None, None, "loader_checkpoint_hex"),
            0x300: ("ret", "", None, None, "loader_checkpoint_draw_line"),
        }
        result = audit.audit_closed_display_helpers(
            symbols, instructions, (0xc0007000, 0xc0008000))
        self.assertEqual(result["helpers_checked"], 3)

    def test_closed_display_helper_cfg_rejects_framebuffer_derived_out_of_range_store(self):
        audit = load_boot_audit()
        symbols = {
            "loader_checkpoint": 0x100,
            "loader_checkpoint_hex": 0x200,
            "loader_checkpoint_draw_line": 0x300,
            "next_symbol": 0x400,
        }
        instructions = {
            0x100: ("stp", "x29, x30, [sp, #-0x10]!", None, None, "loader_checkpoint"),
            0x104: ("movz", "x9, #0x0000", None, None, "loader_checkpoint"),
            0x108: ("movk", "x9, #0x9c00, lsl #16", None, None, "loader_checkpoint"),
            0x10C: ("add", "x9, x9, #0x1347f00", None, None, "loader_checkpoint"),
            0x110: ("str", "x0, [x9]", None, None, "loader_checkpoint"),
            0x114: ("ldp", "x29, x30, [sp], #0x10", None, None, "loader_checkpoint"),
            0x118: ("ret", "", None, None, "loader_checkpoint"),
            0x200: ("ret", "", None, None, "loader_checkpoint_hex"),
            0x300: ("ret", "", None, None, "loader_checkpoint_draw_line"),
        }
        with self.assertRaisesRegex(ValueError, "framebuffer"):
            audit.audit_closed_display_helpers(
                symbols, instructions, (0xc0007000, 0xc0008000))

    def test_closed_display_helper_cfg_rejects_shifted_dynamic_framebuffer_store(self):
        audit = load_boot_audit()
        symbols = {
            "loader_checkpoint": 0x100,
            "loader_checkpoint_hex": 0x200,
            "loader_checkpoint_draw_line": 0x300,
            "next_symbol": 0x400,
        }
        instructions = {
            0x100: ("stp", "x29, x30, [sp, #-0x10]!", None, None, "loader_checkpoint"),
            0x104: ("movz", "x9, #0x0000", None, None, "loader_checkpoint"),
            0x108: ("movk", "x9, #0x9c00, lsl #16", None, None, "loader_checkpoint"),
            0x10C: ("add", "x9, x9, x10", None, None, "loader_checkpoint"),
            0x110: ("add", "x9, x9, #4", None, None, "loader_checkpoint"),
            0x114: ("str", "w0, [x9]", None, None, "loader_checkpoint"),
            0x118: ("ldp", "x29, x30, [sp], #0x10", None, None, "loader_checkpoint"),
            0x11C: ("ret", "", None, None, "loader_checkpoint"),
            0x200: ("ret", "", None, None, "loader_checkpoint_hex"),
            0x300: ("ret", "", None, None, "loader_checkpoint_draw_line"),
        }
        with self.assertRaisesRegex(ValueError, "framebuffer"):
            audit.audit_closed_display_helpers(
                symbols, instructions, (0xc0007000, 0xc0008000))

    def test_closed_display_helper_cfg_rejects_loop_carried_framebuffer_pointer(self):
        audit = load_boot_audit()
        symbols = {
            "loader_checkpoint": 0x100,
            "loader_checkpoint_hex": 0x200,
            "loader_checkpoint_draw_line": 0x300,
            "next_symbol": 0x400,
        }
        instructions = {
            0x100: ("stp", "x29, x30, [sp, #-0x10]!", None, None, "loader_checkpoint"),
            0x104: ("movz", "x9, #0x0000", None, None, "loader_checkpoint"),
            0x108: ("movk", "x9, #0x9c00, lsl #16", None, None, "loader_checkpoint"),
            0x10C: ("add", "x9, x9, x10", None, None, "loader_checkpoint"),
            0x110: ("add", "x9, x9, #4", None, None, "loader_checkpoint"),
            0x114: ("str", "w0, [x9]", None, None, "loader_checkpoint"),
            0x118: ("cbz", "x8, 0x120", 0x120, None, "loader_checkpoint"),
            0x11C: ("b", "0x110", 0x110, None, "loader_checkpoint"),
            0x120: ("ldp", "x29, x30, [sp], #0x10", None, None, "loader_checkpoint"),
            0x124: ("ret", "", None, None, "loader_checkpoint"),
            0x200: ("ret", "", None, None, "loader_checkpoint_hex"),
            0x300: ("ret", "", None, None, "loader_checkpoint_draw_line"),
        }
        with self.assertRaisesRegex(ValueError, "framebuffer"):
            audit.audit_closed_display_helpers(
                symbols, instructions, (0xc0007000, 0xc0008000))

    def test_closed_display_helper_cfg_rejects_fallthrough_and_escape(self):
        audit = load_boot_audit()
        symbols = {
            "loader_checkpoint": 0x100,
            "loader_checkpoint_hex": 0x200,
            "loader_checkpoint_draw_line": 0x300,
            "next_symbol": 0x400,
        }
        good_tail = {
            0x200: ("ret", "", None, None, "loader_checkpoint_hex"),
            0x300: ("ret", "", None, None, "loader_checkpoint_draw_line"),
        }
        mutants = (
            {0x100: ("nop", "", None, None, "loader_checkpoint")},
            {0x100: ("b", "0x200", 0x200, None, "loader_checkpoint")},
        )
        for first in mutants:
            with self.subTest(first=first):
                with self.assertRaises(ValueError):
                    audit.audit_closed_display_helpers(
                        symbols, {**good_tail, **first}, (0xc0007000, 0xc0008000))


if __name__ == "__main__":
    unittest.main(verbosity=2)
