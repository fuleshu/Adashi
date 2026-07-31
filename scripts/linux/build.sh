#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
project_root="$(cd -- "${script_dir}/../.." && pwd)"
linux_build_dir="${ADASHI_LINUX_BUILD_DIR:-${project_root}/build/linux}"

export CARGO_TARGET_DIR="${linux_build_dir}/cargo-target"

cd "${project_root}"

build_target="${1:-all}"
version="$(node -p "require('./package.json').version")"

echo "Building Adashi ${version} for Linux (${build_target})"

case "${build_target}" in
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

if [[ "${build_target}" == "all" || "${build_target}" == "desktop" ]]; then
  architecture="$(dpkg --print-architecture)"
  package_path="${linux_build_dir}/cargo-target/release/bundle/deb/Adashi_${version}_${architecture}.deb"

  if [[ ! -f "${package_path}" ]]; then
    echo "Expected Debian package was not created: ${package_path}" >&2
    exit 1
  fi

  echo "Debian package: ${package_path}"
fi

echo "Linux build output: ${linux_build_dir}"
