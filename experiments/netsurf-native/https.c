/* SPDX-License-Identifier: GPL-2.0-only
 * Asynchronous NetSurf fetcher. TLS/TCP/entropy/RTC live in separate PDs.
 * Read-only results remain stable until the frontend submits another job.
 */
#include <microkit.h>
#include <stdatomic.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include "https.h"
#include "native.h"
#include "utils/nsurl.h"
#include "content/fetch.h"
#include "content/fetchers.h"
#include "utils/corestrings.h"
#ifdef NATIVE_JAVASCRIPT
#define SCRIPT_STATE "JS on"
#else
#define SCRIPT_STATE "JS off"
#endif
#define MAGIC 0x31535448u
#define BODY_MAX (2u*1024*1024)
uintptr_t https_request,https_response;
struct request {uint32_t magic;_Atomic uint32_t seq;uint32_t command,length;char url[2048];};
struct response {uint32_t magic,seq;_Atomic uint32_t state;uint32_t error,status,headers_len,body_len,reserved;unsigned char headers[16384],body[BODY_MAX];};
struct context {struct fetch *parent;nsurl *url;struct context *next;bool started,aborted,stopped,unsupported;uint32_t seq;size_t sent;bool headers_sent;uint64_t started_at;};
_Static_assert(sizeof(struct response)<=0x205000,"reply mapping must cover its full payload");
static struct context *queue,*active;
static unsigned queued;static uint32_t sequence;
void native_https_stop(void){for(struct context *c=queue;c;c=c->next)c->stopped=true;}
static void submit(const char *url,unsigned command){
    struct request *r=(void*)https_request;r->magic=MAGIC;r->command=command;r->length=url?strlen(url):0;
    if(url)memcpy(r->url,url,r->length);
    atomic_store_explicit(&r->seq,++sequence,memory_order_release);microkit_notify(3);
}
static bool initialise(lwc_string *scheme){(void)scheme;return true;}
static bool acceptable(const nsurl *url){return nsurl_length(url)<2048;}
static void *setup(struct fetch *parent,nsurl *url,bool only2xx,bool downgrade,const char *post,const struct fetch_multipart_data *multi,const char **headers){
    (void)only2xx;(void)downgrade;(void)headers;
    if(queued>=32)return NULL;
    struct context *c=calloc(1,sizeof(*c));if(!c)return NULL;
    c->parent=parent;c->url=nsurl_ref(url);c->unsupported=post||multi;
    struct context **tail=&queue;while(*tail)tail=&(*tail)->next;*tail=c;++queued;return c;
}
static bool start(void *p){((struct context*)p)->started=true;return true;}
static void abort_fetch(void *p){((struct context*)p)->aborted=true;}
static void free_fetch(void *p){struct context *c=p;nsurl_unref(c->url);free(c);}
static void finish(struct context *c){
    struct context **link=&queue;while(*link&&*link!=c)link=&(*link)->next;if(*link)*link=c->next;
    if(active==c)active=NULL;--queued;fetch_remove_from_queues(c->parent);fetch_free(c->parent);
}
static void fail(struct context *c,const char *text){
    printf("NETSURF|HTTPS_ERROR|id=%u|%s\n",c->seq,text);
    fetch_msg msg={.type=FETCH_ERROR,.data.error=text};fetch_send_callback(&msg,c->parent);
}
static bool polling;
static void poll_inner(lwc_string *scheme){
    (void)scheme;
    // Callbacks may append or abort contexts. Only this poll removes them.
    for(struct context *c=queue,*next;c;c=next){next=c->next;if(c->aborted || c->stopped){if(active==c)submit(NULL,2);if(c->stopped&&!c->aborted)fail(c,"Request stopped");finish(c);}}
    if(!active){for(struct context *c=queue;c;c=c->next)if(c->started){active=c;break;}}
    struct context *c=active;if(!c)return;
    if(c->unsupported){fail(c,"HTTPS POST is not supported yet");finish(c);return;}
    if(!c->seq){submit(nsurl_access(c->url),1);c->seq=sequence;c->started_at=native_ms();return;}
    const struct response *r=(void*)https_response;
    unsigned state=atomic_load_explicit(&r->state,memory_order_acquire);
    if(!state||r->seq!=c->seq){if(native_ms()-c->started_at>50000){submit(NULL,2);fail(c,"HTTPS worker timed out");finish(c);}return;}
    if(r->magic!=MAGIC||r->headers_len>sizeof(r->headers)||r->body_len>BODY_MAX){fail(c,"Invalid HTTPS broker reply");finish(c);return;}
    if(state!=1){char error[161];size_t n=r->headers_len;if(n>160)n=160;memcpy(error,r->headers,n);error[n]=0;fail(c,error);finish(c);return;}
    if(!c->headers_sent){
        fetch_set_http_code(c->parent,r->status);
        char location[2048]={0};size_t at=0;
        while(at<r->headers_len&&!c->aborted){
            size_t end=at;while(end<r->headers_len&&r->headers[end]!='\n')++end;
            size_t n=end-at;if(n&&r->headers[at+n-1]=='\r')--n;
            if(n>=9&&!strncasecmp((const char*)r->headers+at,"Location:",9)){
                size_t p=at+9;while(p<at+n&&(r->headers[p]==' '||r->headers[p]=='\t'))++p;
                size_t len=at+n-p;if(len>=sizeof(location)){fail(c,"Redirect URL is too long");finish(c);return;}memcpy(location,r->headers+p,len);location[len]=0;
            }
            fetch_msg msg={.type=FETCH_HEADER,.data.header_or_data={r->headers+at,end-at+(end<r->headers_len)}};fetch_send_callback(&msg,c->parent);at=end+1;
        }
        if(!c->aborted&&r->status>=300&&r->status<400&&location[0]){
            // NetSurf resolves relative URLs and bounds its own redirect count.
            // Only HTTPS has a network fetcher: no silent HTTP downgrade.
            fetch_msg msg={.type=FETCH_REDIRECT,.data.redirect=location};fetch_send_callback(&msg,c->parent);finish(c);return;
        }
        char header[80];int n=snprintf(header,sizeof(header),"Content-Length: %u\r\n",r->body_len);
        if(!c->aborted){fetch_msg msg={.type=FETCH_HEADER,.data.header_or_data={(uint8_t*)header,n}};fetch_send_callback(&msg,c->parent);}
        c->headers_sent=true;
    }
    // At most 16 KiB of parsing/layout input per poll; return to UI scheduling.
    if(!c->aborted&&c->sent<r->body_len){size_t n=r->body_len-c->sent;if(n>16384)n=16384;
        fetch_msg msg={.type=FETCH_DATA,.data.header_or_data={r->body+c->sent,n}};c->sent+=n;fetch_send_callback(&msg,c->parent);
    }
    if(c->aborted){finish(c);return;}
    if(c->sent==r->body_len){
        printf("NETSURF|HTTPS|id=%u|status=%u|bytes=%u\n",c->seq,r->status,r->body_len);
        fetch_msg msg={.type=FETCH_FINISHED};fetch_send_callback(&msg,c->parent);finish(c);
    }
}
static void poll(lwc_string *s){if(polling)return;polling=true;poll_inner(s);polling=false;}
static void finalise(lwc_string *s){(void)s;}
nserror native_https_register(void){
    const struct fetcher_operation_table ops={.initialise=initialise,.acceptable=acceptable,.setup=setup,.start=start,.abort=abort_fetch,.free=free_fetch,.poll=poll,.finalise=finalise};
    return fetcher_add(lwc_string_ref(corestring_lwc_https),&ops);
}
