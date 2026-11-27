"""Dedicated public map disk, only in generated disposable fixtures."""
import hashlib,shutil,uuid
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
def configure(guest,data,cfg,manifest,*,reuse=False):
    guest,data=Path(guest).resolve(),Path(data).resolve()
    assert guest.is_relative_to(ROOT/'build') and (guest/'.disposable-build').is_file()
    assert data.parent.parent==guest and data.parent.suffix=='.utm'
    assert manifest['saved_data_used'] is False and manifest['map_storage'] is True
    disk=data/'maps.img';assert not disk.is_symlink()
    if reuse:
        assert len(cfg['Drive'])==2 and cfg['Drive'][1]['ImageName']=='maps.img' and disk.stat().st_size==32*1024*1024
        return
    assert len(cfg['Drive'])==1 and not disk.exists()
    pack=guest/'maps.img';assert not pack.is_symlink() and pack.stat().st_size==32*1024*1024
    assert hashlib.sha256(pack.read_bytes()).hexdigest()==manifest['map_pack_sha256']
    shutil.copyfile(pack,disk)
    drive=str(uuid.uuid4()).upper()
    cfg['Drive'].append(dict(Identifier=drive,ImageName='maps.img',ImageType='Disk',Interface='None',InterfaceVersion=1,ReadOnly=False))
    cfg['QEMU']['AdditionalArguments'].append('-device virtio-blk-device,drive=drive'+drive+',bus=virtio-mmio-bus.16,indirect_desc=off,event_idx=off')
