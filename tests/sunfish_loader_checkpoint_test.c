#include <assert.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
static volatile uint32_t *test_fb;
#define CHECKPOINT_FB test_fb
#include "../platforms/sunfish/loader-boot-checkpoints.h"

int main(void)
{
    const unsigned pixels=1080*2340;
    uint32_t *storage=malloc((2*pixels+2)*sizeof(uint32_t));
    assert(storage);
    test_fb=storage+1;
    const char *labels[]={"IMAGE ENTRY","IMAGE RELOCATED","IMAGE COPYING","IMAGE COPIED","IMAGE KERNEL","IMAGE ENTRY HOLD","IMAGE COPY START","IMAGE COPY CHECK","IMAGE COPY PASS","IMAGE COPY FAIL FFFFFFFE","IMAGE COPY SKIP","IMAGE COPY READ","IMAGE COPY WRITE","IMAGE COPY 046513C4","IMAGE VERIFY1 046513C4","IMAGE VERIFY2 046513C4","IMAGE CACHE START","IMAGE CACHE CLEAN","IMAGE JUMP","IMAGE MAIN","IMAGE LOAD 0A","IMAGE LOAD PASS","IMAGE LOAD FAIL C00001A0","123456789012345678901234567890"};
    for(unsigned n=0;n<sizeof(labels)/sizeof(labels[0]);n++) {
        for(unsigned i=0;i<2*pixels+2;i++) storage[i]=0xa5a5a5a5;
        loader_checkpoint(labels[n]);
        assert(storage[0]==0xa5a5a5a5 && storage[2*pixels+1]==0xa5a5a5a5);
        unsigned drawn=0;
        for(unsigned i=0;i<pixels;i++) {
            unsigned y=i/1080;
            assert(storage[1+i]==storage[1+pixels+i]);
            if(y<160 || y>=320) assert(storage[1+i]==0xa5a5a5a5);
            else {
                assert(storage[1+i]==0x00122b52 || storage[1+i]==0x0000e5e5);
                drawn+=storage[1+i]==0x0000e5e5;
            }
        }
        assert(drawn>0);
    }
    free(storage);
    return 0;
}
