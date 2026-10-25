//! Explicit cache budgets. The imagery budgets apply when the desktop constructs its
//! cache; the border tile budget applies at once.
use serde::Serialize;
const BYTES_PER_MIB: u64 = 1024 * 1024;
pub const MIN_CACHE_MIB: u32 = 32;
pub const MAX_DECODED_CACHE_MIB: u32 = 2048;
pub const MAX_DISK_CACHE_MIB: u32 = 16384;
/// Bounds of the border tile cache (OpenStreetMap borders and names, compact format).
pub const MIN_BORDER_CACHE_MIB: u32 = 8;
pub const MAX_BORDER_CACHE_MIB: u32 = 1024;
/// The imagery disk cache by default. 128 MiB (the default before 2026-10-25) filled
/// within one session (137 MiB downloaded), so places already seen were downloaded
/// again.
pub const DEFAULT_DISK_CACHE_MIB: u32 = 1024;
/// The old default: a saved value of exactly this was never chosen by the user and
/// moves to the new default.
const OLD_DEFAULT_DISK_CACHE_MIB: u64 = 128;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct CachePreferences {
    pub decoded_mib: u32,
    pub disk_mib: u32,
    pub borders_mib: u32,
}
impl Default for CachePreferences {
    fn default() -> Self {
        Self {
            decoded_mib: 256,
            disk_mib: DEFAULT_DISK_CACHE_MIB,
            borders_mib: 64,
        }
    }
}
impl CachePreferences {
    pub fn decoded_bytes(self) -> usize {
        (u64::from(self.decoded_mib) * BYTES_PER_MIB) as usize
    }
    pub fn disk_bytes(self) -> u64 {
        u64::from(self.disk_mib) * BYTES_PER_MIB
    }
    pub fn border_bytes(self) -> u64 {
        u64::from(self.borders_mib) * BYTES_PER_MIB
    }
    pub(super) fn apply(&mut self, value: &serde_json::Value) {
        for (key, target, min, max) in [
            (
                "decoded_mib",
                &mut self.decoded_mib,
                MIN_CACHE_MIB,
                MAX_DECODED_CACHE_MIB,
            ),
            (
                "disk_mib",
                &mut self.disk_mib,
                MIN_CACHE_MIB,
                MAX_DISK_CACHE_MIB,
            ),
            (
                "borders_mib",
                &mut self.borders_mib,
                MIN_BORDER_CACHE_MIB,
                MAX_BORDER_CACHE_MIB,
            ),
        ] {
            if let Some(number) = value.get(key).and_then(|v| v.as_u64()) {
                *target = number.clamp(u64::from(min), u64::from(max)) as u32;
            }
        }
        if value.get("disk_mib").and_then(|v| v.as_u64()) == Some(OLD_DEFAULT_DISK_CACHE_MIB) {
            self.disk_mib = DEFAULT_DISK_CACHE_MIB;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_old_imagery_cache_default_moves_to_the_new_one_and_choices_stay() {
        let mut prefs = CachePreferences::default();
        prefs.apply(&serde_json::json!({"disk_mib": 128}));
        assert_eq!(prefs.disk_mib, DEFAULT_DISK_CACHE_MIB);
        prefs.apply(&serde_json::json!({"disk_mib": 300}));
        assert_eq!(prefs.disk_mib, 300);
    }
}
