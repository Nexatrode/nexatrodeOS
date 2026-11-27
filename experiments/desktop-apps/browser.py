"""Windowed NetSurf client and private HTTPS broker for the disposable desktop."""
import sys
import xml.etree.ElementTree as ET


def build(root, out, inputs, base, resize=False):
    sys.path.insert(0, str(root / 'experiments/netsurf-native'))
    from build import frontend, run, export_licenses
    from network import build_worker
    # Reuse only the pinned public engine/dependency builds. Never a VM disk.
    assert base.is_relative_to(root / 'build/netsurf-native')
    assert (base / 'deps/netsurf/netsurf-native.a').is_file()
    frontend(base, out, inputs, 2, https=True, javascript=True, desktop=True,resize=resize)
    build_worker(base, out, inputs, run, desktop=True)
    import subprocess
    symbols=subprocess.check_output(['/opt/homebrew/opt/llvm/bin/llvm-nm',out/'browser.elf'],text=True)
    assert ' duk_pcall' in symbols and ' js_exec' in symbols
    assert ' native_reflow_test_arm' not in symbols and ' native_dom_test_expire' not in symbols
    ca=base/'test-fixture-tls/ca.der'
    if ca.exists():assert ca.read_bytes() not in (out/'https-worker.elf').read_bytes()

    # Export the same upstream license inventory as the standalone browser.
    export_licenses(base, out, javascript=True)


def attach(system, manager, inputs, channel, mapping, mr):
    def pd(name):return system.find(f"protection_domain[@name='{name}']")
    def domain(name,image,priority,budget,stack):
        d=ET.SubElement(system,'protection_domain',name=name,priority=str(priority),budget=str(budget),period='10000',stack_size=hex(stack))
        ET.SubElement(d,'program_image',path=image)
        return d
    for name,size,page in [('browser_frame',0x400000,0x200000),('browser_input',4096,4096),
                            ('browser_heap',0x4000000,0x200000),('browser_https_heap',0x1000000,0x200000),
                            ('browser_request',4096,4096),('browser_response',0x205000,4096),('browser_tcp_io',4096,4096)]:
        mr(name,size,page_size=hex(page))
    browser=domain('native_browser','browser.elf',96,2000,0x100000)
    for name,va,perms,symbol in [('browser_frame',0x60000000,'rw','browser_frame'),('browser_heap',0x64000000,'rw','browser_heap'),
                                ('browser_input',0x70000000,'rw','input_handoff'),('browser_request',0x71000000,'rw','https_request'),
                                ('browser_response',0x72000000,'r','https_response')]:mapping(browser,name,va,perms,symbol)
    worker=domain('browser_https','https-worker.elf',101,2000,0x40000)
    # PL031 is shared read-only with the clock; there is no device write cap.
    rtc=next((m.get('name') for m in system.findall('memory_region') if int(m.get('phys_addr','0').replace('_',''),0)==0x9010000),None)
    if rtc is None:
        rtc='browser_rtc';mr(rtc,4096,phys_addr='0x9010000')
    for name,va,perms,symbol in [('browser_https_heap',0x64000000,'rw','https_heap'),('browser_request',0x68000000,'r','https_request'),
                                ('browser_response',0x69000000,'rw','https_response'),('browser_tcp_io',0x70000000,'rw','haskell_tcp_page'),
                                (rtc,0x71000000,'r','https_rtc')]:mapping(worker,name,va,perms,symbol)
    next(m for m in worker.findall('map') if m.get('mr')==rtc).set('cached','false')
    mapping(manager,'browser_frame',0x64000000,'r','surface_c')
    mapping(manager,'browser_input',0x66004000,'rw','input_c')
    mapping(pd('dhcp_client'),'browser_tcp_io',0x49001000,'rw','browser_tcp_page')
    # Slot numbers within one mapped page are not a memory boundary. Wallet
    # and scan-worker retain their wire offsets, but their physical page is
    # now separate from the agent's and browser's pages.
    mr('wallet_tcp_io',4096)
    mapping(pd('dhcp_client'),'wallet_tcp_io',0x49002000,'rw','wallet_tcp_page')
    for name in ('wallet_app','scan_worker'):
        pd(name).find("map[@mr='haskell_tcp_io']").set('mr','wallet_tcp_io')
    for a,x,b,y,pp in [('native_browser',0,'desktop_manager',27,False),('native_browser',1,'desktop_manager',26,False),('native_browser',2,'timer_driver',9,True),
                       ('native_browser',3,'browser_https',0,False),('native_browser',4,'browser_https',4,True),
                       ('browser_https',1,'dhcp_client',8,True),('browser_https',2,'entropy_rng',0,True),
                       ('browser_https',3,'timer_driver',10,True)]:channel(a,x,b,y,pp)
