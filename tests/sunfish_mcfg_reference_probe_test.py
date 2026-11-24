"""Run pinned Linux reader/option functions with in-memory I/O, without QRTR."""
from pathlib import Path
import hashlib, json, os, subprocess, tempfile, unittest

ROOT=Path(__file__).resolve().parents[1]
SOURCE=ROOT/'.cache/sunfish-mobian-comparison/wifi-preparation/source/tqftpserv/tqftpserv.c'
PIN='e8dc6a2ced3315f2c2e8b544f710b69fb6e8d464a92ce88ed800cf891edf22f1'
def function(source,name):
    start=source.index('static ',source.index('\nstatic ',source.index('struct tftp_client')))
    marker=source.index(name+'(',start)
    start=source.rfind('\nstatic ',0,marker)+1
    end=source.index('{',marker)+1;depth=1
    while depth:
        depth+=(source[end]=='{')-(source[end]=='}');end+=1
    return source[start:end]

class LinuxMcfgProbe(unittest.TestCase):
    def test_actual_linux_options_and_stat_termination(self):
        if not SOURCE.exists():self.skipTest('Pinned private Linux reference source is not available')
        source=SOURCE.read_bytes();self.assertEqual(hashlib.sha256(source).hexdigest(),PIN)
        functions='\n\n'.join(function(source.decode(),name) for name in ['tftp_send_data','tftp_send_oack','parse_options','handle_reader'])
        code=r'''
#include <arpa/inet.h>
#include <sys/types.h>
#include <sys/socket.h>
#include <assert.h>
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
enum {OP_RRQ=1,OP_WRQ,OP_DATA,OP_ACK,OP_ERROR,OP_OACK,ERROR_END_OF_TRANSFER=9};
struct sockaddr_qrtr {sa_family_t sq_family; uint32_t sq_node,sq_port;};
struct tftp_client {struct sockaddr_qrtr sq;int sock,fd;size_t block,blksize,rsize,wsize;unsigned int timeoutms;off_t seek;};
static unsigned char sent[512],incoming[128];static size_t sent_len,incoming_len;static unsigned sends;
static ssize_t fake_send(int fd,const void*b,size_t n,int flags){assert(n<=sizeof(sent));memcpy(sent,b,n);sent_len=n;sends++;return n;}
static ssize_t fake_pread(int fd,void*b,size_t n,off_t off){if(off)return 0;assert(n);*(char*)b='X';return 1;}
static ssize_t fake_recvfrom(int fd,void*b,size_t n,int flags,struct sockaddr*sa,socklen_t*sl){assert(n>=incoming_len);memcpy(b,incoming,incoming_len);struct sockaddr_qrtr peer={0,0,0x4a};memcpy(sa,&peer,sizeof(peer));*sl=sizeof(peer);return incoming_len;}
#define send fake_send
#define recvfrom fake_recvfrom
#define pread fake_pread
FUNCTIONS
static void load(const unsigned char*b,size_t n){assert(n<=sizeof(incoming));memcpy(incoming,b,n);incoming_len=n;}
int main(void){
 const char stat[]="blksize\0007680\0timeoutms\0001000\0wsize\00010\0tsize\0000";
 const char zero[]="blksize\0007680\0timeoutms\0001000\0wsize\00010\0rsize\0000";
 size_t b=512,w=0,r=0;ssize_t t=-1;off_t seek=0;unsigned timeout=1000;
 parse_options(stat,sizeof(stat),&b,&t,&w,&timeout,&r,&seek);assert(b==7680&&w==10&&t==0&&r==0);
 t=1; /* actual handle_rrq substitutes fstat size when tsize was requested */
 tftp_send_oack(1,&b,(size_t*)&t,&w,&timeout,NULL,NULL);
 const char expected[]="\000\006blksize\0007680\0timeoutms\0001000\0tsize\0001\0wsize\00010";
 assert(sent_len==sizeof(expected)&&!memcmp(sent,expected,sizeof(expected)));
 printf("LINUX_STAT_OACK_BYTES=%zu\n",sent_len);
 b=512;w=0;r=0;t=-1;timeout=1000;
 parse_options(zero,sizeof(zero),&b,&t,&w,&timeout,&r,&seek);assert(b==7680&&w==10&&r==0&&t==-1);
 tftp_send_oack(1,&b,(size_t*)&t,&w,&timeout,r?&r:NULL,NULL);
 assert(sent_len==39); /* Explicit zero rsize is accepted and omitted from OACK. */
 const char unlink[]="unlink\0000";parse_options(unlink,sizeof(unlink),&b,&t,&w,&timeout,&r,&seek);assert(b==7680&&w==10&&r==0);
 struct tftp_client client={.sq={0,0,0x4a},.sock=1,.fd=2,.blksize=7680,.rsize=0,.wsize=10,.timeoutms=1000};
 const unsigned char end[]={0,5,0,9,'E','n','d',' ','o','f',' ','T','r','a','n','s','f','e','r',0};
 load(end,sizeof(end));unsigned before=sends;assert(handle_reader(&client)==-1);assert(sends==before);
 const unsigned char ack0[]={0,4,0,0};load(ack0,sizeof(ack0));assert(handle_reader(&client)==1);assert(sent_len==5&&!memcmp(sent,"\000\003\000\001X",5));
 const unsigned char ack1[]={0,4,0,1};load(ack1,sizeof(ack1));before=sends;assert(handle_reader(&client)==0&&sends==before);
 puts("LINUX_PROBE_ORACLE_PASS: tsize, zero-rsize, unknown option, ERROR9 stat end, one-byte DATA/ACK");
}
'''.replace('FUNCTIONS',functions)
        out=os.environ.get('SUNFISH_MCFG_REFERENCE_OUT')
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(out) if out else Path(tmp);p.mkdir(parents=True,exist_ok=True)
            (p/'linux-probe.c').write_text(code)
            subprocess.run(['cc','-std=c11','-Wno-unused-parameter',str(p/'linux-probe.c'),'-o',str(p/'linux-probe')],check=True)
            result=subprocess.run([str(p/'linux-probe')],text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,check=True)
            self.assertIn('LINUX_PROBE_ORACLE_PASS',result.stdout);print(result.stdout)
            if out:
                (p/'linux-probe.log').write_text(result.stdout)
                (p/'linux-source-pins.json').write_text(json.dumps({'source':str(SOURCE),'sha256':PIN,'functions':['tftp_send_data','tftp_send_oack','parse_options','handle_reader'],'io':'In-memory send/recvfrom/pread stubs only; no sockets, device operations or real files'},indent=2)+'\n')
if __name__=='__main__':unittest.main()
