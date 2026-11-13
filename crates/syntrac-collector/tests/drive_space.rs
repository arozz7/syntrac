//! The real Windows free-space probe, against the drive the tests run on.

use syntrac_collector::drive_space::DriveFreeSpace;
use syntrac_core::storage_guard::FreeSpace;

#[test]
fn reports_a_plausible_amount_of_free_space_for_an_existing_folder() {
    let dir = tempfile::tempdir().unwrap();
    let free = DriveFreeSpace::new(dir.path().to_path_buf()).free_bytes().unwrap();
    assert!(free > 1_000_000, "{free} bytes free on the test drive");
}

#[test]
fn a_folder_that_does_not_exist_is_an_error_not_zero() {
    let missing = std::env::temp_dir().join("syntrac-no-such-folder-for-the-probe").join("nested");
    assert!(DriveFreeSpace::new(missing).free_bytes().is_err());
}
