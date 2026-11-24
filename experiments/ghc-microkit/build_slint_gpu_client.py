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
    args = parser.parse_args()
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
        f.write(b'P6\n1080 2340\n255\n')
        f.write(bytes(1080 * 2340 * 3))
    compat = root / 'tools/sddf-gpu-compat'
    flags = f'-mstrict-align -I{compat} -I{include} -include {compat / "ialloc_compat.h"}'
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
