//! Process hardening.

/// Makes sure a crash cannot write the process memory to disk. A core dump holds whatever the process had in memory:
/// the passphrase, the master key and the plaintext of the item being handled. The core size limit is set to zero for
/// good and, on Linux, the process is marked as not dumpable. Call this first thing in `main`.
#[cfg(unix)]
pub fn disable_core_dumps() {
    let zero = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: both calls only change limits and flags of the calling process and take no pointers that outlive the call.
    unsafe {
        libc::setrlimit(libc::RLIMIT_CORE, &zero);
        #[cfg(target_os = "linux")]
        libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0);
    }
}

/// Core dumps of other platforms are outside what this function controls.
#[cfg(not(unix))]
pub fn disable_core_dumps() {}

#[cfg(all(test, unix))]
mod tests {
    #[test]
    fn the_core_size_limit_is_zero_afterwards_and_cannot_be_raised_again() {
        super::disable_core_dumps();
        let mut now = libc::rlimit { rlim_cur: 1, rlim_max: 1 };
        // SAFETY: reads the limit of the calling process into a local variable
        assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_CORE, &mut now) }, 0);
        assert_eq!((now.rlim_cur, now.rlim_max), (0, 0));
    }
}
