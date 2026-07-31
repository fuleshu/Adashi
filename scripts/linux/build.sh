#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
project_root="$(cd -- "${script_dir}/../.." && pwd)"
linux_build_dir="${ADASHI_LINUX_BUILD_DIR:-${project_root}/build/linux}"

export CARGO_TARGET_DIR="${linux_build_dir}/cargo-target"

cd "${project_root}"

case "${1:-all}" in
  all)
    npm run tauri -- build
    ;;
  desktop)
    npm run tauri -- build --bundles deb
    ;;
  frontend)
    npm run build:frontend:linux
    ;;
  mcp)
    cargo build \
      --no-default-features \
      --release \
      --manifest-path src-tauri/Cargo.toml \
      --bin adashi-mcp
    ;;
  *)
    echo "Usage: $0 [all|desktop|frontend|mcp]" >&2
    exit 2
    ;;
esac

echo "Linux build output: ${linux_build_dir}"
