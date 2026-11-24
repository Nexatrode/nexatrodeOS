#include <assert.h>
#include <limits.h>
#include "../experiments/desktop-shell/protocol.h"
int main(void) {
    struct shell_state s = {.top=2, .windows={{68,126,1,1},{650,110,1,1}}};
    assert(shell_hit(&s,100,200)==1); assert(shell_hit(&s,700,200)==2);
    assert(shell_hit(&s,100,100)==3); assert(shell_window_at(&s,100,100)==1);
    assert(shell_hit(&s,640,730)==3);
    s.windows[1].x=100; assert(shell_hit(&s,200,200)==2);
    s.top=1; assert(shell_hit(&s,200,200)==1);
    s.windows[0].visible=0; assert(shell_hit(&s,200,200)==2);
    shell_position(&s.windows[1],INT32_MIN,INT32_MAX);
    assert(s.windows[1].x==12 && s.windows[1].y==152);
    shell_position(&s.windows[1],INT32_MAX,INT32_MIN);
    assert(s.windows[1].x==708 && s.windows[1].y==92);
    for (int x=-200; x<1600; x+=13) for (int y=-200; y<1000; y+=17) {
        shell_position(&s.windows[1],x,y);
        assert(s.windows[1].x>=12 && s.windows[1].x+APP_WIDTH+1<SHELL_WIDTH);
        assert(s.windows[1].y>=43 && s.windows[1].y+APP_HEIGHT<SHELL_HEIGHT-114);
    }
}
