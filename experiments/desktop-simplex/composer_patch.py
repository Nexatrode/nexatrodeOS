"""Opt-in aliases for restarting the trusted SimpleX service group.

Only its parent gets aliases of writable image/stack/IPC frames and notification
receive caps. Children start suspended, so the parent can capture their public
boot state before any secret is loaded. This is a bounded static service group,
not a general application loader or authority revocation mechanism.
"""

NAMES = ('session_agent', 'session_messages', 'session_transcript',
         'session_photo', 'session_reader', 'session_client')
BASE = 0x2000000000
STRIDE = 0x20000000
FIRST_CHILD = 32


def patch(builder, replace):
    replace(builder, '    address_space: AddressSpace,\n}',
            '    address_space: AddressSpace,\n    writable_frames: Vec<(u64, ObjectId, u64)>,\n}')
    replace(builder, '        let mut frame_sequence = 0; // For object naming purpose only.',
            '        let mut writable_frames = Vec::new();\n        let mut frame_sequence = 0; // For object naming purpose only.')
    replace(builder, '                let frame_cap = capdl_util_make_frame_cap(\n                    frame_obj_id,',
            '                if segment.is_writable() { writable_frames.push((cur_vaddr, frame_obj_id, page_size_bytes)); }\n                let frame_cap = capdl_util_make_frame_cap(\n                    frame_obj_id,')
    replace(builder, '            while cur_vaddr < seg_base_vaddr + seg_mem_size {', '''
            while cur_vaddr < seg_base_vaddr + seg_mem_size {
                let page_size = if cur_vaddr % 0x200000 == 0 &&
                    seg_base_vaddr + seg_mem_size - cur_vaddr >= 0x200000 {
                    PageSize::Large
                } else { PageSize::Small };
                let page_size_bytes = page_size as u64;''')
    replace(builder, '                    PageSize::Small.fixed_size_bits(sel4_config) as u8,',
            '                    page_size.fixed_size_bits(sel4_config) as u8,')
    replace(builder, '            address_space,\n        })', '            address_space,\n            writable_frames,\n        })')
    names = ','.join('"' + name + '"' for name in NAMES)
    replace(builder, '    let mut pd_stack_bottoms: Vec<u64> = Vec::new();', f'''
    let mut pd_stack_bottoms: Vec<u64> = Vec::new();
    let session_names = [{names}];
    let mut session_mirrors: Vec<(u64, ObjectId, u64)> = Vec::new();''')
    replace(builder, '        let pd_tcb_obj_id = pd_elf_spec.tcb;', '''
        let session_index = session_names.iter().position(|name| *name == pd.name.as_ref());
        if let Some(i) = session_index {
            if pd.parent.as_deref() != Some("runtime_manager") || pd.id != Some(32 + i as u64) || pd.passive {
                return Err("managed SimpleX services require their fixed parent/id and active scheduling".into());
            }
        }
        let mut session_frames = pd_elf_spec.writable_frames.clone();
        let pd_tcb_obj_id = pd_elf_spec.tcb;''')
    replace(builder, '        let ipcbuf_frame_cap =\n',
            '        session_frames.push((0x10000000, ipcbuf_frame_obj_id, 4096));\n        let ipcbuf_frame_cap =\n')
    replace(builder, '            let stack_frame_cap =\n',
            '            session_frames.push((0x11000000 + stack_frame_seq * 4096, stack_frame_obj_id, 4096));\n            let stack_frame_cap =\n')
    replace(builder, '            pd_tcb.extra.resume = true;', '            pd_tcb.extra.resume = session_index.is_none();')
    replace(builder, '        pd_shadow_cspaces.insert(', f'''
        if let Some(i) = session_index {{
            for (address, frame, size) in session_frames {{
                session_mirrors.push(({BASE}u64 + i as u64 * {STRIDE} + address, frame, size));
            }}
        }}
        pd_shadow_cspaces.insert(''')
    anchor = '    // Step 4. Postprocessing steps for PDs'
    replace(builder, anchor, anchor + '''
    // Mirrors are parent-only, non-executable, and never accessible to another app.
    if !session_mirrors.is_empty() {
        let parent = &pd_shadow_cspaces["runtime_manager"];
        let space = AddressSpace::VSpace { name: "session_parent".into(), root: parent.vspace, x86_ept: false };
        for (address, frame, size) in session_mirrors {
            space.map_page(&mut spec_container, kernel_config,
                capdl_util_make_frame_cap(frame, true, true, false, true), size, address)?;
        }
        for (i, name) in session_names.iter().enumerate() {
            let child = &pd_shadow_cspaces[*name];
            parent.insert_cap_into_root_cnode(&mut spec_container, 8 + i as u32,
                capdl_util_make_ntfn_cap(child.notification, RIGHTS_NTFN_RECEIVE, 0));
        }
    }
''')
