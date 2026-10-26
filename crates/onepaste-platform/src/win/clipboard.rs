//! Windows 剪贴板：`AddClipboardFormatListener` 监听，原生格式读写。

use super::window::{os_err, wide, MessageWindow};
use crate::clipboard::{classify, Content, Event, Image, Snapshot, WriteOptions};
use crate::error::PlatformError;
use std::ffi::c_void;
use std::time::Duration;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
    OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};

const CF_DIB: u32 = 8;
const CF_UNICODETEXT: u32 = 13;
const WM_CLIPBOARDUPDATE: u32 = 0x031D;

const FMT_PNG: &str = "PNG";
const FMT_ORIGIN: &str = "OnePaste.Origin";
const FMT_EXCLUDE_MONITOR: &str = "ExcludeClipboardContentFromMonitorProcessing";
const FMT_HISTORY: &str = "CanIncludeInClipboardHistory";

fn format_id(name: &str) -> u32 {
    let w = wide(name);
    unsafe { RegisterClipboardFormatW(PCWSTR(w.as_ptr())) }
}

fn available(format: u32) -> bool {
    format != 0 && unsafe { IsClipboardFormatAvailable(format).is_ok() }
}

/// 剪贴板可能被别的程序短暂占用，重试几次。
struct Opened;

impl Opened {
    fn new() -> Result<Opened, PlatformError> {
        for _ in 0..20 {
            if unsafe { OpenClipboard(None) }.is_ok() {
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

/// 必须在剪贴板已打开时调用。
fn get_bytes(format: u32) -> Option<Vec<u8>> {
    unsafe {
        let h = GetClipboardData(format).ok()?;
        let g = HGLOBAL(h.0);
        let size = GlobalSize(g);
        let p = GlobalLock(g);
        if p.is_null() {
            return None;
        }
        let out = std::slice::from_raw_parts(p as *const u8, size).to_vec();
        let _ = GlobalUnlock(g);
        Some(out)
    }
}

/// 必须在剪贴板已打开时调用。
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
        // 成功后内存归系统所有；失败才需要自己释放。
        if let Err(e) = SetClipboardData(format, Some(HANDLE(g.0 as *mut c_void))) {
            let _ = GlobalFree(Some(g));
            return Err(os_err(e));
        }
        Ok(())
    }
}

fn is_sensitive() -> bool {
    if available(format_id(FMT_EXCLUDE_MONITOR)) {
        return true;
    }
    let history = format_id(FMT_HISTORY);
    if available(history) {
        if let Some(b) = get_bytes(history) {
            if b.len() >= 4 && u32::from_le_bytes([b[0], b[1], b[2], b[3]]) == 0 {
                return true;
            }
        }
    }
    false
}

pub(crate) fn read() -> Result<Snapshot, PlatformError> {
    let _open = Opened::new()?;
    let from_self = available(format_id(FMT_ORIGIN));
    if is_sensitive() {
        return Ok(Snapshot { content: None, sensitive: true, from_self });
    }
    Ok(Snapshot { content: read_content(), sensitive: false, from_self })
}

fn read_content() -> Option<Content> {
    if available(CF_UNICODETEXT) {
        let bytes = get_bytes(CF_UNICODETEXT)?;
        let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
        let text = String::from_utf16_lossy(&units[..end]);
        return if text.is_empty() { None } else { Some(Content::Text(text)) };
    }
    let png_fmt = format_id(FMT_PNG);
    if available(png_fmt) {
        if let Some(png) = get_bytes(png_fmt) {
            if let Some((width, height)) = crate::png_info::dimensions(&png) {
                return Some(Content::Image(Image { png, width, height }));
            }
        }
    }
    if available(CF_DIB) {
        let dib = get_bytes(CF_DIB)?;
        let (png, width, height) = dib_to_png(&dib)?;
        return Some(Content::Image(Image { png, width, height }));
    }
    None
}

pub(crate) fn write(content: &Content, opts: WriteOptions) -> Result<(), PlatformError> {
    let _open = Opened::new()?;
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

/// 只支持 24 位和 32 位的 `BI_RGB` / `BI_BITFIELDS` 位图；其他格式返回 `None`。
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
        offset += 12; // 三个颜色掩码
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
    // 很多程序写 32 位位图时不填透明度（全 0），这种情况按不透明处理。
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

/// PNG → 32 位自底向上的 `BI_RGB` 位图（`CF_DIB`）。
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
    out.extend_from_slice(&[0u8; 16]); // 分辨率、调色板
    for y in (0..h).rev() {
        for p in &rgba[y * w..(y + 1) * w] {
            out.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
        }
    }
    Ok(out)
}

// ---- 监听 ----

pub(crate) struct Monitor {
    _win: MessageWindow,
}

impl Monitor {
    pub(crate) fn start(on_event: Box<dyn Fn(Event) + Send>) -> Result<Monitor, PlatformError> {
        let win = MessageWindow::spawn(
            "OnePasteClipboardMonitor",
            true,
            |hwnd| unsafe { AddClipboardFormatListener(hwnd) }.map_err(os_err),
            move |msg, _, _| {
                if msg == WM_CLIPBOARDUPDATE {
                    if let Some(ev) = read().ok().and_then(classify) {
                        on_event(ev);
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
