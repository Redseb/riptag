#!/bin/bash
# Drives the real app on macOS so a change can be checked by hand or by an agent.
# Needs Accessibility and Screen Recording permission for the calling terminal app.
#
#   scripts/drive-app.sh start           launch `tauri dev` and wait for the window
#   scripts/drive-app.sh key down down   press keys: up down left right enter esc
#   scripts/drive-app.sh shot out.png    screenshot the app window
#   scripts/drive-app.sh stop            quit the app and its dev server
set -u

cd "$(dirname "$0")/.."
LOG="${TMPDIR:-/tmp}/riptag-dev.log"
PID_FILE="${TMPDIR:-/tmp}/riptag-dev.pid"
APP_MATCH="target/debug/riptag"
DEV_PORT=1420

app_pid() { pgrep -f "$APP_MATCH" | head -1; }

require_app() {
  PID=$(app_pid)
  if [ -z "$PID" ]; then
    echo "app is not running" >&2
    exit 1
  fi
}

focus() {
  osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $PID) to true" || exit 1
  sleep 0.3
}

key_code() {
  case "$1" in
    up) echo 126 ;;
    down) echo 125 ;;
    left) echo 123 ;;
    right) echo 124 ;;
    enter) echo 36 ;;
    esc) echo 53 ;;
    *) echo "unknown key: $1" >&2; exit 1 ;;
  esac
}

case "${1:-}" in
  start)
    if [ -n "$(app_pid)" ]; then
      echo "app is already running" >&2
      exit 1
    fi
    [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
    nohup npm run tauri dev >"$LOG" 2>&1 </dev/null &
    echo $! >"$PID_FILE"
    for _ in $(seq 1 180); do
      PID=$(app_pid)
      if [ -n "$PID" ] && osascript -e "tell application \"System Events\" to get name of window 1 of (first process whose unix id is $PID)" >/dev/null 2>&1; then
        echo "running, pid $PID, log $LOG"
        exit 0
      fi
      sleep 1
    done
    echo "timed out waiting for the window, see $LOG" >&2
    exit 1
    ;;
  key)
    shift
    require_app
    focus
    for name in "$@"; do
      code=$(key_code "$name") || exit 1
      osascript -e "tell application \"System Events\" to key code $code" || exit 1
      sleep 0.2
    done
    ;;
  shot)
    require_app
    focus
    BOUNDS=$(osascript \
      -e "tell application \"System Events\" to tell window 1 of (first process whose unix id is $PID) to set {p, s} to {position, size}" \
      -e 'return (item 1 of p as text) & "," & (item 2 of p as text) & "," & (item 1 of s as text) & "," & (item 2 of s as text)') || exit 1
    screencapture -x -R"$BOUNDS" "${2:?usage: shot <path>}" || exit 1
    ;;
  stop)
    pkill -f "$APP_MATCH"
    [ -f "$PID_FILE" ] && kill "$(cat "$PID_FILE")" 2>/dev/null
    rm -f "$PID_FILE"
    lsof -ti "tcp:$DEV_PORT" | xargs kill 2>/dev/null
    exit 0
    ;;
  *)
    sed -n '2,8p' "$0"
    exit 1
    ;;
esac
