//! Fresh, unfunded test identity. Never reads an installed wallet or keychain.
use std::{fs::OpenOptions, io::{Read, Write}, os::unix::fs::OpenOptionsExt, path::PathBuf};
use walletcore_core::{Network, WalletIdentity};
use zeroize::Zeroizing;
fn main() {
    let destination=PathBuf::from(std::env::args_os().nth(1).expect("test output path"));
    let root=std::env::current_dir().unwrap().join("build/desktop-apps").canonicalize().unwrap();
    assert!(destination.parent().unwrap().canonicalize().unwrap().starts_with(root));
    let mut entropy=Zeroizing::new([0u8;64]);
    std::fs::File::open("/dev/urandom").unwrap().read_exact(&mut entropy[..]).unwrap();
    let (_,words)=WalletIdentity::generate_from_entropy(entropy,0,Network::Mainnet).unwrap();
    let mut file=OpenOptions::new().write(true).create_new(true).mode(0o600).open(destination).unwrap();
    file.write_all(words.as_bytes()).unwrap();file.sync_all().unwrap();
    println!("Created a fresh disposable wallet fixture; no installed wallet was accessed");
}
