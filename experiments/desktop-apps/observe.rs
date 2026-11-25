//! Explicit opt-in, disposable-data-only observation page. No control API.
use personal_os_slint_notes_spike::NotesWindow;
use sel4_microkit::memory_region_symbol;
use slint::Model;
pub fn publish(ui: &NotesWindow) {
    let p=memory_region_symbol!(desktop_test_observation: *mut u64).as_ptr();
    let uri=ui.get_message_invitation_uri(); let transcript=ui.get_message_transcript();
    let status=ui.get_wallet_status();let sync=ui.get_wallet_sync_detail();
    let photos=ui.get_message_bubble_photo_ready();
    let values=[0x4e584445534b3031,ui.get_page() as u64,ui.get_message_view() as u64,
        ui.get_message_chats().row_count() as u64,ui.get_message_service_ready() as u64,
        ui.get_message_body().len() as u64,(0..photos.row_count()).filter(|i|photos.row_data(*i)==Some(true)).count() as u64,
        ui.get_wallet_tab() as u64,ui.get_wallet_setup() as u64];
    unsafe {
        for(i,v)in values.iter().enumerate(){p.add(i).write_volatile(*v);}
        for(index,text)in [uri,transcript,status,sync].iter().enumerate(){
            let limit=if index<2 {3000}else{256};let offset=[128,3200,6400,6680][index];
            p.add(9+index).write_volatile(text.len().min(limit) as u64);
            for(i,b)in text.as_bytes().iter().take(limit).enumerate(){p.cast::<u8>().add(offset+i).write_volatile(*b);}
        }
    }
}
