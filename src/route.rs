use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::browsers;
use crate::engine::Engine;
use crate::log;

static ENGINE: OnceLock<Mutex<Engine>> = OnceLock::new();
static LAST_SEEN: Mutex<Option<(String, Instant)>> = Mutex::new(None);

/// Windows of time in which a repeat of the same URL is treated as a duplicate
/// delivery of a single click.
const DEDUPE_WINDOW: Duration = Duration::from_millis(250);

pub fn init(config_path: PathBuf) {
    let engine = Engine::load(&config_path);
    let _ = ENGINE.set(Mutex::new(engine));
}

/// Handle one URL arriving from the OS. Cheap: a stat, a regex pass, a spawn.
pub fn handle_url(url: &str) {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        log::log(&format!("ignoring non-http url: {url}"));
        return;
    }
    if is_duplicate(url) {
        return;
    }

    let (browser, profile, rule) = {
        let engine = ENGINE.get().expect("route::init was not called");
        let mut engine = engine.lock().unwrap_or_else(|e| e.into_inner());
        engine.reload_if_changed();
        let (action, rule) = engine.resolve(url);
        (
            action.browser.clone(),
            action.profile.clone(),
            rule.map(str::to_owned),
        )
    };

    let rule = rule.as_deref().unwrap_or("<default>");
    match browsers::launch(&browser, profile.as_deref(), url) {
        Ok(()) => match &profile {
            Some(profile) => log::log(&format!("{url} -> {browser} / {profile} [{rule}]")),
            None => log::log(&format!("{url} -> {browser} [default profile] [{rule}]")),
        },
        Err(err) => log::log(&format!("failed to open {url}: {err}")),
    }
}

fn is_duplicate(url: &str) -> bool {
    let mut last = LAST_SEEN.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    if let Some((previous, at)) = last.as_ref() {
        if previous == url && now.duration_since(*at) < DEDUPE_WINDOW {
            return true;
        }
    }
    *last = Some((url.to_string(), now));
    false
}
