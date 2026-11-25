/* SPDX-License-Identifier: MIT
 * Native optional fragment-parser limits. Included in libdom's Hubbub adapter.
 * Ordinary document parsers leave native_now NULL and retain upstream behavior.
 */
static bool native_fragment_step(dom_hubbub_parser *p, bool create, void *parent)
{
    if(p->native_now==NULL)return true;
    if(p->native_reason)return false;
    if(++p->native_steps>p->native_max_nodes*32u)p->native_reason=4;
    else if(p->native_now()>=p->native_deadline)p->native_reason=3;
    else if(create && ++p->native_nodes>p->native_max_nodes)p->native_reason=1;
    if(parent && !p->native_reason){
        unsigned depth=0;
        for(dom_node_internal *n=parent;n;n=n->parent)
            if(++depth>p->native_max_depth){p->native_reason=2;break;}
    }
    return p->native_reason==0;
}

void dom_hubbub_parser_native_limits(dom_hubbub_parser *p,unsigned nodes,
        unsigned depth,uint64_t deadline,uint64_t (*now)(void))
{
    p->native_nodes=p->native_steps=p->native_reason=0;
    p->native_max_nodes=nodes;p->native_max_depth=depth;
    p->native_deadline=deadline;p->native_now=now;
}
unsigned dom_hubbub_parser_native_result(dom_hubbub_parser *p,unsigned *nodes)
{
    *nodes=p->native_nodes;
    return p->native_reason;
}
