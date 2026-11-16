//! Text that comes from outside (a file's version resource, a registry value, a downloaded data file) is attacker-influenced.
//! Before it is stored or shown, control characters and direction tricks are removed, the ends are trimmed and the length is
//! capped.

/// Characters that change how neighbouring text is displayed (direction overrides and marks): removed so a name cannot disguise
/// itself.
fn is_display_trick(c: char) -> bool {
    matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// `text` without control characters and direction tricks, trimmed and cut to `max_chars` characters. `None` when nothing
/// printable is left.
pub fn clean_text(text: &str, max_chars: usize) -> Option<String> {
    let cleaned: String = text.chars().filter(|c| !c.is_control() && !is_display_trick(*c)).collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.chars().take(max_chars).collect())
    }
}
