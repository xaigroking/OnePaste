//! Login registration transitions. State is always read from the operating system, never a preference file.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error { Read, Write, Unsupported, UnstablePath }

impl Error {
    pub fn code(self) -> &'static str {
        match self { Self::Read => "startup_read", Self::Write => "startup_write", Self::Unsupported => "startup_unsupported", Self::UnstablePath => "startup_unstable_path" }
    }
}

pub trait Registration {
    fn read(&self) -> Result<Option<String>, Error>;
    fn write(&self, command: Option<&str>) -> Result<(), Error>;
}

pub fn enabled(port: &impl Registration) -> Result<bool, Error> { Ok(port.read()?.is_some()) }

pub fn set(port: &impl Registration, command: Option<&str>) -> Result<bool, Error> {
    port.write(command)?;
    enabled(port)
}

/// A normal launch from a moved portable package updates an existing registration, without opting a user in.
pub fn repair_existing(port: &impl Registration, command: &str) -> Result<(), Error> {
    if let Some(previous) = port.read()? {
        if previous != command { port.write(Some(command))?; }
    }
    Ok(())
}

pub fn windows_command(exe: &str) -> Result<String, Error> {
    let command = format!("\"{exe}\" --hidden");
    if exe.contains(['"', '\0', '\r', '\n']) || command.encode_utf16().count() > 260 { return Err(Error::UnstablePath); }
    Ok(command)
}

pub fn launch_agent(exe: &str, label: &str) -> Result<String, Error> {
    if exe.contains('\0') || exe.contains("/AppTranslocation/") { return Err(Error::UnstablePath); }
    fn xml(s: &str) -> String { s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;") }
    Ok(format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict><key>Label</key><string>{}</string><key>ProgramArguments</key><array><string>{}</string><string>--hidden</string></array><key>RunAtLoad</key><true/><key>LimitLoadToSessionType</key><string>Aqua</string><key>AssociatedBundleIdentifiers</key><array><string>dev.onepaste.spike</string></array></dict></plist>", xml(label), xml(exe)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    #[derive(Default)]
    struct Fake { value: RefCell<Option<String>>, writes: RefCell<Vec<Option<String>>> }
    impl Registration for Fake {
        fn read(&self) -> Result<Option<String>, Error> { Ok(self.value.borrow().clone()) }
        fn write(&self, value: Option<&str>) -> Result<(), Error> {
            *self.value.borrow_mut() = value.map(str::to_string); self.writes.borrow_mut().push(value.map(str::to_string)); Ok(())
        }
    }
    #[test]
    fn off_by_default_on_off_on_and_external_deletion_are_read_from_the_registration() {
        let io = Fake::default(); assert!(!enabled(&io).unwrap());
        for desired in [Some("new --hidden"), None, Some("new --hidden")] { assert_eq!(set(&io, desired).unwrap(), desired.is_some()); }
        *io.value.borrow_mut() = None; assert!(!enabled(&io).unwrap());
        repair_existing(&io, "newer --hidden").unwrap(); assert!(!enabled(&io).unwrap()); assert_eq!(io.writes.borrow().len(), 3);
    }
    #[test]
    fn relocation_repairs_only_an_existing_registration_and_does_not_rewrite_the_same_path() {
        let io = Fake::default(); repair_existing(&io, "new --hidden").unwrap(); assert!(io.writes.borrow().is_empty());
        *io.value.borrow_mut() = Some("old --hidden".into()); repair_existing(&io, "new --hidden").unwrap();
        assert_eq!(io.read().unwrap().as_deref(), Some("new --hidden")); repair_existing(&io, "new --hidden").unwrap(); assert_eq!(io.writes.borrow().len(), 1);
    }
    #[test]
    fn registration_errors_do_not_look_like_a_successful_disable() {
        struct Broken;
        impl Registration for Broken {
            fn read(&self) -> Result<Option<String>, Error> { Err(Error::Read) }
            fn write(&self, _: Option<&str>) -> Result<(), Error> { Err(Error::Write) }
        }
        assert_eq!(enabled(&Broken), Err(Error::Read)); assert_eq!(set(&Broken, Some("new")), Err(Error::Write));
    }
    #[test]
    fn portable_paths_are_quoted_or_xml_escaped_and_temporary_translocation_is_rejected() {
        assert_eq!(windows_command(r"C:\Portable App\OnePaste.exe").unwrap(), "\"C:\\Portable App\\OnePaste.exe\" --hidden");
        assert_eq!(windows_command("bad\"path"), Err(Error::UnstablePath)); assert_eq!(windows_command(&"x".repeat(260)), Err(Error::UnstablePath));
        let plist = launch_agent("/Applications/A&B <App>/OnePaste", "test.label").unwrap(); assert!(plist.contains("A&amp;B &lt;App&gt;")); assert!(plist.contains("--hidden"));
        assert_eq!(launch_agent("/private/var/AppTranslocation/random/OnePaste", "test"), Err(Error::UnstablePath));
    }
}
