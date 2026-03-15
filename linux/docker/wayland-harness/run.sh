#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname "$0")/../../.." && pwd)
image_name="${CCVV_WAYLAND_HARNESS_IMAGE:-ccvv-wayland-harness}"

docker build -t "$image_name" -f "$repo_root/linux/docker/wayland-harness/Dockerfile" "$repo_root"

docker run --rm \
  -e CCVV_WAYLAND_HARNESS=1 \
  -v "$repo_root:/workspace" \
  -w /workspace \
  "$image_name" \
  "$@"
