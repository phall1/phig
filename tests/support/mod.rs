use std::{
    ops::{Deref, DerefMut},
    process::{Command, Stdio},
    time::Duration,
};

type ChildHandle = Box<dyn portable_pty::Child + Send + Sync>;

/// portable-pty starts a new session/process group. Clean up the entire test
/// group on unwinding, including command-substitution children of a shell.
pub struct PtyChild {
    child: ChildHandle,
    group: Option<u32>,
}

impl PtyChild {
    pub fn new(child: ChildHandle) -> Self {
        let group = child.process_id();
        Self { child, group }
    }
}

impl Deref for PtyChild {
    type Target = ChildHandle;
    fn deref(&self) -> &Self::Target {
        &self.child
    }
}

impl DerefMut for PtyChild {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.child
    }
}

impl Drop for PtyChild {
    fn drop(&mut self) {
        // The leader may have exited while background descendants still own
        // the slave descriptors. Keep the original group identity until drop.
        if let Some(pid) = self.group {
            let _ = Command::new("kill")
                .args(["-KILL", "--", &format!("-{pid}")])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Readiness is a functional assertion, not the release latency benchmark.
/// CI retains its original deadline; busy development hosts opt in explicitly.
pub fn readiness_timeout(timeout: Duration) -> Duration {
    let multiplier = std::env::var("PHIG_TEST_READINESS_MULTIPLIER")
        .map(|value| {
            value
                .parse::<u32>()
                .expect("PHIG_TEST_READINESS_MULTIPLIER must be an integer")
        })
        .unwrap_or(1);
    assert!(
        (1..=10).contains(&multiplier),
        "PHIG_TEST_READINESS_MULTIPLIER must be 1..=10"
    );
    timeout.saturating_mul(multiplier)
}
