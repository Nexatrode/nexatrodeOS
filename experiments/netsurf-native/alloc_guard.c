/* SPDX-License-Identifier: GPL-2.0-only
 * Track real allocator failures within a synchronous layout transaction.
 * Failure injection is compiled only into the explicitly marked test image.
 */
#include <stddef.h>
#include <stdio.h>
#include <limits.h>
#include "native_reflow_guard.h"
static bool scope,failed;
static unsigned allocations;
#ifdef NATIVE_BROWSER_TESTS
static unsigned fail_at,armed;
void native_reflow_test_arm(unsigned n){armed=n;printf("NETSURF|FAULT|armed=%u\n",n);}
#endif
void native_reflow_alloc_begin(void){scope=true;failed=false;allocations=0;}
void native_reflow_layout_begin(void){
#ifdef NATIVE_BROWSER_TESTS
    if(armed==UINT_MAX)fail_at=allocations+1;
#endif
}
bool native_reflow_alloc_end(void){
    scope=false;
#ifdef NATIVE_BROWSER_TESTS
    if(armed)printf("NETSURF|FAULT|allocation=%u|calls=%u|failed=%u\n",armed,allocations,failed);
    armed=fail_at=0;
#endif
    return failed;
}
static bool inject(void){
    if(!scope)return false;
    allocations++;
#ifdef NATIVE_BROWSER_TESTS
    if(armed && !fail_at)fail_at=armed;
    if(fail_at && allocations==fail_at){failed=true;return true;}
#endif
    return false;
}
void *__real_malloc(size_t);
void *__real_calloc(size_t,size_t);
void *__real_realloc(void *,size_t);
void *__wrap_malloc(size_t n){if(inject())return NULL;void *p=__real_malloc(n);if(scope && n && !p)failed=true;return p;}
void *__wrap_calloc(size_t n,size_t m){if(inject())return NULL;void *p=__real_calloc(n,m);if(scope && n && m && !p)failed=true;return p;}
void *__wrap_realloc(void *old,size_t n){if(inject())return NULL;void *p=__real_realloc(old,n);if(scope && n && !p)failed=true;return p;}
