# SPDX-License-Identifier: BSD-2-Clause
"""Opt-in parser-internal budgets, applied only to staged innerHTML fragments."""
from pathlib import Path
import shutil

HERE=Path(__file__).resolve().parent


def patch(source):
    base=source.parent/'libdom/bindings/hubbub'
    p=base/'parser.c';s=p.read_text()
    if 'uint64_t (*native_now)' not in s:
        s=s.replace('hubbub_parser *parser;\t\t/**< Hubbub parser instance */', '''hubbub_parser *parser;\t\t/**< Hubbub parser instance */
    uint64_t (*native_now)(void);
    uint64_t native_deadline;
    unsigned native_nodes,native_steps,native_reason,native_max_nodes,native_max_depth;''',1)
        s=s.replace('/* Forward declaration to break reference loop */','#include "native_fragment_limits.inc"\n\n/* Forward declaration to break reference loop */',1)
        assert s.count('binding->parser = NULL;')==2
        s=s.replace('binding->parser = NULL;','binding->parser = NULL;\n    binding->native_now=NULL;')
        # Check before creating nodes, including clones produced by HTML error
        # recovery, rather than waiting for a potentially huge DOM to exist.
        for name in ('create_comment','create_doctype','create_element','create_text'):
            a=s.index('static hubbub_error '+name+'(');b=s.index('\nstatic ',a+1)
            part=s[a:b];assert '*result = NULL;' in part
            part=part.replace('*result = NULL;','*result = NULL;\n    if(!native_fragment_step(dom_parser,true,NULL))return HUBBUB_NOMEM;',1)
            s=s[:a]+part+s[b:]
        for name in ('append_child','insert_before','remove_child','clone_node','reparent_children'):
            a=s.index('static hubbub_error '+name+'(');b=s.index('\nstatic ',a+1)
            part=s[a:b];anchor='\tdom_exception err;';assert anchor in part
            parent='parent' if name in ('append_child','insert_before','remove_child') else ('new_parent' if name=='reparent_children' else 'NULL')
            create='true' if name=='clone_node' else 'false'
            check=f'\n    if(!native_fragment_step(dom_parser,{create},{parent}))return HUBBUB_NOMEM;'
            if name!='reparent_children':check='\n    *result=NULL;'+check
            if name=='clone_node':check+='\n    if(dom_parser->native_now && deep){dom_parser->native_reason=4;return HUBBUB_NOMEM;}'
            part=part.replace(anchor,anchor+check,1)
            if name=='reparent_children':
                part=part.replace('while(true) {','while(true) {\n        if(!native_fragment_step(dom_parser,false,new_parent))return HUBBUB_NOMEM;',1)
            s=s[:a]+part+s[b:]
        p.write_text(s)
    shutil.copyfile(HERE/'fragment_limits.inc',base/'native_fragment_limits.inc')
    p=base/'parser.h';s=p.read_text()
    if 'dom_hubbub_parser_native_limits(' not in s:
        # Public headers use an include guard; keep declarations inside it.
        at=s.rindex('#endif')
        s=s[:at]+'''/* NexatrodeOS: opt-in limits for detached dynamic fragments only. */
void dom_hubbub_parser_native_limits(dom_hubbub_parser *,unsigned,unsigned,uint64_t,uint64_t (*)(void));
unsigned dom_hubbub_parser_native_result(dom_hubbub_parser *,unsigned *);

'''+s[at:];p.write_text(s)
