# Maintaining releases

The default branch is `main`. Open pull requests for regular changes; CI checks formatting, strict Clippy, tests, both README command examples and release packaging tests on Linux, Windows and macOS.

## Publish a preview

1. Update `workspace.package.version` in Cargo.toml and Cargo.lock, CHANGELOG.md and `docs/releases/preview.md`. Keep both README languages and the AI skill synchronized with command changes.
2. For a bundled-engine version change, update `DEFAULT_VERSION` in the Rust installer and `VERSION` in `scripts/bundle_core.py` together. Check upstream license/source and supported platform assets. Test offline install and rollback.
3. Merge only after CI passes. Create and push an annotated `vMAJOR.MINOR.PATCH` tag matching the Cargo version.
4. The Release workflow reruns cross-platform CI, builds static musl CLI binaries natively on Linux x86_64/ARM64, bundles the verified engine and corresponding source, and runs real-engine smoke tests on both architectures.
5. Only after every check succeeds does it publish a GitHub prerelease with both archives and a combined SHA256SUMS. Inspect the assets, installation steps and release notes. Never move a published version tag to different source.

`workflow_dispatch` on a branch validates and packages artifacts without publishing. To retry publication after a transient failure, rerun the tagged workflow; uploads to the same prerelease are idempotent. Do not retry over a source defect: commit the fix and release a new version.

All releases remain previews until privileged systemd/TUN/DNS/SSH tests and their recovery paths have been accepted on native Linux hosts. Windows/macOS are compilation-tested only until their platform backends receive native acceptance. Change the publish policy and notes deliberately when declaring a stable release.

Dependabot opens weekly grouped Cargo and GitHub Actions updates. Review and merge them after CI; automatic merging is not enabled. Issues and pull requests must not include real credentials or subscriptions.
