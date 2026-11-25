/* SPDX-License-Identifier: GPL-2.0-only
 * Native deferred layout transactions. Included in html.c so the original
 * initial-load machinery remains private and is never called a second time.
 */
#include <dom/walk.h>
#include <malloc.h>
#include "desktop/textarea.h"
#include "html/interaction.h"
#include "native_reflow_guard.h"
#include "native_page.h"
extern void nscss_native_reset_node_data(dom_node *node);
extern int native_textarea_caret(struct textarea *ta);
extern nserror dom_to_box_sync(dom_node *, html_content *, box_construct_complete_cb, void **);
static void html_native_rebuild(void *pw);
void native_reflow_stop(struct content *content)
{
    html_content *c=(html_content *)content;
    guit->misc->schedule(-1,html_native_rebuild,c);
    c->native_dom_scheduled=false;c->native_dom_pending=false;c->native_dom_stopped=true;
}
static void native_schedule_rebuild(html_content *c)
{
    if (!c->native_dom_scheduled && c->bw != NULL &&
        guit->misc->schedule(16,html_native_rebuild,c)==NSERROR_OK)
        c->native_dom_scheduled=true;
}

void html_native_dom_changed(html_content *c)
{
    if (!c->enable_scripting || c->native_dom_building || c->select_ctx == NULL) return;
    c->native_dom_stopped=false;
    c->native_dom_pending = true;
    native_schedule_rebuild(c);
}

struct native_walk { unsigned nodes, depth; bool allowed, reset; };
static enum dom_walk_cmd native_walk(enum dom_walk_stage stage,
        dom_node_type type, dom_node *node, void *pw)
{
    struct native_walk *w=pw;
    if (stage == DOM_WALK_STAGE_LEAVE) { w->depth--; return DOM_WALK_CMD_CONTINUE; }
    if (++w->nodes > 20000 || ++w->depth > 512) {
        w->allowed=false; return DOM_WALK_CMD_ABORT;
    }
    if (type == DOM_ELEMENT_NODE) {
        dom_html_element_type tag;
        if (dom_html_element_get_tag_type(node,&tag)==DOM_NO_ERR &&
            (tag==DOM_HTML_ELEMENT_TYPE_IFRAME || tag==DOM_HTML_ELEMENT_TYPE_FRAMESET)) {
            w->allowed=false; return DOM_WALK_CMD_ABORT;
        }
    }
    if (w->reset) {
        void *old=NULL;
        nscss_native_reset_node_data(node);
        dom_node_set_user_data(node,corestring_dom___ns_key_box_node_data,NULL,NULL,&old);
    }
    return DOM_WALK_CMD_CONTINUE;
}

/* talloc visits all boxes, including invisible nodes, floats and list markers. */
static void native_box_cleanup(const void *ptr,int depth,int max_depth,int is_ref,void *pw)
{
    (void)depth;(void)max_depth;
    if (is_ref || strcmp(talloc_get_name(ptr),"struct box")!=0) return;
    struct box *b=(struct box *)ptr;
    if (pw != NULL) {
        void *old=NULL;
        if (b->node) dom_node_set_user_data(b->node,
            corestring_dom___ns_key_box_node_data,b,NULL,&old);
    } else if (b->gadget && !(b->flags & CLONE) && b->gadget->form==NULL) {
        form_free_control(b->gadget); b->gadget=NULL;
    }
}

static void native_release_layout(html_content *c)
{
    html_object_close_objects(c);
    html_object_free_objects(c);
    imagemap_destroy(c);
    if (c->bctx) talloc_report_depth_cb(c->bctx,0,-1,native_box_cleanup,NULL);
    while (c->forms) { struct form *f=c->forms; c->forms=f->prev; form_free(f); }
    if (c->bctx) talloc_free(c->bctx);
    if (c->select_ctx) css_select_ctx_destroy(c->select_ctx);
}

static bool native_forms(html_content *c)
{
    c->forms=html_forms_get_forms(c->encoding,(dom_html_document *)c->document);
    dom_html_collection *forms=NULL;uint32_t expected=0,actual=0;
    if(dom_html_document_get_forms((dom_html_document *)c->document,&forms)!=DOM_NO_ERR)return false;
    dom_exception ex=dom_html_collection_get_length(forms,&expected);dom_html_collection_unref(forms);
    if(ex!=DOM_NO_ERR)return false;
    for(struct form *f=c->forms;f;f=f->prev)actual++;
    if(actual!=expected)return false;
    for (struct form *f=c->forms; f; f=f->prev) {
        nsurl *action=NULL;
        const char *s=f->action && *f->action ? f->action : nsurl_access(content_get_url(&c->base));
        if (nsurl_join(c->base_url,s,&action)!=NSERROR_OK) return false;
        free(f->action); f->action=strdup(nsurl_access(action)); nsurl_unref(action);
        if (!f->document_charset) f->document_charset=strdup(c->encoding ? c->encoding : "UTF-8");
        if (!f->action || !f->document_charset) return false;
    }
    return true;
}

static void native_boxes_done(html_content *c,bool success)
{
    c->box_conversion_context=NULL;
    c->native_dom_build_ok=success && c->layout!=NULL;
}

static unsigned native_pending_images(html_content *c)
{
    unsigned count=0;
    for(struct content_html_object *o=c->object_list;o;o=o->next)
        if(o->box && o->content && o->permitted_types==CONTENT_IMAGE &&
            (!hlcache_handle_get_content(o->content) ||
             (content_get_status(o->content)!=CONTENT_STATUS_DONE &&
              content_get_status(o->content)!=CONTENT_STATUS_ERROR)))count++;
    return count;
}

/* Layout belongs to the transaction too: upstream html_reformat ignores the
 * layout_document failure result. Do not discard the last usable frame before
 * wrapping/normalisation and their allocations have also succeeded. */
static bool native_format(html_content *c)
{
    c->reflowing=true;
    c->unit_len_ctx.viewport_width=css_unit_device2css_px(
        INTTOFIX(c->base.available_width),c->unit_len_ctx.device_dpi);
    c->unit_len_ctx.viewport_height=css_unit_device2css_px(
        INTTOFIX(c->base.available_height),c->unit_len_ctx.device_dpi);
    c->unit_len_ctx.root_style=c->layout->style;
    native_reflow_layout_begin();
    bool ok=layout_document(c,c->base.available_width,c->base.available_height);
    c->reflowing=false;
    if(!ok)return false;
    struct box *b=c->layout;
    c->base.width=b->x+b->padding[LEFT]+b->width+b->padding[RIGHT]+b->border[RIGHT].width+b->margin[RIGHT];
    c->base.height=b->y+b->padding[TOP]+b->height+b->padding[BOTTOM]+b->border[BOTTOM].width+b->margin[BOTTOM];
    if(c->base.width<b->x+b->descendant_x1)c->base.width=b->x+b->descendant_x1;
    if(c->base.height<b->y+b->descendant_y1)c->base.height=b->y+b->descendant_y1;
    c->had_initial_layout=true;
    return true;
}

static void html_native_rebuild(void *pw)
{
    html_content *c=pw;
    c->native_dom_scheduled=false;
    if (!c->native_dom_pending || c->native_dom_stopped || c->aborted || c->bw==NULL) return;
    unsigned pending_images=native_pending_images(c);
    if (c->base.locked || c->box_conversion_context || c->reflowing || c->base.active!=pending_images ||
        (c->base.status!=CONTENT_STATUS_READY && c->base.status!=CONTENT_STATUS_DONE)) {
        native_schedule_rebuild(c); return;
    }
    for (unsigned i=0;i<c->stylesheet_count;i++) if(c->stylesheets[i].modified) {
        native_schedule_rebuild(c); return;
    }
    uint64_t started=0,finished=0;nsu_getmonotonic_ms(&started);
    dom_node *root=NULL;
    if (dom_document_get_document_element(c->document,(void **)&root)!=DOM_NO_ERR || !root) return;
    struct native_walk walk={.allowed=true};
    dom_exception walk_error=libdom_treewalk(DOM_WALK_ENABLE_ENTER|DOM_WALK_ENABLE_LEAVE,native_walk,root,&walk);
    if (walk_error!=DOM_NO_ERR || !walk.allowed || c->frameset || c->iframe) {
        c->native_dom_pending=false;dom_node_unref(root);
        native_page_layout_notice(true);
        printf("NETSURF|REFLOW|unsupported document bounds or frames\n"); return;
    }
    html_content old=*c; /* borrowed document/JS; owned render fields below */
    dom_node *focused=NULL; int caret_position=-1;
    if (c->focus_type==HTML_FOCUS_TEXTAREA && c->focus_owner.textarea->gadget) {
        struct form_control *g=c->focus_owner.textarea->gadget;
        focused=dom_node_ref(g->node);
        if (g->data.text.ta) caret_position=native_textarea_caret(g->data.text.ta);
    }
    c->native_dom_building=true;c->native_dom_pending=false;c->native_dom_build_ok=false;
    c->bctx=NULL;c->layout=NULL;c->forms=NULL;c->imagemaps=NULL;
    c->object_list=NULL;c->num_objects=0;c->select_ctx=NULL;
    c->base.active-=pending_images;
    c->unit_len_ctx.root_style=NULL;c->had_initial_layout=false;
    walk=(struct native_walk){.allowed=true,.reset=true};
    libdom_treewalk(DOM_WALK_ENABLE_ENTER|DOM_WALK_ENABLE_LEAVE,native_walk,root,&walk);
    native_reflow_alloc_begin();
    nserror err=html_css_new_selection_context(c,&c->select_ctx);
    if (err==NSERROR_OK) {
        if (!native_forms(c)) err=NSERROR_NOMEM;
        else err=dom_to_box_sync(root,c,native_boxes_done,&c->box_conversion_context);
    }
    if (err==NSERROR_OK && c->native_dom_build_ok) err=imagemap_extract(c);
    if (err==NSERROR_OK && c->native_dom_build_ok && !native_format(c))err=NSERROR_NOMEM;
    if(native_reflow_alloc_end())err=NSERROR_NOMEM;
    if(err==NSERROR_OK && !c->native_dom_build_ok)err=NSERROR_BOX_CONVERT;
    dom_node_unref(root);
    if (err!=NSERROR_OK || !c->native_dom_build_ok) {
        native_release_layout(c);
        c->bctx=old.bctx;c->layout=old.layout;c->forms=old.forms;c->imagemaps=old.imagemaps;
        c->object_list=old.object_list;c->num_objects=old.num_objects;c->select_ctx=old.select_ctx;
        c->unit_len_ctx.root_style=old.unit_len_ctx.root_style;
        c->unit_len_ctx.viewport_width=old.unit_len_ctx.viewport_width;
        c->unit_len_ctx.viewport_height=old.unit_len_ctx.viewport_height;
        c->base.active=old.base.active;c->background_colour=old.background_colour;
        c->base.width=old.base.width;c->base.height=old.base.height;
        c->had_initial_layout=old.had_initial_layout;c->box_conversion_context=NULL;
        c->native_dom_building=false;
        if (c->bctx) talloc_report_depth_cb(c->bctx,0,-1,native_box_cleanup,c);
        c->native_dom_pending=false;
        native_page_layout_notice(true);
        printf("NETSURF|REFLOW|rollback|error=%d\n",err);
        dom_node_unref(focused);return;
    }
    /* No event-loop turn occurs across build/swap: old image callbacks cannot
     * run against a partially constructed tree. JS and its DOM stay intact. */
    c->drag_type=HTML_DRAG_NONE;c->drag_owner.no_owner=true;
    c->selection_type=HTML_SELECTION_NONE;c->selection_owner.none=true;
    c->visible_select_menu=NULL;c->focus_type=HTML_FOCUS_SELF;c->focus_owner.self=true;
    union content_msg_data caret_remove={.caret={.type=CONTENT_CARET_REMOVE}};
    content_broadcast(&c->base,CONTENT_MSG_CARET,&caret_remove);
    selection_clear(c->sel,false);
    native_release_layout(&old);
    selection_reinit(c->sel);
    union content_msg_data reformatted={.background=false};
    content_broadcast(&c->base,CONTENT_MSG_REFORMAT,&reformatted);
    if (focused) {
        struct box *box=box_for_node(focused);
        if (box && box->gadget &&
            (box->gadget->type==GADGET_TEXTBOX || box->gadget->type==GADGET_PASSWORD || box->gadget->type==GADGET_TEXTAREA) && box->gadget->data.text.ta) {
            c->focus_type=HTML_FOCUS_TEXTAREA;c->focus_owner.textarea=box;
            textarea_set_caret(box->gadget->data.text.ta,caret_position);
        }
        dom_node_unref(focused);
    }
    union content_msg_data data={.redraw={.x=0,.y=0,.width=c->base.width,.height=c->base.height}};
    content_broadcast(&c->base,CONTENT_MSG_REDRAW,&data);
    c->native_dom_building=false;c->native_dom_revision++;
    native_page_layout_notice(false);
    nsu_getmonotonic_ms(&finished);
    printf("NETSURF|REFLOW|done|revision=%u|nodes=%u|ms=%llu|height=%d|layout_bytes=%zu|heap_live=%zu\n",
        c->native_dom_revision,walk.nodes,(unsigned long long)(finished-started),
        c->base.height,talloc_total_size(c->bctx),(size_t)mallinfo().uordblks);
}
