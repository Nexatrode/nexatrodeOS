#!/usr/bin/env python3
"""Run the pinned sDDF timer example's Microkit system generator."""

from pathlib import Path
import runpy

repo = Path(__file__).resolve().parents[3]
sddf_meta = repo / ".cache/sddf-personal-os/examples/timer/meta.py"
runpy.run_path(str(sddf_meta), run_name="__main__")
