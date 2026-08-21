//! Windows 应用图标提取
//!
//! 注册表 `DisplayIcon` 通常指向 exe / dll / ico。用 `PrivateExtractIconsW`
//! 按 48×48 取出 HICON，画到 32 位 DIB 再编成 PNG。

use std::io::Cursor;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::Mutex;

use winapi::shared::minwindef::{HICON, UINT};
use winapi::um::wingdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
};
use winapi::um::winuser::{DestroyIcon, DrawIconEx, GetDC, ReleaseDC, DI_NORMAL};

/// GDI 对象不宜跨线程乱抢，整段提取串行。
static EXTRACT_LOCK: Mutex<()> = Mutex::new(());

const ICON_PX: i32 = 48;

/// 从 exe / dll / ico / 位图文件提取 PNG。目录路径返回 None。
pub fn app_icon_png(path: &Path) -> Option<Vec<u8>> {
    app_icon_from_bundle(path)
}

/// 与 macOS 同名，方便 UI 走同一套「先并行文件、再回退」流程。
/// Windows 没有 icns，这一步就是从文件抽图标。
pub fn app_icon_from_bundle(path: &Path) -> Option<Vec<u8>> {
    let path = expand_env_path(path);
    if !path.is_file() {
        return None;
    }
    if let Some(png) = png_from_image_file(&path) {
        return Some(png);
    }
    icon_from_module(&path)
}

fn expand_env_path(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    if !raw.contains('%') {
        return path.to_path_buf();
    }
    use std::os::windows::ffi::OsStringExt;
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut buf = vec![0u16; 1024];
    let n = unsafe {
        winapi::um::processenv::ExpandEnvironmentStringsW(
            wide.as_ptr(),
            buf.as_mut_ptr(),
            buf.len() as u32,
        )
    };
    if n == 0 || (n as usize) > buf.len() {
        return path.to_path_buf();
    }
    std::path::PathBuf::from(std::ffi::OsString::from_wide(&buf[..n as usize - 1]))
}

fn png_from_image_file(path: &Path) -> Option<Vec<u8>> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "bmp" | "gif" | "webp") {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let img = image::load_from_memory(&bytes).ok()?.into_rgba8();
    let rgba = if img.width() > 256 || img.height() > 256 {
        image::imageops::resize(
            &img,
            ICON_PX as u32,
            ICON_PX as u32,
            image::imageops::FilterType::Triangle,
        )
    } else {
        img
    };
    encode_rgba(rgba.width(), rgba.height(), rgba.as_raw())
}

fn icon_from_module(path: &Path) -> Option<Vec<u8>> {
    let _guard = EXTRACT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe { hicon_from_file(&wide).and_then(hicon_to_png) }
}

unsafe fn hicon_from_file(wide: &[u16]) -> Option<HICON> {
    let mut icon: HICON = ptr::null_mut();
    let mut icon_id: UINT = 0;
    let n = PrivateExtractIconsW(
        wide.as_ptr(),
        0,
        ICON_PX,
        ICON_PX,
        &mut icon,
        &mut icon_id,
        1,
        0,
    );
    if n > 0 && !icon.is_null() {
        return Some(icon);
    }

    let mut large: HICON = ptr::null_mut();
    let mut small: HICON = ptr::null_mut();
    let n = winapi::um::shellapi::ExtractIconExW(wide.as_ptr(), 0, &mut large, &mut small, 1);
    if !small.is_null() {
        DestroyIcon(small);
    }
    if n > 0 && !large.is_null() {
        Some(large)
    } else {
        None
    }
}

unsafe fn hicon_to_png(icon: HICON) -> Option<Vec<u8>> {
    let result = hicon_to_png_inner(icon);
    DestroyIcon(icon);
    result
}

unsafe fn hicon_to_png_inner(icon: HICON) -> Option<Vec<u8>> {
    let hdc_screen = GetDC(ptr::null_mut());
    if hdc_screen.is_null() {
        return None;
    }
    let hdc = CreateCompatibleDC(hdc_screen);
    ReleaseDC(ptr::null_mut(), hdc_screen);
    if hdc.is_null() {
        return None;
    }

    let mut bmi: BITMAPINFO = std::mem::zeroed();
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = ICON_PX;
    bmi.bmiHeader.biHeight = -ICON_PX;
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;

    let mut bits: *mut winapi::ctypes::c_void = ptr::null_mut();
    let hbmp = CreateDIBSection(
        hdc,
        &bmi,
        DIB_RGB_COLORS,
        &mut bits,
        ptr::null_mut(),
        0,
    );
    if hbmp.is_null() || bits.is_null() {
        if !hbmp.is_null() {
            DeleteObject(hbmp as *mut _);
        }
        DeleteDC(hdc);
        return None;
    }

    let old = SelectObject(hdc, hbmp as *mut _);
    let nbytes = (ICON_PX * ICON_PX * 4) as usize;
    ptr::write_bytes(bits as *mut u8, 0, nbytes);
    DrawIconEx(
        hdc,
        0,
        0,
        icon,
        ICON_PX,
        ICON_PX,
        0,
        ptr::null_mut(),
        DI_NORMAL,
    );
    SelectObject(hdc, old);

    let bgra = std::slice::from_raw_parts(bits as *const u8, nbytes);
    let mut rgba = Vec::with_capacity(nbytes);
    for px in bgra.chunks_exact(4) {
        rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
    }
    // DrawIconEx 在部分系统上不写 alpha，全 0 时当成不透明。
    if rgba.chunks_exact(4).all(|p| p[3] == 0) {
        for px in rgba.chunks_exact_mut(4) {
            px[3] = 255;
        }
    }

    DeleteObject(hbmp as *mut _);
    DeleteDC(hdc);
    encode_rgba(ICON_PX as u32, ICON_PX as u32, &rgba)
}

fn encode_rgba(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let img = image::RgbaImage::from_raw(width, height, rgba.to_vec())?;
    let dyn_img = image::DynamicImage::ImageRgba8(img);
    let mut out = Vec::new();
    dyn_img
        .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
        .ok()?;
    Some(out)
}

#[link(name = "user32")]
extern "system" {
    fn PrivateExtractIconsW(
        lpsz_file: *const u16,
        n_icon_index: i32,
        cx_icon: i32,
        cy_icon: i32,
        phicon: *mut HICON,
        piconid: *mut UINT,
        n_icons: UINT,
        flags: UINT,
    ) -> UINT;
}