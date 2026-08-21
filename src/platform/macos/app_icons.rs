//! macOS 应用图标提取
//!
//! 优先从 bundle 里的 `.icns` 抽出已经压好的 64/128 PNG——这是纯文件读取，
//! 可并行，一百多个应用通常几十毫秒。只有 icns 里没有 PNG（JPEG2000 / Asset
//! Catalog）才回退到 `NSWorkspace.iconForFile:`。

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use objc::runtime::{Object, BOOL, YES};
use objc::{class, msg_send, sel, sel_impl};

/// NSWorkspace / NSImage 不是线程安全的，AppKit 回退路径必须串行。
static EXTRACT_LOCK: Mutex<()> = Mutex::new(());

const ICON_PX: u32 = 64;
const PNG_MAGIC: &[u8] = &[0x89, b'P', b'N', b'G'];

/// 提取指定 .app 路径的图标，返回约 64×64 的 PNG bytes。
pub fn app_icon_png(path: &Path) -> Option<Vec<u8>> {
    app_icon_from_bundle(path).or_else(|| icon_from_workspace(path))
}

/// 只读 bundle 内的 icns/png，不碰 AppKit。可并行。
pub fn app_icon_from_bundle(path: &Path) -> Option<Vec<u8>> {
    if let Some(icon_path) = find_bundle_icon_file(path) {
        if let Some(png) = png_from_icon_file(&icon_path) {
            return Some(png);
        }
    }
    png_from_common_files(path)
}

fn png_from_icon_file(icon_path: &Path) -> Option<Vec<u8>> {
    if icon_path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
    {
        let png = std::fs::read(icon_path).ok()?;
        return Some(maybe_downscale(&png));
    }
    let data = std::fs::read(icon_path).ok()?;
    let png = best_png_in_icns(&data)?;
    Some(maybe_downscale(png))
}

fn png_from_common_files(app: &Path) -> Option<Vec<u8>> {
    let resources = app.join("Contents").join("Resources");
    for name in [
        "assets/icons/64x64.png",
        "assets/icons/128x128.png",
        "assets/icon.png",
        "icon.png",
        "AppIcon.png",
    ] {
        let path = resources.join(name);
        if path.is_file() {
            if let Ok(png) = std::fs::read(&path) {
                if png.starts_with(PNG_MAGIC) {
                    return Some(maybe_downscale(&png));
                }
            }
        }
    }
    None
}

fn find_bundle_icon_file(app: &Path) -> Option<PathBuf> {
    let resources = app.join("Contents").join("Resources");
    if !resources.is_dir() {
        return None;
    }

    if let Some(name) =
        plist_string_value(&app.join("Contents").join("Info.plist"), "CFBundleIconFile")
    {
        if let Some(path) = resolve_icon_name(&resources, &name) {
            return Some(path);
        }
    }

    let stem = app.file_stem()?.to_string_lossy();
    for name in ["AppIcon.icns", "icon.icns", "app.icns", "Icon.icns"] {
        let path = resources.join(name);
        if path.is_file() {
            return Some(path);
        }
    }
    let named = resources.join(format!("{stem}.icns"));
    if named.is_file() {
        return Some(named);
    }

    let mut single: Option<PathBuf> = None;
    let mut count = 0usize;
    if let Ok(rd) = std::fs::read_dir(&resources) {
        for entry in rd.flatten() {
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("icns"))
                && !is_document_icns(&path)
            {
                count += 1;
                single = Some(path);
                if count > 1 {
                    return None;
                }
            }
        }
    }
    single
}

fn is_document_icns(path: &Path) -> bool {
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    name.contains("document")
        || name.contains("badge")
        || name.contains("alias")
        || name.contains("folder")
}

fn resolve_icon_name(resources: &Path, name: &str) -> Option<PathBuf> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let direct = resources.join(name);
    if direct.is_file() {
        return Some(direct);
    }
    for ext in [".icns", ".png"] {
        if name.ends_with(ext) {
            continue;
        }
        let path = resources.join(format!("{name}{ext}"));
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

/// ICNS 里 PNG 类型的优先级：越接近 64px 越好。`None` 表示忽略。
fn png_type_score(typ: &[u8; 4]) -> Option<u32> {
    match typ {
        b"ic12" | b"icp6" => Some(0), // 64
        b"ic07" => Some(1),           // 128
        b"ic13" | b"ic08" => Some(2), // 256
        b"ic11" | b"icp5" => Some(3), // 32
        b"ic14" | b"ic09" => Some(4), // 512
        b"ic10" => Some(5),           // 1024
        _ => None,
    }
}

fn best_png_in_icns(data: &[u8]) -> Option<&[u8]> {
    if data.len() < 8 || &data[..4] != b"icns" {
        return None;
    }
    let file_len = u32::from_be_bytes(data[4..8].try_into().ok()?) as usize;
    let end = file_len.min(data.len());
    let mut off = 8;
    let mut best: Option<&[u8]> = None;
    let mut best_score = u32::MAX;
    while off + 8 <= end {
        let typ: [u8; 4] = data[off..off + 4].try_into().ok()?;
        let len = u32::from_be_bytes(data[off + 4..off + 8].try_into().ok()?) as usize;
        if len < 8 || off + len > end {
            break;
        }
        let payload = &data[off + 8..off + len];
        off += len;
        if !payload.starts_with(PNG_MAGIC) {
            continue;
        }
        let Some(score) = png_type_score(&typ) else {
            continue;
        };
        if score < best_score {
            best_score = score;
            best = Some(payload);
            if score == 0 {
                break;
            }
        }
    }
    best
}

fn png_dimensions(png: &[u8]) -> Option<(u32, u32)> {
    if png.len() < 24 || !png.starts_with(PNG_MAGIC) || &png[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(png[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(png[20..24].try_into().ok()?);
    Some((width, height))
}

fn maybe_downscale(png: &[u8]) -> Vec<u8> {
    match png_dimensions(png) {
        Some((w, h)) if w > 0 && w <= 256 && h <= 256 => png.to_vec(),
        Some((w, h)) if w > 256 || h > 256 => downscale_png(png).unwrap_or_else(|| png.to_vec()),
        _ => png.to_vec(),
    }
}

fn downscale_png(png: &[u8]) -> Option<Vec<u8>> {
    let img = image::load_from_memory(png).ok()?.into_rgba8();
    let resized = image::imageops::resize(
        &img,
        ICON_PX,
        ICON_PX,
        image::imageops::FilterType::Triangle,
    );
    let dyn_img = image::DynamicImage::ImageRgba8(resized);
    let mut out = Vec::new();
    dyn_img
        .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
        .ok()?;
    Some(out)
}

fn plist_string_value(plist_path: &Path, key: &str) -> Option<String> {
    let bytes = std::fs::read(plist_path).ok()?;
    if bytes.starts_with(b"bplist") {
        bplist_string(&bytes, key)
    } else {
        xml_plist_string(&bytes, key)
    }
}

fn xml_plist_string(bytes: &[u8], key: &str) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    let marker = format!("<key>{key}</key>");
    let rest = text.split(&marker).nth(1)?.trim_start();
    let rest = rest.strip_prefix("<string>")?;
    let value = rest.split("</string>").next()?.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn bplist_string(data: &[u8], key: &str) -> Option<String> {
    if data.len() < 40 {
        return None;
    }
    let trailer = &data[data.len() - 32..];
    let offset_size = trailer[6] as usize;
    let ref_size = trailer[7] as usize;
    if offset_size == 0 || ref_size == 0 || offset_size > 8 || ref_size > 8 {
        return None;
    }
    let num_objects = read_be(trailer, 8, 8)? as usize;
    let top = read_be(trailer, 16, 8)? as usize;
    let table_off = read_be(trailer, 24, 8)? as usize;
    let plist = BPlist {
        data,
        offset_size,
        ref_size,
        num_objects,
        table_off,
    };
    plist.dict_string(top, key)
}

struct BPlist<'a> {
    data: &'a [u8],
    offset_size: usize,
    ref_size: usize,
    num_objects: usize,
    table_off: usize,
}

impl BPlist<'_> {
    fn obj_offset(&self, idx: usize) -> Option<usize> {
        if idx >= self.num_objects {
            return None;
        }
        read_be(
            self.data,
            self.table_off
                .checked_add(idx.checked_mul(self.offset_size)?)?,
            self.offset_size,
        )
        .map(|v| v as usize)
    }

    fn dict_string(&self, idx: usize, key: &str) -> Option<String> {
        let off = self.obj_offset(idx)?;
        let marker = *self.data.get(off)?;
        if marker >> 4 != 0xD {
            return None;
        }
        let (count, payload_off) = self.sized_count(off, marker & 0x0F)?;
        let refs_bytes = count.checked_mul(self.ref_size)?.checked_mul(2)?;
        let payload = self
            .data
            .get(payload_off..payload_off.checked_add(refs_bytes)?)?;
        for i in 0..count {
            let key_ref = read_be(payload, i * self.ref_size, self.ref_size)? as usize;
            if self.as_str(key_ref).as_deref() != Some(key) {
                continue;
            }
            let val_ref = read_be(payload, (count + i) * self.ref_size, self.ref_size)? as usize;
            return self.as_str(val_ref);
        }
        None
    }

    fn as_str(&self, idx: usize) -> Option<String> {
        let off = self.obj_offset(idx)?;
        let marker = *self.data.get(off)?;
        let kind = marker >> 4;
        let nibble = marker & 0x0F;
        let (len, payload_off) = self.sized_count(off, nibble)?;
        match kind {
            0x5 => {
                let bytes = self.data.get(payload_off..payload_off.checked_add(len)?)?;
                Some(std::str::from_utf8(bytes).ok()?.to_string())
            }
            0x6 => {
                let bytes = self
                    .data
                    .get(payload_off..payload_off.checked_add(len.checked_mul(2)?)?)?;
                let units: Vec<u16> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_be_bytes(*c))
                    .collect();
                String::from_utf16(&units).ok()
            }
            _ => None,
        }
    }

    fn sized_count(&self, off: usize, nibble: u8) -> Option<(usize, usize)> {
        if nibble != 0x0F {
            return Some((nibble as usize, off + 1));
        }
        let int_off = off + 1;
        let int_marker = *self.data.get(int_off)?;
        if int_marker >> 4 != 0x1 {
            return None;
        }
        let nbytes = 1usize << (int_marker & 0x0F);
        let value = read_be(self.data, int_off + 1, nbytes)? as usize;
        Some((value, int_off + 1 + nbytes))
    }
}

fn read_be(data: &[u8], off: usize, size: usize) -> Option<u64> {
    if size == 0 || size > 8 {
        return None;
    }
    let slice = data.get(off..off.checked_add(size)?)?;
    let mut v = 0u64;
    for b in slice {
        v = (v << 8) | u64::from(*b);
    }
    Some(v)
}

fn icon_from_workspace(path: &Path) -> Option<Vec<u8>> {
    let path_str = path.to_str()?;
    let _guard = EXTRACT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let pool: *mut Object = msg_send![class!(NSAutoreleasePool), new];
        let raw = workspace_icon_png(path_str);
        let _: () = msg_send![pool, drain];
        raw.map(|png| maybe_downscale(&png))
    }
}

unsafe fn workspace_icon_png(path_str: &str) -> Option<Vec<u8>> {
    let ws: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
    if ws.is_null() {
        return None;
    }

    let ns_path: *mut Object = msg_send![class!(NSString), alloc];
    let ns_path: *mut Object = msg_send![
        ns_path,
        initWithBytes: path_str.as_ptr() as *const std::ffi::c_void
        length: path_str.len()
        encoding: 4usize // NSUTF8StringEncoding
    ];
    if ns_path.is_null() {
        return None;
    }

    let icon: *mut Object = msg_send![ws, iconForFile: ns_path];
    let _: () = msg_send![ns_path, release];
    if icon.is_null() {
        return None;
    }

    if let Some(png) = png_from_bitmap_reps(icon) {
        return Some(png);
    }

    // 没有合适的位图表示时才走整图 TIFF（少数 JPEG2000 / Asset Catalog 应用）。
    let tiff: *mut Object = msg_send![icon, TIFFRepresentation];
    if tiff.is_null() {
        return None;
    }
    let rep: *mut Object = msg_send![class!(NSBitmapImageRep), imageRepWithData: tiff];
    if rep.is_null() {
        return None;
    }
    nsdata_png_bytes(rep)
}

unsafe fn png_from_bitmap_reps(icon: *mut Object) -> Option<Vec<u8>> {
    let reps: *mut Object = msg_send![icon, representations];
    if reps.is_null() {
        return None;
    }
    let count: usize = msg_send![reps, count];
    let bitmap_cls = class!(NSBitmapImageRep);
    let mut best: *mut Object = std::ptr::null_mut();
    let mut best_score = i32::MAX;
    for i in 0..count {
        let rep: *mut Object = msg_send![reps, objectAtIndex: i];
        if rep.is_null() {
            continue;
        }
        let is_bitmap: BOOL = msg_send![rep, isKindOfClass: bitmap_cls];
        if is_bitmap != YES {
            continue;
        }
        let width: isize = msg_send![rep, pixelsWide];
        if width <= 0 {
            continue;
        }
        let score = (width as i32 - ICON_PX as i32).abs();
        if score < best_score {
            best_score = score;
            best = rep;
        }
    }
    if best.is_null() {
        return None;
    }
    nsdata_png_bytes(best)
}

unsafe fn nsdata_png_bytes(rep: *mut Object) -> Option<Vec<u8>> {
    let empty_dict: *mut Object = msg_send![class!(NSDictionary), dictionary];
    let png: *mut Object = msg_send![
        rep,
        representationUsingType: 4usize // NSPNGFileType
        properties: empty_dict
    ];
    if png.is_null() {
        return None;
    }
    let length: usize = msg_send![png, length];
    let bytes_ptr: *const std::ffi::c_void = msg_send![png, bytes];
    if length == 0 || bytes_ptr.is_null() {
        return None;
    }
    let slice = std::slice::from_raw_parts(bytes_ptr as *const u8, length);
    Some(slice.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::time::Instant;

    #[test]
    fn extracts_png_for_system_app() {
        let candidates = [
            "/System/Applications/Utilities/Terminal.app",
            "/System/Library/CoreServices/Finder.app",
            "/Applications/Safari.app",
            "/System/Applications/System Settings.app",
        ];
        let png = candidates
            .iter()
            .find_map(|p| app_icon_png(Path::new(p)))
            .expect("should extract at least one system app icon");
        assert!(png.starts_with(PNG_MAGIC), "payload is not PNG");
        let img = image::load_from_memory(&png).expect("extracted PNG should decode");
        assert!(img.width() >= 32 && img.width() <= 128, "w={}", img.width());
        assert!(
            img.height() >= 32 && img.height() <= 128,
            "h={}",
            img.height()
        );
    }

    #[test]
    fn picks_app_icns_not_document() {
        let zed = Path::new("/Applications/Zed.app");
        if !zed.exists() {
            return;
        }
        let icns = find_bundle_icon_file(zed).expect("Zed should have an icns");
        assert_eq!(icns.file_name().unwrap(), "Zed.icns");
        assert!(app_icon_png(zed).is_some());
    }

    #[test]
    fn reads_code_icns_from_plist() {
        let code = Path::new("/Applications/Visual Studio Code.app");
        if !code.exists() {
            return;
        }
        let icns = find_bundle_icon_file(code).expect("VS Code should have Code.icns");
        assert_eq!(icns.file_name().unwrap(), "Code.icns");
    }

    #[test]
    fn parallel_extract_applications_is_fast() {
        let dir = Path::new("/Applications");
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        let paths: Vec<_> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("app"))
            .collect();
        assert!(!paths.is_empty());
        let started = Instant::now();
        let mut bundle = 0usize;
        let mut miss = 0usize;
        for p in &paths {
            if app_icon_from_bundle(p).is_some() {
                bundle += 1;
            } else {
                miss += 1;
                eprintln!("bundle miss {}", p.display());
            }
        }
        let bundle_elapsed = started.elapsed();
        eprintln!(
            "bundle-only {bundle}/{} ({} miss) in {bundle_elapsed:?}",
            paths.len(),
            miss
        );
        assert!(
            bundle_elapsed.as_millis() < 1500,
            "icns extract too slow: {bundle_elapsed:?} for {} apps",
            paths.len()
        );
    }
}
