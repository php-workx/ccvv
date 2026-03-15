#!/bin/sh
set -eu

export PATH="/usr/local/cargo/bin:${PATH}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/tmp/ccvv-wayland-runtime}"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"

export WLR_BACKENDS="${WLR_BACKENDS:-headless}"
export WLR_LIBINPUT_NO_DEVICES=1
export WLR_RENDERER_ALLOW_SOFTWARE=1
export WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-wayland-1}"
export CCVV_WAYLAND_HARNESS="${CCVV_WAYLAND_HARNESS:-1}"

if [ "$(id -u)" -eq 0 ]; then
  mkdir -p /home/ccvv
  chown -R ccvv:ccvv "$XDG_RUNTIME_DIR" /home/ccvv
  exec runuser -u ccvv -- env \
    HOME=/home/ccvv \
    PATH="/usr/local/cargo/bin:${PATH}" \
    XDG_RUNTIME_DIR="$XDG_RUNTIME_DIR" \
    WLR_BACKENDS="$WLR_BACKENDS" \
    WLR_LIBINPUT_NO_DEVICES="$WLR_LIBINPUT_NO_DEVICES" \
    WLR_RENDERER_ALLOW_SOFTWARE="$WLR_RENDERER_ALLOW_SOFTWARE" \
    WAYLAND_DISPLAY="$WAYLAND_DISPLAY" \
    /usr/local/bin/wayland-harness-entrypoint "$@"
fi

dbus-run-session -- sh -eu -c '
  sway --unsupported-gpu -c /etc/ccvv/sway.conf >/tmp/ccvv-sway.log 2>&1 &
  sway_pid=$!

  for _ in $(seq 1 100); do
    if [ -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ]; then
      break
    fi
    sleep 0.1
  done

  if [ ! -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ]; then
    cat /tmp/ccvv-sway.log >&2 || true
    kill "$sway_pid" || true
    wait "$sway_pid" || true
    exit 1
  fi

  export WAYLAND_DISPLAY
  export XDG_RUNTIME_DIR

  if [ "$#" -eq 0 ]; then
    set -- sh
  fi

  "$@"
  status=$?

  kill "$sway_pid" 2>/dev/null || true
  wait "$sway_pid" || true
  exit "$status"
' -- "$@"
