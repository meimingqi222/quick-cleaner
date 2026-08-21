//! Windows 应用图标提取
//!
//! 注册表 `DisplayIcon` 通常指向 exe / dll / ico。用 `PrivateExtractIconsW`
//! 按 48×48 取出 HICON，画到 32 位 DIB 再编成 PNG。
//!
//! MSI 软件（坚果云等）经常不写 `DisplayIcon`，调用方会把安装目录当缓存键。
//! 目录本身抽不出图标，这时在目录里找同名 exe / ico。

use std::io::Cursor;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::Mutex;

use winapi::shared::minwindef::{DWORD, UINT};
use winapi::shared::windef::HICON;
use winapi::um::shellapi::{
    ExtractIconExW, SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON,
};
use winapi::um::wingdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
};
use winapi::um::winuser::{DestroyIcon, DrawIconEx, GetDC, ReleaseDC};

/// DrawIconEx 的标志，winapi 0.3 部分版本未导出此常量。
const DI_NORMAL: DWORD = 0x0003;

/// GDI 对象不宜跨线程乱抢，整段提取串行。
static EXTRACT_LOCK: Mutex<()> = Mutex::new(());

const ICON_PX: i32 = 48;

/// 从 exe / dll / ico / 位图文件提取 PNG。
///
/// 传入安装目录时，会在目录内找主程序图标（MSI 常没有 DisplayIcon）。
pub fn app_icon_png(path: &Path) -> Option<Vec<u8>> {
    app_icon_from_bundle(path)
}

/// 与 macOS 同名，方便 UI 走同一套「先并行文件、再回退」流程。
/// Windows 没有 icns，这一步就是从文件（或安装目录里的主程序）抽图标。
pub fn app_icon_from_bundle(path: &Path) -> Option<Vec<u8>> {
    let path = strip_trailing_sep(expand_env_path(path));
    if path.is_file() {
        return icon_from_file(&path);
    }
    if path.is_dir() {
        for cand in icon_candidates_in_dir(&path) {
            if let Some(png) = icon_from_file(&cand) {
                return Some(png);
            }
        }
    }
    None
}

fn icon_from_file(path: &Path) -> Option<Vec<u8>> {
    png_from_image_file(path).or_else(|| icon_from_module(path))
}

/// 注册表 `InstallLocation` 经常带末尾反斜杠，`file_name()` 会变成 None。
fn strip_trailing_sep(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    let trimmed = s.trim_end_matches(['\\', '/']);
    if trimmed.is_empty() || trimmed.len() == s.len() {
        path
    } else {
        PathBuf::from(trimmed)
    }
}

/// 安装目录里最像主程序图标的文件，按优先级排列。
fn icon_candidates_in_dir(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Some(stem) = dir
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|s| !s.is_empty())
    else {
        return out;
    };

    push_if_file(&mut out, dir.join(format!("{stem}.exe")));
    push_if_file(&mut out, dir.join(format!("{stem}.ico")));
    push_if_file(&mut out, dir.join("icons").join(format!("{stem}.ico")));
    push_if_file(&mut out, dir.join("icon.ico"));

    for sub in ["bin", "application", "app", "program"] {
        push_if_file(&mut out, dir.join(sub).join(format!("{stem}.exe")));
    }

    let stem_l = stem.to_ascii_lowercase();
    let mut others = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for ent in rd.flatten() {
            let path = ent.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let lower = name.to_ascii_lowercase();
            if !lower.ends_with(".exe") || is_helper_exe_name(&lower) || !path.is_file() {
                continue;
            }
            if out.iter().any(|p| paths_eq(p, &path)) {
                continue;
            }
            others.push(path);
        }
    }
    others.sort_by(|a, b| {
        let an = file_stem_lower(a);
        let bn = file_stem_lower(b);
        bn.contains(&stem_l)
            .cmp(&an.contains(&stem_l))
            .then_with(|| an.cmp(&bn))
    });
    out.extend(others);
    out
}

fn push_if_file(out: &mut Vec<PathBuf>, path: PathBuf) {
    if path.is_file() {
        out.push(path);
    }
}

fn file_stem_lower(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn paths_eq(a: &Path, b: &Path) -> bool {
    a.as_os_str().eq_ignore_ascii_case(b.as_os_str())
}

fn is_helper_exe_name(name_lower: &str) -> bool {
    const NEEDLES: &[&str] = &[
        "unins",
        "uninstall",
        "setup",
        "update",
        "crashpad",
        "crash_handler",
        "helper",
        "installer",
        "maintenance",
        "elevate",
    ];
    NEEDLES.iter().any(|n| name_lower.contains(n))
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
    if !matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "bmp" | "gif" | "webp"
    ) {
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
    unsafe { hicon_from_file(&wide).and_then(|h| hicon_to_png(h)) }
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
    let n = ExtractIconExW(wide.as_ptr(), 0, &mut large, &mut small, 1);
    if !small.is_null() {
        DestroyIcon(small);
    }
    if n > 0 && !large.is_null() {
        return Some(large);
    }
    if !large.is_null() {
        DestroyIcon(large);
    }

    let mut info: SHFILEINFOW = std::mem::zeroed();
    let r = SHGetFileInfoW(
        wide.as_ptr(),
        0,
        &mut info,
        std::mem::size_of::<SHFILEINFOW>() as u32,
        SHGFI_ICON | SHGFI_LARGEICON,
    );
    if r != 0 && !info.hIcon.is_null() {
        Some(info.hIcon)
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
    let hbmp = CreateDIBSection(hdc, &bmi, DIB_RGB_COLORS, &mut bits, ptr::null_mut(), 0);
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
    for px in bgra.as_chunks::<4>().0 {
        rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
    }
    // DrawIconEx 在部分系统上不写 alpha，全 0 时当成不透明。
    if rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 0) {
        for px in rgba.as_chunks_mut::<4>().0 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const PNG_MAGIC: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

    fn workspace_ico() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/icon.ico")
    }

    fn unique_temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "qc_icon_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn extracts_png_from_system_exe() {
        let notepad = Path::new(r"C:\Windows\System32\notepad.exe");
        if !notepad.is_file() {
            return;
        }
        let png = app_icon_png(notepad).expect("notepad.exe should have an icon");
        assert!(png.starts_with(PNG_MAGIC), "payload is not PNG");
    }

    #[test]
    fn directory_picks_stem_named_ico() {
        let src = workspace_ico();
        assert!(src.is_file(), "assets/icon.ico should exist");
        let dir = unique_temp_dir();
        let stem = dir.file_name().unwrap().to_string_lossy().to_string();
        std::fs::copy(&src, dir.join(format!("{stem}.ico"))).unwrap();

        let png = app_icon_from_bundle(&dir).expect("named ico in dir should extract");
        assert!(png.starts_with(PNG_MAGIC));

        let trailing = PathBuf::from(format!("{}\\", dir.display()));
        assert!(
            app_icon_from_bundle(&trailing).is_some(),
            "trailing slash on install dir must still work"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn directory_prefers_named_exe_over_uninstaller() {
        let notepad = Path::new(r"C:\Windows\System32\notepad.exe");
        if !notepad.is_file() {
            return;
        }
        let dir = unique_temp_dir();
        let stem = dir.file_name().unwrap().to_string_lossy().to_string();
        std::fs::copy(notepad, dir.join("uninstall.exe")).unwrap();
        std::fs::copy(notepad, dir.join(format!("{stem}.exe"))).unwrap();

        let cands = icon_candidates_in_dir(&dir);
        let expected = format!("{stem}.exe");
        assert_eq!(
            cands
                .first()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str()),
            Some(expected.as_str()),
            "named exe should beat uninstall.exe: {cands:?}"
        );
        assert!(
            app_icon_from_bundle(&dir).is_some(),
            "named exe copied from notepad should extract"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_directory_has_no_icon() {
        let dir = unique_temp_dir();
        assert!(icon_candidates_in_dir(&dir).is_empty());
        assert!(app_icon_from_bundle(&dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extracts_nutstore_from_install_dir() {
        let dir = Path::new(r"C:\Program Files\Nutstore");
        if !dir.is_dir() {
            return;
        }
        let png = app_icon_png(dir).expect("坚果云安装目录应能抽出 Nutstore.exe 图标");
        assert!(png.starts_with(PNG_MAGIC));

        let trailing = Path::new(r"C:\Program Files\Nutstore\");
        assert!(
            app_icon_png(trailing).is_some(),
            "InstallLocation 带末尾反斜杠时也应抽出图标"
        );
    }
}
