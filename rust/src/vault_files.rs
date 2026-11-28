// Included only in the opt-in large-Files Vault binary.
use personal_os_rust_server::files_catalogue::{self,Entry};
static mut FILES_UPLOAD:Option<Entry>=None;
static mut FILES_CACHE:Option<(u64,Entry)>=None;
impl Vault {
 fn files_metadata(&mut self,id:u64)->Result<Entry,MessageLabel>{
  if let Some((cached,e))=unsafe{&*core::ptr::addr_of!(FILES_CACHE)} {if *cached==id{return Ok(e.clone());}}
  let page=files_transfer();set_mr(0,id);self.history_request_on_page(MessageInfo::new(vault_protocol::REQUEST_HISTORY_READ,1),page)?;
  if get_mr(0)!=vault_protocol::LIBRARY_OWNER{return Err(REPLY_DENIED);}
  let bytes=unsafe{&*page}.payload().ok_or(REPLY_CORRUPT)?;let e=Entry::decode(bytes).ok_or(REPLY_CORRUPT)?;
  unsafe{*core::ptr::addr_of_mut!(FILES_CACHE)=Some((id,e.clone()));}Ok(e)
 }
 fn files_large(&mut self,request:MessageInfo)->Result<MessageInfo,MessageLabel>{
  let page=files_transfer();
  match (request.label(),request.count()) {
   (0x45a,1)=>{
    let mut cursor=get_mr(0);let end=self.history_end(self.block.capacity_blocks()?)?;
    while cursor<end {
     cursor+=1;set_mr(0,cursor);match self.history_request_on_page(MessageInfo::new(vault_protocol::REQUEST_HISTORY_READ,1),page){Ok(_)=>{},Err(REPLY_NO_BLOB)=>continue,Err(e)=>return Err(e)}
     if get_mr(0)!=vault_protocol::LIBRARY_OWNER{continue;}
     let data=unsafe{&*page}.payload().ok_or(REPLY_CORRUPT)?;
     if !data.starts_with(b"NXFILE3\0"){continue;}
     let e=Entry::decode(data).ok_or(REPLY_CORRUPT)?;
     unsafe{&mut *page}.set_payload(&e.name[..e.name_len]);set_mr(0,cursor);set_mr(1,cursor);set_mr(2,e.length as u64);return Ok(MessageInfo::new(REPLY_OK,3));
    }Err(REPLY_NO_BLOB)
   },
   (0x45b,3)=>{
    let id=get_mr(0);let offset=usize::try_from(get_mr(1)).map_err(|_|REPLY_BAD_REQUEST)?;let n=get_mr(2) as usize;
    let e=self.files_metadata(id)?;if n==0||n>vault_protocol::PHOTO_CHUNK||offset>=e.length||n>e.length-offset{return Err(REPLY_BAD_REQUEST);}
    let index=offset/files_catalogue::PART_BYTES;let within=offset%files_catalogue::PART_BYTES;let part=within/vault_protocol::PHOTO_CHUNK;let skip=within%vault_protocol::PHOTO_CHUNK;
    set_mr(0,e.parts[index]);set_mr(1,part as u64);
    self.photo_request_on_page_checked(MessageInfo::new(vault_protocol::REQUEST_PHOTO_READ,2),page,Some(files_catalogue::PART_OWNER),e.part_length(index))?;
    let bytes=unsafe{&*page}.payload().ok_or(REPLY_CORRUPT)?;if skip>=bytes.len(){return Err(REPLY_CORRUPT);}let take=n.min(bytes.len()-skip);let mut chunk=[0u8;vault_protocol::PHOTO_CHUNK];chunk[..take].copy_from_slice(&bytes[skip..skip+take]);unsafe{&mut *page}.set_payload(&chunk[..take]);Ok(MessageInfo::new(REPLY_OK,0))
   },
   #[cfg(feature="utm-files-picker-fixture")]
   (0x45c,1)=>{
    let length=usize::try_from(get_mr(0)).map_err(|_|REPLY_BAD_REQUEST)?;let name=unsafe{&*page}.payload().ok_or(REPLY_BAD_REQUEST)?;
    let e=Entry::begin(length,name).ok_or(REPLY_BAD_REQUEST)?;unsafe{*core::ptr::addr_of_mut!(FILES_UPLOAD)=Some(e);}Ok(MessageInfo::new(REPLY_OK,0))
   },
   #[cfg(feature="utm-files-picker-fixture")]
   (0x45d,1)=>{
    let n=get_mr(0) as usize;let e=unsafe{&mut *core::ptr::addr_of_mut!(FILES_UPLOAD)}.as_mut().ok_or(REPLY_BAD_REQUEST)?;
    if e.part_length(e.count)!=Some(n){return Err(REPLY_BAD_REQUEST);}
    // Snapshot metadata before calling another &mut self operation.
    set_mr(0,files_catalogue::PART_OWNER);set_mr(1,n as u64);self.photo_request_on_page(MessageInfo::new(vault_protocol::REQUEST_ATTACHMENT_COMMIT,2),page,None)?;let id=get_mr(0);
    let e=unsafe{&mut *core::ptr::addr_of_mut!(FILES_UPLOAD)}.as_mut().ok_or(REPLY_BAD_REQUEST)?;if !e.append(id,n){return Err(REPLY_BAD_REQUEST);}Ok(MessageInfo::new(REPLY_OK,0))
   },
   #[cfg(feature="utm-files-picker-fixture")]
   (0x45e,0)=>{
    let e=unsafe{&*core::ptr::addr_of!(FILES_UPLOAD)}.as_ref().ok_or(REPLY_BAD_REQUEST)?;let mut data=[0u8;files_catalogue::HEADER+120];let n=e.encode(&mut data).ok_or(REPLY_BAD_REQUEST)?;
    unsafe{&mut *page}.set_payload(&data[..n]);set_mr(0,vault_protocol::LIBRARY_OWNER);let r=self.history_request_on_page(MessageInfo::new(vault_protocol::REQUEST_HISTORY_APPEND,1),page)?;
    if let Some(e)=unsafe{&mut *core::ptr::addr_of_mut!(FILES_UPLOAD)}{e.clear();}unsafe{*core::ptr::addr_of_mut!(FILES_UPLOAD)=None;}Ok(r)
   },
   _=>Err(REPLY_DENIED)
  }
 }
}
