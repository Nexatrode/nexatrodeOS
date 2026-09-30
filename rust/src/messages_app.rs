#![no_std]
#![no_main]

mod app_view;
mod input_protocol;
mod notes;
mod vault_app_test_common;
mod vault_protocol;

use core::ptr::NonNull;

use app_view::AppViewState;
use personal_os_rust_server::messages_net_tx::MessagesNetTxPage;
use personal_os_rust_server::vault_blob_transfer::BlobTransferPage;
use sel4_microkit::{memory_region_symbol, protection_domain};
use vault_app_test_common::NamespaceTest;

#[protection_domain]
fn init() -> NamespaceTest {
    let app_view = memory_region_symbol!(messages_view: *mut AppViewState);
    let blob_transfer = memory_region_symbol!(blob_transfer: *mut BlobTransferPage);
    let messages_net_tx = memory_region_symbol!(messages_net_tx: *mut MessagesNetTxPage);
    unsafe { app_view.as_ptr().write(AppViewState::new()) };
    unsafe { blob_transfer.as_ptr().write(BlobTransferPage::new()) };
    // Do not clear messages_net_tx: dhcp_client sets NET1 magic when the net merge
    // maps this page; wiping it would make SEND_SEAL_DEMO look like offline.
    NamespaceTest::with_net_tx(
        "MESSAGES",
        1,
        app_view,
        None,
        blob_transfer,
        Some(unsafe { NonNull::new_unchecked(messages_net_tx.as_ptr()) }),
    )
}
