#!/usr/bin/env bash
# Build urwhere and assemble it into dist/urwhere.app
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

APP="$ROOT/dist/urwhere.app"

echo "==> Building release binary"
cargo build --release

echo "==> Assembling $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$ROOT/target/release/urwhere" "$APP/Contents/MacOS/urwhere"
cp "$ROOT/resources/Info.plist" "$APP/Contents/Info.plist"

echo "==> Ad-hoc signing"
codesign --force --sign - "$APP" >/dev/null 2>&1 || echo "   (codesign failed, continuing)"

echo "==> Done: $APP"
