#!/usr/bin/env python3
"""Cross-build a pinned, offline NetSurf; all third-party/build output stays in build/."""
import argparse
import hashlib
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import tarfile
import urllib.request
import xml.etree.ElementTree as ET
from image import scene, decoder_fixtures

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
LLVM = Path('/opt/homebrew/opt/llvm/bin')
LLD = Path('/opt/homebrew/bin/ld.lld')


def run(command, log, **kwargs):
    with log.open('w') as stream:
        result = subprocess.run(list(map(str, command)), stdout=stream,
                                stderr=subprocess.STDOUT, **kwargs)
    if result.returncode:
        raise RuntimeError(f'{command[0]} failed; {log}\n{log.read_text(errors="replace")[-5000:]}')


def prepare(base):
    deps = base / 'deps'
    deps.mkdir(parents=True, exist_ok=True)
    for entry in json.loads((HERE / 'sources.json').read_text()):
        archive = deps / (entry['name'] + '.tar.gz')
        if not archive.exists():
            with urllib.request.urlopen(entry['url'], timeout=90) as response:
                archive.write_bytes(response.read())
        if hashlib.sha256(archive.read_bytes()).hexdigest() != entry['sha256']:
            raise ValueError('source digest mismatch: ' + entry['name'])
        target = deps / entry['name']
        if target.exists():
            continue
        target.mkdir()
        with tarfile.open(archive) as source:
            for item in source.getmembers():
                parts = Path(item.name).parts
                if len(parts) > 1:
                    item.name = str(Path(*parts[1:]))
                    source.extract(item, target, filter='data')


def libc(base):
    config = base / 'picolibc-cross.ini'
    config.write_text(f'''[binaries]
c = ['{LLVM}/clang', '--target=aarch64-none-elf', '-mcpu=cortex-a53']
ar = '{LLVM}/llvm-ar'
strip = '{LLVM}/llvm-strip'
[host_machine]
system = 'none'
cpu_family = 'aarch64'
cpu = 'cortex-a53'
endian = 'little'
[properties]
needs_exe_wrapper = true
skip_sanity_check = true
[built-in options]
c_args = ['-ffreestanding', '-fno-stack-protector', '-ffunction-sections', '-fdata-sections']
c_link_args = ['-nostdlib', '-fuse-ld=lld']
''')
    if not (base / 'picolibc-build/build.ninja').exists():
        run(['meson', 'setup', base / 'picolibc-build', base / 'deps/picolibc',
             '--cross-file', config, '--prefix', base / 'sysroot',
             '-Dmultilib=false', '-Dpicocrt=false', '-Dpicocrt-lib=false',
             '-Dsemihost=false', '-Dtests=false', '-Dthread-local-storage=false',
             '-Dsingle-thread=true', '-Dnewlib-global-errno=true', '--buildtype=release'],
            base / 'picolibc-config.log')
    run(['ninja', '-C', base / 'picolibc-build', '-j4', 'install'], base / 'picolibc-build.log')


def patch_dom_events(base):
    # The pinned libDOM includes the target in its ancestor list, then visits
    # it again at-target. A single click therefore runs non-capturing handlers
    # twice. Capture/bubble walks must start at the target's parent.
    path = base / 'deps/libdom/src/core/node.c'
    text = path.read_text()
    old = 'for (; target != NULL; target = target->parent) {'
    new = 'for (target = target->parent; target != NULL; target = target->parent) {'
    if old in text:
        assert text.count(old) == 1
        path.write_text(text.replace(old, new))


def libraries(base):
    patch_dom_events(base)
    prefix = base / 'libraries'
    (prefix / 'include').mkdir(parents=True, exist_ok=True)
    compiler = base / 'cc.py'
    flags = [str(LLVM / 'clang'), '--target=aarch64-none-elf', '-mcpu=cortex-a53',
             '-ffreestanding', '-fno-stack-protector', '-ffunction-sections', '-fdata-sections',
             '-I' + str(HERE / 'include'), '-I' + str(base / 'sysroot/include'),
             '-I' + str(prefix / 'include')]
    compiler.write_text('#!/usr/bin/env python3\nimport os,sys\n'
                        f'os.execv({flags[0]!r}, {flags!r} + sys.argv[1:])\n')
    compiler.chmod(0o755)
    env = dict(os.environ, PKG_CONFIG_LIBDIR=str(prefix / 'lib/pkgconfig'),
               PKG_CONFIG_PATH=str(prefix / 'lib/pkgconfig'),
               CFLAGS='-O2 -DWITHOUT_ICONV_FILTER -Wno-error -Wno-undef '
                      '-D__STDC_WANT_LIB_EXT1__=0 -D_XOPEN_SOURCE=700')
    for name in ('libwapcaplet', 'libparserutils', 'libhubbub', 'libcss', 'libdom', 'libnsutils', 'libnsbmp'):
        run(['make', '-C', base / 'deps' / name, '-j4', 'install',
             'NSSHARED=' + str(base / 'deps/buildsystem'), 'PREFIX=' + str(prefix),
             'HOST=aarch64-none-elf', 'BUILD=aarch64-apple-darwin', 'CC=' + str(compiler),
             'AR=' + str(LLVM / 'llvm-ar'), 'RANLIB=' + str(LLVM / 'llvm-ranlib'),
             'PKGCONFIG=/opt/homebrew/bin/pkg-config', 'BUILD_CC=cc', 'WITH_EXPAT_BINDING=no'],
            base / (name + '.log'), env=env)
        print('Built ' + name, flush=True)
    zlib = base / 'deps/zlib'
    objects = []
    for name in 'adler32 crc32 compress deflate gzclose gzlib gzread gzwrite infback inffast inflate inftrees trees uncompr zutil'.split():
        obj = base / (name + '.o')
        run([compiler, '-O2', '-DHAVE_UNISTD_H', '-I' + str(zlib), '-c', zlib / (name + '.c'), '-o', obj],
            base / 'zlib-compile.log')
        objects.append(obj)
    subprocess.run([str(LLVM / 'llvm-ar'), 'rcs', str(prefix / 'lib/libz.a'), *map(str, objects)], check=True)
    for name in ('zlib.h', 'zconf.h'):
        shutil.copyfile(zlib / name, prefix / 'include' / name)
    return compiler, env


def engine(base, compiler, env, javascript=False):
    source = base / 'deps/netsurf'
    from reflow import patch as patch_reflow
    patch_reflow(source)
    from hardening import patch as patch_hardening
    patch_hardening(source)
    from dom_work import patch as patch_dom_work
    patch_dom_work(source)
    path = source/'content/fetchers/resource.c'
    text = path.read_text()
    if '"reflow.html",' not in text:
        path.write_text(text.replace('"adblock.css",','"reflow.html",\n\t"adblock.css",'))
    text=path.read_text()
    if '"recovery.html",' not in text:
        path.write_text(text.replace('"adblock.css",','"recovery.html",\n\t"adblock.css",'))
    text=path.read_text()
    if '"dom-work.html",' not in text:
        path.write_text(text.replace('"adblock.css",','"dom-work.html",\n\t"adblock.css",'))
    # Also rebuild this small library with --engine-only, which otherwise
    # reuses dependencies prepared before JavaScript was enabled.
    patch_dom_events(base)
    run(['make','-C',base/'deps/libdom','-j4','install',
         'NSSHARED='+str(base/'deps/buildsystem'),'PREFIX='+str(base/'libraries'),
         'HOST=aarch64-none-elf','BUILD=aarch64-apple-darwin','CC='+str(compiler),
         'AR='+str(LLVM/'llvm-ar'),'RANLIB='+str(LLVM/'llvm-ranlib'),
         'PKGCONFIG=/opt/homebrew/bin/pkg-config','BUILD_CC=cc','WITH_EXPAT_BINDING=no'],
        base/'libdom.log',env=dict(env,CFLAGS='-O2 -DWITHOUT_ICONV_FILTER -Wno-error -Wno-undef '
                                 '-D__STDC_WANT_LIB_EXT1__=0 -D_XOPEN_SOURCE=700'))
    target = source / 'frontends/nexatrode'
    target.mkdir(exist_ok=True)
    for name in ('Makefile.tools', 'Makefile.defaults'):
        (target / name).write_text('')
    (target / 'Makefile').write_text(f'''CFLAGS += -std=c11 -O2 -DNS_NEXATRODE -D_POSIX_C_SOURCE=200809L -DNDEBUG -include native_compat.h -Wno-error -Wno-undef -Wno-implicit-fallthrough
SOURCES = $(filter-out content/fetchers/file/%,$(S_COMMON) $(S_IMAGE) $(S_BROWSER))
EXETARGET := netsurf-native-unused
.SECONDEXPANSION:
netsurf-native.a: $$(OBJECTS)
\t{LLVM}/llvm-ar rcs $@ $(OBJECTS)
''')
    path = source / 'frontends/Makefile.hts'
    text = path.read_text()
    if 'VLDTARGET := nexatrode' not in text:
        assert 'VLDTARGET := amiga' in text
        path.write_text(text.replace('VLDTARGET := amiga', 'VLDTARGET := nexatrode amiga'))
    path = source / 'utils/config.h'
    text = path.read_text()
    text = re.sub(r'#ifdef NS_NEXATRODE\n.*?#endif\n\n', '', text, flags=re.S)
    if '#ifdef NS_NEXATRODE' not in text:
        end = text.rfind('#endif')
        text = text[:end] + '''#ifdef NS_NEXATRODE
#undef HAVE_MMAP
#undef WITH_MMAP
#undef HAVE_REGEX
#undef HAVE_SCANDIR
#undef HAVE_DIRFD
#undef HAVE_UNLINKAT
#undef HAVE_FSTATAT
#undef HAVE_SIGPIPE
#undef HAVE_UTSNAME
#define HAVE_POSIX_INET_HEADERS
#endif

''' + text[end:]
        path.write_text(text)
    path = source / 'content/fetch.c'
    text = path.read_text()
    if '#ifndef NS_NEXATRODE' not in text:
        block = '\tret = fetch_file_register();\n\tif (ret != NSERROR_OK) {\n\t\treturn ret;\n\t}'
        assert block in text
        path.write_text(text.replace(block, '#ifndef NS_NEXATRODE\n' + block + '\n#endif'))
    path = source / 'content/fetchers/resource.c'
    text = path.read_text()
    if '"index.html",' not in text:
        text = text.replace('"adblock.css",', '"index.html", "lab.css", "scene.bmp", "second.html", "result.html",\n\t"adblock.css",')
        path.write_text(text)
    if '"web.html",' not in text:
        path.write_text(path.read_text().replace('"adblock.css",', '"web.html",\n\t"adblock.css",'))
    text = path.read_text()
    if '"images.html",' not in text:
        path.write_text(text.replace('"adblock.css",', '"images.html", "alpha.png", "interlaced.png", "baseline.jpg", "progressive.jpg", "cmyk.jpg", "corrupt.png", "corrupt.jpg", "oversize.png", "oversize.jpg",\n\t"adblock.css",'))
    text = path.read_text()
    if '"script.html",' not in text:
        path.write_text(text.replace('"adblock.css",', '"script.html", "script.js", "loop.html",\n\t"adblock.css",'))
    # Reject image dimensions before large decoder allocations or size arithmetic.
    path = source / 'content/handlers/image/png.c'
    text = path.read_text()
    if '#include "native_image_limits.h"' not in text:
        text = text.replace('#include <png.h>', '#include <png.h>\n#include "native_image_limits.h"')
        text = text.replace('interlace = png_get_interlace_type(png_s, info);', 'interlace = png_get_interlace_type(png_s, info);\n\tif (!native_image_size_ok(width, height)) png_error(png_s, "Image exceeds native limits");')
        text = text.replace('png_c->base.size += width * height * 4;', 'png_c->base.size += (size_t)width * height * 4;')
        path.write_text(text)
    path = source / 'content/handlers/image/jpeg.c'
    text = path.read_text()
    if '#include "native_image_limits.h"' not in text:
        text = text.replace('#include <setjmp.h>', '#include <setjmp.h>\n#include "native_image_limits.h"')
        # The conversion and metadata paths must both reject oversize input.
        needle = 'jpeg_read_header(&cinfo, TRUE);'
        assert text.count(needle) == 2
        parts = text.split(needle)
        guard = '\n\tif (!native_image_size_ok(cinfo.image_width, cinfo.image_height)) {\n\t\tjpeg_destroy_decompress(&cinfo);\n\t\treturn %s;\n\t}\n'
        text = parts[0] + needle + guard % 'NULL' + parts[1] + needle + guard % 'false' + parts[2]
        path.write_text(text)
    settings = {k: 'NO'  for k in ('CURL', 'OPENSSL', 'DUKTAPE', 'JPEG', 'JPEGXL', 'PNG', 'GIF', 'WEBP',
                                  'NSPSL', 'NSLOG', 'UTF8PROC', 'VIDEO', 'HARU_PDF', 'NSSVG', 'ROSPRITE')}
    if javascript:
        from javascript import build_generator
        env = dict(env,PATH=build_generator(base,run))
    path = source/'content/handlers/javascript/duktape/Makefile'
    text = path.read_text()
    if '.native-bindings' not in text:
        old = '$(OBJROOT)/duktape/binding.h $(OBJROOT)/duktape/Makefile: content/handlers/javascript/duktape/netsurf.bnd $(BINDINGS)'
        new = ('$(OBJROOT)/duktape/binding.h $(OBJROOT)/duktape/Makefile: $(OBJROOT)/duktape/.native-bindings\n'
               '\t$(Q)touch $@\n\n'
               '$(OBJROOT)/duktape/.native-bindings: content/handlers/javascript/duktape/netsurf.bnd $(BINDINGS)')
        assert old in text
        text = text.replace(old,new).replace('$(Q)nsgenbind $(GBFLAGS) -I content/handlers/javascript/WebIDL $< $(OBJROOT)/duktape', '$(Q)nsgenbind $(GBFLAGS) -I content/handlers/javascript/WebIDL $< $(OBJROOT)/duktape\n\t$(Q)touch $@')
        path.write_text(text)
    # Property setters also have to refresh the existing form widget.
    for interface, dom_name in [('HTMLInputElement','input'),('HTMLTextAreaElement','text_area')]:
        path = source/'content/handlers/javascript/duktape'/(interface+'.bnd')
        text = path.read_text()
        if 'native_form_value_changed' not in text:
            text += '\nprologue '+interface+'()\n%{\n#include "native_javascript.h"\n%}\n'
            method = ('setter '+interface+'::value()\n%{\n'
                '\tdom_string *value;\n\tduk_size_t length;\n'
                '\tconst char *s = duk_is_null(ctx,0) ? "" : duk_safe_to_lstring(ctx,0,&length);\n'
                '\tif (duk_is_null(ctx,0)) length=0;\n'
                '\tif (dom_string_create((const uint8_t *)s,length,&value)!=DOM_NO_ERR) return 0;\n'
                '\tdom_node *node=((node_private_t*)priv)->node;\n'
                '\tdom_exception error=dom_html_'+dom_name+'_element_set_value((struct dom_html_'+dom_name+'_element*)node,value);\n'
                '\tdom_string_unref(value);\n'
                '\tif (error==DOM_NO_ERR) native_form_value_changed(node);\n\treturn 0;\n%}')
            text = text.replace('setter '+interface+'::value();',method)
            path.write_text(text)
    path = source/'content/handlers/javascript/duktape/duk_custom.h'
    text = path.read_text()
    if 'native_js_date_ms' not in text:
        text += '\n#ifdef NS_NEXATRODE\nextern double native_js_date_ms(void);\nextern double native_js_mono_ms(void);\n#define DUK_USE_DATE_GET_NOW(ctx) native_js_date_ms()\n#define DUK_USE_GET_MONOTONIC_TIME(ctx) native_js_mono_ms()\n#endif\n'
        path.write_text(text)
    # Keep the upstream interrupt mechanism, with a smaller browser-test budget.
    path = source/'content/handlers/javascript/duktape/dukky.c'
    text = path.read_text()
    text = text.replace('#define JS_EXEC_TIMEOUT_MS 10000 /* 10 seconds */', '#define JS_EXEC_TIMEOUT_MS 250 /* native per-call budget */')
    if 'NETSURF|JS_ERROR|' not in text:
        text = text.replace('#include <stdint.h>', '#include <stdint.h>\n#include <stdio.h>')
        text = text.replace('NSLOG(jserrors, WARNING,', 'printf("NETSURF|JS_ERROR|%.160s\\n",duk_safe_to_string(ctx,-1));\n\tNSLOG(jserrors, WARNING,')
    path.write_text(text)
    settings.update(BMP='YES',PNG='YES',JPEG='YES',DUKTAPE='YES' if javascript else 'NO')
    (source / 'Makefile.config').write_text(''.join('override NETSURF_USE_' + k + ' := ' + v + '\n' for k, v in settings.items())
                                          + 'override NETSURF_FS_BACKING_STORE := NO\noverride NETSURF_STRIP_BINARY := NO\n')
    # Upstream make does not invalidate every object when feature flags change.
    # A stale image_init would silently leave linked decoders unregistered.
    stamp = source / '.nexatrode-features.json'
    fingerprint = json.dumps({'settings':settings,'frontend':(target/'Makefile').read_text()},sort_keys=True)
    if not stamp.exists() or stamp.read_text() != fingerprint:
        shutil.rmtree(source/'build/Darwin-nexatrode',ignore_errors=True)
        (source/'netsurf-native.a').unlink(missing_ok=True)
    run(['make', '-C', source, '-j4', 'TARGET=nexatrode', 'CC=' + str(compiler), 'CCACHE=',
         'PKG_CONFIG=/opt/homebrew/bin/pkg-config', 'PKGCONFIG=/opt/homebrew/bin/pkg-config',
         'PREFIX=' + str(base / 'libraries'), 'netsurf-native.a'], base / 'engine.log', env=env)
    stamp.write_text(fingerprint)
    print('Built native ARM64 NetSurf engine archive', flush=True)


def frontend(base, out, inputs, scale, https=False, javascript=False, test_hooks=False, desktop=False, resize=False, mobile=False):
    sdk = inputs / 'sdk'
    board = sdk / 'board/qemu_virt_aarch64_el1/smp-debug'
    compiler = base / 'cc.py'
    source = base / 'deps/netsurf'
    assets = {p.name: p.read_bytes() for p in (HERE / 'assets').iterdir() if p.is_file()}
    if mobile:
        assets['index.html']=(HERE.parent/'mobile-browser/lab.html').read_bytes()
        assets['lab.css']+=b'\n@media (max-width:600px) { main { padding:20px 16px; } h1 { font-size:32px; } .form,.picture { float:none; width:auto; } input[type=text] { max-width:90%; } }\n'
    assets['scene.bmp'] = scene()
    assets.update(decoder_fixtures(out))
    for name in ('default.css', 'quirks.css', 'internal.css', 'adblock.css'):
        assets[name] = (source / 'resources' / name).read_bytes()
    assets['user.css'] = b''
    messages = []
    for line in (source / 'resources/FatMessages').read_text().splitlines():
        if line.startswith('en.all.'):
            messages.append(line[len('en.all.'):])
    assets['Messages'] = ('\n'.join(messages) + '\n').encode()
    for name, data in assets.items():
        (out / name).write_bytes(data)
    font = ROOT / 'experiments/slint-notes/assets/fonts/Roboto-Variable.ttf'
    assembly = []
    entries = []
    for index, (name, data) in enumerate(assets.items()):
        symbol = 'resource_' + str(index)
        assembly.append(f'.section .rodata\n.balign 8\n.global {symbol}\n{symbol}:\n.incbin {json.dumps(str(out / name))}\n')
        entries.append(f'{{{json.dumps(name)}, {symbol}, {len(data)}}}')
    assembly.append(f'.section .rodata\n.balign 8\n.global native_font\nnative_font:\n.incbin {json.dumps(str(font))}\n')
    (out / 'assets.S').write_text(''.join(assembly))
    (out / 'assets.c').write_text('#include "native.h"\n' + ''.join(f'extern const unsigned char resource_{i}[];\n' for i in range(len(assets)))
        + 'const struct asset native_assets[] = {' + ','.join(entries) + '};\n'
        + f'const size_t native_asset_count = {len(assets)};\nconst size_t native_font_size = {font.stat().st_size};\n')
    flags = ['-std=gnu11', '-O2', '-DNS_NEXATRODE', '-D_POSIX_C_SOURCE=200809L', '-DNDEBUG',
             '-include', 'native_compat.h', '-I' + str(board / 'include'), '-I' + str(HERE),
             '-I' + str(source), '-I' + str(source / 'include'), '-I' + str(base / 'deps/stb'),
             f'-DNATIVE_SCALE={scale}', f'-DSLINT_FRAME_WIDTH={(360 if mobile else 560 if desktop else 1280)*scale}u', f'-DSLINT_FRAME_HEIGHT={(720 if mobile else 520 if desktop else 800)*scale}u',
             ('-DSLINT_FRAME_REGION_BYTES=0x800000u' if mobile else '-DSLINT_FRAME_REGION_BYTES=0x400000u' if desktop else '-DSLINT_FRAME_REGION_BYTES=0x1000000u')]
    if desktop: flags += ['-DNATIVE_DESKTOP=1']
    if mobile:
        assert not desktop and not resize and scale == 3
        flags += ['-DNATIVE_MOBILE=1']
    if https: flags += ['-DNATIVE_HTTPS=1']
    if javascript: flags += ['-DNATIVE_JAVASCRIPT=1']
    if resize:
        assert desktop
        flags += ['-DNATIVE_DISPLAY_RESIZE=1']
    if test_hooks: flags += ['-DNATIVE_BROWSER_TESTS=1']
    objects = []
    for unit in (('browser.c', 'platform.c', 'raster.c', 'page.c', 'alloc_guard.c', 'dom_budget.c', 'assets.c', 'assets.S') + (('https.c',) if https else ()) + (('javascript_forms.c',) if javascript else ())):
        obj = out / (unit.replace('.', '_') + '.o')
        unit_flags = ['--target=aarch64-none-elf'] if unit.endswith('.S') else flags
        run([LLVM / 'clang' if unit.endswith('.S') else compiler, *unit_flags,
             '-c', out / unit if unit.startswith('assets.') else HERE / unit, '-o', obj],
            out / (unit + '.log'))
        objects.append(obj)
    libraries = [source / 'netsurf-native.a'] + [base / 'libraries/lib' / ('lib' + name + '.a')
        for name in ('css', 'dom', 'hubbub', 'parserutils', 'wapcaplet', 'nsutils', 'nsbmp', 'png', 'jpeg', 'z')]
    run([LLD, '--gc-sections', '--wrap=malloc','--wrap=calloc','--wrap=realloc', '-L', board / 'lib', '-L', base / 'sysroot/lib',
         '-T', board / 'lib/microkit.ld', '-Map=' + str(out / 'browser.map'), *objects, '--start-group',
         *libraries, '-lmicrokit', '-lc', '-lm', '--end-group', '-o', out / 'browser.elf'], out / 'link.log')
    symbols = subprocess.check_output([LLVM/'llvm-nm',out/'browser.elf'],text=True)
    assert ' nspng_init' in symbols and ' nsjpeg_init' in symbols, 'image handlers must be registered and retained'
    return flags


def guest(base, inputs, surfaces, display, scale, https=False, javascript=False, test_hooks=False):
    display_build = json.loads((display / 'build.json').read_text())
    if display_build.get('scale') != scale or display_build.get('resolution') != [1280*scale,800*scale]:
        raise ValueError('prepared display dimensions/density must match the browser')
    out = base / ('guest-' + ('https-' if https else '') + ('js-' if javascript else '') + ('test-' if test_hooks else '') + str(scale) + 'x')
    out.mkdir(exist_ok=True)
    sdk=inputs/'sdk';board=sdk/'board/qemu_virt_aarch64_el1/smp-debug'
    compiler=base/'cc.py'
    flags=frontend(base,out,inputs,scale,https,javascript,test_hooks)
    symbols = subprocess.check_output([LLVM/'llvm-nm',out/'browser.elf'],text=True)
    run([compiler, *flags, '-c', HERE / 'poller.c', '-o', out / 'poller.o'], out / 'poller.log')
    run([LLD, '-L', board / 'lib', '-T', board / 'lib/microkit.ld', out / 'poller.o',
         '-lmicrokit', '-o', out / 'poller.elf'], out / 'poller-link.log')
    # Only the previously tested display/input/clock driver definitions are reused.
    original = ET.parse(display / 'desktop.system').getroot()
    system = ET.Element('system')
    names = {'timer_driver', 'gpu_driver', 'gpu_virt', 'gpu_client', 'input_driver'}
    for pd in original.findall('protection_domain'):
        if pd.get('name') in names:
            system.append(pd)
    for channel in original.findall('channel'):
        if all(end.get('pd') in names for end in channel.findall('end')):
            system.append(channel)
    used = {m.get('mr') for pd in system.findall('protection_domain') for m in pd.findall('map')}
    for region in original.findall('memory_region'):
        if region.get('name') in used:
            system.insert(0, region)
    for name, size in [('browser_heap', 64*1024*1024)]:
        ET.SubElement(system, 'memory_region', name=name, size=hex(size), page_size='0x200000')
    browser = ET.SubElement(system, 'protection_domain', name='native_browser', priority='100',
                            budget='8000', period='10000', stack_size='0x100000')
    ET.SubElement(browser, 'program_image', path='browser.elf')
    def mapping(pd, name, address, symbol):
        ET.SubElement(pd, 'map', mr=name, vaddr=hex(address), perms='rw', cached='true', setvar_vaddr=symbol)
    for name, address, symbol in [('slint_frame',0x60000000,'browser_frame'),('browser_heap',0x64000000,'browser_heap'),
                                   ('input_handoff',0x70000000,'input_handoff'),('ui_focus',0x70001000,'ui_focus'),('ui_pointer',0x70002000,'ui_pointer')]:
        mapping(browser,name,address,symbol)
    poller = ET.SubElement(system, 'protection_domain', name='timer_poller', priority='1', budget='1000', period='10000')
    ET.SubElement(poller, 'program_image', path='poller.elf')
    mapping(poller,'hvf_timer_shared',0x50000000,'hvf_timer_state')
    for left, lid, right, rid, pp in [('native_browser',0,'gpu_client',2,False),('native_browser',1,'input_driver',2,False),
                                    ('native_browser',2,'timer_driver',1,True),('timer_poller',0,'timer_driver',0,False)]:
        channel = ET.SubElement(system, 'channel')
        ET.SubElement(channel, 'end', pd=left, id=str(lid), **({'pp':'true'} if pp else {}))
        ET.SubElement(channel, 'end', pd=right, id=str(rid))
    if https:
        from network import attach
        names |= attach(system,base,out,inputs,run,test_hooks)
    actual = {pd.get('name') for pd in system.iter('protection_domain')}
    assert actual == names | {'native_browser','timer_poller'}
    browser_maps = {m.get('mr') for m in browser.findall('map')}
    assert browser_maps == {'slint_frame','browser_heap','input_handoff','ui_focus','ui_pointer'} | ({'https_request','https_response'} if https else set())
    assert https or not any(any(x in (region.get('name') or '') for x in ('vault','wallet','disk','net')) for region in system.findall('memory_region'))
    ET.ElementTree(system).write(out / 'native.system', encoding='utf-8', xml_declaration=True)
    for name in ('gpu_driver.elf','gpu_virt.elf','gpu_client.elf','input_driver.elf','timer_driver.elf'):
        donor = next(directory / name for directory in (display,surfaces,surfaces/'gpu',inputs/'pds') if (directory/name).exists())
        shutil.copyfile(donor,out/name)
    run([sdk / 'bin/microkit', out / 'native.system', '--board', 'qemu_virt_aarch64_el1', '--config', 'smp-debug',
         '--search-path', out, '-o', out / 'loader.img', '-r', out / 'report.txt'], out / 'compose.log',
        env=dict(os.environ, MICROKIT_SDK=str(sdk)))
    source_inputs = [p for p in HERE.rglob('*') if p.is_file() and '__pycache__' not in str(p)]
    if https:
        source_inputs += list((ROOT/'experiments/guest-https/src').glob('*.rs'))
        source_inputs += [ROOT/'experiments/guest-https/Cargo.toml',ROOT/'rust/src/freestanding.rs',ROOT/'rust/.cargo/config.toml',ROOT/'rust/targets/aarch64-sel4-microkit.json',ROOT/'experiments/hvf-smp/accelerated.system']
    digest = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
    export_licenses(base, out, javascript)
    shutil.copyfile(HERE/'sources.json',out/'sources.json')
    (out / 'build.json').write_text(json.dumps({'resolution':[1280*scale,800*scale], 'logical_resolution':[1280,800],
        'image_formats':['bmp','ico','png','jpeg'],'image_max_side':4096,'image_max_pixels':4194304,
        'scale':scale,'network':https,'https':https,'certificate_verification':https,'storage':False,'javascript':javascript,'javascript_engine':'Duktape 2.7.0' if javascript else None,'script_budget_ms':250 if javascript else None,'saved_data_used':False,
        'dom_reflow':javascript,'dom_coalesce_ms':16,'dom_max_nodes':20000,'dom_max_depth':512,'dom_dynamic_frames':False,
        'test_hooks':test_hooks,'reflow_allocation_guard':True,'document_page_status':True,
        'protection_domains':sorted(actual),'browser_mappings':sorted(browser_maps),
        'browser_elf_bytes':(out/'browser.elf').stat().st_size,'heap_bytes':64*1024*1024,
        'engine_revision':'170d89462442c286f80c0604f30e4c7f2a801265',
        'sources':{str(p.relative_to(ROOT)):digest(p) for p in source_inputs},
        'artifacts':{p.name:digest(p) for p in out.glob('*.elf')},'loader_sha256':digest(out / 'loader.img')},indent=2)+'\n')
    if javascript:
        assert ' duk_pcall' in symbols and ' js_exec' in symbols, 'JavaScript must be linked'
    assert (' native_reflow_test_arm' in symbols) is test_hooks, 'fault controls must be confined to test images'
    assert (' native_dom_test_expire' in symbols) is test_hooks, 'deadline fault controls must be confined to test images'
    if https and (base/'test-fixture-tls/ca.der').exists():
        test_ca=(base/'test-fixture-tls/ca.der').read_bytes()
        assert (test_ca in (out/'https-worker.elf').read_bytes()) is test_hooks, 'fixture trust must be confined to test images'
    print('Built disposable browser guest: ' + str(out / 'loader.img'), flush=True)


def export_licenses(base, out, javascript):
    notices = out / 'licenses'
    notices.mkdir(exist_ok=True)
    for entry in json.loads((HERE / 'sources.json').read_text()):
        for path in (base / 'deps' / entry['name']).iterdir():
            if path.is_file() and path.name.lower().startswith(('copying','license','notice')):
                shutil.copyfile(path,notices/(entry['name']+'-'+path.name))
    shutil.copyfile(ROOT/'experiments/slint-notes/assets/fonts/Roboto-OFL.txt',notices/'Roboto-OFL.txt')
    if javascript:
        bundled = (base/'deps/netsurf/content/handlers/javascript/duktape/duktape.c').read_text()
        notice = bundled.split('/* LICENSE.txt */',1)[1].split('/* AUTHORS.rst */',1)[0]
        (notices/'Duktape-LICENSE.txt').write_text(notice.strip()+'\n')
        authors = bundled.split('/* AUTHORS.rst */',1)[1].split('*/',1)[0]+'*/'
        (notices/'Duktape-AUTHORS.txt').write_text(authors.strip()+'\n')

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--build', type=Path, default=ROOT / 'build/netsurf-native')
    parser.add_argument('--engine-only', action='store_true', help='reuse previously built pinned dependencies')
    parser.add_argument('--guest', action='store_true')
    parser.add_argument('--javascript', action='store_true', help='enable Duktape and the upstream DOM bindings in a separate browser image')
    parser.add_argument('--test-hooks', action='store_true', help='compile disposable-test-only allocation fault controls')
    parser.add_argument('--https', action='store_true', help='include browser-only native HTTPS worker + network drivers')
    parser.add_argument('--inputs', type=Path)
    parser.add_argument('--surfaces', type=Path)
    parser.add_argument('--display', type=Path)
    parser.add_argument('--scale', type=int, choices=(1,2), default=2)
    args = parser.parse_args()
    if args.javascript and not args.https:parser.error('--javascript currently requires --https for the guest RTC service')
    if args.test_hooks and not args.javascript:parser.error('--test-hooks requires --javascript')
    base = args.build.resolve()
    if not base.is_relative_to(ROOT / 'build') or '.utm' in str(base):
        parser.error('build output must be inside this worktree build/ and outside VM bundles')
    base.mkdir(parents=True, exist_ok=True)
    if args.engine_only:
        prepare(base)
        compiler = base / 'cc.py'
        env = dict(os.environ, PKG_CONFIG_LIBDIR=str(base / 'libraries/lib/pkgconfig'),
                   PKG_CONFIG_PATH=str(base / 'libraries/lib/pkgconfig'))
    else:
        prepare(base)
        libc(base)
        compiler, env = libraries(base)
    from image_libraries import build as build_images
    build_images(base, compiler, run)
    engine(base, compiler, env, args.javascript)
    if args.guest:
        if not all((args.inputs,args.surfaces,args.display)):
            parser.error('--guest requires explicit prepared public --inputs, --surfaces and --display directories')
        guest(base,args.inputs.resolve(),args.surfaces.resolve(),args.display.resolve(),args.scale,args.https,args.javascript,args.test_hooks)


if __name__ == '__main__':
    main()
