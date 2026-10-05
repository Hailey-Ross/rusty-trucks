# 27. Release builds from a fork

## Problem

The Windows release workflow (`.github/workflows/release.yml`) could not publish a fork's own build. The
repository `SK8-ENGINE/skate-3-rust-engine` was hard-coded in three places:

- `tools/updater.py` (`REPO`): the publish scripts (`publish_branch.py`, `publish_experimental.py`) use it as their
  target, and the packaged updater (`skate3update.exe`) uses it to look for updates.
- `scripts/Build-Release.ps1`: the `repository` field of `release.json`, which the updater checks.

On a fork, a publish step therefore talked to upstream with the fork's token, and a fork build would have looked
for updates on upstream.

## Change

- `release.yml` sets `SKATE3RUST_REPOSITORY: ${{ github.repository }}` for every job. On upstream this is
  `SK8-ENGINE/skate-3-rust-engine`, so upstream behaviour is unchanged.
- `Build-Release.ps1` reads `SKATE3RUST_REPOSITORY` (default upstream, validated as `owner/repo`), writes it into
  `release.json`, and bakes it into the updater as a generated module
  (`target/updater-build/generated/release_repository.py`, added through PyInstaller `--paths`).
- `updater.py` takes `REPO` from the baked module, then `SKATE3RUST_REPOSITORY`, then the upstream default.
- On `Hailey-Ross/rusty-trucks`, pushes to `hails-additions` also publish a rolling `hails-additions` prerelease
  through the existing `publish_branch.py`. Numbered releases (the `release: published` event) work on any
  repository as before.

## Files

- `.github/workflows/release.yml`
- `scripts/Build-Release.ps1`
- `tools/updater.py`

## Verification

- `python -m unittest test_updater` (in `tools/`): 15 tests pass.
- `SKATE3RUST_REPOSITORY=Hailey-Ross/rusty-trucks python -c "import updater; print(updater.REPO)"` prints the fork.
- First fork run: the `early-alpha-1` prerelease of `hails-additions` on `Hailey-Ross/rusty-trucks`.

## Open questions

- Upstream may prefer the repository-aware updater as a small separate PR; the fork-only publish step stays
  fork-only.
