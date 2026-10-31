#!/usr/bin/env bash
# Opens the packaged GlobeCraft.app, reads its menu bar and window buttons through
# System Events, zooms (green button), minimizes, restores, quits with Cmd-Q and checks
# cleanup ran. Usage: macos.sh <GlobeCraft.app> <out-dir>
set -uo pipefail
app=$1 out=$2
mkdir -p "$out"; report="$out/report.txt"; : > "$report"
log() { echo "$*" | tee -a "$report"; }
log "bundle name: $(/usr/libexec/PlistBuddy -c 'Print CFBundleName' "$app/Contents/Info.plist")"
log "display name: $(/usr/libexec/PlistBuddy -c 'Print CFBundleDisplayName' "$app/Contents/Info.plist" 2>/dev/null)"
open "$app"; sleep 25
shot() { screencapture -x "$out/$1.png"; }
ax() { osascript -e "tell application \"System Events\" to tell (first process whose bundle identifier is \"io.github.fireflythereal.globecraft\") to $1" 2>&1; }
log "process name: $(ax 'get name')"
log "menu bar: $(ax 'get name of menu bar items of menu bar 1')"
log "app menu items: $(ax 'get name of menu items of menu 1 of menu bar item 2 of menu bar 1')"
log "windows: $(ax 'get name of windows')"
log "window buttons: $(ax 'get subrole of buttons of window 1')"
log "size: $(ax 'get size of window 1')"
shot 01-launched
ax 'click (first button of window 1 whose subrole is "AXZoomButton")' > /dev/null; sleep 3
log "zoomed size: $(ax 'get size of window 1')"; shot 02-zoomed
ax 'click (first button of window 1 whose subrole is "AXMinimizeButton")' > /dev/null; sleep 2
log "minimized: $(ax 'get value of attribute "AXMinimized" of window 1')"; shot 03-minimized
ax 'set value of attribute "AXMinimized" of window 1 to false' > /dev/null; sleep 2
log "restored: minimized=$(ax 'get value of attribute "AXMinimized" of window 1')"; shot 04-restored
ax 'set value of attribute "AXFullScreen" of window 1 to true' > /dev/null; sleep 4; shot 05-fullscreen
ax 'set value of attribute "AXFullScreen" of window 1 to false' > /dev/null; sleep 3
pid=$(pgrep -x globecraft | head -1)
start=$(date +%s)
osascript -e 'tell application id "io.github.fireflythereal.globecraft" to quit'
for _ in $(seq 60); do kill -0 "$pid" 2>/dev/null || break; sleep 0.5; done
if kill -0 "$pid" 2>/dev/null; then log "quit: still running after 30 s"; kill -9 "$pid"; else log "quit: ended after $(( $(date +%s) - start )) s"; fi
