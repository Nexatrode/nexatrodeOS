"""Static checks for the disposable desktop app wiring; not a security proof."""
from pathlib import Path
import struct, subprocess, sys
import xml.etree.ElementTree as ET

def validate(path, directory, browser=False):
    system=ET.parse(path).getroot();directory=Path(directory)
    domains={p.get('name'):p for p in system.findall('protection_domain')}
    edges={}
    for channel in system.findall('channel'):
        a,b=list(channel)
        for source,peer in ((a,b),(b,a)):
            key=(source.get('pd'),int(source.get('id')))
            assert key not in edges,('duplicate channel',key)
            edges[key]=(peer.get('pd'),int(peer.get('id')))
    def maps(name):return {m.get('mr') for m in domains[name].findall('map')}
    assert edges['chat_ui',6][0]=='denied_services'
    assert edges['chat_ui',5][0]=='messages_app'
    assert edges['wallet_ui',5][0]=='denied_services'
    assert edges['wallet_ui',6][0]=='wallet_app'
    assert maps('chat_ui') & maps('wallet_ui') == {'clock_view'}
    for name in ('chat','wallet'):
        for suffix in ('frame','focus','slint_heap','observation'):
            assert name+'_'+suffix in maps(name+'_ui')
        assert edges[name+'_ui',1][0]=='desktop_manager'
        assert edges[name+'_ui',8][0]=='timer_driver'
    assert 'passive' not in domains['simplex_agent'].attrib
    assert 'passive' not in domains['messages_app'].attrib
    workers=[domains['wallet_scan_'+str(i)].get('cpu') for i in (1,2,3)]
    assert workers==['1','2','3'],workers
    mapped={m.get('mr'):int(m.get('vaddr').replace('_',''),0) for m in domains['vault'].findall('map')}
    config=(directory/'blk-client-public-config.data').read_bytes()
    assert len(config)==80 and config[:5]==b'sDDF\x02'
    for offset,suffix in ((8,'storage_info'),(24,'request'),(40,'response'),(64,'data')):
        assert struct.unpack_from('<Q',config,offset)[0]==mapped['blk_client_vault_'+suffix]
    symbols=subprocess.check_output(['/opt/homebrew/opt/llvm/bin/llvm-nm',directory/'dhcp_client.elf'],text=True)
    assert 'haskell_simplex_network_active' not in symbols,'unlocked fixture accidentally contains the initially-locked network gate'
    assert 'haskell_simplex_network_transition' not in symbols
    if browser:
        assert maps('native_browser') == {'browser_frame','browser_heap','browser_input','browser_request','browser_response'}
        assert maps('browser_https') & maps('wallet_app') == set()
        assert maps('browser_https') & maps('simplex_agent') == set()
        assert edges['browser_https',1]==('dhcp_client',8)
        assert edges['native_browser',0]==('desktop_manager',27)
        assert edges['native_browser',1]==('desktop_manager',26)
        assert edges['browser_https',2]==('entropy_rng',0)
        assert 'browser_tcp_io' in maps('dhcp_client') and 'haskell_tcp_io' not in maps('browser_https')
        assert 'browser_tcp_page' in symbols
        assert 'native_reflow_test_arm' not in subprocess.check_output(['/opt/homebrew/opt/llvm/bin/llvm-nm',directory/'browser.elf'],text=True)
    print('PASS desktop frontend authority separation, private mappings, scan affinity, block relocation and unlocked networking')

if __name__=='__main__':validate(sys.argv[1],sys.argv[2])
