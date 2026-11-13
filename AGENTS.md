# Execution and release policy

- Build, run, test, benchmark and mine with the Rust/Metal core only on the local Mac. Do not use GitHub-hosted runners for any of these operations or for packaging, which builds the core.
- Do not add or re-enable hosted build/test/release workflows. Use `scripts/ci_local.sh` for local verification. GitHub hosts source code, pull requests and uploaded, locally built release artifacts.
- The build script, CLI and local CI refuse `GITHUB_ACTIONS` environments as an accidental-use safeguard. Keep this guard.
- The default local CI uses offline GPU checks and a simulated localhost pool. Any public-pool mining test must be an explicitly scoped, bounded local test using a separate wallet.
- Preserve user wallets and private test keys outside Git and distribution archives. Never print private keys in validation logs.
- When the user authorizes temporarily pausing another miner, record its process state and restore it after local validation. Do not change its wallet or schedule.
