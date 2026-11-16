//! Cleaning text that comes from outside.

use syntrac_core::clean_text;

#[test]
fn control_characters_and_direction_tricks_are_removed_and_the_ends_trimmed() {
    assert_eq!(clean_text("  Acme\u{0000} Corp\u{202E}gnp  \n", 100).as_deref(), Some("Acme Corpgnp"));
}

#[test]
fn nothing_printable_left_is_none() {
    assert_eq!(clean_text("\u{0007}\u{200E}  ", 100), None);
    assert_eq!(clean_text("", 100), None);
}

#[test]
fn the_length_is_capped_in_characters_not_bytes() {
    assert_eq!(clean_text("ééééé", 3).as_deref(), Some("ééé"));
}
