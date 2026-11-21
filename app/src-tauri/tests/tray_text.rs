//! The words of the tray icon (docs/ALERTS-PLAN.md, slice T1): its tooltip and the Findings menu item.

use syntrac_app_lib::tray_text::{findings_item_label, tooltip};

#[test]
fn the_tooltip_says_how_many_findings_are_open() {
    assert_eq!(tooltip(Some(0), true), "SYNTRAC: no open findings");
    assert_eq!(tooltip(Some(1), true), "SYNTRAC: 1 open finding");
    assert_eq!(tooltip(Some(3), true), "SYNTRAC: 3 open findings");
}

#[test]
fn when_the_findings_cannot_be_read_the_tooltip_is_just_the_name() {
    assert_eq!(tooltip(None, true), "SYNTRAC");
}

#[test]
fn a_collector_that_is_not_running_comes_first_and_the_count_still_follows() {
    assert_eq!(tooltip(Some(2), false), "SYNTRAC: the collector is not running; 2 open findings");
    assert_eq!(tooltip(Some(0), false), "SYNTRAC: the collector is not running");
    assert_eq!(tooltip(None, false), "SYNTRAC: the collector is not running");
}

#[test]
fn the_findings_item_carries_the_count_when_there_is_one() {
    assert_eq!(findings_item_label(Some(3)), "Findings (3 open)");
    assert_eq!(findings_item_label(Some(0)), "Findings");
    assert_eq!(findings_item_label(None), "Findings");
}
