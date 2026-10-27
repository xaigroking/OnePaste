//! Test-only crash points. In a build with the `fault-injection` feature, a process whose `ONEPASTE_CRASH_AT` names
//! a point is terminated there at once: no destructors run and no buffers are flushed. Without the feature, a crash
//! point does nothing and the code that reads the environment is not even compiled.

#[cfg(feature = "fault-injection")]
pub fn crash_point(name: &str) {
    if std::env::var("ONEPASTE_CRASH_AT").as_deref() == Ok(name) {
        std::process::abort();
    }
}

#[cfg(not(feature = "fault-injection"))]
#[inline(always)]
pub fn crash_point(_name: &str) {}
