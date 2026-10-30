//! One program per data directory.

use onepaste_app::instance::{send_request, take_request, InstanceLock, Request};
use std::path::PathBuf;

fn dir(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("onepaste-instance-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    p
}

#[test]
fn a_second_program_cannot_take_the_lock_until_the_first_lets_go() {
    let d = dir("lock");
    let first = InstanceLock::acquire(&d).unwrap().expect("the first takes the lock");
    assert!(InstanceLock::acquire(&d).unwrap().is_none(), "a second one is held back");
    assert!(InstanceLock::acquire(&d).unwrap().is_none(), "and again");
    drop(first);
    assert!(InstanceLock::acquire(&d).unwrap().is_some(), "after the first has ended the lock is free");
}

#[cfg(unix)]
#[test]
fn dropping_the_owner_releases_the_lock_while_a_forked_child_is_alive() {
    use std::io::Read;
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;

    let d = dir("inherited");
    let held = InstanceLock::acquire(&d).unwrap().unwrap();
    let (mut parent, child) = UnixStream::pair().unwrap();
    // SAFETY: the child uses only async-signal-safe libc calls and never runs Rust destructors.
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0);
    if pid == 0 {
        unsafe {
            libc::close(parent.as_raw_fd());
            let ready = [1u8];
            libc::write(child.as_raw_fd(), ready.as_ptr().cast(), 1);
            loop { libc::pause(); }
        }
    }
    drop(child);
    let mut ready = [0u8];
    parent.read_exact(&mut ready).unwrap();
    drop(held);
    // This must work before the child closes its inherited copy, with no retries or sleep.
    let reacquired = InstanceLock::acquire(&d);
    // Reap the child before asserting, so the expected pre-fix failure cannot leave it behind.
    unsafe { libc::kill(pid, libc::SIGKILL); }
    let mut status = 0;
    loop {
        let waited = unsafe { libc::waitpid(pid, &mut status, 0) };
        if waited == pid { break; }
        assert_eq!(std::io::Error::last_os_error().kind(), std::io::ErrorKind::Interrupted);
    }
    assert!(reacquired.unwrap().is_some(), "the dropped owner must release the lock even during a concurrent fork");
}

#[test]
fn directories_are_independent_and_requests_are_taken_once() {
    let (a, b) = (dir("a"), dir("b"));
    let _la = InstanceLock::acquire(&a).unwrap().unwrap();
    let _lb = InstanceLock::acquire(&b).unwrap().expect("another data directory has its own lock");
    assert_eq!(take_request(&a), None);
    send_request(&a, Request::Show).unwrap();
    assert_eq!(take_request(&b), None, "the request is for the other directory");
    assert_eq!(take_request(&a), Some(Request::Show));
    assert_eq!(take_request(&a), None, "it was taken");
    send_request(&a, Request::Show).unwrap();
    send_request(&a, Request::Quit).unwrap();
    assert_eq!(take_request(&a), Some(Request::Quit), "the latest request counts");
}

/// The second program really is another process: it must be held back and the data files must not be touched.
#[test]
fn a_second_process_is_held_back_by_a_lock_held_in_this_one() {
    if std::env::var("ONEPASTE_INSTANCE_CHILD").is_ok() {
        // The child: report whether it got the lock
        let d = PathBuf::from(std::env::var("ONEPASTE_INSTANCE_DIR").unwrap());
        let got = InstanceLock::acquire(&d).unwrap().is_some();
        std::process::exit(if got { 10 } else { 11 });
    }
    let d = dir("process");
    let held = InstanceLock::acquire(&d).unwrap().unwrap();
    let exe = std::env::current_exe().unwrap();
    let run = || {
        std::process::Command::new(&exe)
            .args(["a_second_process_is_held_back_by_a_lock_held_in_this_one", "--exact", "--nocapture"])
            .env("ONEPASTE_INSTANCE_CHILD", "1")
            .env("ONEPASTE_INSTANCE_DIR", &d)
            .status()
            .unwrap()
            .code()
    };
    assert_eq!(run(), Some(11), "the other process did not get the lock");
    assert!(!d.join("onepaste.db").exists() && !d.join("history.db").exists(), "and it touched no database");
    drop(held);
    assert_eq!(run(), Some(10), "once the lock is free the other process gets it");
}
