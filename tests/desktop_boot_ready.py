"""Structured readiness for the disposable VirGL desktop; no app text is read."""
import struct

APP_MAGIC = 0x4e584445534b3031

def ready(gpu, after, chat, wallet):
    return (gpu[0] != 0 and not gpu[0]&1 and gpu[0] == after and
            gpu[2] >= 2 and gpu[13] == 0x4d4f5431 and
            1600 <= gpu[9] <= 3840 and 1200 <= gpu[10] <= 2160 and
            chat == wallet == APP_MAGIC)

def gpu_boot_state(q, data):
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
    return dict(ready=bool(ready(gpu,after,chat,wallet)),gpu=gpu,
                chat_initialized=chat==APP_MAGIC,wallet_initialized=wallet==APP_MAGIC)
