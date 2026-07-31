#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
project_root="$(cd -- "${script_dir}/../.." && pwd)"
linux_build_dir="${ADASHI_LINUX_BUILD_DIR:-${project_root}/build/linux}"

cd "${project_root}"

version="$(node -p "require('./package.json').version")"
architecture="$(dpkg --print-architecture)"
package_path="${linux_build_dir}/cargo-target/release/bundle/deb/Adashi_${version}_${architecture}.deb"

"${script_dir}/build.sh" desktop

if [[ ! -f "${package_path}" ]]; then
  echo "Debian package was not created: ${package_path}" >&2
  exit 1
fi

staged_package="$(mktemp --tmpdir --suffix=.deb "adashi-${version}-${architecture}.XXXXXX")"

cleanup() {
  rm -f -- "${staged_package}"
}
trap cleanup EXIT

install -m 0644 -- "${package_path}" "${staged_package}"

echo "Installing Adashi ${version} from ${package_path}"
sudo apt-get install --reinstall --yes "${staged_package}"

echo "Adashi ${version} installed successfully."
