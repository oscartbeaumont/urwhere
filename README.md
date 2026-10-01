# urwhere

A tiny, fast macOS default browser that routes URLs to different Chrome
profiles using regex rules. A Rust re-implementation of a Finicky config.

- **Receives** URLs from macOS as an Apple Event (it is registered as the
  default `http`/`https` handler).
- **Matches** the full URL against an ordered list of regexes compiled into a
  single `RegexSet` (one pass, however many rules you have).
- **Launches** Chrome with `--profile-directory=...` for the chosen profile.

The binary is a background agent (`LSUIElement`), so there is no Dock icon and
no cold start on every click.

## Install

```sh
./scripts/install.sh
```

This builds a release binary, assembles `~/Applications/urwhere.app`, registers
it with LaunchServices, seeds `~/.config/urwhere.json` if missing, and tries to
make it the default browser.

macOS usually requires the final step to be done by hand:

> System Settings → Desktop & Dock → Default web browser → **urwhere**

## Config: `~/.config/urwhere.json`

Reloaded automatically when the file's mtime changes.

```json
{
  "default": { "browser": "Google Chrome" },
  "rules": [
    {
      "name": "ZephyrCloudIO (work)",
      "match": "(?i)^https?://(?:www\\.)?github\\.com/zephyrcloudio(?:/|$)",
      "browser": "Google Chrome",
      "profile": "Zephyr"
    }
  ]
}
```

- `match` is a regular expression tested against the full URL. First match
  wins. Use `(?i)` for case-insensitive.
- `profile` is the Chrome profile name as shown in the profile picker (e.g.
  `Zephyr`), or its directory name (e.g. `Profile 1`). Directory names always
  work; names are resolved via the profile cache (see below).
- `browser` may be omitted and defaults to `Google Chrome`. It also accepts an
  app path like `/Applications/Brave Browser.app`.

## CLI

```sh
urwhere --test <url>      # show which rule/profile a URL would use
urwhere --profiles        # list Chrome profiles and their directories
urwhere --sync-profiles   # refresh the cached profile map from Chrome's data
urwhere --set-default     # ask macOS to make urwhere the default browser
urwhere --log             # print the routing log
```

Logs go to `~/Library/Logs/urwhere.log`.

## macOS 27+ profile access (Golden Gate)

macOS 27 stamps `~/Library/Application Support/Google/Chrome` with a
`com.apple.macl` xattr that allowlists only Chrome's own code identity, so any
other app gets `EPERM` (operation not permitted) reading `Local State` — and no
user-granted privacy permission overrides it.

urwhere therefore resolves profile names in this order:

1. a live read of Chrome's `Local State` (used when permitted, and it refreshes
   the cache);
2. a cache of the last known name -> directory map at
   `~/Library/Application Support/urwhere/profiles.json`, populated by
   `urwhere --sync-profiles` (the installer runs this for you);
3. the `profile` value used verbatim, when it is a directory name such as
   `Default` or `Profile 1`.

If you add or rename a Chrome profile, refresh the cache from a Terminal:

```sh
urwhere --sync-profiles
```

If that reports `blocked`, the Terminal itself lacks read access — grant it Full
Disk Access in System Settings -> Privacy & Security, or set `profile` to the
directory name instead. You can still edit the cache by hand; it is plain JSON.

## Why direct binary invocation?

Launching `/Applications/Google Chrome.app/Contents/MacOS/Google Chrome
--profile-directory=... <url>` works even when Chrome is already running: the
new process hands the command line to the running browser, which opens the tab
in the requested profile. `open -a ... --args` does not reliably pass the flag
through, and `--user-data-dir` spawns a competing instance.
