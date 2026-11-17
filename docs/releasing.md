# Releasing

A release has two parts: the `whatsapp-mcp` binaries, and then the Claude Code plugin that downloads them. The plugin pins the binaries' checksums, which exist only once the release is built, so the plugin follows in a second pull request.

## 1. The binaries

1. On a `release/X.Y.Z` branch from `dev-main`:
   - set `version` in the workspace `Cargo.toml` (and run `cargo build`, so `Cargo.lock` follows);
   - turn the CHANGELOG's "Unreleased" section into `## X.Y.Z`.
2. Pull request to `dev-main`, and merge it when CI passes.
3. Bring `main` up to `dev-main` (`main` is the latest release, see [Branches](../README.md#branches)).
4. Tag `main`: `git tag vX.Y.Z && git push origin vX.Y.Z`. The [release workflow](../.github/workflows/release.yml) builds the five binaries, checks each one's `--version` against the tag, and attaches them with `SHA256SUMS` to a **draft** release.
5. Check the draft: download the binary for your platform, check it against `SHA256SUMS`, run it. Then publish the release.

## 2. The plugin

The plugin keeps running the previous binaries until this step.

1. On a `release/plugin-X.Y.Z` branch from `dev-main`:

   ```sh
   scripts/bump-plugin.sh X.Y.Z
   ```

   This fetches the published release's `SHA256SUMS` (it refuses a draft, and a list with other files than before) and sets:
   - `plugin/scripts/SHA256SUMS`: the new checksums;
   - `VERSION` in `plugin/scripts/launch.sh`: the release the launcher downloads;
   - `version` in `plugin/.claude-plugin/plugin.json`: how Claude Code sees that there is an update;
   - the release link in `plugin/README.md`.

   Then it runs the launcher once, which downloads this platform's binary and checks it against the new checksums.
2. Add a CHANGELOG line, pull request to `dev-main`, and merge it when CI passes. CI checks that `plugin.json` and the launcher name the same version, and the launcher job downloads and checks the binary on every platform.
3. Bring `main` up to `dev-main` again. The marketplace is read from `main`, so users see the update from here on.

## What users do

```sh
claude plugin update whatsapp-mcp@erwinvaneijk
```

Then:
1. Start a new Claude Code session. The launcher downloads the new binary into the plugin's data folder, in place of the old one.
2. Restart the bridge on it: `whatsapp-mcp service install` (it stops the running bridge first). The service runs the file in the plugin's data folder, so it picks up the new binary.

The bridge keeps running the old binary until it restarts. The plugin's [README](../plugin/README.md#updating) says the same.
