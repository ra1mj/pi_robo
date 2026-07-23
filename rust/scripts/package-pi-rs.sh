#!/usr/bin/env bash

set -euo pipefail

target="${1:-x86_64-unknown-linux-gnu}"
output_dir="${2:-target/pi-rs-artifacts}"
commit_sha="${3:-${GITHUB_SHA:-$(git rev-parse HEAD)}}"

if [[ "${target}" != "x86_64-unknown-linux-gnu" ]]; then
  echo "unsupported pi-rs milestone-1 artifact target: ${target}" >&2
  exit 2
fi
if [[ ! "${commit_sha}" =~ ^[A-Za-z0-9._-]+$ ]]; then
  echo "invalid artifact commit identifier: ${commit_sha}" >&2
  exit 2
fi
if [[ -e "${output_dir}" ]]; then
  echo "artifact output directory already exists: ${output_dir}" >&2
  exit 2
fi

cargo build --release --locked -p pi-cli --target "${target}"

binary="target/${target}/release/pi-rs"
test -x "${binary}"

mkdir -p "${output_dir}"

stage_dir="$(mktemp -d)"
trap 'rm -rf "${stage_dir}"' EXIT
install -m 0755 "${binary}" "${stage_dir}/pi-rs"

archive="pi-rs-linux-x64-${commit_sha}.tar.gz"
tar -C "${stage_dir}" -czf "${output_dir}/${archive}" pi-rs

package_id="$(cargo pkgid -p pi-cli)"
package_version="${package_id##*#}"
rustc_version="$(rustc --version)"
cargo_version="$(cargo --version)"
lock_sha256="$(sha256sum Cargo.lock | cut -d ' ' -f 1)"

printf '%s\n' \
  '{' \
  '  "schemaVersion": 1,' \
  "  \"sourceCommit\": \"${commit_sha}\"," \
  "  \"workspaceVersion\": \"${package_version}\"," \
  "  \"target\": \"${target}\"," \
  '  "profile": "release",' \
  "  \"rustc\": \"${rustc_version}\"," \
  "  \"cargo\": \"${cargo_version}\"," \
  "  \"cargoLockSha256\": \"${lock_sha256}\"" \
  '}' > "${output_dir}/build-info.json"

(
  cd "${output_dir}"
  sha256sum "${archive}" build-info.json > SHA256SUMS
)
