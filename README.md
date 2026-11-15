# sshawty

A small Rust desktop app for sending screenshots to remote Codex/Claude sessions over SSH. Add hosts, drop an image onto a host card, then paste the copied absolute path into your remote chat.

<img src="https://raw.githubusercontent.com/lkuich/sshawty/main/docs/screenshots/hosts.jpg" alt="sshawty host overview with two drop targets, icon controls, and recent upload history" width="960">

<details>
<summary>Focused view</summary>

Focus one host with a double-click or the expand icon. Return with **All hosts**, Escape, or another double-click.

<img src="https://raw.githubusercontent.com/lkuich/sshawty/main/docs/screenshots/focused.jpg" alt="sshawty focused view with a large drop target and a clear All hosts back button" width="960">

</details>

*macOS screenshots shown with sample hosts and upload history.*

## Download

Download the archive for your platform from [GitHub Releases](https://github.com/lkuich/sshawty/releases/latest):

| Platform | Download |
| --- | --- |
| macOS, Apple Silicon | `sshawty-macos-arm64.zip` |
| macOS, Intel | `sshawty-macos-x86_64.zip` |
| Windows, x64 | `sshawty-windows-x86_64.zip` |
| Linux, x64 | `sshawty-linux-x86_64.tar.gz` |

Extract the archive and launch the app. Linux builds target Ubuntu 24.04 or compatible systems and require X11/XWayland. macOS builds require macOS 12+ and are ad-hoc signed, not notarized. Windows builds are unsigned. All platforms need OpenSSH 9+ installed. Checksums are included in each release.

For development builds, open a successful [Actions run](https://github.com/lkuich/sshawty/actions/workflows/check.yml) and download its platform artifact (GitHub sign-in required).

## Run from source

On macOS, open `dist/sshawty.app`, or build it with:

```sh
./scripts/package-macos.sh
open dist/sshawty.app
```

On Windows and Linux:

```sh
cargo run --release --locked
```

Rust 1.95 is pinned in `rust-toolchain.toml`. GitHub Actions builds macOS, Windows, and Linux artifacts. The macOS bundle is locally ad-hoc signed, not notarized for public distribution.

System requirements: OpenSSH **9 or newer**, with both `ssh` and `scp` on PATH; OpenGL-capable desktop. Linux requires **X11 or XWayland** (`DISPLAY` must be set); native Wayland drag/drop is not supported by the pinned winit backend. The UI deliberately uses X11 there, while clipboard support includes Wayland data-control where available.

## Use

1. Verify `ssh your-host` works in Terminal. Accept the host fingerprint only after verifying it, and load encrypted keys into your SSH agent.
2. Add a display name, an SSH alias or `user@host`, and a destination such as `~/screenshots`. Set ports, identity files, and jump hosts in your normal SSH config. Bracket IPv6 addresses.
3. Click a host card in the grid to select it and show only its recent uploads. Click it again to clear the selection and show all uploads. Drop PNG, JPEG, or WebP files on a card. On macOS, floating screenshot previews are received through AppKit file promises. Use the expand icon or double-click a host card to focus it. Double-click again, use **All hosts**, or press Escape to return. Icon buttons have tooltips and accessibility labels.
4. Select a host and use **Paste image**, or Cmd+V / Ctrl+V outside a text field, for clipboard screenshots.
5. Successful uploads copy absolute remote paths. A multi-file batch copies one path per line. Click any history path to copy it again.

Uploads run sequentially in the background. Each uses a unique remote filename and a private local snapshot. A temporary remote file is published without overwriting an existing destination. Existing source files are never modified. Destinations must be Linux/macOS SSH servers with a POSIX-compatible login shell, SFTP, and hard-link support on the destination filesystem.

Limits: 50 MiB per file, the image decoder's memory limit, 256 MiB for clipboard pixel data, five minutes per SSH command. Cancel stops active child processes and skips queued work. Cleanup is best effort after network loss; a hidden `.sshawty-*.part` file may remain remotely. A disconnect during final publication can leave a complete file without a success record; retry uses a new name.

History and 160×100-or-smaller thumbnails persist until you clear them. Clearing history or removing a host never deletes remote screenshots. No full-resolution local image copies are retained after a batch finishes.

## Local data

- macOS: `~/Library/Application Support/sshawty/`
- Linux: `$XDG_DATA_HOME/sshawty/` (normally `~/.local/share/sshawty/`)
- Windows: `%LOCALAPPDATA%\sshawty\data\`

`state.json` stores versioned host settings, selection, and completed uploads; `thumbnails/` holds previews. Writes replace the JSON atomically. An unreadable/unsupported state file is preserved and disables saving for that session. Move it aside or repair it before restarting. Thumbnails contain screenshot content, so treat the app-data directory accordingly.

## App icon

The app uses the same mint upload mark in its interface, macOS bundle, Windows executable, and window/taskbar icon. The editable source is `assets/icon.svg`; regenerate the checked-in PNG, ICO, and ICNS files with `scripts/generate-icons.sh` on macOS with ImageMagick installed. Normal builds use the checked-in files.

Windows builds need the Windows SDK resource compiler (`rc.exe`) with MSVC, or MinGW-w64 resource tools when cross-compiling. GitHub runners include the required Windows SDK.

## Checks

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

Tests cover validation, persistence corruption, shell quoting, unique upload paths, byte preservation, permissions, cancellation, errors, and partial cleanup. On macOS a native AppKit harness also exercises saved-file drops and simulated screenshot promises with both immediate and deferred delivery. The controlled-command test executes actual POSIX remote scripts against an isolated temporary directory, using fake SSH/SCP launchers.

An optional real OpenSSH test uploads a generated 8×8 PNG to a unique `/tmp/sshawty-test-*` directory, compares the remote bytes, and removes the test file and directory:

```sh
SSHAWTY_TEST_HOST=user@host cargo test real_openssh_upload -- --ignored
```

CI checks formatting, Clippy, tests, and release builds on all supported platforms, including native AppKit file and screenshot-promise drop tests on macOS. Real SSH transfers, floating screenshot previews, and the macOS interface have also been checked locally. Windows and Linux clipboard/drop behavior still need manual GUI testing.

No password storage, remote folder browser, tray/global shortcuts, automatic remote cleanup, updater, or telemetry.

## Publishing a release

Every main-branch push and pull request builds and tests all four platform archives. To publish a release, update the version in `Cargo.toml` (and `Cargo.lock`), commit it, then push a matching tag:

```sh
git tag v0.1.4
git push origin v0.1.4
```

The workflow checks that the tag matches the package version, waits for all platform checks and builds, then publishes the archives and SHA-256 checksums to GitHub Releases. The repository's built-in `GITHUB_TOKEN` is sufficient; no additional release secret is needed.
