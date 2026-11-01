#!/usr/bin/env bash
# Launches GlobeCraft under X11 (Xvfb with a decorating window manager), Wayland on
# KDE's KWin (virtual output, server-side title bars as on a Plasma desktop) or Wayland
# on headless Weston (no server-side title bars: winit draws its own), then maximizes,
# minimizes, restores and closes it the way the title bar buttons do.
# Usage: linux.sh x11|kde|wayland <binary> <out-dir>
set -uo pipefail
mode=$1 bin=$2 out=$3
mkdir -p "$out"
report="$out/report.txt"; : > "$report"
log() { echo "$*" | tee -a "$report"; }
if [ "$mode" = x11 ]; then
  Xvfb :99 -screen 0 1600x1000x24 & sleep 2
  export DISPLAY=:99
  openbox & sleep 2
  unset WAYLAND_DISPLAY
  shot() { import -window root "$out/$1.png"; }
elif [ "$mode" = kde ]; then
  export XDG_RUNTIME_DIR=$(mktemp -d); chmod 700 "$XDG_RUNTIME_DIR"
  # KWin needs a session bus for its scripting interface, which drives the window.
  eval "$(dbus-launch --sh-syntax)"
  # A copy without the binary's file capabilities, which a container may refuse.
  cp "$(command -v kwin_wayland)" "$XDG_RUNTIME_DIR/kwin_wayland"
  "$XDG_RUNTIME_DIR/kwin_wayland" --virtual --no-lockscreen --width 1600 --height 1000 \
    --socket wayland-kde > "$out/kwin.log" 2>&1 & sleep 5
  export WAYLAND_DISPLAY=wayland-kde
  unset DISPLAY
  shot() { :; }
  # Runs a KWin script against the GlobeCraft window; its print() lines land in
  # kwin.log prefixed "js:".
  n=0
  kwin() {
    n=$((n + 1))
    local file="$out/step$n.js"
    cat > "$file" <<JS
var list = workspace.windowList ? workspace.windowList() : workspace.clientList();
for (var i = 0; i < list.length; i++) {
  var w = list[i];
  if (w.caption.indexOf("GlobeCraft") < 0) continue;
  $1
  var f = w.frameGeometry;
  // The frame around the window's own surface: KWin's title bar and borders.
  var inner = w.clientGeometry || w.bufferGeometry;
  var frame = inner ? (f.height - inner.height) + "px in height" : "unknown";
  print("SMOKE $2: caption=" + w.caption + " noBorder=" + w.noBorder
    + " decoration=" + frame + " minimized=" + w.minimized
    + " frame=" + f.width + "x" + f.height + "+" + f.x + "+" + f.y);
}
JS
    local id
    id=$(gdbus call --session --dest org.kde.KWin --object-path /Scripting \
      --method org.kde.kwin.Scripting.loadScript "$file" "smoke$n" | tr -dc '0-9-')
    # The script's object: /Scripting/Script<id> on KWin 6, /<id> on KWin 5.
    gdbus call --session --dest org.kde.KWin --object-path "/Scripting/Script$id" \
      --method org.kde.kwin.Script.run > /dev/null 2>&1 ||
      gdbus call --session --dest org.kde.KWin --object-path "/$id" \
        --method org.kde.kwin.Script.run > /dev/null
    sleep 2
    gdbus call --session --dest org.kde.KWin --object-path /Scripting \
      --method org.kde.kwin.Scripting.unloadScript "smoke$n" > /dev/null
    grep "SMOKE $2:" "$out/kwin.log" | tail -1 | sed 's/.*SMOKE /  /' | tee -a "$report"
  }
  # Does `action`, then reports the window as `label` once KWin has applied it.
  step() { kwin "$1" "$2-requested" > /dev/null; kwin "" "$2"; }
else
  export XDG_RUNTIME_DIR=$(mktemp -d); chmod 700 "$XDG_RUNTIME_DIR"
  weston --backend=headless --renderer=pixman --width=1600 --height=1000 \
    --socket=wayland-1 --debug > "$out/weston.log" 2>&1 & sleep 3
  export WAYLAND_DISPLAY=wayland-1
  unset DISPLAY
  shot() { weston-screenshooter > /dev/null 2>&1 && mv wayland-screenshot-*.png "$out/$1.png" 2>/dev/null || true; }
fi
"$bin" > "$out/app.log" 2>&1 &
pid=$!
sleep 25
if ! kill -0 $pid 2>/dev/null; then wait $pid; log "app exited early with $?"; tail -40 "$out/app.log" | tee -a "$report"; exit 1; fi
log "running under $mode"
if [ "$mode" = x11 ]; then
  win=$(xdotool search --sync --pid $pid --onlyvisible | head -1)
  log "title: $(xdotool getwindowname "$win")"
  log "WM_CLASS: $(xprop -id "$win" WM_CLASS | cut -d= -f2)"
  log "frame extents (native decorations): $(xprop -id "$win" _NET_FRAME_EXTENTS | cut -d= -f2)"
  state() { log "$1: $(xprop -id "$win" _NET_WM_STATE | cut -d= -f2) $(xdotool getwindowgeometry "$win" | tr '\n' ' ')"; }
  state launched; shot 01-launched
  wmctrl -i -r "$win" -b add,maximized_vert,maximized_horz; sleep 3; state maximized; shot 02-maximized
  xdotool windowminimize "$win"; sleep 2; state minimized; shot 03-minimized
  wmctrl -i -a "$win"; sleep 2
  wmctrl -i -r "$win" -b remove,maximized_vert,maximized_horz; sleep 3; state restored; shot 04-restored
  # Drag the window by its title bar 200 px to the right.
  eval "$(xdotool getwindowgeometry --shell "$win")"
  # xdotool reports the frame's offset in Y; the title bar is the frame's top extent.
  top=$(xprop -id "$win" _NET_FRAME_EXTENTS | cut -d= -f2 | cut -d, -f3 | tr -d ' ')
  xdotool mousemove $((X + 200)) $((Y - 2 * top + top / 2)); sleep 0.3; xdotool mousedown 1; sleep 0.3
  for _ in 1 2 3 4; do xdotool mousemove_relative 50 0; sleep 0.2; done
  xdotool mouseup 1; sleep 1
  state dragged; shot 05-dragged
  start=$(date +%s.%N)
  wmctrl -i -c "$win"   # what the close button sends
elif [ "$mode" = kde ]; then
  kwin "" launched
  step "w.setMaximize(true, true);" maximized
  step "w.minimized = true;" minimized
  step "w.minimized = false; w.setMaximize(false, false);" restored
  start=$(date +%s.%N)
  kwin "w.closeWindow();" close   # what the title bar's close button does
else
  log "Wayland: xdg-shell window; winit draws client-side decorations (Weston has no server-side ones)"
  grep -i -E "xdg|decoration|error" "$out/app.log" | head -20 >> "$report" || true
  shot 01-launched
  start=$(date +%s.%N)
  # No server-side close request exists for a headless Weston client; SIGTERM ends it.
  kill -TERM $pid
fi
for _ in $(seq 60); do kill -0 $pid 2>/dev/null || break; sleep 0.5; done
if kill -0 $pid 2>/dev/null; then log "closed: still running after 30 s"; kill -9 $pid; exit 1; fi
wait $pid; code=$?
log "closed: exit $code after $(echo "$(date +%s.%N) - $start" | bc) s"
tail -15 "$out/app.log" >> "$report"
