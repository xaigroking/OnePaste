//! Windows clipboard: `AddClipboardFormatListener` for monitoring, native formats for reading and writing.

use super::window::{os_err, wide, MessageWindow};
use crate::clipboard::{classify, Content, Event, Image, Snapshot, WriteOptions};
use crate::error::PlatformError;
use std::ffi::c_void;
use std::time::Duration;
use std::sync::{Arc, Mutex, atomic::{AtomicUsize, Ordering}};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
    OpenClipboard, RegisterClipboardFormatW, SetClipboardData, RemoveClipboardFormatListener,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::WindowsAndMessaging::{SetTimer, KillTimer, WM_TIMER, WM_DESTROY};

const CF_DIB: u32 = 8;
const CF_UNICODETEXT: u32 = 13;
const WM_CLIPBOARDUPDATE: u32 = 0x031D;

const FMT_PNG: &str = "PNG";
const FMT_ORIGIN: &str = "OnePaste.Origin";
const FMT_EXCLUDE_MONITOR: &str = "ExcludeClipboardContentFromMonitorProcessing";
const FMT_HISTORY: &str = "CanIncludeInClipboardHistory";

// Keep the owner alive and serialize the entire open/read-or-write/close transaction.
// A concurrent reader must not close the clipboard while another caller is copying its data.
static CLIPBOARD: Mutex<Option<MessageWindow>> = Mutex::new(None);

fn format_id(name: &str) -> u32 {
    let w = wide(name);
    unsafe { RegisterClipboardFormatW(PCWSTR(w.as_ptr())) }
}

fn available(format: u32) -> bool {
    format != 0 && unsafe { IsClipboardFormatAvailable(format).is_ok() }
}

/// Another program may hold the clipboard briefly, so retry a few times.
struct Opened;

impl Opened {
    fn new() -> Result<Opened, PlatformError> {
        Self::with_owner(None)
    }

    fn with_owner(owner: Option<windows::Win32::Foundation::HWND>) -> Result<Opened, PlatformError> {
        for _ in 0..20 {
            if unsafe { OpenClipboard(owner) }.is_ok() {
                return Ok(Opened);
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        Err(PlatformError::ClipboardBusy)
    }
}

impl Drop for Opened {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

/// Must be called while the clipboard is open.
fn get_bytes(format: u32) -> Result<Vec<u8>, PlatformError> {
    unsafe {
        let h = GetClipboardData(format).map_err(os_err)?;
        let g = HGLOBAL(h.0);
        let size = GlobalSize(g);
        if size == 0 { return Ok(Vec::new()); }
        let p = GlobalLock(g);
        if p.is_null() { return Err(os_err(windows::core::Error::from_thread())); }
        let out = std::slice::from_raw_parts(p as *const u8, size).to_vec();
        let _ = GlobalUnlock(g);
        Ok(out)
    }
}

/// Must be called while the clipboard is open.
fn set_bytes(format: u32, data: &[u8]) -> Result<(), PlatformError> {
    unsafe {
        let g = GlobalAlloc(GMEM_MOVEABLE, data.len().max(1)).map_err(os_err)?;
        let p = GlobalLock(g);
        if p.is_null() {
            let _ = GlobalFree(Some(g));
            return Err(PlatformError::Os(-1));
        }
        std::ptr::copy_nonoverlapping(data.as_ptr(), p as *mut u8, data.len());
        let _ = GlobalUnlock(g);
        // After success the memory belongs to the system; it only needs freeing on failure.
        if let Err(e) = SetClipboardData(format, Some(HANDLE(g.0 as *mut c_void))) {
            let _ = GlobalFree(Some(g));
            return Err(os_err(e));
        }
        Ok(())
    }
}

fn is_sensitive() -> Result<bool, PlatformError> {
    if available(format_id(FMT_EXCLUDE_MONITOR)) { return Ok(true); }
    let history = format_id(FMT_HISTORY);
    if available(history) {
        let bytes = get_bytes(history)?;
        let value: [u8; 4] = bytes.get(..4).ok_or(PlatformError::BadFormat)?.try_into().map_err(|_| PlatformError::BadFormat)?;
        return Ok(u32::from_le_bytes(value) == 0);
    }
    Ok(false)
}

pub(crate) fn read() -> Result<Snapshot, PlatformError> {
    let _access = CLIPBOARD.lock().unwrap_or_else(|error| error.into_inner());
    let _open = Opened::new()?;
    let from_self = available(format_id(FMT_ORIGIN));
    if is_sensitive()? { return Ok(Snapshot { content: None, sensitive: true, from_self }); }
    Ok(Snapshot { content: read_content()?, sensitive: false, from_self })
}

fn read_content() -> Result<Option<Content>, PlatformError> {
    if available(CF_UNICODETEXT) {
        let bytes = get_bytes(CF_UNICODETEXT)?;
        if bytes.len() % 2 != 0 { return Err(PlatformError::BadFormat); }
        let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
        let text = String::from_utf16_lossy(&units[..end]);
        return Ok(if text.is_empty() { None } else { Some(Content::Text(text)) });
    }
    let png_fmt = format_id(FMT_PNG);
    if available(png_fmt) {
        let png = get_bytes(png_fmt)?;
        if let Some((width, height)) = crate::png_info::dimensions(&png) {
            return Ok(Some(Content::Image(Image { png, width, height })));
        }
        if !available(CF_DIB) { return Err(PlatformError::BadFormat); }
    }
    if available(CF_DIB) {
        let dib = get_bytes(CF_DIB)?;
        let (png, width, height) = dib_to_png(&dib).ok_or(PlatformError::BadFormat)?;
        return Ok(Some(Content::Image(Image { png, width, height })));
    }
    Ok(None)
}

pub(crate) fn write(content: &Content, opts: WriteOptions) -> Result<(), PlatformError> {
    // EmptyClipboard and SetClipboardData require a real owner window, including on Windows 10.
    let mut writer = CLIPBOARD.lock().unwrap_or_else(|error| error.into_inner());
    if writer.is_none() {
        *writer = Some(MessageWindow::spawn("OnePasteClipboardWriter", true, |_| Ok(()), |_, _, _| {})?);
    }
    let owner = writer.as_ref().unwrap();
    let _open = Opened::with_owner(Some(owner.hwnd()))?;
    unsafe { EmptyClipboard() }.map_err(os_err)?;
    match content {
        Content::Text(s) => {
            let bytes: Vec<u8> = s.encode_utf16().chain(std::iter::once(0)).flat_map(|u| u.to_le_bytes()).collect();
            set_bytes(CF_UNICODETEXT, &bytes)?;
        }
        Content::Image(img) => {
            let dib = png_to_dib(&img.png)?;
            set_bytes(CF_DIB, &dib)?;
            set_bytes(format_id(FMT_PNG), &img.png)?;
        }
    }
    if opts.origin_marker {
        set_bytes(format_id(FMT_ORIGIN), &[1])?;
    }
    if opts.sensitive_marker {
        set_bytes(format_id(FMT_EXCLUDE_MONITOR), &[1])?;
    }
    Ok(())
}

// ---- DIB <-> PNG ----

fn le_u32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Supports only 24-bit and 32-bit `BI_RGB` / `BI_BITFIELDS` bitmaps; other formats return `None`.
fn dib_to_png(dib: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let header = le_u32(dib, 0)? as usize;
    if header < 40 {
        return None;
    }
    let width = le_u32(dib, 4)? as i32;
    let height = le_u32(dib, 8)? as i32;
    let bits = u16::from_le_bytes(dib.get(14..16)?.try_into().ok()?);
    let compression = le_u32(dib, 16)?;
    if width <= 0 || height == 0 || !(bits == 24 || bits == 32) || !(compression == 0 || compression == 3) {
        return None;
    }
    let top_down = height < 0;
    let (w, h) = (width as usize, height.unsigned_abs() as usize);
    let mut offset = header;
    if compression == 3 && header == 40 {
        offset += 12; // the three color masks
    }
    let stride = (w * usize::from(bits) + 31) / 32 * 4;
    let pixels = dib.get(offset..offset.checked_add(stride.checked_mul(h)?)?)?;
    let bytes_pp = usize::from(bits) / 8;
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let row_idx = if top_down { y } else { h - 1 - y };
        let row = &pixels[row_idx * stride..row_idx * stride + w * bytes_pp];
        for px in row.chunks_exact(bytes_pp) {
            let a = if bytes_pp == 4 { px[3] } else { 255 };
            rgba.extend_from_slice(&[px[2], px[1], px[0], a]);
        }
    }
    // Many programs leave the alpha channel at 0 in 32-bit bitmaps; treat that case as opaque.
    if bytes_pp == 4 && rgba.chunks_exact(4).all(|p| p[3] == 0) {
        for p in rgba.chunks_exact_mut(4) {
            p[3] = 255;
        }
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().ok()?;
        writer.write_image_data(&rgba).ok()?;
    }
    Some((out, w as u32, h as u32))
}

/// PNG to a 32-bit bottom-up `BI_RGB` bitmap (`CF_DIB`).
fn png_to_dib(png_bytes: &[u8]) -> Result<Vec<u8>, PlatformError> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(png_bytes));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = dec.read_info().map_err(|_| PlatformError::BadFormat)?;
    let mut buf = vec![0u8; reader.output_buffer_size().ok_or(PlatformError::BadFormat)?];
    let info = reader.next_frame(&mut buf).map_err(|_| PlatformError::BadFormat)?;
    let (w, h) = (info.width as usize, info.height as usize);
    let src = &buf[..info.buffer_size()];
    let rgba: Vec<[u8; 4]> = match info.color_type {
        png::ColorType::Rgba => src.chunks_exact(4).map(|p| [p[0], p[1], p[2], p[3]]).collect(),
        png::ColorType::Rgb => src.chunks_exact(3).map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => src.chunks_exact(2).map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => src.iter().map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err(PlatformError::BadFormat),
    };
    let mut out = Vec::with_capacity(40 + w * h * 4);
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&((w * h * 4) as u32).to_le_bytes());
    out.extend_from_slice(&[0u8; 16]); // resolution and palette fields
    for y in (0..h).rev() {
        for p in &rgba[y * w..(y + 1) * w] {
            out.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
        }
    }
    Ok(out)
}

// ---- Monitoring ----

pub(crate) struct Monitor {
    _win: MessageWindow,
}

impl Monitor {
    pub(crate) fn start(on_event: Box<dyn Fn(Event) + Send>) -> Result<Monitor, PlatformError> {
        let hwnd = Arc::new(AtomicUsize::new(0));
        let initialized = hwnd.clone();
        let mut retries = 0u8;
        let win = MessageWindow::spawn(
            "OnePasteClipboardMonitor",
            true,
            move |hwnd| {
                unsafe { AddClipboardFormatListener(hwnd) }.map_err(os_err)?;
                initialized.store(hwnd.0 as usize, Ordering::Release);
                Ok(())
            },
            move |msg, wp, _| {
                if msg == WM_DESTROY {
                    let handle = hwnd.load(Ordering::Acquire);
                    if handle != 0 {
                        unsafe { let _ = RemoveClipboardFormatListener(windows::Win32::Foundation::HWND(handle as *mut c_void)); }
                        on_event(Event::MonitorStopped);
                    }
                    return;
                }
                let timer = msg == WM_TIMER && wp.0 == 1;
                if msg == WM_CLIPBOARDUPDATE || timer {
                    if !timer { retries = 3; }
                    let owner = windows::Win32::Foundation::HWND(hwnd.load(Ordering::Acquire) as *mut c_void);
                    match read() {
                        Ok(snapshot) => {
                            retries = 0;
                            unsafe { let _ = KillTimer(Some(owner), 1); }
                            on_event(classify(snapshot).unwrap_or(Event::SkippedUnsupported));
                        }
                        Err(error) => {
                            on_event(Event::ReadFailed(error.code()));
                            if retries > 0 {
                                retries -= 1;
                                if unsafe { SetTimer(Some(owner), 1, 250, None) } == 0 {
                                    retries = 0;
                                    on_event(Event::MonitorStopped);
                                }
                            } else { unsafe { let _ = KillTimer(Some(owner), 1); } }
                        }
                    }
                }
            },
        )?;
        Ok(Monitor { _win: win })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_CLIPBOARD: Mutex<()> = Mutex::new(());

    #[test]
    fn a_busy_clipboard_reports_failure_then_recovers_without_another_copy() {
        let _exclusive = TEST_CLIPBOARD.lock().unwrap_or_else(|error| error.into_inner());
        use std::sync::mpsc::channel;
        use windows::Win32::Foundation::{WPARAM, LPARAM};
        use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
        let (tx,rx)=channel();
        let monitor=Monitor::start(Box::new(move |event| { let _=tx.send(event); })).unwrap();
        let owner=MessageWindow::spawn("OnePasteBusyClipboardProbe",true,|_|Ok(()),|_,_,_|{}).unwrap();
        let opened=Opened::with_owner(Some(owner.hwnd())).unwrap();
        unsafe { EmptyClipboard().unwrap(); }
        let bytes:Vec<u8>="recoverable clipboard contention probe".encode_utf16().chain(std::iter::once(0)).flat_map(u16::to_le_bytes).collect();
        set_bytes(CF_UNICODETEXT,&bytes).unwrap();
        unsafe { PostMessageW(Some(monitor._win.hwnd()),WM_CLIPBOARDUPDATE,WPARAM(0),LPARAM(0)).unwrap(); }
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(),Event::ReadFailed("clipboard_busy".into()));
        drop(opened);
        let deadline=std::time::Instant::now()+Duration::from_secs(5);
        loop {
            let event=rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())).unwrap();
            if let Event::Captured(content)=event {
                assert_eq!(content,Content::Text("recoverable clipboard contention probe".into()));break;
            }
        }
        drop(monitor);
    }

    #[test]
    fn concurrent_reads_keep_the_clipboard_open_until_the_snapshot_is_complete() {
        let _exclusive = TEST_CLIPBOARD.lock().unwrap_or_else(|error| error.into_inner());
        let expected = Content::Text("concurrent clipboard reader fixture".into());
        write(&expected, WriteOptions { origin_marker: false, sensitive_marker: false }).unwrap();
        let ready = Arc::new(std::sync::Barrier::new(8));
        let readers: Vec<_> = (0..8).map(|_| {
            let ready = ready.clone();
            let expected = expected.clone();
            std::thread::spawn(move || {
                ready.wait();
                for _ in 0..100 {
                    let snapshot = read().expect("another local reader must not close this transaction");
                    assert_eq!(snapshot.content.as_ref(), Some(&expected));
                    assert!(!snapshot.sensitive);
                    assert!(!snapshot.from_self);
                }
            })
        }).collect();
        for reader in readers { reader.join().unwrap(); }
    }

    #[test]
    fn png_dib_roundtrip_keeps_pixels() {
        let rgba = [[10u8, 20, 30, 255], [40, 50, 60, 200], [70, 80, 90, 255], [1, 2, 3, 255]];
        let mut png_bytes = Vec::new();
        {
            let mut e = png::Encoder::new(&mut png_bytes, 2, 2);
            e.set_color(png::ColorType::Rgba);
            e.set_depth(png::BitDepth::Eight);
            let mut w = e.write_header().unwrap();
            w.write_image_data(&rgba.concat()).unwrap();
        }
        let dib = png_to_dib(&png_bytes).unwrap();
        let (back, w, h) = dib_to_png(&dib).unwrap();
        assert_eq!((w, h), (2, 2));
        let mut dec = png::Decoder::new(std::io::Cursor::new(back));
        dec.set_transformations(png::Transformations::normalize_to_color8());
        let mut r = dec.read_info().unwrap();
        let mut buf = vec![0u8; r.output_buffer_size().unwrap()];
        let info = r.next_frame(&mut buf).unwrap();
        assert_eq!(&buf[..info.buffer_size()], rgba.concat().as_slice());
    }
}
