//! Multipart backing parts cannot be opened through ordinary photo previews.
#[path="../rust/src/vault_photo_access.rs"] mod access;
#[test] fn multipart_parts_are_not_chat_photos(){
    assert!(!access::permits(12,34,u64::MAX-1));
    assert!(!access::permits(12,u64::MAX-1,u64::MAX-1));
    assert!(!access::permits(12,u64::MAX-1,u64::MAX));
}
