# Updates

**Updated:** 2026-10-12 (version 0.10.1)

OpenPremier updates itself from its GitHub releases
(`crates/op-application/src/update.rs`).

## When it looks

- When the program starts, if Preferences > Updates is on (the default). Nothing is shown
  when there is no newer version, the check fails, or the newest version was skipped.
- With Help > Check for Updates..., which always shows the result.

The check reads `https://api.github.com/repos/xXKuroiKenshiXx/OpenPremier/releases` (one
request, no account). Prereleases count, since every OpenPremier version so far is one; drafts do
not. The newest version above the running one is offered, with the start of its release notes.

## Installing

Download and Install fetches the package for the system (`-windows-x64.zip`,
`-x86_64.AppImage` or `-macos-arm64.dmg` / `-macos-x86_64.dmg`) into the cache folder
(`Updates`), in the background with a progress bar, and checks its SHA-256 against the release's
`SHA256SUMS.txt`; a mismatch is reported and the file deleted. The update is installed when the
program closes, after the usual offer to save (Restart and Update closes it right away):

- **Windows:** the ZIP is extracted next to the download. The new `OpenPremier.exe` is started
  from there with `--finish-update <program folder> <process id>`; it waits for the old process
  to end, copies the package over the program folder (files that are not in the package, such as
  `portable.txt` or `UserData`, stay), deletes the old program and starts the new one. If the
  copy fails the old program file is put back and started.
- **Linux:** the downloaded AppImage replaces the running one at the same path, so shortcuts keep
  working, and is started.
- **macOS:** the downloaded disk image is opened to drag OpenPremier to Applications.

A copy that was not installed from a release package (a development build, or Linux without
`APPIMAGE`) cannot replace itself and only links to the releases page.

## Tests

`cargo xtask cargo test -p op-application --lib update` covers version comparison, choosing the
release and its package, reading checksums, extracting (entries outside the folder are refused)
and copying over an installed folder. With `-- --ignored` it also reads the real releases and
downloads and checks the published package.
