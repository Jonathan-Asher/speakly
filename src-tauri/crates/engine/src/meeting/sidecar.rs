//! Spawning and supervising the ScreenCaptureKit sidecar process.

use std::io::Write;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

pub struct SidecarProc {
    pub child: Child,
    pub stdout: ChildStdout,
    pub stdin: ChildStdin,
}

pub fn spawn_capture(
    sidecar_path: &str,
    bundle_ids: &[String],
    system: bool,
    rate: u32,
) -> Result<SidecarProc, String> {
    let mut cmd = Command::new(sidecar_path);
    if system {
        cmd.arg("--system");
    } else {
        for id in bundle_ids {
            cmd.arg("--bundle-id").arg(id);
        }
    }
    cmd.arg("--rate").arg(rate.to_string());
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("spawn sidecar '{sidecar_path}': {e}"))?;
    let stdout = child.stdout.take().ok_or("sidecar stdout missing")?;
    let stdin = child.stdin.take().ok_or("sidecar stdin missing")?;
    Ok(SidecarProc {
        child,
        stdout,
        stdin,
    })
}

/// Ask the sidecar to stop; escalate to kill if it lingers.
pub fn stop(stdin: &mut ChildStdin, child: &mut Child) {
    let _ = stdin.write_all(b"{\"cmd\":\"stop\"}\n");
    let _ = stdin.flush();
    let deadline = Instant::now() + Duration::from_millis(700);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => break,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}
