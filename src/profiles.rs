use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;

/// Chrome's on-disk profiles. `map` resolves any alias (display name, account
/// name, or directory name) to its directory; `list` is the display list.
struct Index {
    map: HashMap<String, String>,
    list: Vec<(String, String)>,
    path: PathBuf,
    mtime: Option<SystemTime>,
}

static CACHE: LazyLock<Mutex<HashMap<String, Index>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

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
    with_index(browser, |index| {
        index.map.get(&name.to_ascii_lowercase()).cloned()
    })
    .flatten()
}

/// Every known profile for a browser as `(directory, display name)`.
pub fn all(browser: &str) -> Vec<(String, String)> {
    with_index(browser, |index| index.list.clone()).unwrap_or_default()
}

fn with_index<T>(browser: &str, f: impl FnOnce(&Index) -> T) -> Option<T> {
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());

    let stale = match cache.get(browser) {
        Some(index) => {
            let current = fs::metadata(&index.path).and_then(|m| m.modified()).ok();
            current != index.mtime
        }
        None => true,
    };
    if stale {
        if let Some(index) = load(browser) {
            cache.insert(browser.to_string(), index);
        }
    }

    cache.get(browser).map(f)
}

fn load(browser: &str) -> Option<Index> {
    let path = user_data_dir(browser)?.join("Local State");
    let mtime = fs::metadata(&path).and_then(|m| m.modified()).ok();
    let text = fs::read_to_string(&path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let info_cache = json.get("profile")?.get("info_cache")?.as_object()?;

    let mut map = HashMap::new();
    let mut list = Vec::new();

    for (directory, info) in info_cache {
        map.insert(directory.to_ascii_lowercase(), directory.clone());
        for key in ["name", "gaia_name", "user_name"] {
            if let Some(value) = info.get(key).and_then(|v| v.as_str()) {
                map.entry(value.to_ascii_lowercase())
                    .or_insert_with(|| directory.clone());
            }
        }
        let name = info
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(directory)
            .to_string();
        list.push((directory.clone(), name));
    }

    list.sort();
    Some(Index {
        map,
        list,
        path,
        mtime,
    })
}
