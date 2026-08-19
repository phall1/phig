//! Unix signal monitoring owned by the terminal driver.

use std::io;

#[cfg(unix)]
pub(super) struct SignalMonitor {
    receiver: std::sync::mpsc::Receiver<i32>,
    handle: signal_hook::iterator::Handle,
    worker: Option<std::thread::JoinHandle<()>>,
}

#[cfg(unix)]
impl SignalMonitor {
    pub(super) fn new() -> io::Result<Self> {
        use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM, SIGTSTP};
        use signal_hook::iterator::Signals;

        let mut signals = Signals::new([SIGINT, SIGTERM, SIGHUP, SIGTSTP])?;
        let handle = signals.handle();
        let (sender, receiver) = std::sync::mpsc::sync_channel(8);
        let worker = std::thread::spawn(move || {
            for signal in signals.forever() {
                if sender.send(signal).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            receiver,
            handle,
            worker: Some(worker),
        })
    }

    pub(super) fn try_recv(&self) -> Option<i32> {
        self.receiver.try_recv().ok()
    }
}

#[cfg(unix)]
impl Drop for SignalMonitor {
    fn drop(&mut self) {
        self.handle.close();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
