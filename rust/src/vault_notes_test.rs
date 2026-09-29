#![no_std]
#![no_main]

mod app_view;
mod input_protocol;
mod notes;
mod vault_app_test_common;
mod vault_protocol;

use app_view::AppViewState;
use core::sync::atomic::{AtomicU32, Ordering};
use personal_os_rust_server::vault_blob_transfer::BlobTransferPage;
use sel4_microkit::{memory_region_symbol, protection_domain};
use vault_app_test_common::NamespaceTest;

#[protection_domain]
fn init() -> NamespaceTest {
    let app_view = memory_region_symbol!(app_view: *mut AppViewState);
    let notes_touch_mode = memory_region_symbol!(notes_touch_mode: *mut AtomicU32);
    let blob_transfer = memory_region_symbol!(blob_transfer: *mut BlobTransferPage);
    unsafe { app_view.as_ptr().write(AppViewState::new()) };
    unsafe { notes_touch_mode.as_ref().store(0, Ordering::Release) };
    unsafe { blob_transfer.as_ptr().write(BlobTransferPage::new()) };
    NamespaceTest::new("NOTES", 2, app_view, Some(notes_touch_mode), blob_transfer)
}
