"""Inventory the assembled desktop, including its current lifecycle limits.

This describes actual SDF mappings, not app-supplied permission requests. Unknown
domains and private cross-app mappings fail the build rather than silently
falling outside the future lock/restart boundary.
"""
import xml.etree.ElementTree as ET

APPS = {
    'simplex': dict(frontend='chat_ui', services=(
        'messages_app', 'simplex_agent', 'transcript_worker', 'photo_reader', 'photo_worker')),
    'nexawal': dict(frontend='wallet_ui', services=(
        'wallet_app', 'scan_worker', 'wallet_scan_1', 'wallet_scan_2', 'wallet_scan_3')),
    'netsurf': dict(frontend='native_browser', services=('browser_https',)),
}
INFRASTRUCTURE = frozenset((
    'vault', 'clock', 'entropy_rng', 'blk_driver', 'blk_virt', 'timer_driver',
    'ethernet_driver', 'net_virt_tx', 'net_virt_rx', 'dhcp_client', 'net_copier',
    'input_driver', 'timer_poller', 'desktop_manager', 'desktop_shell',
    'denied_services', 'desktop_gpu', 'display_agent',
    'gpu_driver', 'gpu_virt', 'gpu_client',
    'lock_auth','lock_probe',
))
PUBLIC_APP_REGIONS = frozenset(('clock_view', 'desktop_display_mode', 'rtc'))


def inventory(path, *, browser=True, maps=False):
    system = ET.parse(path).getroot()
    domains = {p.get('name'): p for p in system.iter('protection_domain')}
    if len(domains) != len(list(system.iter('protection_domain'))):
        raise ValueError('duplicate domain')
    apps = {k: v for k, v in APPS.items() if browser or k != 'netsurf'}
    if maps: apps['maps'] = dict(frontend='offline_map', services=('map_store',) if 'map_store' in domains else ())
    if maps and 'lock_auth' in domains:
        children={p.get('name') for p in domains['desktop_manager'].findall('protection_domain')} if 'desktop_manager' in domains else set()
        if not {'offline_map','map_store'}<=children:raise ValueError('Maps is outside the curated lock group')
    owner = {name: app for app, spec in apps.items()
             for name in (spec['frontend'], *spec['services'])}
    missing = set(owner) - domains.keys()
    unknown = domains.keys() - (owner.keys() | INFRASTRUCTURE)
    if missing or unknown:
        raise ValueError(f'unclassified lifecycle domains: missing={sorted(missing)}, unknown={sorted(unknown)}')
    regions = {r.get('name'): r for r in system.findall('memory_region')}
    if len(regions) != len(system.findall('memory_region')):
        raise ValueError('duplicate memory region')
    mappings = {}
    for name, pd in domains.items():
        seen = set()
        for m in pd.findall('map'):
            region = m.get('mr')
            if region not in regions or region in seen:
                raise ValueError(f'missing or duplicate mapping: {name}/{region}')
            seen.add(region)
            mappings.setdefault(region, []).append(dict(domain=name, perms=m.get('perms')))
    for region, uses in mappings.items():
        app_owners = {owner[u['domain']] for u in uses if u['domain'] in owner}
        if region in PUBLIC_APP_REGIONS and any('w' in u['perms'] for u in uses if u['domain'] in owner):
            raise ValueError(f'writable public app region: {region}')
        if len(app_owners) > 1 and region not in PUBLIC_APP_REGIONS:
            raise ValueError(f'private cross-app region: {region}/{sorted(app_owners)}')
        if region == 'rtc' and app_owners:
            if int(regions[region].get('phys_addr','0').replace('_',''),0) != 0x9010000:
                raise ValueError('unexpected RTC device mapping')
    groups = {}
    for app, spec in apps.items():
        members = [spec['frontend'], *spec['services']]
        touched = sorted(r for r, uses in mappings.items()
                         if any(u['domain'] in members for u in uses))
        devices=[r for r in touched if r=='map_disk_regs']
        private = [r for r in touched if r not in PUBLIC_APP_REGIONS and r not in devices]
        locked='lock_auth' in domains
        groups[app] = dict(frontend=spec['frontend'], services=list(spec['services']),
            device_regions=devices,
            private_regions=private,
            public_readonly_regions=[r for r in touched if r in PUBLIC_APP_REGIONS],
            shared_private_bytes=sum(int(regions[r].get('size').replace('_', ''), 0) for r in private),
            private_region_peers={r: [u for u in mappings[r] if u['domain'] not in members]
                                  for r in private},
            lifecycle=('curated: close/minimize hides; full lock scrubs and restarts the fixed service group' if locked else
                       'static: close/minimize hides; services retain memory and authority'))
    return dict(schema=1, architecture='aarch64', runtime='seL4/Microkit', apps=groups,
        infrastructure=sorted(domains.keys() - owner.keys()),
        guarantees=dict(independent_app_address_spaces=True, wrapped_key_lock='lock_auth' in domains,
                        private_memory_cleanup_on_full_lock='lock_auth' in domains,
                        private_memory_cleanup_on_close=False, dynamic_installation=False),
        integration_required=([
            'Gate all app groups on authenticated vault activation',
            'Stop and reconstruct frontends and every listed worker on full lock',
            'Clear private shared regions and each service writable ELF/stack/IPC/heap',
            'Cancel all private network slots and clear trusted provider caches',
            'Cover and clear retained display surfaces before reporting lock complete',
        ] if 'lock_auth' not in domains else [
            'General package installation and capability revocation beyond curated fixed roles',
            'Production kernel and debug-interface lockdown',
            'Hardware-backed retry and rollback protection; whole-memory/SIMD erasure audit',
        ]))
