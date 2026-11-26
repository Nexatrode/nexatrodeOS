#include <assert.h>
#include <stdio.h>
#include "../experiments/desktop-gpu/pacing.h"
int main(void) {
    struct frame_pacer p={.period=16667};
    assert(!frame_pacer_ready(&p,0,false));frame_pacer_request(&p,100);
    assert(frame_pacer_ready(&p,100,false));assert(frame_pacer_take(&p,100)==0);
    frame_pacer_request(&p,200);frame_pacer_request(&p,300);
    assert(p.coalesced==1&&!frame_pacer_ready(&p,20000,true));
    assert(!frame_pacer_ready(&p,16766,false)&&frame_pacer_ready(&p,16767,false));
    assert(frame_pacer_take(&p,50000)==49800); /* Late completion skips catch-up. */
    frame_pacer_request(&p,50001);assert(!frame_pacer_ready(&p,50001,false));
    assert(frame_pacer_ready(&p,66667,false));frame_pacer_take(&p,66667);
    unsigned frames=0;uint64_t last=0,done=0;
    for(uint64_t now=70000;now<1070000;now+=100){
        if(now%1000==0)frame_pacer_request(&p,now); /* 1000 Hz input storm. */
        if(frame_pacer_ready(&p,now,now<done)){
            assert(!last||now-last>=p.period);last=now;frame_pacer_take(&p,now);
            done=now+10000;++frames;
        }
    }
    assert(frames>=58&&frames<=60&&p.coalesced>900);
    assert(!frame_pacer_ready(&p,done-1,true));
    puts("PASS 60 Hz cadence, one-frame gate, coalesced input storm, late completion and no catch-up burst");
}
