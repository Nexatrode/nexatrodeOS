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
        self.assertEqual(prepare.RUNTIME_SOURCE, 0xB0081000)
        self.assertIn("diagnostic_only", inspect.signature(prepare.native_startup_entry).parameters)
        code = prepare.native_startup_entry(ENTRY, diagnostic_only=True)
        self.assertIn("ldr x1, =LINK_ADDRESS", code)
        self.assertIn("ldr x1, =0xb0081000", code)
        self.assertIn("ldr x1, =0x20000", code)
        self.assertIn("ldr x1, =0x6000000", code)

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


if __name__ == "__main__":
    unittest.main(verbosity=2)
