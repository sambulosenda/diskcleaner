//! The fixture's traps are real traps: the safety crate refuses each one.
//! This checks the fixture, not the safety crate (that has its own tests).

use std::os::unix::fs::MetadataExt;

use fixtures::paths;
use safety::{validate, Refusal};

#[test]
fn safety_refuses_every_trap() {
    let dir = fixtures::home();
    let home = fixtures::canonical_root(&dir).unwrap();
    let dev = std::fs::symlink_metadata(&home).unwrap().dev();
    let check = |rel: &str| validate(&home, dev, &home.join(rel), true);

    for rel in [paths::TRAP_SYMLINK_ETC, paths::TRAP_SYMLINK_DOCUMENTS] {
        assert!(matches!(check(rel), Err(Refusal::Symlink { .. })), "{rel}");
    }
    // A path that goes through the Documents symlink is refused too.
    let through = format!("{}/Letters/Landlord.txt", paths::TRAP_SYMLINK_DOCUMENTS);
    assert!(matches!(check(&through), Err(Refusal::Symlink { .. })));

    for rel in [
        paths::TRAP_KEYSTORE,
        paths::TRAP_GIT_DIR,
        "projects/a",
        paths::PHOTOS_LIBRARY,
    ] {
        assert!(matches!(check(rel), Err(Refusal::HardBlocked(_))), "{rel}");
    }

    // The build-output child of a repository is allowed through the gate.
    assert!(check(paths::TRAP_GIT_NODE_MODULES).is_ok());
}
