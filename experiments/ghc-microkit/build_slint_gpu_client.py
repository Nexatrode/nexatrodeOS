#!/usr/bin/env python3
"""Build the GPU client with the dimensions used by the live Slint surface."""

import argparse
import os
from pathlib import Path
import shutil
import subprocess


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--sdk', type=Path, required=True)
    parser.add_argument('--sddf', type=Path, required=True)
    parser.add_argument('--build', type=Path, required=True)
    parser.add_argument('--board', default='qemu_virt_aarch64')
    parser.add_argument('--config', default='debug')
    parser.add_argument('--perf-trace', action='store_true')
    parser.add_argument('--width', type=int, default=1080)
    parser.add_argument('--height', type=int, default=2340)
    parser.add_argument('--cursor-scale', type=int, choices=(4,8), default=4)
    parser.add_argument('--frame-region-bytes', type=lambda s: int(s, 0), default=0x800000)
    args = parser.parse_args()
    if not (320 <= args.width <= 2560 and 240 <= args.height <= 2560
            and args.frame_region_bytes in (0x800000, 0x1000000)
            and args.width * args.height * 3 + 28 <= args.frame_region_bytes):
        parser.error('frame dimensions must fit the bounded shared surface')
    root = Path(__file__).resolve().parents[2]
    sdk, sddf, build = (p.resolve() for p in (args.sdk, args.sddf, args.build))
    overlay = build / 'overlay'
    include = overlay / 'include'
    include.mkdir(parents=True, exist_ok=True)
    shutil.copy2(root / 'tests/input/gpu_client.c', overlay / 'client.c')
    shutil.copy2(root / 'tests/input/slint_frame.h', include / 'slint_frame.h')
    shutil.copy2(sddf / 'examples/gpu/fb_img.S', overlay / 'fb_img.S')
    config = (sddf / 'examples/gpu/include/gpu_config.h').read_text()
    old = '#define GPU_DATA_REGION_SIZE_CLI0           0x200000'
    if config.count(old) != 1:
        raise RuntimeError('unexpected sDDF GPU configuration')
    (include / 'gpu_config.h').write_text(config.replace(old, old.replace('0x200000', '0x1000000')))
    shutil.copy2(sddf / 'examples/gpu/gpu.mk', build / 'Makefile')
    image = build / 'initial.ppm'
    with image.open('wb') as f:
        f.write(f'P6\n{args.width} {args.height}\n255\n'.encode())
        f.write(bytes(args.width * args.height * 3))
    compat = root / 'tools/sddf-gpu-compat'
    flags = f'-mstrict-align -I{compat} -I{include} -include {compat / "ialloc_compat.h"}'
    flags += f' -DSLINT_FRAME_WIDTH={args.width}u -DSLINT_FRAME_HEIGHT={args.height}u -DSLINT_FRAME_REGION_BYTES={args.frame_region_bytes}u -DNEXATRODE_CURSOR_SCALE={args.cursor_scale}'
    # Make does not track changed command-line dimensions: always rebuild this TU.
    (build / 'client.o').unlink(missing_ok=True)
    if args.perf_trace:
        flags += ' -DNEXATRODE_GPU_PERF_TRACE=1'
    env = os.environ.copy()
    env['PATH'] = '/opt/homebrew/opt/llvm/bin:/opt/homebrew/sbin:' + env.get('PATH', '')
    subprocess.run(['make', '-C', str(build), 'client.elf', f'MICROKIT_SDK={sdk}',
                    f'MICROKIT_BOARD={args.board}', f'MICROKIT_CONFIG={args.config}',
                    f'SDDF={sddf}', f'BUILD_DIR={build}', f'FB_IMG={image}', 'BLOB=0',
                    f'TOP={overlay}', f'CFLAGS_gpu={flags}'], check=True, env=env)


if __name__ == '__main__':
    main()
