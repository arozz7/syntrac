//! The pipe's access list, built as an SDDL string. Pure, so it is unit-tested; the SIDs come from configuration,
//! and an unchecked SID could inject extra permission entries (for example "Everyone"), so every one is validated.

#[derive(Debug, thiserror::Error)]
#[error("`{0}` is not a valid SID (expected a string like S-1-5-21-...)")]
pub struct SidError(pub String);

const MAX_SID_LENGTH: usize = 200;

/// Accepts `S-1-<authority>-<sub-authority>...` made only of digits and hyphens.
pub fn validate_sid(sid: &str) -> Result<(), SidError> {
    let invalid = || SidError(sid.chars().take(60).collect());
    if sid.len() > MAX_SID_LENGTH {
        return Err(invalid());
    }
    let mut parts = sid.split('-');
    if parts.next() != Some("S") || parts.next() != Some("1") {
        return Err(invalid());
    }
    let numbers: Vec<&str> = parts.collect();
    let all_numeric = numbers.iter().all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    if numbers.len() < 2 || !all_numeric {
        return Err(invalid());
    }
    Ok(())
}

/// SYSTEM and Administrators get full control; each named user gets read/write. Nobody else, and the list is
/// protected (`P`) so it inherits nothing. Duplicates are collapsed.
pub fn build_pipe_sddl(user_sids: &[&str]) -> Result<String, SidError> {
    let mut sddl = String::from("D:P(A;;GA;;;SY)(A;;GA;;;BA)");
    let mut seen: Vec<&str> = Vec::new();
    for sid in user_sids {
        validate_sid(sid)?;
        if !seen.contains(sid) {
            seen.push(sid);
            sddl.push_str(&format!("(A;;GRGW;;;{sid})"));
        }
    }
    Ok(sddl)
}
