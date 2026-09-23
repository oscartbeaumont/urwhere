#!/usr/bin/env bash
# Build urwhere, install it to ~/Applications, register it with LaunchServices,
# and seed ~/.config/urwhere.json.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$HOME/Applications/urwhere.app"
CONFIG="$HOME/.config/urwhere.json"
LSREGISTER="/System/Library/Frameworks/CoreServices.framework/Versions/A/Frameworks/LaunchServices.framework/Versions/A/Support/lsregister"

"$ROOT/scripts/build.sh"

echo "==> Stopping any running instance"
pkill -f "$DEST/Contents/MacOS/urwhere" >/dev/null 2>&1 || true

echo "==> Installing to $DEST"
mkdir -p "$HOME/Applications"
rm -rf "$DEST"
cp -R "$ROOT/dist/urwhere.app" "$DEST"

echo "==> Registering with LaunchServices"
"$LSREGISTER" -f "$DEST"

# Don't leave the intermediate build copy registered; two bundles with the same
# identifier make the System Settings browser list ambiguous.
"$LSREGISTER" -u "$ROOT/dist/urwhere.app" >/dev/null 2>&1 || true
rm -rf "$ROOT/dist"

if [ -f "$CONFIG" ]; then
	echo "==> Keeping existing config at $CONFIG"
else
	echo "==> Writing default config to $CONFIG"
	mkdir -p "$(dirname "$CONFIG")"
	cp "$ROOT/config.example.json" "$CONFIG"
fi

echo "==> Asking macOS to make urwhere the default browser"
"$DEST/Contents/MacOS/urwhere" --set-default || true

echo "==> Starting the background agent"
open "$DEST"

cat <<'EOF'

Installed. Next steps:
  1. Verify routing without changing anything:
       ~/Applications/urwhere.app/Contents/MacOS/urwhere --test https://github.com/zephyrcloudio/urwhere
  2. If the default browser is not urwhere yet, set it manually:
       System Settings -> Desktop & Dock -> Default web browser -> urwhere
  3. Click a link and check ~/Library/Logs/urwhere.log (or run: urwhere --log)
EOF
