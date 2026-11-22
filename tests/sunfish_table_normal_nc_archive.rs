//! Inspect the actual serialized initializer specification extracted from loader.img.
//! Host use only, compiled against the exact isolated initializer types/lockfile.
use sel4_capdl_initializer_types::{ArchivedCap, ArchivedObject, SpecForInitializer};

fn main() {
    let input = std::fs::read(std::env::args().nth(1).unwrap()).unwrap();
    let mut bytes = rkyv::util::AlignedVec::<16>::new();
    bytes.extend_from_slice(&input);
    let spec = SpecForInitializer::access(&bytes).expect("valid initializer archive");
    let expected_objects:usize=std::env::args().nth(2).expect("audited object count").parse().unwrap();
    assert_eq!(spec.objects.len(), expected_objects);
    let expected = [128u32, 129, 130, 131, 133, 134, 135, 138];
    let mut actual = Vec::new();
    for obj in spec.objects.iter() {
        if let ArchivedObject::PageTable(pt) = &obj.object {
            for entry in pt.slots.iter() {
                if let ArchivedCap::Frame(frame) = &entry.cap {
                    if frame.normal_nc {
                        assert!(!frame.cached && !frame.executable);
                        assert!(frame.rights.read && frame.rights.write);
                        assert!(!frame.rights.grant && !frame.rights.grant_reply);
                        actual.push(u32::from(frame.object.0));
                    }
                }
            }
        }
    }
    actual.sort();
    assert_eq!(actual, expected);
    println!("Actual packed initializer archive: {expected_objects} objects, exactly eight NC table mapping caps with original IDs and RW/NX/uncached flags.");
}
