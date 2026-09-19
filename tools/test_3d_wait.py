#!/usr/bin/env python3
"""Host regression test of the production completion wait (no GPU required)."""
from pathlib import Path
import subprocess
import tempfile

root = Path(__file__).resolve().parents[1]
source = (root / 'src/core/graphics/gpu_renderer.rs').read_text()
start = source.index("fn wait_for_3d_ready<'a>(")
end = source.index('\nfn record_3d_wait(', start)
helper = source[start:end]
# The render path must not clear the work-request flag before worker completion.
start = source.index('let wait_started = Instant::now();')
end = source.index('self.renderer_3d.render(', start)
path = source[start:end]
assert path.index('wait_for_3d_ready(') < path.index('self.rendering_3d = false;')

tests = r'''
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::Duration;
use std::thread;

#[test]
fn already_ready_does_not_wait() {
    let ready = Mutex::new(true);
    let cv = Condvar::new();
    let guard = wait_for_3d_ready(&cv, ready.lock().unwrap(), Duration::from_millis(10), || panic!("unexpected timeout"));
    assert!(*guard);
}

#[test]
fn timeouts_and_notifications_cannot_publish_unfinished_data() {
    let pair = Arc::new((Mutex::new(false), Condvar::new()));
    let (timeout_tx, timeout_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let waiter_pair = Arc::clone(&pair);
    let waiter = thread::spawn(move || {
        let (lock, cv) = &*waiter_pair;
        let guard = wait_for_3d_ready(cv, lock.lock().unwrap(), Duration::from_millis(10), || {
            timeout_tx.send(()).unwrap();
        });
        assert!(*guard);
        done_tx.send(()).unwrap();
    });
    // Simulate preparation taking longer than multiple timeout intervals.
    timeout_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(done_rx.try_recv(), Err(mpsc::TryRecvError::Empty));
    // A wakeup alone does not transfer ownership; the predicate is still false.
    pair.1.notify_all();
    timeout_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(done_rx.try_recv(), Err(mpsc::TryRecvError::Empty));
    *pair.0.lock().unwrap() = true;
    pair.1.notify_all();
    done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    waiter.join().unwrap();
}

#[test]
fn completed_worker_is_observed_on_each_new_frame() {
    let pair = Arc::new((Mutex::new(false), Condvar::new()));
    for _ in 0..20 {
        *pair.0.lock().unwrap() = false;
        let worker_pair = Arc::clone(&pair);
        let worker = thread::spawn(move || {
            *worker_pair.0.lock().unwrap() = true;
            worker_pair.1.notify_one();
        });
        let guard = wait_for_3d_ready(&pair.1, pair.0.lock().unwrap(), Duration::from_millis(10), || {});
        assert!(*guard);
        drop(guard);
        worker.join().unwrap();
    }
}
'''
with tempfile.TemporaryDirectory() as tmp:
    src = Path(tmp) / 'wait.rs'
    exe = Path(tmp) / 'wait_tests'
    src.write_text(helper + tests)
    subprocess.run(['rustc', '+stable', '--edition=2021', '--test', str(src), '-o', str(exe)], check=True)
    subprocess.run([str(exe)], check=True)
