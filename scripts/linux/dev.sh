#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
project_root="$(cd -- "${script_dir}/../.." && pwd)"
linux_build_dir="${ADASHI_LINUX_BUILD_DIR:-${project_root}/build/linux}"

export CARGO_TARGET_DIR="${linux_build_dir}/cargo-target"

cd "${project_root}"
npm run tauri -- dev
