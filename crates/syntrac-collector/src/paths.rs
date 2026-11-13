//! Converts NT device paths (what ETW reports) to drive-letter paths (what people recognise).

/// Maps `\Device\HarddiskVolumeN` prefixes to drive letters using an injected table, so the logic
/// is testable without touching the operating system.
#[derive(Debug, Clone, Default)]
pub struct NtPathConverter {
    /// `(device prefix, drive)` pairs, longest device name first so `Volume10` is tried before `Volume1`.
    devices: Vec<(String, String)>,
}

impl NtPathConverter {
    /// `pairs` are `(r"\Device\HarddiskVolume7", "C:")`.
    pub fn new(mut pairs: Vec<(String, String)>) -> Self {
        pairs.sort_by_key(|(device, _)| std::cmp::Reverse(device.len()));
        Self { devices: pairs }
    }

    /// Unknown prefixes (network shares, bare names like `jq.exe`) pass through unchanged.
    pub fn convert(&self, path: &str) -> String {
        if let Some(rest) = path.strip_prefix(r"\??\") {
            return rest.to_owned();
        }
        for (device, drive) in &self.devices {
            let Some(prefix) = path.get(..device.len()) else { continue };
            if !prefix.eq_ignore_ascii_case(device) {
                continue;
            }
            let remainder = &path[device.len()..];
            // Match whole path components only: Volume7 must not match Volume70.
            if remainder.is_empty() {
                return format!(r"{drive}\");
            }
            if remainder.starts_with('\\') {
                return format!("{drive}{remainder}");
            }
        }
        path.to_owned()
    }
}
