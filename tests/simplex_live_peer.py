"""Disposable native upstream peer; loopback WebSocket API, actual SMP relay.

Never uses the default SimpleX database or an existing user's conversations.
Only fresh files beneath a caller-owned private temporary directory are used.
"""
import base64
import hashlib
import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import threading
import time

BINARY=Path(os.environ.get('SIMPLEX_LIVE_CLI','/Volumes/untitled/sel4-personal-os/.cache/simplex-chat-upstream/dist-newstyle/build/aarch64-osx/ghc-9.6.3/simplex-chat-7.0.0.12/x/simplex-chat/build/simplex-chat/simplex-chat'))
RELAY='smp://u2dS9sG8nMNURyZwqASV4yROM28Er0luVTx5X1CsMrU=@smp4.simplex.im:5223'

class Peer:
    def __init__(self, directory):
        self.directory=Path(directory); self.directory.mkdir(mode=0o700)
        self.events=[];self.condition=threading.Condition();self.error=None;self.sequence=0;self.closed=False
        self.send_lock=threading.Lock();self.sock=None;self.process=None
        with socket.socket() as reserve:
            reserve.bind(('127.0.0.1',0));port=reserve.getsockname()[1]
        self.log=(self.directory/'cli.log').open('wb')
        self.process=subprocess.Popen([str(BINARY),'-d',str(self.directory/'peer'),'-s',RELAY,
            '--user-display-name','NexaLockPeer','-y','--host-mode','public','--smp-proxy','never',
            '--smp-web-port-servers','off','--files-folder',str(self.directory/'files'),
            '--temp-folder',str(self.directory/'tmp'),'--disable-backup','-p',str(port)],
            stdin=subprocess.DEVNULL,stdout=self.log,stderr=subprocess.STDOUT)
        try:
            deadline=time.monotonic()+60
            while True:
                if self.process.poll() is not None: raise RuntimeError('fresh CLI exited; inspect private peer/cli.log')
                try: self.sock=socket.create_connection(('127.0.0.1',port),timeout=2);break
                except OSError:
                    if time.monotonic()>deadline: raise RuntimeError('fresh CLI WebSocket startup timeout')
                    time.sleep(.2)
            key=base64.b64encode(os.urandom(16)).decode()
            self.sock.sendall((f'GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n').encode())
            header=b''
            while not header.endswith(b'\r\n\r\n'):
                chunk=self.sock.recv(1)
                if not chunk:raise EOFError('WebSocket handshake closed')
                header+=chunk
                if len(header)>8192: raise RuntimeError('WebSocket header bound')
            expected=base64.b64encode(hashlib.sha1((key+'258EAFA5-E914-47DA-95CA-C5AB0DC85B11').encode()).digest())
            assert b' 101 ' in header and expected.lower() in header.lower(),header
            self.sock.settimeout(None)
            self.reader=threading.Thread(target=self._read,daemon=True);self.reader.start()
        except BaseException:
            self.close();raise

    def _exact(self,n):
        data=b''
        while len(data)<n:
            chunk=self.sock.recv(n-len(data))
            if not chunk: raise EOFError('peer WebSocket closed')
            data+=chunk
        return data

    def _frame(self,body,opcode=1):
        assert len(body)<=4*1024*1024
        mask=os.urandom(4);n=len(body)
        length=bytes([0x80|n]) if n<126 else b'\xfe'+struct.pack('!H',n) if n<=65535 else b'\xff'+struct.pack('!Q',n)
        frame=bytes([0x80|opcode])+length+mask+bytes(c^mask[i%4] for i,c in enumerate(body))
        with self.send_lock:self.sock.sendall(frame)

    def _read(self):
        pending=bytearray()
        try:
            while True:
                first,second=self._exact(2);op=first&15;n=second&127
                if n==126:n=struct.unpack('!H',self._exact(2))[0]
                elif n==127:n=struct.unpack('!Q',self._exact(8))[0]
                if n>4*1024*1024 or second&128:raise RuntimeError('invalid server frame')
                body=self._exact(n)
                if op==8:raise EOFError('peer closed')
                if op==9:self._frame(body,10);continue
                if op==10:continue
                assert op in (0,1,2)
                if len(pending)+len(body)>4*1024*1024:raise RuntimeError('WebSocket message bound')
                pending.extend(body)
                if first&128:
                    event=json.loads(pending);pending.clear()
                    with self.condition:
                        self.events.append(event);self.condition.notify_all()
        except BaseException as e:
            with self.condition:self.error=e;self.condition.notify_all()

    def wait(self,predicate,timeout=120):
        deadline=time.monotonic()+timeout
        with self.condition:
            while True:
                for event in self.events:
                    if predicate(event):return event
                if self.error:raise RuntimeError('peer reader failed') from self.error
                remaining=deadline-time.monotonic()
                if remaining<=0:raise TimeoutError('peer event timeout')
                self.condition.wait(min(remaining,1))

    def command(self,cmd,timeout=60):
        self.sequence+=1;corr=str(self.sequence)
        self._frame(json.dumps({'corrId':corr,'cmd':cmd}).encode())
        response=self.wait(lambda e:e.get('corrId')==corr,timeout)['resp']
        if response.get('type') in ('chatCmdError','chatError'):raise RuntimeError('peer command failed '+json.dumps(response))
        return response

    def received(self,text,timeout=120):
        def received(event):
            if event.get('corrId') is not None:return False
            response=event.get('resp',{})
            if response.get('type')!='newChatItems':return False
            return any(isinstance(item,dict) and item.get('chatItem',{}).get('content',{}).get('type')=='rcvMsgContent' and
                item['chatItem']['content'].get('msgContent',{}).get('text')==text for item in response.get('chatItems',[]))
        return self.wait(received,timeout)

    def save_events(self,destination):
        with self.condition:Path(destination).write_text(json.dumps(self.events,indent=2)+'\n')

    def close(self):
        self.closed=True
        if self.sock:
            try:self.sock.shutdown(socket.SHUT_RDWR)
            except OSError:pass
            self.sock.close()
        if self.process and self.process.poll() is None:
            self.process.terminate()
            try:self.process.wait(timeout=10)
            except subprocess.TimeoutExpired:self.process.kill();self.process.wait(timeout=5)
        self.log.close()
