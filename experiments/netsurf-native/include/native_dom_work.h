/* SPDX-License-Identifier: GPL-2.0-only */
#pragma once
#include <stdint.h>
#include <stdio.h>
#include <dom/dom.h>
#include <dom/walk.h>
#include "native_page.h"
uint64_t native_ms(void);
uint64_t native_dom_deadline(void);
#ifdef NATIVE_BROWSER_TESTS
void native_dom_test_expire(void);
#endif
#define NATIVE_DOM_MAX_BYTES (128u*1024u)
#define NATIVE_DOM_MAX_NODES 4096u
#define NATIVE_DOM_MAX_DEPTH 256u
#define NATIVE_DOM_PARSE_MS 100u

struct native_fragment_walk {unsigned nodes,depth,reason;};
static inline enum dom_walk_cmd native_fragment_visit(enum dom_walk_stage stage,
        dom_node_type type,dom_node *node,void *pw)
{
    (void)type;(void)node;
    struct native_fragment_walk *w=pw;
    if(stage==DOM_WALK_STAGE_LEAVE){w->depth--;return DOM_WALK_CMD_CONTINUE;}
    if(++w->nodes>NATIVE_DOM_MAX_NODES)w->reason=1;
    else if(++w->depth>NATIVE_DOM_MAX_DEPTH)w->reason=2;
    return w->reason?DOM_WALK_CMD_ABORT:DOM_WALK_CMD_CONTINUE;
}
static inline unsigned native_fragment_check(dom_node *fragment)
{
    struct native_fragment_walk w={0};
    if(libdom_treewalk(DOM_WALK_ENABLE_ENTER|DOM_WALK_ENABLE_LEAVE,
        native_fragment_visit,fragment,&w)!=DOM_NO_ERR)return 4;
    return w.reason;
}
