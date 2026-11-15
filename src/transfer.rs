use crate::state::{Host, Upload};
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

pub struct Job {
    pub host: Host,
    pub sources: Vec<PathBuf>,
    pub temporary: Option<tempfile::TempDir>,
    pub cancel: Arc<AtomicBool>,
}
pub enum Event {
    Status(String, String),
    Uploaded(Upload, Vec<u8>),
    Finished(String, Vec<String>, Vec<String>),
}
pub fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
fn command(program: &str) -> Command {
    let mut c = Command::new(program);
    c.args([
        "-oBatchMode=yes",
        "-oStrictHostKeyChecking=yes",
        "-oConnectTimeout=10",
        "-oServerAliveInterval=10",
        "-oServerAliveCountMax=2",
        "-oForwardAgent=no",
        "-oClearAllForwardings=yes",
    ]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
    }
    c
}
fn run(mut c: Command, cancel: &AtomicBool) -> Result<String, String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("Cancelled".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0);
    }
    let mut child = c
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            format!("Cannot start OpenSSH: {e}. Install OpenSSH 9+ and ensure ssh/scp are on PATH.")
        })?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    // Drain both pipes concurrently so a verbose SSH error cannot deadlock a transfer.
    let read = |mut pipe: Box<dyn Read + Send>| {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let mut buffer = [0; 8192];
            while let Ok(n) = pipe.read(&mut buffer) {
                if n == 0 {
                    break;
                }
                let keep = n.min(65536usize.saturating_sub(bytes.len()));
                bytes.extend_from_slice(&buffer[..keep]);
            }
            bytes
        })
    };
    let out = read(Box::new(stdout));
    let err = read(Box::new(stderr));
    let start = Instant::now();
    let status = loop {
        if cancel.load(Ordering::Relaxed) || start.elapsed() > Duration::from_secs(300) {
            #[cfg(unix)]
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                let _ = Command::new("taskkill")
                    .creation_flags(0x08000000)
                    .args(["/F", "/T", "/PID", &child.id().to_string()])
                    .output();
            }
            let _ = child.kill();
            let _ = child.wait();
            return Err(
                "Cancelled or transfer exceeded five minutes; a remote partial file may remain."
                    .into(),
            );
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        thread::sleep(Duration::from_millis(50));
    };
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    if !status.success() {
        return Err(format!(
            "{}\nCheck SSH login and host trust in Terminal first.",
            String::from_utf8_lossy(&stderr).trim()
        ));
    }
    String::from_utf8(stdout).map_err(|_| "Remote response is not UTF-8".into())
}
fn ssh(host: &Host, script: &str, cancel: &AtomicBool) -> Result<String, String> {
    let mut c = command("ssh");
    c.args(["-T", "-oRemoteCommand=none", "--", &host.target, script]);
    run(c, cancel)
}
pub fn prepare_script(folder: &str) -> String {
    let folder = if let Some(tail) = folder.strip_prefix("~/") {
        format!("\"$HOME\"/{}", quote(tail))
    } else {
        quote(folder)
    };
    format!("umask 077; mkdir -p {folder} && cd {folder} && printf '\\nSSHAWTY_PATH=' && pwd -P")
}
fn check_openssh() -> Result<(), String> {
    let mut c = Command::new("ssh");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
    }
    let out = c
        .arg("-V")
        .output()
        .map_err(|e| format!("Install OpenSSH 9+ (ssh/scp on PATH): {e}"))?;
    let version = String::from_utf8_lossy(&out.stderr);
    let major = version
        .strip_prefix("OpenSSH_for_Windows_")
        .or_else(|| version.strip_prefix("OpenSSH_"))
        .and_then(|s| s.split('.').next())
        .and_then(|s| s.parse::<u32>().ok());
    if major.is_some_and(|v| v >= 9) {
        Ok(())
    } else {
        Err(
            "OpenSSH 9+ is required for SFTP-backed scp. Upgrade your system OpenSSH client."
                .into(),
        )
    }
}
fn prepare(host: &Host, cancel: &AtomicBool) -> Result<String, String> {
    host.validate()?;
    let output = ssh(host, &prepare_script(&host.folder), cancel)?;
    let path = output
        .lines()
        .find_map(|l| l.strip_prefix("SSHAWTY_PATH="))
        .ok_or("Remote folder could not be resolved")?;
    if !path.starts_with('/') || path.chars().any(char::is_control) {
        return Err("Invalid remote path".into());
    }
    Ok(path.trim_end_matches('/').to_owned())
}
fn upload(
    host: &Host,
    source: &Path,
    folder: &str,
    cancel: &AtomicBool,
) -> Result<(Upload, Vec<u8>), String> {
    let meta = source.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() > 50 * 1024 * 1024 {
        return Err("Choose an image file smaller than 50 MiB.".into());
    }
    // Snapshot once: preview and upload must describe the same bytes, with private permissions.
    let filename = source_name(source);
    let mut staged = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
    let input = std::fs::File::open(source).map_err(|e| e.to_string())?;
    let size = std::io::copy(&mut input.take(50 * 1024 * 1024 + 1), &mut staged)
        .map_err(|e| e.to_string())?;
    if size > 50 * 1024 * 1024 {
        return Err("Image exceeds 50 MiB".into());
    }
    let source = staged.path();
    let mut reader = image::ImageReader::open(source)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let format = reader.format().ok_or("Unknown image format")?;
    let ext = match format {
        image::ImageFormat::Png => "png",
        image::ImageFormat::Jpeg => "jpg",
        image::ImageFormat::WebP => "webp",
        _ => return Err("Supported formats: PNG, JPEG, WebP".into()),
    };
    reader.limits(image::Limits::default());
    let img = reader.decode().map_err(|e| e.to_string())?;
    let mut thumb = std::io::Cursor::new(Vec::new());
    img.thumbnail(160, 100)
        .write_to(&mut thumb, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    drop(img);
    let id = uuid::Uuid::new_v4().to_string();
    let path = format!("{folder}/screenshot-{id}.{ext}");
    let partial = format!("{folder}/.sshawty-{id}.part");
    let source = source.canonicalize().map_err(|e| e.to_string())?;
    let mut c = command("scp");
    c.arg("-B")
        .arg("--")
        .arg(&source)
        .arg(format!("{}:{partial}", host.target));
    let result = run(c, cancel).and_then(|_| {
        // Hard-link publication is atomic and fails if the final name already exists.
        ssh(
            host,
            &format!(
                "chmod 600 {p} && ln {p} {f} && rm -f {p}",
                p = quote(&partial),
                f = quote(&path)
            ),
            cancel,
        )
    });
    if let Err(e) = result {
        let cleanup = AtomicBool::new(false);
        let _ = ssh(host, &format!("rm -f {}", quote(&partial)), &cleanup);
        return Err(e);
    }
    Ok((
        Upload {
            id,
            host_id: host.id.clone(),
            host_name: host.name.clone(),
            target: host.target.clone(),
            filename,
            path,
            time: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        },
        thumb.into_inner(),
    ))
}
fn source_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}
pub fn worker(ctx: eframe::egui::Context) -> (mpsc::Sender<Job>, mpsc::Receiver<Event>) {
    let (jobs_tx, jobs_rx) = mpsc::channel::<Job>();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for job in jobs_rx {
            let _keep_alive = job.temporary;
            let mut paths = vec![];
            let mut errors = vec![];
            match check_openssh().and_then(|_| prepare(&job.host, &job.cancel)) {
                Err(e) => errors.push(e),
                Ok(folder) => {
                    for source in job.sources {
                        if job.cancel.load(Ordering::Relaxed) {
                            errors.push("Cancelled".into());
                            break;
                        }
                        let _ = tx.send(Event::Status(
                            job.host.id.clone(),
                            format!("Uploading {} → {}", source_name(&source), job.host.name),
                        ));
                        ctx.request_repaint();
                        match upload(&job.host, &source, &folder, &job.cancel) {
                            Ok((record, thumb)) => {
                                paths.push(record.path.clone());
                                let _ = tx.send(Event::Uploaded(record, thumb));
                            }
                            Err(e) => errors.push(format!("{}: {e}", source_name(&source))),
                        }
                        ctx.request_repaint();
                    }
                }
            }
            let _ = tx.send(Event::Finished(job.host.id, paths, errors));
            ctx.request_repaint();
        }
    });
    (jobs_tx, rx)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quotes_are_shell_safe() {
        assert_eq!(quote("a'b"), "'a'\\''b'");
        #[cfg(unix)]
        {
            let folder = "/tmp/a'$(echo INJECTED); path";
            let script = format!("printf %s {}", quote(folder));
            let out = Command::new("sh").args(["-c", &script]).output().unwrap();
            assert_eq!(String::from_utf8(out.stdout).unwrap(), folder);
        }
    }
    #[test]
    fn home_only_expands_at_start() {
        let s = prepare_script("~/$(touch bad)/'x'");
        assert!(s.contains("\"$HOME\"/'$(touch bad)/'"));
    }
}

#[cfg(all(test, unix))]
mod integration_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn fixture_child() {
        let Ok(root) = std::env::var("SSHAWTY_FIXTURE") else {
            return;
        };
        let dir = PathBuf::from(root);
        let host = Host {
            name: "Fixture".into(),
            target: "fixture".into(),
            folder: dir
                .join("remote 'quotes' $(literal) ☃")
                .to_string_lossy()
                .into(),
            ..Default::default()
        };
        let cancel = AtomicBool::new(false);
        check_openssh().unwrap();
        let folder = prepare(&host, &cancel).unwrap();
        let source = dir.join("source.png");
        image::RgbaImage::from_pixel(4, 4, image::Rgba([100, 120, 140, 255]))
            .save(&source)
            .unwrap();
        let (a, thumb) = upload(&host, &source, &folder, &cancel).unwrap();
        let (b, _) = upload(&host, &source, &folder, &cancel).unwrap();
        assert_ne!(a.path, b.path);
        assert_eq!(
            std::fs::read(&a.path).unwrap(),
            std::fs::read(&source).unwrap()
        );
        assert!(!thumb.is_empty());
        assert_eq!(
            std::fs::metadata(&a.path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(a.filename, "source.png");
        std::fs::write(dir.join("fail-copy"), "").unwrap();
        assert!(upload(&host, &source, &folder, &cancel).is_err());
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 2);
        cancel.store(true, Ordering::Relaxed);
        assert!(upload(&host, &source, &folder, &cancel).is_err());
        let mut fail = Command::new("sh");
        fail.args(["-c", "echo denied >&2; exit 1"]);
        assert!(
            run(fail, &AtomicBool::new(false))
                .unwrap_err()
                .contains("denied")
        );
    }
    #[test]
    fn cancellation_stops_active_process_group() {
        let cancel = Arc::new(AtomicBool::new(false));
        let signal = cancel.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            signal.store(true, Ordering::Relaxed);
        });
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 30 & wait"]);
        let start = Instant::now();
        assert!(run(command, &cancel).is_err());
        assert!(start.elapsed() < Duration::from_secs(3));
    }
    #[test]
    fn transfer_flow_with_controlled_openssh_commands() {
        let temp = tempfile::tempdir().unwrap();
        let ssh = temp.path().join("ssh");
        let scp = temp.path().join("scp");
        std::fs::write(&ssh, "#!/bin/sh\nif [ \"$1\" = -V ]; then echo OpenSSH_10.0 >&2; exit 0; fi\nfor arg do last=$arg; done\nexec /bin/sh -c \"$last\"\n").unwrap();
        std::fs::write(&scp, "#!/bin/sh\nfor arg do previous=$last; last=$arg; done\ndestination=${last#*:}\nif [ -f \"$SSHAWTY_FIXTURE/fail-copy\" ]; then touch \"$destination\"; echo 'simulated disconnect' >&2; exit 1; fi\ncp \"$previous\" \"$destination\"\n").unwrap();
        for file in [&ssh, &scp] {
            std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let out = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "transfer::integration_tests::fixture_child",
                "--nocapture",
            ])
            .env("SSHAWTY_FIXTURE", temp.path())
            .env("PATH", format!("{}:/usr/bin:/bin", temp.path().display()))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    #[test]
    #[ignore = "Requires SSHAWTY_TEST_HOST with preconfigured SSH trust and keys"]
    fn real_openssh_upload() {
        let target = std::env::var("SSHAWTY_TEST_HOST").expect("Set SSHAWTY_TEST_HOST");
        let folder = format!("/tmp/sshawty-test-{} 'quoted'", uuid::Uuid::new_v4());
        let host = Host {
            name: "Live test".into(),
            target,
            folder: folder.clone(),
            ..Default::default()
        };
        let cancel = AtomicBool::new(false);
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("test screenshot.png");
        image::RgbaImage::from_pixel(8, 8, image::Rgba([30, 100, 220, 255]))
            .save(&source)
            .unwrap();
        let resolved = prepare(&host, &cancel).unwrap();
        let result = upload(&host, &source, &resolved, &cancel);
        let (record, _) = result.expect("Live upload failed");
        let remote_hex = ssh(
            &host,
            &format!("od -An -tx1 {}", quote(&record.path)),
            &cancel,
        )
        .unwrap();
        let remote_hex: String = remote_hex.chars().filter(|c| !c.is_whitespace()).collect();
        let local_hex = std::fs::read(&source)
            .unwrap()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let cleanup = ssh(
            &host,
            &format!("rm -f {} && rmdir {}", quote(&record.path), quote(&folder)),
            &cancel,
        );
        assert_eq!(remote_hex, local_hex);
        cleanup.unwrap();
    }
}
