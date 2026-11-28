/* Locked startup must have a complete scene order before apps are visible. */
#include <assert.h>
#include <stdio.h>
#include "../experiments/desktop-shell/protocol.h"
int main(void) {
    struct shell_state state={0};shell_extra_windows(&state);
    unsigned order[DESKTOP_APP_COUNT],seen=0;shell_order(&state,order);
    for(unsigned i=0;i<DESKTOP_APP_COUNT;++i){
        assert(order[i]>=1&&order[i]<=DESKTOP_APP_COUNT);
        assert(!(seen&(1u<<order[i])));seen|=1u<<order[i];
        assert(!state.windows[i].open&&!state.windows[i].visible);
    }
    shell_raise(&state,1);shell_order(&state,order);assert(order[DESKTOP_APP_COUNT-1]==1);
#if DESKTOP_APP_COUNT == 4
    assert(state.windows[3].x==460&&state.windows[3].y==148);
#endif
    puts("PASS complete hidden-window order and first authenticated focus");
}
