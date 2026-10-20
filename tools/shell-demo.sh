#!/usr/bin/env bash
# Phase 6 demo, nested in your desktop: fern-comp with fern-wm and the
# shell (menu bar, dock, Search, notifications, control center), the
# settings daemon on a private session bus, and three demo apps in the
# dock. Settings live in build/shell-demo/, never in your own ~/.config.
#
# Run by `just shell-demo` (which builds everything first).
set -euo pipefail
cd "$(dirname "$0")/.."
root=$PWD
demo=$root/build/shell-demo
mkdir -p "$demo/data/applications" "$demo/config/fern"

client=$root/target/debug/fern-testclient
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
    "$1/target/debug/fern-settingsd" --dir "$XDG_CONFIG_HOME/fern" &
    exec "$1/target/debug/fern-comp" --nested --shell "$1/build/de/shell/fern-shell"
' sh "$root"
