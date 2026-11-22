#!/usr/bin/env python3
"""Execute the actual isolated kernel attribute selectors with host C types."""
from pathlib import Path
import os,re,subprocess,tempfile,unittest
ROOT=Path(__file__).resolve().parents[1]
SOURCE=Path(os.environ.get('SUNFISH_NC_KERNEL_SOURCE',ROOT/'.cache/sunfish-native-wifi/private/native-wrq-table-normal-nc-v1/kernel-source/src/arch/arm/64/kernel/vspace.c'))
def function(source,name):
    start=source.index('static ',source.index(name)-40)
    brace=source.index('{',source.index(name));depth=1;end=brace+1
    while depth:
        depth+=(source[end]=='{')-(source[end]=='}');end+=1
    return source[start:end]
class KernelAttributes(unittest.TestCase):
    def test_actual_guard_and_pte_selector_preserve_defaults_and_reject_device_nc(self):
        s=SOURCE.read_text()
        guard=function(s,'normalNonCacheableAttributesValid')
        selector=function(s,'makeUserPagePTE')
        prefix=r'''
#include <stdint.h>
#include <stdbool.h>
#include <assert.h>
typedef uint64_t word_t;typedef uint64_t paddr_t;typedef unsigned vm_rights_t;
typedef unsigned vm_page_size_t;typedef unsigned vm_attributes_t;typedef bool bool_t;
#define CONST
#define SMP_TERNARY(a,b) (b)
#define SMP_SHARE 3
#define ARMSmallPage 0
#define NORMAL 4
#define DEVICE_nGnRnE 0
#define S2_NORMAL 15
#define S2_DEVICE_nGnRnE 0
#define NORMAL_NC 3
#define vm_attributes_get_armExecuteNever(a) (((a)>>2)&1)
#define vm_attributes_get_armPageCacheable(a) ((a)&1)
#define vm_attributes_get_armNormalNonCacheable(a) (((a)>>3)&1)
#define APFromVMRights(x) (x)
typedef struct {unsigned xn,ng,sh,ap,attr;paddr_t pa;}pte_t;
pte_t pte_pte_4k_page_new(unsigned xn,paddr_t pa,unsigned ng,unsigned af,unsigned sh,unsigned ap,unsigned attr){return(pte_t){xn,ng,sh,ap,attr,pa};}
#define pte_pte_page_new pte_pte_4k_page_new
'''
        main=r'''
int main(void){
 for(unsigned attr=0;attr<16;attr++)for(unsigned device=0;device<2;device++){
  bool nc=!!(attr&8),cached=!!(attr&1);
#ifdef CONFIG_ARM_HYPERVISOR_SUPPORT
  bool valid=!nc;
#else
  bool valid=!nc||(!cached&&!device);
#endif
  assert(normalNonCacheableAttributesValid(attr,device)==valid);
  if(!valid)continue;
  for(unsigned size=0;size<2;size++){
   pte_t p=makeUserPagePTE(0x80003000,1,attr,size);
   assert(p.pa==0x80003000&&p.ap==1&&p.xn==!!(attr&4));
#ifdef CONFIG_ARM_HYPERVISOR_SUPPORT
   assert(p.attr==(cached?15:0)&&p.ng==0);
#else
   assert(p.attr==(nc?3:cached?4:0)&&p.sh==(nc?2:0)&&p.ng==1);
#endif
  }
 }
}
'''
        with tempfile.TemporaryDirectory() as tmp:
            c=Path(tmp)/'selectors.c';c.write_text(prefix+guard+'\n'+selector+'\n'+main)
            for hyp in [False,True]:
                binary=Path(tmp)/('hyp' if hyp else 'el1')
                subprocess.run(['cc','-std=c11','-Werror',*(['-DCONFIG_ARM_HYPERVISOR_SUPPORT'] if hyp else []),str(c),'-o',str(binary)],check=True,capture_output=True)
                subprocess.run([str(binary)],check=True,capture_output=True)
    def test_map_guard_precedes_pte_creation_and_mapping_mutation(self):
        body=function(SOURCE.read_text(),'decodeARMFrameInvocation')
        guard=body.index('normalNonCacheableAttributesValid')
        self.assertLess(guard,body.index('makeUserPagePTE'))
        self.assertLess(guard,body.index('setThreadState'))
        self.assertIn('invalidArgumentNumber = 2',body[guard:guard+550])
if __name__=='__main__':unittest.main()
