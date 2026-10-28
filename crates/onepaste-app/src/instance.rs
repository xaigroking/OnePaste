//! One program per data directory. Two programs writing the same databases can damage them, and a second start must not
//! make a second copy that nobody can see (the window is hidden, the tray icon is easy to miss). So the data directory has
//! a lock file that the running program holds for as long as it lives (the system releases it when the process ends, even
//! after a crash); a program that cannot take it asks the one that holds it to show its window and ends. The lock does not
//! depend on the path of the program, so copies of it in different places (macOS runs a downloaded app from a different
//! random place each time) are held back as well.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

const LOCK_FILE: &str = "onepaste.lock";
const REQUEST_FILE: &str = "onepaste.request";

/// What a second start asks of the running program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    /// Bring the window to the front.
    Show,
    /// Quit.
    Quit,
}

/// Held for as long as this process is the one program using the data directory.
pub struct InstanceLock {
    _file: File,
}

impl InstanceLock {
    /// Takes the lock. `Ok(None)` means another program holds it.
    pub fn acquire(dir: &Path) -> io::Result<Option<InstanceLock>> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(LOCK_FILE);
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path)?;
            // SAFETY: the descriptor is valid for as long as `file` lives.
            let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            if rc == 0 {
                return Ok(Some(InstanceLock { _file: file }));
            }
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::WouldBlock {
                return Ok(None);
            }
            Err(err)
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // Sharing mode 0: nobody else can open the file while this handle is open
            match OpenOptions::new().read(true).write(true).create(true).truncate(false).share_mode(0).open(&path) {
                Ok(file) => Ok(Some(InstanceLock { _file: file })),
                // ERROR_SHARING_VIOLATION and ERROR_ACCESS_DENIED: the file is open in another process
                Err(e) if matches!(e.raw_os_error(), Some(32) | Some(5)) => Ok(None),
                Err(e) => Err(e),
            }
        }
    }
}

fn request_path(dir: &Path) -> PathBuf {
    dir.join(REQUEST_FILE)
}

/// Asks the running program to do something. The running program looks for the request a few times a second.
pub fn send_request(dir: &Path, request: Request) -> io::Result<()> {
    let text = match request {
        Request::Show => "show",
        Request::Quit => "quit",
    };
    std::fs::write(request_path(dir), text)
}

/// Takes the request that is waiting, if there is one. A quit request wins over a show request made before it.
pub fn take_request(dir: &Path) -> Option<Request> {
    let path = request_path(dir);
    let text = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    match text.trim() {
        "quit" => Some(Request::Quit),
        "show" => Some(Request::Show),
        _ => None,
    }
}
