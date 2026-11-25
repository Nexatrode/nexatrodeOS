/* SPDX-License-Identifier: GPL-2.0-only
 * Synchronize a JS value setter with NetSurf's existing form widget and damage
 * path. This does not claim general dynamic DOM or CSS layout support.
 */
#include "native_javascript.h"
#include "native.h"
#include <stdio.h>
#include "content/content.h"
#include "content/handlers/html/private.h"
#include "content/handlers/html/box.h"
#include "content/handlers/html/box_construct.h"
#include "content/handlers/html/form_internal.h"
void native_form_value_changed(dom_node *node) {
    struct box *box=box_for_node(node);
    if (!box || !box->gadget || !box->gadget->html) return;
    form_gadget_sync_with_dom(box->gadget);
    html__redraw_a_box(box->gadget->html,box);
}
