mod agent;
mod browsers;
mod config;
mod engine;
mod log;
mod profiles;
mod route;

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        // When LaunchServices starts us to handle a link it passes no arguments
        // (or a legacy `-psn_...` process serial number).
        None => start_agent(),
        Some(arg) if arg.starts_with("-psn") => start_agent(),
        Some("--test") => {
            let Some(url) = args.get(1) else {
                eprintln!("usage: urwhere --test <url>");
                return ExitCode::from(2);
            };
            test(url);
            ExitCode::SUCCESS
        }
        Some("--profiles") => {
            list_profiles();
            ExitCode::SUCCESS
        }
        Some("--sync-profiles") => {
            sync_profiles();
            ExitCode::SUCCESS
        }
        Some("--set-default") => {
            set_default();
            ExitCode::SUCCESS
        }
        Some("--log") => {
            match log::log_path().and_then(|p| std::fs::read_to_string(p).ok()) {
                Some(contents) => print!("{contents}"),
                None => println!("no log yet"),
            }
            ExitCode::SUCCESS
        }
        Some("--help" | "-h") => {
            usage();
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("urwhere: unknown argument {other:?}");
            usage();
            ExitCode::from(2)
        }
    }
}

fn config_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join(".config/urwhere.json")
}

fn start_agent() -> ! {
    route::init(config_path());
    agent::run()
}

fn test(url: &str) {
    let engine = engine::Engine::load(&config_path());
    let (action, rule) = engine.resolve(url);

    println!("url      {url}");
    println!("rule     {}", rule.unwrap_or("<default>"));
    println!("browser  {}", action.browser);
    match &action.profile {
        Some(profile) => {
            println!("profile  {profile}");
            match profiles::resolve(&action.browser, profile) {
                Some(directory) => println!("dir      {directory}"),
                None => println!("dir      !! not found, would fall back to the default profile"),
            }
        }
        None => println!("profile  <default>"),
    }
}

fn list_profiles() {
    for browser in profiles::KNOWN_BROWSERS {
        let entries = profiles::all(browser);
        if entries.is_empty() {
            continue;
        }
        println!("{browser}:");
        for (directory, name) in entries {
            println!("  {name:<28} -> {directory}");
        }
    }
}

fn sync_profiles() {
    let (refreshed, blocked) = profiles::sync();
    if refreshed.is_empty() && blocked.is_empty() {
        println!("no Chromium profiles found");
        return;
    }
    for browser in &refreshed {
        println!("synced  {browser}");
    }
    for browser in &blocked {
        println!("blocked {browser}");
    }
    if !blocked.is_empty() {
        println!();
        println!(
            "Some profiles could not be read. macOS protects these directories from \
             third-party apps; run this command from a Terminal granted Full Disk Access, \
             or use a directory name (e.g. \"Profile 1\") in ~/.config/urwhere.json."
        );
    }
}

fn set_default() {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use block2::RcBlock;
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSDate, NSError, NSRunLoop, NSString, NSURL};

    let Some(bundle) = bundle_path() else {
        eprintln!("urwhere is not running from inside a .app bundle");
        return;
    };

    let workspace = NSWorkspace::sharedWorkspace();
    let bundle_url = NSURL::fileURLWithPath(&NSString::from_str(&bundle.to_string_lossy()));

    // The change is asynchronous: the completion handler only runs while the
    // run loop is being pumped, so we drive it here rather than sleeping.
    let (sender, receiver) = mpsc::channel();
    let completion = RcBlock::new(move |error: *mut NSError| {
        let _ = sender.send(error.is_null());
    });

    for scheme in ["http", "https"] {
        let scheme = NSString::from_str(scheme);
        workspace.setDefaultApplicationAtURL_toOpenURLsWithScheme_completionHandler(
            &bundle_url,
            &scheme,
            Some(&completion),
        );
    }

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut done = 0;
    while done < 2 && Instant::now() < deadline {
        NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.1));
        while receiver.try_recv().is_ok() {
            done += 1;
        }
    }
    if done < 2 {
        eprintln!(
            "note: macOS did not accept the change programmatically; use System Settings below"
        );
    }

    if let Some(probe) = NSURL::URLWithString(&NSString::from_str("https://example.com")) {
        let handler = workspace
            .URLForApplicationToOpenURL(&probe)
            .and_then(|url| url.path())
            .map(|path| path.to_string())
            .unwrap_or_else(|| "<none>".to_string());
        println!("current https handler: {handler}");
    }

    println!();
    println!("If that isn't urwhere yet, macOS needs you to confirm it manually:");
    println!("  System Settings -> Desktop & Dock -> Default web browser -> urwhere");
    println!("Or open just that pane with:");
    println!("  open \"x-apple.systempreferences:com.apple.Desktop-Settings.extension\"");
}

fn bundle_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let app = exe.parent()?.parent()?.parent()?;
    (app.extension().map(|e| e == "app").unwrap_or(false)).then(|| app.to_path_buf())
}

fn usage() {
    println!(
        "\
urwhere {} - route URLs to Chrome profiles with regex rules

USAGE:
  urwhere                 Run as the background URL handler (used by macOS)
  urwhere --test <url>    Show which rule/profile a URL would use
  urwhere --profiles      List known Chrome profiles and their directories
  urwhere --sync-profiles Refresh the cached profile map from Chrome's data
  urwhere --set-default   Ask macOS to make urwhere the default browser
  urwhere --log           Print the routing log
  urwhere --help          Show this message

CONFIG:
  ~/.config/urwhere.json  (reloaded automatically when it changes)
  Cache: ~/Library/Application Support/urwhere/profiles.json
  Log: ~/Library/Logs/urwhere.log",
        env!("CARGO_PKG_VERSION")
    );
}
