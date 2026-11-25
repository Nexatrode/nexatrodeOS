# SPDX-License-Identifier: BSD-2-Clause
"""Reproducible native DOM/layout integration for the pinned NetSurf tree."""
from pathlib import Path
import shutil

HERE = Path(__file__).resolve().parent


def patch(source):
    def edit(name, marker, old, new):
        p = source / name
        text = p.read_text()
        if marker not in text:
            assert old in text, name
            p.write_text(text.replace(old, new, 1))

    # Updating an existing class attribute did not refresh libDOM's token
    # cache. Fix its value setter, covering className/setAttribute/Attr.value.
    dom=source.parent/'libdom/src/core'
    p=dom/'element.c'
    if '_dom_element_native_class_change(' not in p.read_text():
        with p.open('a') as f:
            f.write('''
dom_exception _dom_element_native_class_change(struct dom_element *element,dom_string *value) {
    return _dom_element_create_classes(element,dom_string_data(value));
}
''')
    p=dom/'element.h'
    if '_dom_element_native_class_change(' not in p.read_text():
        with p.open('a') as f:
            f.write('\ndom_exception _dom_element_native_class_change(struct dom_element *,dom_string *);\n')
    p=dom/'attr.c';text=p.read_text()
    if 'Native class-token cache update' not in text:
        old='attr->specified = true;\n\n\treturn DOM_NO_ERR;'
        assert old in text
        text=text.replace(old,'''attr->specified = true;
    /* Native class-token cache update after replacing the attribute value. */
    if (a->parent && a->namespace==NULL && dom_string_isequal(a->name,a->owner->class_string))
        return _dom_element_native_class_change((struct dom_element *)a->parent,value);

    return DOM_NO_ERR;''',1)
        p.write_text(text)

    html = 'content/handlers/html/'
    edit('content/handlers/javascript/duktape/netsurf.bnd', 'setter CharacterData::data()',
         'init CharacterData(struct dom_node_character_data *character_data::node);', '''init CharacterData(struct dom_node_character_data *character_data::node);
getter CharacterData::data()
%{
    dom_string *data=NULL;
    if (dom_characterdata_get_data((dom_characterdata *)priv->parent.node,&data)!=DOM_NO_ERR) return 0;
    if (data) { duk_push_lstring(ctx,dom_string_data(data),dom_string_byte_length(data));dom_string_unref(data); }
    else duk_push_string(ctx,"");
    return 1;
%}
setter CharacterData::data()
%{
    dom_string *data=NULL;duk_size_t length;
    const char *s=duk_safe_to_lstring(ctx,0,&length);
    if (dom_string_create((const uint8_t *)s,length,&data)!=DOM_NO_ERR) return 0;
    dom_characterdata_set_data((dom_characterdata *)priv->parent.node,data);
    dom_string_unref(data);return 0;
%}''')
    edit('utils/corestringlist.h', 'CORESTRING_DOM_STRING(DOMCharacterDataModified);',
         'CORESTRING_DOM_STRING(DOMSubtreeModified);',
         'CORESTRING_DOM_STRING(DOMSubtreeModified);\nCORESTRING_DOM_STRING(DOMCharacterDataModified);')
    edit(html+'private.h', 'native_dom_pending', 'bool reflowing;', '''bool reflowing;
    /* Native deferred DOM-to-layout transaction. */
    bool native_dom_pending, native_dom_building, native_dom_scheduled;
    unsigned native_dom_revision;
    bool native_dom_build_ok;''')
    edit(html+'private.h', 'native_dom_scheduled;', 'bool native_dom_pending, native_dom_building;',
         'bool native_dom_pending, native_dom_building, native_dom_scheduled;')
    edit(html+'private.h', 'bool native_dom_stopped;', 'bool native_dom_build_ok;',
         'bool native_dom_build_ok;\n    bool native_dom_stopped;')
    edit(html+'private.h', 'html_native_dom_changed(', 'void html_finish_conversion(html_content *htmlc);',
         'void html_finish_conversion(html_content *htmlc);\nvoid html_native_dom_changed(html_content *c);')
    edit(html+'html.c', '#include "native_reflow.inc"', '/* exported function documented in html/html_internal.h */\nvoid html_finish_conversion',
         '#include "native_reflow.inc"\n\n/* exported function documented in html/html_internal.h */\nvoid html_finish_conversion')
    edit(html+'html.c', 'html_native_dom_changed(htmlc);', 'if (htmlc->select_ctx != NULL) {',
         'if (htmlc->select_ctx != NULL) {\n        html_native_dom_changed(htmlc);')
    edit(html+'html.c', 'c->native_dom_pending = false;', 'c->reflowing = false;',
         'c->reflowing = false;\n    c->native_dom_pending = false; c->native_dom_building = false;\n    c->native_dom_revision = 0; c->native_dom_build_ok = false;')
    edit(html+'html.c', 'c->native_dom_scheduled = false;', 'c->native_dom_revision = 0;',
         'c->native_dom_scheduled = false;\n    c->native_dom_revision = 0;')
    edit(html+'html.c', 'c->native_dom_stopped = false;', 'c->native_dom_revision = 0;',
         'c->native_dom_stopped = false;\n    c->native_dom_revision = 0;')
    edit(html+'html.c', 'if (!htmlc->native_dom_stopped) html_native_dom_changed(htmlc);',
         'html_native_dom_changed(htmlc);', 'if (!htmlc->native_dom_stopped) html_native_dom_changed(htmlc);')
    edit(html+'html.c', 'schedule(-1, html_native_rebuild, html);', '/* If we\'re still converting a layout, cancel it */',
         'guit->misc->schedule(-1, html_native_rebuild, html);\n\n\t/* If we\'re still converting a layout, cancel it */')
    edit(html+'html.c', 'native_box_cleanup,NULL);', '/* Destroy forms */',
         'if (html->bctx) talloc_report_depth_cb(html->bctx,0,-1,native_box_cleanup,NULL);\n\n\t/* Destroy forms */')
    edit(html+'html.c', 'html_native_dom_changed(html);', 'html_object_open_objects(html, bw);',
         'html_object_open_objects(html, bw);\n    if (html->native_dom_pending) html_native_dom_changed(html);')
    edit(html+'html.c', 'schedule(-1, html_native_rebuild, htmlc);', 'selection_clear(htmlc->sel, false);',
         'guit->misc->schedule(-1, html_native_rebuild, htmlc);\n    selection_clear(htmlc->sel, false);')
    edit(html+'html.c', 'htmlc->native_dom_scheduled = false;', 'guit->misc->schedule(-1, html_native_rebuild, htmlc);',
         'guit->misc->schedule(-1, html_native_rebuild, htmlc);\n    htmlc->native_dom_scheduled = false;')
    edit(html+'dom_event.c', 'Native mutation invalidation', 'if (htmlc->jsthread != NULL)\n\t\tjs_event_cleanup', '''/* Native mutation invalidation runs after the DOM operation, never
     * rebuilding from inside a JavaScript or DOM event stack. */
    dom_string *type = NULL;
    if (dom_event_get_type(evt, &type) == DOM_NO_ERR && type != NULL) {
        if (dom_string_isequal(type, corestring_dom_DOMSubtreeModified) ||
            dom_string_isequal(type, corestring_dom_DOMAttrModified) ||
            dom_string_isequal(type, corestring_dom_DOMCharacterDataModified))
            html_native_dom_changed(htmlc);
        dom_string_unref(type);
    }
    if (htmlc->jsthread != NULL)
        js_event_cleanup''')
    edit(html+'box_construct.c', 'bool native_synchronous;', 'int *bctx;\t\t\t/**< talloc context */',
         'int *bctx;\t\t\t/**< talloc context */\n    bool native_synchronous;')
    edit(html+'box_construct.c', 'ctx->native_synchronous ? UINT32_MAX', 'const uint32_t max_processed_before_yield = 10;',
         'const uint32_t max_processed_before_yield = ctx->native_synchronous ? UINT32_MAX : 10;')
    edit(html+'box_construct.c', 'native_dom_to_box(', 'nserror\ndom_to_box(dom_node *n,', 'static nserror\nnative_dom_to_box(dom_node *n,')
    edit(html+'box_construct.c', 'void **box_conversion_context, bool synchronous)', 'void **box_conversion_context)\n{\n\tstruct box_construct_ctx *ctx;',
         'void **box_conversion_context, bool synchronous)\n{\n\tstruct box_construct_ctx *ctx;')
    edit(html+'box_construct.c', 'ctx->native_synchronous = synchronous;', 'ctx->content = c;',
         'ctx->native_synchronous = synchronous;\n    ctx->content = c;')
    edit(html+'box_construct.c', 'if (synchronous) {', '*box_conversion_context = ctx;\n\n\treturn guit->misc->schedule',
         '*box_conversion_context = ctx;\n    if (synchronous) { convert_xml_to_box(ctx); return NSERROR_OK; }\n\n\treturn guit->misc->schedule')
    p = source/(html+'box_construct.c')
    if 'nserror dom_to_box_sync(' not in p.read_text():
        with p.open('a') as f:
            f.write('''
nserror dom_to_box(dom_node *n, html_content *c,
        box_construct_complete_cb cb, void **context) {
    return native_dom_to_box(n,c,cb,context,false);
}
nserror dom_to_box_sync(dom_node *n, html_content *c,
        box_construct_complete_cb cb, void **context) {
    return native_dom_to_box(n,c,cb,context,true);
}
''')
    # A detached node can outlive its old render box through a JS reference.
    edit(html+'box_manipulate.c', '#include "utils/corestrings.h"', '#include "utils/errors.h"',
         '#include "utils/errors.h"\n#include "utils/corestrings.h"')
    edit(html+'box_manipulate.c', 'Native conditional backlink retirement', 'if (b->node != NULL) {\n\t\tdom_node_unref(b->node);', '''if (b->node != NULL) {
        /* Native conditional backlink retirement also covers floats/markers. */
        void *current = NULL, *old = NULL;
        dom_node_get_user_data(b->node,corestring_dom___ns_key_box_node_data,&current);
        if (current == b) dom_node_set_user_data(b->node,
            corestring_dom___ns_key_box_node_data,NULL,NULL,&old);
        dom_node_unref(b->node);''')
    edit('content/handlers/css/select.c', 'nscss_native_reset_node_data(', '/* Handler for libcss_node_data, stored as libdom node user data */', '''/* Drop the previous cascade's bloom filters and shared partial styles. */
void nscss_native_reset_node_data(dom_node *node) {
    void *data = NULL;
    if (dom_node_set_user_data(node,corestring_dom___ns_key_libcss_node_data,
            NULL,NULL,&data) == DOM_NO_ERR && data != NULL)
        css_libcss_node_data_handler(&selection_handler,CSS_NODE_DELETED,
            NULL,node,NULL,data);
}

/* Handler for libcss_node_data, stored as libdom node user data */''')
    # Track linked sheets just as inline sheets are tracked, including removal.
    edit(html+'css.c', 'native link owner', 'htmlc->stylesheets[htmlc->stylesheet_count].node = NULL;',
         'htmlc->stylesheets[htmlc->stylesheet_count].node = dom_node_ref(node); /* native link owner */')
    edit(html+'css.c', 'Native detached stylesheet filter', 'if (hsheet->unused) {', '''/* Native detached stylesheet filter. */
        bool connected = true;
        if (hsheet->node != NULL)
            dom_node_contains(c->document,hsheet->node,&connected);
        if (!connected || hsheet->unused) {''')
    # Snapshot character index, not pixel position, across a form re-layout.
    p=source/'desktop/textarea.c'
    if 'native_textarea_caret(' not in p.read_text():
        with p.open('a') as f:
            f.write('''
int native_textarea_caret(struct textarea *ta) {
    if (ta->caret_pos.byte_off < 0) return -1;
    return utf8_bounded_length(ta->show->data,textarea_get_caret(ta));
}
''')
    shutil.copyfile(HERE/'dom_reflow.inc',source/(html+'native_reflow.inc'))
