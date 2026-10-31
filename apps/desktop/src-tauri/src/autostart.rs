//! Per-user login startup. No installation, elevated rights, or cached enable flag.

use onepaste_platform::autostart::{self, Error, Registration};
use serde::Serialize;
use std::sync::Mutex;

static ACCESS: Mutex<()> = Mutex::new(());
const LABEL: &str = "dev.onepaste.spike.autostart";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info { pub enabled: bool, pub can_enable: bool, pub reason: Option<&'static str> }

fn desired() -> Result<String, Error> {
    let exe = std::env::current_exe().map_err(|_| Error::UnstablePath)?;
    let exe = exe.to_str().ok_or(Error::UnstablePath)?;
    #[cfg(windows)]
    return autostart::windows_command(exe);
    #[cfg(target_os = "macos")]
    return autostart::launch_agent(exe, LABEL);
    #[cfg(not(any(windows, target_os = "macos")))]
    { let _ = exe; Err(Error::Unsupported) }
}

fn snapshot(port: &impl Registration) -> Result<Info, Error> {
    let enabled = autostart::enabled(port)?;
    let reason = desired().err().map(Error::code);
    Ok(Info { enabled, can_enable: reason.is_none(), reason })
}

pub fn get() -> Result<Info, String> {
    let _guard = ACCESS.lock().unwrap();
    snapshot(&native::port()?).map_err(|e| e.code().into())
}

pub fn set(enabled: bool) -> Result<Info, String> {
    let _guard = ACCESS.lock().unwrap();
    let port = native::port()?;
    let command = if enabled { Some(desired().map_err(|e| e.code().to_string())?) } else { None };
    autostart::set(&port, command.as_deref()).map_err(|e| e.code().to_string())?;
    snapshot(&port).map_err(|e| e.code().into())
}

pub fn repair() -> Result<(), String> {
    let _guard = ACCESS.lock().unwrap();
    let port = native::port()?;
    // Translocated downloads must first be moved to a stable location. Never replace a good path with a temporary one.
    let command = desired().map_err(|e| e.code().to_string())?;
    autostart::repair_existing(&port, &command).map_err(|e| e.code().into())
}

#[cfg(windows)]
mod native {
    use super::*;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows::Win32::System::Registry::{RegCloseKey, RegCreateKeyW, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ};
    const RUN: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }
    pub struct Native { name: String }
    pub fn port() -> Result<Native, String> { Ok(Native { name: LABEL.into() }) }
    impl Registration for Native {
        fn read(&self) -> Result<Option<String>, Error> {
            let key = wide(RUN); let name = wide(&self.name); let mut data = [0u16; 1024]; let mut size = (data.len() * 2) as u32;
            let status = unsafe { RegGetValueW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()), PCWSTR(name.as_ptr()), RRF_RT_REG_SZ, None, Some(data.as_mut_ptr().cast()), Some(&mut size)) };
            if status == ERROR_FILE_NOT_FOUND { return Ok(None); }
            if status != ERROR_SUCCESS || size < 2 || size as usize > data.len() * 2 { return Err(Error::Read); }
            String::from_utf16(&data[..size as usize / 2 - 1]).map(Some).map_err(|_| Error::Read)
        }
        fn write(&self, command: Option<&str>) -> Result<(), Error> {
            let key = wide(RUN); let name = wide(&self.name);
            let status = if let Some(command) = command {
                let mut handle = HKEY::default();
                if unsafe { RegCreateKeyW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()), &mut handle) } != ERROR_SUCCESS { return Err(Error::Write); }
                let data = wide(command);
                let result = unsafe { RegSetKeyValueW(handle, PCWSTR::null(), PCWSTR(name.as_ptr()), REG_SZ.0, Some(data.as_ptr().cast()), (data.len() * 2) as u32) };
                let _ = unsafe { RegCloseKey(handle) }; result
            } else { unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()), PCWSTR(name.as_ptr())) } };
            if status == ERROR_SUCCESS || (command.is_none() && status == ERROR_FILE_NOT_FOUND) { Ok(()) } else { Err(Error::Write) }
        }
    }
    #[cfg(test)]
    #[test]
    fn real_per_user_registry_round_trip_external_delete_and_relocation() {
        let io = Native { name: format!("{LABEL}.test.{}", std::process::id()) };
        let result = std::panic::catch_unwind(|| {
            io.write(None).unwrap(); assert!(!autostart::enabled(&io).unwrap());
            let old = autostart::windows_command(r"C:\Old Folder\OnePaste.exe").unwrap();
            let new = desired().unwrap();
            for v in [Some(old.as_str()), None, Some(old.as_str())] { assert_eq!(autostart::set(&io, v).unwrap(), v.is_some()); }
            autostart::repair_existing(&io, &new).unwrap(); assert_eq!(io.read().unwrap(), Some(new));
            let key = wide(RUN); let name = wide(&io.name);
            assert_eq!(unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()), PCWSTR(name.as_ptr())) }, ERROR_SUCCESS);
            assert!(!autostart::enabled(&io).unwrap());
        });
        io.write(None).unwrap(); result.unwrap();
    }
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;
    pub struct Native { path: PathBuf, label: String }
    pub fn port() -> Result<Native, String> {
        let home = std::env::var_os("HOME").ok_or_else(|| Error::Read.code().to_string())?;
        Ok(Native { path: PathBuf::from(home).join("Library/LaunchAgents").join(format!("{LABEL}.plist")), label: LABEL.into() })
    }
    impl Registration for Native {
        fn read(&self) -> Result<Option<String>, Error> {
            match std::fs::metadata(&self.path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(_) => return Err(Error::Read), Ok(_) => {}
            }
            let out = Command::new("/usr/bin/plutil").args(["-convert", "json", "-o", "-"]).arg(&self.path).output().map_err(|_| Error::Read)?;
            if !out.status.success() { return Err(Error::Read); }
            let value: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|_| Error::Read)?;
            if value["Label"].as_str() != Some(self.label.as_str()) { return Err(Error::Read); }
            if value["RunAtLoad"] != true || value["Disabled"] == true { return Ok(None); }
            let args = value["ProgramArguments"].as_array().ok_or(Error::Read)?;
            if args.len() != 2 || args[1] != "--hidden" { return Err(Error::Read); }
            let exe = args[0].as_str().ok_or(Error::Read)?;
            autostart::launch_agent(exe, &self.label).map(Some)
        }
        fn write(&self, command: Option<&str>) -> Result<(), Error> {
            if let Some(command) = command {
                use std::io::Write;
                use std::os::unix::fs::OpenOptionsExt;
                std::fs::create_dir_all(self.path.parent().ok_or(Error::Write)?).map_err(|_| Error::Write)?;
                let temporary = self.path.with_extension(format!("{}.tmp", std::process::id()));
                let result = (|| {
                    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&temporary)?;
                    file.write_all(command.as_bytes())?; file.sync_all()?;
                    std::fs::rename(&temporary, &self.path)
                })();
                if result.is_err() { let _ = std::fs::remove_file(&temporary); }
                result.map_err(|_| Error::Write)
            } else {
                match std::fs::remove_file(&self.path) { Ok(()) => Ok(()), Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()), Err(_) => Err(Error::Write) }
            }
        }
    }
    #[cfg(test)]
    #[test]
    fn real_launch_agent_round_trip_external_delete_and_relocation() {
        let label = format!("{LABEL}.test.{}", std::process::id());
        let io = Native { path: std::env::temp_dir().join(format!("{label}.plist")), label };
        let result = std::panic::catch_unwind(|| {
            assert!(!autostart::enabled(&io).unwrap());
            let old = autostart::launch_agent("/Applications/Old & Portable/OnePaste.app/Contents/MacOS/onepaste-desktop", &io.label).unwrap();
            let current = autostart::launch_agent(std::env::current_exe().unwrap().to_str().unwrap(), &io.label).unwrap();
            for v in [Some(old.as_str()), None, Some(old.as_str())] { assert_eq!(autostart::set(&io, v).unwrap(), v.is_some()); }
            autostart::repair_existing(&io, &current).unwrap(); assert_eq!(io.read().unwrap(), Some(current));
            std::fs::remove_file(&io.path).unwrap(); assert!(!autostart::enabled(&io).unwrap());
        });
        let _ = std::fs::remove_file(&io.path); result.unwrap();
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod native {
    use super::*;
    pub struct Native;
    pub fn port() -> Result<Native, String> { Ok(Native) }
    impl Registration for Native {
        fn read(&self) -> Result<Option<String>, Error> { Err(Error::Unsupported) }
        fn write(&self, _: Option<&str>) -> Result<(), Error> { Err(Error::Unsupported) }
    }
}
