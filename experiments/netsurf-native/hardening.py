# SPDX-License-Identifier: BSD-2-Clause
"""Pinned-source hooks for main-document state and Stop lifecycle handling."""


def patch(source):
    def edit(name,marker,old,new):
        p=source/name;s=p.read_text()
        if marker not in s:
            assert old in s,name
            p.write_text(s.replace(old,new,1))
    edit('content/llcache.c','long native_http_code;', 'struct llcache_object {',
         'struct llcache_object {\n    long native_http_code;')
    edit('content/llcache.c','object->native_http_code = fetch_http_code', 'case FETCH_HEADER:\n\t\t/* Received a fetch header */',
         'case FETCH_HEADER:\n        object->native_http_code = fetch_http_code(object->fetch.fetch);\n\t\t/* Received a fetch header */')
    edit('content/llcache.c','destination->native_http_code = source->native_http_code;',
         '/* ETag must be first, as it can fail when deep cloning */',
         'destination->native_http_code = source->native_http_code;\n\t/* ETag must be first, as it can fail when deep cloning */')
    p=source/'content/llcache.c'
    if 'long llcache_handle_native_http_code(' not in p.read_text():
        with p.open('a') as f:f.write('\nlong llcache_handle_native_http_code(const llcache_handle *h){return h->object->native_http_code;}\n')
    p=source/'content/llcache.h'
    if 'long llcache_handle_native_http_code(' not in p.read_text():
        with p.open('a') as f:f.write('\nlong llcache_handle_native_http_code(const llcache_handle *);\n')
    edit('content/content.c','c->http_code = llcache_handle_native_http_code', 'c->http_code = 0;',
         'c->http_code = llcache_handle_native_http_code(llcache);')
    edit('desktop/browser_window.c','#include "native_page.h"', '#include <stdlib.h>',
         '#include <stdlib.h>\n#include "native_page.h"')
    edit('desktop/browser_window.c','#include "native_reflow_guard.h"', '#include "native_page.h"',
         '#include "native_page.h"\n#include "native_reflow_guard.h"')
    edit('desktop/browser_window.c','Native Stop also cancels queued layout on a completed document',
         'void browser_window_stop(struct browser_window *bw)\n{\n\tint children, index;',
         '''void browser_window_stop(struct browser_window *bw)
{
    int children, index;
    /* Native Stop also cancels queued layout on a completed document. */
    if(bw->current_content && content_get_type(bw->current_content)==CONTENT_HTML)
        native_reflow_stop(hlcache_handle_get_content(bw->current_content));''')
    edit('desktop/browser_window.c','Native main-document lifecycle',
         'struct browser_window *bw = pw;\n\tnserror res = NSERROR_OK;',
         '''struct browser_window *bw = pw;
    nserror res = NSERROR_OK;
    /* Native main-document lifecycle: subresource callbacks never enter here. */
    if (!bw->parent && !bw->internal_nav && (c==bw->loading_content || c==bw->current_content)) {
        native_page_content_event(c,event->type,event->type==CONTENT_MSG_ERROR ?
            (event->data.errordata.errormsg ? event->data.errordata.errormsg :
                messages_get_errorcode(event->data.errordata.errorcode)) : NULL);
        if (event->type==CONTENT_MSG_REDIRECT) native_page_redirect(event->data.redirect.to);
    }''')
    edit('desktop/browser_window.c','Native loading document takes precedence over the old visible one',
         'if (!bw->parent && !bw->internal_nav && (c==bw->loading_content || c==bw->current_content)) {',
         '''/* Native loading document takes precedence over the old visible one. */
    if (!bw->parent && !bw->internal_nav &&
        (bw->loading_content ? c==bw->loading_content : c==bw->current_content)) {''')
    edit('desktop/browser_window.c','native_page_begin(params->url);',
         'NSLOG(netsurf, INFO, "Loading \'%s\'", nsurl_access(params->url));',
         'if (!bw->parent && !bw->internal_nav) native_page_begin(params->url);\n\tNSLOG(netsurf, INFO, "Loading \'%s\'", nsurl_access(params->url));')
    edit('content/handlers/html/html.c','Native Stop cancels deferred reflow',
         'static void html_stop(struct content *c)\n{\n\thtml_content *htmlc = (html_content *) c;',
         '''static void html_stop(struct content *c)
{
    html_content *htmlc = (html_content *) c;
    /* Native Stop cancels deferred reflow before abort callbacks can run. */
    guit->misc->schedule(-1,html_native_rebuild,htmlc);
    htmlc->native_dom_scheduled=false;htmlc->native_dom_pending=false;
    htmlc->native_dom_stopped=true;''')
