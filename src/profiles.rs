//! Chrome profile discovery.
//!
//! Historically we read Chrome's `Local State` directly to map a profile's
//! display name (`Zephyr`) to its on-disk directory (`Profile 1`). On macOS 27
//! ("Golden Gate") Chrome's `~/Library/Application Support/Google/Chrome` is
//! stamped with a `com.apple.macl` xattr that allowlists only Chrome's own code
//! identity, so a third-party app gets `EPERM` reading `Local State` no matter
//! what privacy permissions the user grants.
//!
//! To keep working we resolve in three steps:
//!
//! 1. a live read of Chrome's `Local State` (preferred, and it refreshes the
//!    cache for later);
//! 2. a cache of the last known name -> directory map, stored in urwhere's own
//!    (unprotected) Application Support directory and populated by any context
//!    that *can* read Chrome (e.g. the installer run from a Terminal);
//! 3. if the profile is plainly a directory name (`Default`, `Profile 1`), use
//!    it verbatim so a config never needs Chrome to be readable at all.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// The Chromium-family browsers we know how to read profiles for.
pub const KNOWN_BROWSERS: &[&str] = &[
    "Google Chrome",
    "Google Chrome Beta",
    "Google Chrome Canary",
    "Chromium",
    "Brave Browser",
    "Microsoft Edge",
    "Vivaldi",
    "Arc",
];

/// A resolved profile index. `map` resolves any alias (display name, account
/// name, or directory name) to its directory; `list` is the display list.
struct Index {
    map: HashMap<String, String>,
    list: Vec<(String, String)>,
    /// File whose mtime gates a reload: Chrome's `Local State` for a live index,
    /// or our own cache file for a fallback index.
    path: PathBuf,
    mtime: Option<SystemTime>,
    /// True when read from Chrome directly, false when from our cache.
    live: bool,
}

static CACHE: LazyLock<Mutex<HashMap<String, Index>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Browsers we have already logged a "live read blocked" warning for, so a
/// blocked read doesn't spam the log on every click.
static WARNED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// The on-disk representation of our own cache.
#[derive(Debug, Default, Serialize, Deserialize)]
struct CacheFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    browsers: HashMap<String, CachedProfiles>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct CachedProfiles {
    #[serde(default)]
    map: HashMap<String, String>,
    #[serde(default)]
    list: Vec<(String, String)>,
}

/// The Chromium user data directory for a browser, if it is one we know about.
pub fn user_data_dir(browser: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let base = PathBuf::from(home).join("Library/Application Support");
    let relative = match browser.trim().to_ascii_lowercase().as_str() {
        "google chrome" => "Google/Chrome",
        "google chrome beta" => "Google/Chrome Beta",
        "google chrome canary" => "Google/Chrome Canary",
        "chromium" => "Chromium",
        "brave browser" | "brave" => "BraveSoftware/Brave-Browser",
        "microsoft edge" | "edge" => "Microsoft Edge",
        "vivaldi" => "Vivaldi",
        "arc" => "Arc/User Data",
        _ => return None,
    };
    Some(base.join(relative))
}

/// Resolve a profile name (or directory name) to its on-disk directory.
pub fn resolve(browser: &str, name: &str) -> Option<String> {
    let key = name.to_ascii_lowercase();
    let raw = is_directory_name(name).then(|| name.trim().to_string());
    with_index(browser, |index| index.map.get(&key).cloned())
        .flatten()
        .or(raw)
}

/// Every known profile for a browser as `(directory, display name)`.
pub fn all(browser: &str) -> Vec<(String, String)> {
    with_index(browser, |index| index.list.clone()).unwrap_or_default()
}

/// Read every known browser's profiles and refresh our cache. Returns
/// `(refreshed, blocked)`: browsers whose profiles were read successfully, and
/// browsers whose live read failed (typically macOS's App Support protection).
pub fn sync() -> (Vec<String>, Vec<String>) {
    let mut refreshed = Vec::new();
    let mut blocked = Vec::new();
    for browser in KNOWN_BROWSERS {
        match read_live(browser) {
            Ok(profiles) => {
                if !profiles.list.is_empty() {
                    update_cache_entry(browser, &profiles);
                    refreshed.push((*browser).to_string());
                }
            }
            Err(err) => {
                // Only report browsers that exist on this machine but whose
                // data we were denied: a missing browser is not an error.
                let exists = user_data_dir(browser)
                    .map(|dir| dir.join("Local State").exists())
                    .unwrap_or(false);
                if exists {
                    blocked.push(format!("{browser} ({err})"));
                }
            }
        }
    }
    // Drop the in-memory index so the next resolve re-reads.
    CACHE.lock().unwrap_or_else(|e| e.into_inner()).clear();
    (refreshed, blocked)
}

/// `Default`, `Profile 1`, `Person 2`, `Guest Profile` are directories Chrome
/// accepts directly, so they can be used without any lookup.
fn is_directory_name(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    lower == "default"
        || lower == "guest profile"
        || lower.starts_with("profile ")
        || lower.starts_with("person ")
}

fn with_index<T>(browser: &str, f: impl FnOnce(&Index) -> T) -> Option<T> {
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());

    let stale = cache.get(browser).map_or(true, |index| {
        if index.live {
            let current = fs::metadata(&index.path).and_then(|m| m.modified()).ok();
            current != index.mtime
        } else {
            // A fallback index is cheap to re-validate against Chrome: if a
            // live read starts working again we switch back to it.
            true
        }
    });
    if stale {
        if let Some(index) = load(browser) {
            cache.insert(browser.to_string(), index);
        } else {
            cache.remove(browser);
        }
    }

    cache.get(browser).map(f)
}

fn load(browser: &str) -> Option<Index> {
    // Prefer a live read (and refresh the cache), but never let a blocked read
    // hide a perfectly good cached mapping.
    match read_live(browser) {
        Ok(profiles) => {
            update_cache_entry(browser, &profiles);
            let path = user_data_dir(browser)?.join("Local State");
            let mtime = fs::metadata(&path).and_then(|m| m.modified()).ok();
            Some(Index {
                map: profiles.map,
                list: profiles.list,
                path,
                mtime,
                live: true,
            })
        }
        Err(err) => {
            warn_blocked(browser, &err);
            let path = cache_path()?;
            let mtime = fs::metadata(&path).and_then(|m| m.modified()).ok();
            let profiles = read_cache().browsers.get(browser)?.clone();
            Some(Index {
                map: profiles.map,
                list: profiles.list,
                path,
                mtime,
                live: false,
            })
        }
    }
}

/// Read Chrome's `Local State` and build the alias map + display list.
fn read_live(browser: &str) -> Result<CachedProfiles, String> {
    let dir = user_data_dir(browser).ok_or_else(|| format!("unknown browser {browser:?}"))?;
    let path = dir.join("Local State");
    let text = fs::read_to_string(&path)
        .map_err(|err| format!("cannot read {}: {err}", path.display()))?;
    let json: serde_json::Value = serde_json::from_str(&text)
        .map_err(|err| format!("invalid JSON in {}: {err}", path.display()))?;
    let info_cache = json
        .get("profile")
        .and_then(|profile| profile.get("info_cache"))
        .and_then(|value| value.as_object())
        .ok_or_else(|| format!("{} has no profile.info_cache", path.display()))?;

    let mut profiles = CachedProfiles::default();
    for (directory, info) in info_cache {
        profiles
            .map
            .insert(directory.to_ascii_lowercase(), directory.clone());
        for key in ["name", "gaia_name", "user_name"] {
            if let Some(value) = info.get(key).and_then(|v| v.as_str()) {
                profiles
                    .map
                    .entry(value.to_ascii_lowercase())
                    .or_insert_with(|| directory.clone());
            }
        }
        let name = info
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(directory)
            .to_string();
        profiles.list.push((directory.clone(), name));
    }
    profiles.list.sort();
    Ok(profiles)
}

fn cache_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/urwhere/profiles.json"))
}

fn read_cache() -> CacheFile {
    cache_path()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn update_cache_entry(browser: &str, profiles: &CachedProfiles) {
    let Some(path) = cache_path() else {
        return;
    };
    let mut cache = read_cache();
    cache.version = 1;
    cache.browsers.insert(browser.to_string(), profiles.clone());
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(&cache) {
        let _ = fs::write(&path, text);
    }
}

fn warn_blocked(browser: &str, err: &str) {
    let mut warned = WARNED.lock().unwrap_or_else(|e| e.into_inner());
    if warned.insert(browser.to_string()) {
        crate::log::log(&format!(
            "profiles: live read of {browser} failed ({err}); using cached mapping. \
             Run `urwhere --sync-profiles` from a Terminal with Full Disk Access to refresh, \
             or use a directory name (e.g. \"Profile 1\") in the config."
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_directory_names() {
        // Chrome accepts these directly, no lookup required.
        assert!(is_directory_name("Default"));
        assert!(is_directory_name("default"));
        assert!(is_directory_name("Profile 1"));
        assert!(is_directory_name("Profile 12"));
        assert!(is_directory_name("Person 2"));
        assert!(is_directory_name("Guest Profile"));
    }

    #[test]
    fn rejects_display_names() {
        assert!(!is_directory_name("Zephyr"));
        assert!(!is_directory_name("Oscar"));
        assert!(!is_directory_name(""));
    }

    #[test]
    fn cache_round_trips() {
        let mut profiles = CachedProfiles::default();
        profiles.map.insert("zephyr".into(), "Profile 1".into());
        profiles.list.push(("Profile 1".into(), "Zephyr".into()));

        let mut file = CacheFile::default();
        file.browsers.insert("Google Chrome".into(), profiles);

        let json = serde_json::to_string(&file).unwrap();
        let back: CacheFile = serde_json::from_str(&json).unwrap();
        let restored = &back.browsers["Google Chrome"];
        assert_eq!(restored.map["zephyr"], "Profile 1");
        assert_eq!(
            restored.list,
            vec![("Profile 1".to_string(), "Zephyr".to_string())]
        );
    }
}
