# SPDX-License-Identifier: BSD-2-Clause
"""Build pinned native image libraries; never link host libraries into a guest."""
from pathlib import Path
import shutil


def build(base, compiler, run):
    prefix = base / 'libraries'
    toolchain = base / 'image-cross.cmake'
    toolchain.write_text(f'''set(CMAKE_SYSTEM_NAME Generic)
set(CMAKE_SYSTEM_PROCESSOR aarch64)
set(CMAKE_C_COMPILER "{compiler}")
set(CMAKE_ASM_COMPILER "{compiler}")
set(CMAKE_AR "/opt/homebrew/opt/llvm/bin/llvm-ar")
set(CMAKE_RANLIB "/opt/homebrew/opt/llvm/bin/llvm-ranlib")
set(CMAKE_TRY_COMPILE_TARGET_TYPE STATIC_LIBRARY)
set(CMAKE_FIND_ROOT_PATH "{prefix}")
set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_PACKAGE ONLY)
''')
    common = ['-G','Ninja', '-DCMAKE_TOOLCHAIN_FILE='+str(toolchain),
              '-DCMAKE_INSTALL_PREFIX='+str(prefix), '-DCMAKE_INSTALL_LIBDIR=lib',
              '-DCMAKE_BUILD_TYPE=Release', '-DCMAKE_C_FLAGS=-O2 -ffunction-sections -fdata-sections']
    for name, flags, target in [
        ('libpng', ['-DPNG_SHARED=OFF','-DPNG_STATIC=ON','-DPNG_TESTS=OFF','-DPNG_TOOLS=OFF',
                    '-DPNG_HARDWARE_OPTIMIZATIONS=ON','-DZLIB_INCLUDE_DIR='+str(prefix/'include'),
                    '-DZLIB_LIBRARY='+str(prefix/'lib/libz.a'),
                    '-DCMAKE_C_FLAGS=-O2 -ffunction-sections -fdata-sections -DPNG_USER_WIDTH_MAX=4096 -DPNG_USER_HEIGHT_MAX=4096 -DPNG_USER_CHUNK_CACHE_MAX=32 -DPNG_USER_CHUNK_MALLOC_MAX=1048576'], 'png_static'),
        ('libjpeg-turbo', ['-DENABLE_SHARED=OFF','-DENABLE_STATIC=ON','-DWITH_TURBOJPEG=OFF',
                          '-DWITH_TOOLS=OFF','-DWITH_TESTS=OFF','-DHAVE_THREAD_LOCAL=0','-DWITH_SIMD=ON','-DREQUIRE_SIMD=ON'], 'jpeg-static'),
    ]:
        out=base/(name+'-build')
        run(['cmake','-S',base/'deps'/name,'-B',out,*common,*flags],base/(name+'-configure.log'))
        run(['cmake','--build',out,'--target',target,'-j4'],base/(name+'-build.log'))
        # Install only the static archive and public headers; cross executables are unused.
        if name=='libpng':
            shutil.copyfile(out/'liblibpng16_static.a',prefix/'lib/libpng.a')
            for header in ('png.h','pngconf.h'):shutil.copyfile(base/'deps/libpng'/header,prefix/'include'/header)
            shutil.copyfile(out/'pnglibconf.h',prefix/'include/pnglibconf.h')
            (prefix/'lib/pkgconfig/libpng.pc').write_text(f'''prefix={prefix}
libdir=${{prefix}}/lib
includedir=${{prefix}}/include
Name: libpng
Description: native static PNG decoder
Version: 1.6.59
Libs: -L${{libdir}} -lpng -lz -lm
Cflags: -I${{includedir}}
''')
        else:
            shutil.copyfile(out/'libjpeg.a',prefix/'lib/libjpeg.a')
            for header in ('jpeglib.h','jmorecfg.h','jerror.h'):shutil.copyfile(base/'deps/libjpeg-turbo/src'/header,prefix/'include'/header)
            shutil.copyfile(out/'jconfig.h',prefix/'include/jconfig.h')
        print('Built ARM64 '+name+' with NEON enabled',flush=True)
