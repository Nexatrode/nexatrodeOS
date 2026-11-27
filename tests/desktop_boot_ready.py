"""Structured readiness for the disposable VirGL desktop; no app text is read."""
import struct

APP_MAGIC = 0x4e584445534b3031

def ready(gpu, after, chat, wallet):
    return (gpu[0] != 0 and not gpu[0]&1 and gpu[0] == after and
            gpu[2] >= 2 and gpu[13] == 0x4d4f5431 and
            1600 <= gpu[9] <= 3840 and 1200 <= gpu[10] <= 2160 and
            chat == wallet == APP_MAGIC)

def gpu_boot_state(q, data, locked=False):
    def read(address, size):
        path=data/'desktop-boot-state.bin'
        try:
            q.send('pmemsave',{'val':address,'size':size,'filename':str(path)})
            return path.read_bytes()
        finally:path.unlink(missing_ok=True)
    gpu=list(struct.unpack('<32I',read(0x5fba0000,128)))
    after=struct.unpack('<I',read(0x5fba0000,4))[0]
    chat=struct.unpack('<Q',read(0x5fbb0000,8))[0]
    wallet=struct.unpack('<Q',read(0x5fbb2000,8))[0]
    if locked:
        status=struct.unpack('<IIQQQIIQQQ',read(0x5fb93000,64))
        status_after=struct.unpack('<I',read(0x5fb93000,4))[0]
        ack=struct.unpack('<IIQQII',read(0x5fb94000,32))
        ack_after=struct.unpack('<I',read(0x5fb94000,4))[0]
        active=struct.unpack('<QQ',read(0x5fb95000,16))[1]
        covered=status[0] and not status[0]&1 and status[1]==1 and status[5]==2 and not status[6] and status[7]>0 and not active and not chat and not wallet and ack[0] and not ack[0]&1 and ack[1]==3 and ack[2]==status[2] and tuple(ack[4:6])==tuple(gpu[9:11])
        return dict(ready=bool(covered and status[0]==status_after and ack[0]==ack_after and gpu[0]==after and not gpu[0]&1 and gpu[2]>=2),gpu=gpu,locked=True,status=list(status),display_ack=list(ack),session_active=active,
                    chat_initialized=chat==APP_MAGIC,wallet_initialized=wallet==APP_MAGIC)
    return dict(ready=bool(ready(gpu,after,chat,wallet)),gpu=gpu,
                chat_initialized=chat==APP_MAGIC,wallet_initialized=wallet==APP_MAGIC)
