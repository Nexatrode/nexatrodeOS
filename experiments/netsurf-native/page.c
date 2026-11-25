/* SPDX-License-Identifier: GPL-2.0-only
 * Main-document state, independent of subresource fetch order and results.
 */
#include <stdio.h>
#include <string.h>
#include "native_page.h"
#include "utils/nsurl.h"
#include "content/content.h"
#include "content/content_protected.h"
#include "content/hlcache.h"
enum phase { IDLE, LOADING, RESOURCES, READY, STOPPED, FAILED };
static enum phase phase=IDLE;
static char target[2048],text[160],reason[100];
static long code;
static bool layout_failed;
#ifdef NATIVE_JAVASCRIPT
#define JS_STATE "JS on"
#else
#define JS_STATE "JS off"
#endif
static void changed(void) {
    const char *state="Ready";
    if(phase==STOPPED)state="Stopped";
    else if(phase==FAILED)state="Failed";
    else if(layout_failed)state="Layout update failed";
    else if(phase==LOADING)state="Loading page";
    else if(phase==RESOURCES)state="Loading resources";
    else if(strncmp(target,"resource:",9)==0 || strncmp(target,"about:",6)==0)state="Local page";
    if(phase==FAILED)snprintf(text,sizeof(text),"Failed: %.90s",reason);
    else if((phase==READY || phase==RESOURCES) && !layout_failed && strncmp(target,"https:",6)==0 && code)
        snprintf(text,sizeof(text),"HTTPS %ld%s / " JS_STATE,code,phase==RESOURCES?" loading":"");
    else snprintf(text,sizeof(text),"%s / " JS_STATE,state);
    printf("NETSURF|PAGE|%s|http=%ld\n",state,code);
    native_ui_status_changed();
}
void native_page_begin(const struct nsurl *url) {
    snprintf(target,sizeof(target),"%s",nsurl_access(url));
    phase=LOADING;code=0;reason[0]=0;layout_failed=false;changed();
}
void native_page_redirect(const struct nsurl *url) {
    snprintf(target,sizeof(target),"%s",nsurl_access(url));code=0;changed();
}
void native_page_content_event(struct hlcache_handle *h,int event,const char *error) {
    if(phase==STOPPED || phase==FAILED)return;
    struct content *c=hlcache_handle_get_content(h);
    if(c && c->http_code)code=c->http_code;
    if(event==CONTENT_MSG_READY)phase=RESOURCES;
    else if(event==CONTENT_MSG_DONE)phase=READY;
    else if(event==CONTENT_MSG_ERROR){phase=FAILED;snprintf(reason,sizeof(reason),"%s",error?error:"Page could not load");}
    else if(event!=CONTENT_MSG_LOADING)return;
    changed();
}
void native_page_stopped(void){phase=STOPPED;layout_failed=false;changed();}
void native_page_failure(const char *error){phase=FAILED;snprintf(reason,sizeof(reason),"%s",error);changed();}
void native_page_layout_notice(bool failed){
    if(phase!=READY && phase!=RESOURCES)return;
    if(layout_failed!=failed){layout_failed=failed;changed();}
}
const char *native_page_status(void){return text;}
const char *native_page_target(void){return target;}
bool native_page_loading(void){return phase==LOADING || phase==RESOURCES;}
bool native_page_error(void){return phase==FAILED || layout_failed;}
