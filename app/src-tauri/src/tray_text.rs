//! The words of the tray icon (docs/ALERTS-PLAN.md, slice T1). Pure, so they are tested.

fn count(open: u32) -> String {
    match open {
        0 => "no open findings".to_owned(),
        1 => "1 open finding".to_owned(),
        n => format!("{n} open findings"),
    }
}

/// The tooltip: how many findings are open, or that the collector is not running (the count, when known, still follows).
pub fn tooltip(open: Option<u32>, collector_running: bool) -> String {
    match (collector_running, open) {
        (true, None) => "SYNTRAC".to_owned(),
        (true, Some(open)) => format!("SYNTRAC: {}", count(open)),
        (false, Some(open)) if open > 0 => format!("SYNTRAC: the collector is not running; {}", count(open)),
        (false, _) => "SYNTRAC: the collector is not running".to_owned(),
    }
}

/// The menu item that opens the window at the findings.
pub fn findings_item_label(open: Option<u32>) -> String {
    match open {
        Some(open) if open > 0 => format!("Findings ({open} open)"),
        _ => "Findings".to_owned(),
    }
}
