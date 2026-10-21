#!/usr/bin/env bash
# Phase 6 demo, nested in your desktop: fern-comp with fern-wm and the
# shell (menu bar, dock, Search, notifications, control center), the
# settings daemon on a private session bus, and three demo apps in the
# dock. Settings live in build/shell-demo/, never in your own ~/.config.
#
# Run by `just shell-demo` (which builds everything first). With --drm
# (`just shell-drm`) it runs on the real display from a text console
# instead, with the release build, and logs to build/shell-drm.log.
set -euo pipefail
cd "$(dirname "$0")/.."
root=$PWD
mode=--nested
bin=$root/target/debug
log=/dev/stderr
if [ "${1:-}" = --drm ]; then
    mode=--drm
    bin=$root/target/release
    log=$root/build/shell-drm.log
    # NixOS keeps the GPU's userspace drivers here.
    export LD_LIBRARY_PATH=/run/opengl-driver/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
    export GBM_BACKENDS_PATH=/run/opengl-driver/lib/gbm
    export __EGL_VENDOR_LIBRARY_DIRS=/run/opengl-driver/share/glvnd/egl_vendor.d
    export FERN_COMP_STATS=1
fi
demo=$root/build/shell-demo
mkdir -p "$demo/data/applications" "$demo/config/fern"

client=$bin/fern-testclient
for app in "org.fern.Notes Notes ff5a5fff" "org.fern.Mail Mail 3a86ffff" "org.fern.Photos Photos 2ec4b6ff"; do
    read -r id name color <<<"$app"
    cat >"$demo/data/applications/$id.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=$name
Exec=$client --title $name --app-id $id --color $color --size 640x420
EOF
done
[ -f "$demo/config/fern/dock.toml" ] ||
    echo 'pinned = ["org.fern.Notes", "org.fern.Mail", "org.fern.Photos"]' >"$demo/config/fern/dock.toml"

export XDG_DATA_HOME=$demo/data
export XDG_DATA_DIRS=$demo/data:${XDG_DATA_DIRS:-/usr/share}
export XDG_CONFIG_HOME=$demo/config
export FERN_FONTS=$root/build/fonts
export QML_IMPORT_PATH=$root/build/de/qml${QML_IMPORT_PATH:+:$QML_IMPORT_PATH}

echo "Keys: Super+Space Search | click the tree for the Fern menu | the clock for notifications"
echo "      try: fern-settings set appearance theme dark (in another terminal, same bus)"
conf=$(dirname "$(dirname "$(command -v dbus-daemon)")")/share/dbus-1/session.conf
exec dbus-run-session --config-file="$conf" -- sh -c '
    "$1/fern-settingsd" --dir "$XDG_CONFIG_HOME/fern" &
    "$1/fern-indexd" --root "$HOME" --db "$XDG_DATA_HOME/index.sqlite" &
    exec "$1/fern-comp" "$2" --shell "$3/build/de/shell/fern-shell"
' sh "$bin" "$mode" "$root" 2>>"$log"
