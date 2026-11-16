# sshawty

**Screenshots for Codex and Claude Code sessions over SSH.**

When screenshot paste fails over SSH, drop the image onto a host in sshawty. It uploads with SCP and copies the remote path for your chat. Small, native, built in Rust.

<img src="https://raw.githubusercontent.com/lkuich/sshawty/main/docs/demo.gif" alt="Uploading a screenshot through sshawty after image paste fails in a remote Codex session" width="960">

[Download the sharper MP4](https://github.com/lkuich/sshawty/raw/refs/heads/main/docs/demo.mp4)

## Download

Get the latest build from [GitHub Releases](https://github.com/lkuich/sshawty/releases/latest), extract it, and launch the app.

| Platform | Download |
| --- | --- |
| macOS 12+, Apple Silicon | `sshawty-macos-arm64.zip` |
| Windows, x64 | `sshawty-windows-x86_64.zip` |
| Linux, x64 (Ubuntu 24.04 or compatible) | `sshawty-linux-x86_64.tar.gz` |

Requires OpenSSH 9+ (`ssh` and `scp`) and an OpenGL-capable desktop. Linux needs X11/XWayland. macOS builds are ad-hoc signed but not notarized; Windows builds are unsigned.

## Use

1. Make sure `ssh your-host` works with your SSH keys. Add a host with **+**, using an SSH alias or `user@host` and a destination such as `~/screenshots`. Your existing SSH configuration handles ports, keys, and jump hosts.
2. Drop a PNG, JPEG, or WebP onto the host, or select it and press **Cmd+V / Ctrl+V** to upload a clipboard image. Floating macOS screenshot previews work too.
3. Paste the copied remote path into your Codex or Claude chat. Click any path in **Recent uploads** to copy it again.

Click a host to filter its history; click again to show all uploads. Double-click to focus or return to the grid.

Files can be up to 50 MiB. Remote hosts must run Linux/macOS with a POSIX shell, SFTP, and a filesystem supporting hard links. Hosts and history are saved locally; clearing history never deletes remote files.

## Build from source

```sh
cargo run --release --locked
```

The Rust version is pinned in `rust-toolchain.toml`. To create a macOS app bundle, run `./scripts/package-macos.sh`.
