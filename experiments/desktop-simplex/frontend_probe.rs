//! Read-only observation of the disposable full UI. No command/injection API.
use personal_os_slint_notes_spike::NotesWindow;
use sel4_microkit::memory_region_symbol;
use slint::Model;
pub fn publish(ui: &NotesWindow) {
    let state=memory_region_symbol!(session_state: *mut u64);
    let result=memory_region_symbol!(session_result: *mut u64);
    let epoch=unsafe{state.as_ptr().read_volatile()};
    assert_eq!(unsafe{state.as_ptr().add(1).read_volatile()},1);
    let uri=ui.get_message_invitation_uri();let transcript=ui.get_message_transcript();
    let n=uri.len().min(2048);let t=transcript.len().min(1920);
    let photos=ui.get_message_bubble_photo_ready();
    let values=[0x4e5846524f4e5431,epoch,ui.get_page() as u64,u64::from(ui.get_message_view()),
        ui.get_edit_target() as u64,ui.get_message_body().len() as u64,n as u64,t as u64,
        ui.get_message_chats().row_count() as u64,u64::from(ui.get_message_service_ready()),
        (0..photos.row_count()).filter(|i|photos.row_data(*i)==Some(true)).count() as u64];
    unsafe{
        let p=result.as_ptr();
        for (i,v) in values.iter().enumerate(){p.add(i).write_volatile(*v);}
        let bytes=p as *mut u8;
        for(i,b)in uri.as_bytes()[..n].iter().enumerate(){bytes.add(128+i).write_volatile(*b);}
        for(i,b)in transcript.as_bytes()[..t].iter().enumerate(){bytes.add(2176+i).write_volatile(*b);}
    }
}
