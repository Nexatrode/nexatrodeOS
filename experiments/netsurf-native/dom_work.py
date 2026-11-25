# SPDX-License-Identifier: BSD-2-Clause
"""Reproducible instrumentation and bounds for native fragment DOM work."""


def patch(source):
    from fragment_limits import patch as parser_limits
    parser_limits(source)
    p=source/'content/handlers/javascript/duktape/Element.bnd'
    s=p.read_text()
    if '#include "native_dom_work.h"' not in s:
        s=s.replace('#include <utils/corestrings.h>', '#include <utils/corestrings.h>\n#include "native_dom_work.h"',1)
    start=s.index('setter Element::innerHTML()');end=s.index('\ngetter Element::attributes()',start)
    body=s[start:end]
    if 'Native fragment work phases' not in body:
        body=body.replace('\tduk_size_t size;', '''    /* Native fragment work phases, without logging page input. */
    uint64_t native_started=native_ms(),native_parsed=0,native_committed=0;
    duk_size_t size;''',1)
        body=body.replace('\t/* Parse is finished, transfer contents of fragment into node */', '''    native_parsed=native_ms();
    printf("NETSURF|DOM_WORK|parsed|bytes=%zu|ms=%llu\\n",(size_t)size,(unsigned long long)(native_parsed-native_started));
    /* Parse is finished, transfer contents of fragment into node */''',1)
        body=body.replace('out:\n\tif (parser != NULL)', '''    native_committed=native_ms();
    printf("NETSURF|DOM_WORK|committed|bytes=%zu|ms=%llu\\n",(size_t)size,(unsigned long long)(native_committed-native_parsed));
out:
    if (parser != NULL)''',1)
        body=body.replace('\treturn 0;\n%}', '''    uint64_t native_finished=native_ms();
    printf("NETSURF|DOM_WORK|finished|bytes=%zu|parse_ms=%llu|commit_ms=%llu|cleanup_ms=%llu|total_ms=%llu\\n",
        (size_t)size,(unsigned long long)(native_parsed?native_parsed-native_started:0),
        (unsigned long long)(native_committed?native_committed-native_parsed:0),
        (unsigned long long)(native_finished-(native_committed?native_committed:native_started)),
        (unsigned long long)(native_finished-native_started));
    return 0;
%}''',1)
    if 'Native bounded detached fragment' not in body:
        body=body.replace('duk_size_t size;', 'duk_size_t size;\n    unsigned native_limit=0,native_nodes=0;',1)
        body=body.replace('\texc = dom_node_get_owner_document', '''    /* Native bounded detached fragment; do not remove live children yet. */
    if(size>NATIVE_DOM_MAX_BYTES){native_limit=5;goto out;}
    exc = dom_node_get_owner_document''',1)
        old='\terror = dom_hubbub_parser_parse_chunk(parser, (const uint8_t*)s, size);\n\tif (error != DOM_HUBBUB_OK) {\n\t\tNSLOG(netsurf, ERROR, "Unable to parse HTML chunk");\n\t\tgoto out;\n\t}'
        assert old in body
        body=body.replace(old,'''    dom_hubbub_parser_native_limits(parser,NATIVE_DOM_MAX_NODES,NATIVE_DOM_MAX_DEPTH,
        native_dom_deadline(),native_ms);
    for(size_t at=0;at<size;){
        size_t amount=size-at;if(amount>512)amount=512;
        error=dom_hubbub_parser_parse_chunk(parser,(const uint8_t *)s+at,amount);
        if(error!=DOM_HUBBUB_OK)goto out;
        at+=amount;
    }''',1)
        body=body.replace('out:\n    if (parser != NULL)', '''out:
    if(parser){unsigned reason=dom_hubbub_parser_native_result(parser,&native_nodes);if(reason)native_limit=reason;}
    if (parser != NULL)''',1)
        body=body.replace('    native_parsed=native_ms();', '''    native_limit=native_fragment_check((dom_node *)fragment);
    if(native_limit)goto out;
    native_parsed=native_ms();''',1)
        body=body.replace('|total_ms=%llu\\n','|total_ms=%llu|limit=%u|nodes=%u\\n',1)
        body=body.replace('(unsigned long long)(native_finished-native_started));',
            '(unsigned long long)(native_finished-native_started),native_limit,native_nodes);',1)
        body=body.replace('    return 0;\n%}', '''    if(native_limit){
        native_page_layout_notice(true);
        return duk_error(ctx,DUK_ERR_RANGE_ERROR,"Native DOM update limit (%u)",native_limit);
    }
    return 0;
%}''',1)
    # Upgrade an already patched development tree too.
    if 'native_fragment_check((dom_node *)fragment)' not in body:
        body=body.replace('    native_parsed=native_ms();', '''    native_limit=native_fragment_check((dom_node *)fragment);
    if(native_limit)goto out;
    native_parsed=native_ms();''',1)
    if 'native_cleanup_started' not in body:
        body=body.replace('native_committed=0;', 'native_committed=0,native_cleanup_started=0;\n    bool native_commit_started=false;',1)
        body=body.replace('    /* Parse is finished, transfer contents', '    native_commit_started=true;\n    /* Parse is finished, transfer contents',1)
        body=body.replace('out:\n', '''out:
    if(!native_parsed)native_parsed=native_ms();
    if(native_commit_started && !native_committed)native_committed=native_ms();
    native_cleanup_started=native_ms();
''',1)
        body=body.replace('native_finished-(native_committed?native_committed:native_started)', 'native_finished-native_cleanup_started',1)
    result=s[:start]+body+s[end:]
    if p.read_text()!=result:p.write_text(result)

    # Detached parse nodes must not install styles/scripts/images, schedule
    # visible reflow or alter page state before the fragment is accepted.
    p=source/'content/handlers/html/dom_event.c';s=p.read_text()
    if 'Native detached insertion filter' not in s:
        a=s.index('dom_default_action_DOMNodeInserted_cb(');b=s.index('dom_default_action_DOMSubtreeModified_cb(',a)
        part=s[a:b];anchor='\texc = dom_node_get_node_type(node, &type);'
        assert anchor in part
        part=part.replace(anchor,'''    /* Native detached insertion filter: parser staging is not a live DOM insertion. */
    bool connected=false;
    if(dom_node_contains(htmlc->document,(dom_node *)node,&connected)!=DOM_NO_ERR || !connected){
        dom_node_unref(node);return;
    }
'''+anchor,1);s=s[:a]+part+s[b:]
    if 'Native connected mutation invalidation' not in s:
        old='''            dom_string_isequal(type, corestring_dom_DOMCharacterDataModified))
            html_native_dom_changed(htmlc);'''
        assert old in s
        s=s.replace(old,'''            dom_string_isequal(type, corestring_dom_DOMCharacterDataModified)) {
            /* Native connected mutation invalidation. */
            dom_event_target *target=NULL;bool connected=false;
            if(dom_event_get_target(evt,&target)==DOM_NO_ERR && target){
                if(dom_node_contains(htmlc->document,(dom_node *)target,&connected)==DOM_NO_ERR && connected)
                    html_native_dom_changed(htmlc);
                dom_node_unref(target);
            }
        }''',1)
    if p.read_text()!=s:p.write_text(s)
