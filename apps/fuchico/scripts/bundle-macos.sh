#!/bin/sh
# Builds dist/Fuchico.app (unsigned), like `pnpm tauri build` did for the
# original: identifier dev.fuleco.fuchico, icon, Productivity category and
# the .md/.markdown "Editor" document types (resources/macos/Info.plist).
#
#   apps/fuchico/scripts/bundle-macos.sh              # release build + bundle
#   PROFILE=debug apps/fuchico/scripts/bundle-macos.sh
#   NO_BUILD=1 apps/fuchico/scripts/bundle-macos.sh   # reuse target/<profile>/fuchico
#   TARGET=x86_64-apple-darwin apps/fuchico/scripts/bundle-macos.sh
set -eu

here=$(cd "$(dirname "$0")/.." && pwd)        # apps/fuchico
root=$(cd "$here/../.." && pwd)               # workspace root
profile=${PROFILE:-release}
version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -n 1)

if [ -z "${NO_BUILD:-}" ]; then
  flag=""
  [ "$profile" = release ] && flag="--release"
  # shellcheck disable=SC2086
  (cd "$root" && cargo build -p fuchico $flag ${TARGET:+--target "$TARGET"})
fi

bin="$root/target/${TARGET:+$TARGET/}$profile/fuchico"
[ -x "$bin" ] || { echo "missing binary: $bin" >&2; exit 1; }

out=${OUT_DIR:-$root/dist}
app="$out/Fuchico.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin" "$app/Contents/MacOS/fuchico"
cp "$here/resources/icons/icon.icns" "$app/Contents/Resources/icon.icns"
sed "s/__VERSION__/$version/g" "$here/resources/macos/Info.plist" > "$app/Contents/Info.plist"
printf 'APPL????' > "$app/Contents/PkgInfo"
plutil -lint "$app/Contents/Info.plist" >/dev/null
echo "$app"
