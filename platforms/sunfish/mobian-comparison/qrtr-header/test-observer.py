#!/usr/bin/env python3
"""Exercise the production observer with synthetic wire headers."""
from pathlib import Path
import subprocess
import tempfile

source = (Path(__file__).with_name('observe.h')).read_text()
body = source[source.index('static void sunfish_qrtr_observe'):]
preamble = r'''
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <string.h>
typedef uint32_t u32;
typedef uint8_t u8;
#define U32_MAX UINT32_MAX
#define ALIGN(v, a) (((v)+(a)-1)&~((a)-1))
#define QRTR_TYPE_DATA 1
#define QRTR_TYPE_DEL_CLIENT 6
#define QRTR_TYPE_RESUME_TX 7
static u32 get_unaligned_le16(const u8 *p) { return p[0] | (u32)p[1]<<8; }
static u32 get_unaligned_le32(const u8 *p) { return get_unaligned_le16(p) | get_unaligned_le16(p+2)<<16; }
struct sunfish_qrtr_header {
 bool tx, confirm;
 u32 version, type, src_node, src_port, dst_node, dst_port, length;
 u32 control, key_node, key_port, qmi_kind, qmi_txn, qmi_msg;
};
static struct sunfish_qrtr_header captured;
static int count;
static bool trace_header_enabled(void) { return true; }
static void trace_header(const struct sunfish_qrtr_header *h) { captured=*h; count++; }
'''
cases = r'''
int main(void) {
 u8 qmi[48] = { [0]=1,[4]=1,[8]=1,[12]=2,[13]=0x40,[16]=1,[20]=14,[28]=0x20,
                [32]=2,[33]=1,[35]=0x20,[37]=7 };
 sunfish_qrtr_observe(true,qmi,sizeof(qmi));
 assert(count==1 && captured.tx && captured.confirm && captured.src_port==0x4002);
 assert(captured.dst_port==0x20 && captured.qmi_kind==2 && captured.qmi_txn==1 && captured.qmi_msg==0x20);
 assert(captured.control==U32_MAX);
 qmi[37]=8;
 sunfish_qrtr_observe(true,qmi,sizeof(qmi));
 assert(count==2 && captured.qmi_msg==U32_MAX);
 sunfish_qrtr_observe(true,qmi,sizeof(qmi)-1);
 assert(count==2);
 u8 ctrl[52] = { [0]=1,[4]=6,[12]=0x1f,[20]=20,[24]=1,[28]=0xff,[29]=0xff,
                 [30]=0xff,[31]=0xff,[32]=6,[40]=0x1f };
 sunfish_qrtr_observe(false,ctrl,sizeof(ctrl));
 assert(count==3 && !captured.tx && captured.control==6 && captured.key_node==0 && captured.key_port==0x1f);
 ctrl[4]=ctrl[32]=7;
 sunfish_qrtr_observe(false,ctrl,sizeof(ctrl));
 assert(count==4 && captured.control==7 && captured.key_port==0x1f);
 u8 v3[44] = { [0]=3,[1]=1,[2]=1,[3]=16,[4]=12,[10]=0x20,[12]=1,[14]=2,[15]=0x40,
               [33]=1,[35]=0x20,[37]=5 };
 sunfish_qrtr_observe(false,v3,sizeof(v3));
 assert(count==5 && captured.version==3 && captured.confirm && captured.qmi_msg==0x20);
 v3[3]=255;
 sunfish_qrtr_observe(false,v3,sizeof(v3));
 assert(count==5);
 return 0;
}
'''
with tempfile.TemporaryDirectory() as temp:
    src = Path(temp) / 'test.c'
    exe = Path(temp) / 'test'
    src.write_text(preamble + body + cases)
    subprocess.run(['cc', '-std=c11', '-Wall', '-Wextra', '-Werror',
                    '-fsanitize=address,undefined', str(src), '-o', str(exe)], check=True)
    subprocess.run([str(exe)], check=True)
print('PASS: QRTR v1/v3, QMI IDs, close and credit keys, invalid size and optional header bounds')
