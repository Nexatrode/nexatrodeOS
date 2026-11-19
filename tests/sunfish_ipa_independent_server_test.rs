// Run this same scenario against the serialized baseline and the independent
// candidate. The real owner, protocol core, session and checked FIFO are used.
include!("sunfish_linux_rmtfs_server_test.rs");
include!("sunfish_ipa_independent_server_cases.rs");
