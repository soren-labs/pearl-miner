#!/usr/bin/env bash
# Local Mac only. Never invoke from a hosted CI service.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ -n "${GITHUB_ACTIONS:-}" || -n "${GITLAB_CI:-}" || -n "${BUILDKITE:-}" ]]; then
  echo "Run this verification on the local Mac, not a hosted CI service." >&2
  exit 1
fi
if [[ "$(uname -s)" != "Darwin" || "$(uname -m)" != "arm64" ]]; then
  echo "Local verification requires an Apple Silicon Mac." >&2
  exit 1
fi
cd "$ROOT_DIR"
VERSION="$(awk -F '"' '/^version = / { print $2; exit }' core/Cargo.toml)"
cargo fmt -p pearl-miner-core --check
cargo test --locked -p pearl-miner-core
cargo clippy --locked -p pearl-miner-core --all-targets --no-deps -- -D warnings
./scripts/build_app.sh "$VERSION"
CORE_BIN="$ROOT_DIR/dist/PearlMiner.app/Contents/MacOS/pearl-miner-core"
python3 - "$CORE_BIN" <<'PY'
import json, os, platform, subprocess, sys
core = sys.argv[1]
blocked = subprocess.run([core, '--help'], env=dict(os.environ, GITHUB_ACTIONS='true'),
                         stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
assert blocked.returncode != 0 and 'disabled in GitHub Actions' in blocked.stderr
hardware = json.loads(subprocess.check_output([core, 'hardware'], text=True))
is_m6 = 'M6' in hardware['device_name'] and int(platform.mac_ver()[0].split('.')[0]) >= 27
args = [core, 'self-test']
if is_m6:
    args += ['--expect-kernel', 'pow_sweep_m6_tensor']
subprocess.run(args, check=True)
if is_m6:
    subprocess.run([core, 'self-test', '--expect-kernel', 'pow_sweep_v2'],
                   env=dict(os.environ, PEARL_METAL_DISABLE_M6='1'), check=True)
PY
python3 scripts/smoke_test.py "$CORE_BIN"
codesign --verify --deep --strict "$ROOT_DIR/dist/PearlMiner.app"
hdiutil verify "$ROOT_DIR/dist/PearlMiner-macOS-arm64.dmg" >/dev/null
cd "$ROOT_DIR/dist"
shasum -a 256 -c SHA256SUMS.txt
echo "Local CI passed. Artifacts are ready for manual upload."
