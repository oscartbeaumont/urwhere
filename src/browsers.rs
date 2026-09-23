use std::path::{Path, PathBuf};
use std::process::Command;

use crate::profiles;

/// Find an application bundle by name (e.g. `Google Chrome`), by path, or by
/// scanning the usual Applications folders case-insensitively.
pub fn app_path(name: &str) -> Option<PathBuf> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }

    if name.ends_with(".app") {
        let path = expand_tilde(name);
        if path.is_dir() {
            return Some(path);
        }
    }

    let mut roots: Vec<PathBuf> = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join("Applications"));
    }

    // Exact match first.
    for root in &roots {
        let candidate = root.join(format!("{name}.app"));
        if candidate.is_dir() {
            return Some(candidate);
        }
    }

    // Then a case-insensitive scan.
    for root in &roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let Some(stem) = file_name
                .to_string_lossy()
                .strip_suffix(".app")
                .map(str::to_owned)
            else {
                continue;
            };
            if stem.eq_ignore_ascii_case(name) {
                return Some(entry.path());
            }
        }
    }

    None
}

/// The executable inside a bundle, read from `CFBundleExecutable` so we handle
/// any Chromium fork without hardcoding binary names.
pub fn executable(app: &Path) -> PathBuf {
    if let Some(exe) = bundle_executable(app) {
        let path = app.join("Contents/MacOS").join(exe);
        if path.exists() {
            return path;
        }
    }
    let stem = app
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    app.join("Contents/MacOS").join(stem)
}

fn bundle_executable(app: &Path) -> Option<String> {
    let info = app.join("Contents/Info");
    let output = Command::new("/usr/bin/defaults")
        .arg("read")
        .arg(&info)
        .arg("CFBundleExecutable")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

/// Open `url` in `browser`, optionally pinned to a specific Chrome profile.
pub fn launch(browser: &str, profile: Option<&str>, url: &str) -> Result<(), String> {
    let app = app_path(browser).ok_or_else(|| format!("browser not found: {browser}"))?;

    match profile {
        Some(profile) => {
            let directory = profiles::resolve(browser, profile)
                .ok_or_else(|| format!("profile not found for {browser}: {profile}"))?;
            let mut command = Command::new(executable(&app));
            // Passing this straight to the binary (rather than via `open`)
            // is what makes an already-running Chrome honour the profile.
            command.arg(format!("--profile-directory={directory}"));
            command.arg(url);
            spawn_detached(&mut command)
        }
        None => {
            let mut command = Command::new("/usr/bin/open");
            command.arg("-a").arg(&app).arg(url);
            spawn_detached(&mut command)
        }
    }
}

fn spawn_detached(command: &mut Command) -> Result<(), String> {
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    // Reap off the main thread so the event loop never blocks and we don't
    // accumulate zombies.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}
