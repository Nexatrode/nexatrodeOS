/* SPDX-License-Identifier: GPL-2.0-only */
#include <stdbool.h>
#include "native_dom_work.h"
#ifdef NATIVE_BROWSER_TESTS
static bool expire;
void native_dom_test_expire(void){expire=true;printf("NETSURF|DOM_WORK|expire_next\n");}
#endif
uint64_t native_dom_deadline(void)
{
#ifdef NATIVE_BROWSER_TESTS
    if(expire){expire=false;return native_ms();}
#endif
    return native_ms()+NATIVE_DOM_PARSE_MS;
}
