//! Windows 已安装软件注册表枚举与卸载引擎

use crate::core::apps::{
    parse_cmd_line, parse_install_date, split_command, AppRegRoot, InstalledApp,
};
use crate::core::safety::is_system_root_dir;
use crate::platform::windows::registry::{from_wide, read_reg_dword, read_reg_string, to_wide};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use winapi::shared::minwindef::{DWORD, HKEY, MAX_PATH};
use winapi::shared::winerror::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
use winapi::um::winnt::{
    KEY_ENUMERATE_SUB_KEYS, KEY_QUERY_VALUE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
};
use winapi::um::winreg::{
    RegCloseKey, RegEnumKeyExW, RegEnumValueW, RegOpenKeyExW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
};

/// 枚举 Windows 注册表中所有已安装软件
pub fn list_installed_apps(live: &AtomicBool) -> Vec<InstalledApp> {
    let mut apps = Vec::new();

    // 1. HKLM 64 位
    if live.load(Ordering::Relaxed) {
        apps.extend(scan_registry_uninstall(
            HKEY_LOCAL_MACHINE,
            KEY_READ | KEY_WOW64_64KEY,
            AppRegRoot::Hklm,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
            live,
        ));
    }

    // 2. HKLM 32 位 (WOW6432Node)
    if live.load(Ordering::Relaxed) {
        apps.extend(scan_registry_uninstall(
            HKEY_LOCAL_MACHINE,
            KEY_READ | KEY_WOW64_32KEY,
            AppRegRoot::Hklm32,
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
            live,
        ));
    }

    // 3. HKCU (当前用户)
    if live.load(Ordering::Relaxed) {
        apps.extend(scan_registry_uninstall(
            HKEY_CURRENT_USER,
            KEY_READ,
            AppRegRoot::Hkcu,
            r"Software\Microsoft\Windows\CurrentVersion\Uninstall",
            live,
        ));
    }

    dedup_and_enrich_apps(&mut apps);
    apps
}

fn scan_registry_uninstall(
    root: HKEY,
    sam: DWORD,
    reg_root: AppRegRoot,
    uninstall_path: &str,
    live: &AtomicBool,
) -> Vec<InstalledApp> {
    let mut out = Vec::new();
    let wide_path = to_wide(uninstall_path);
    let mut h_uninstall: HKEY = std::ptr::null_mut();

    unsafe {
        if RegOpenKeyExW(
            root,
            wide_path.as_ptr(),
            0,
            sam | KEY_ENUMERATE_SUB_KEYS | KEY_QUERY_VALUE,
            &mut h_uninstall,
        ) as u32 != ERROR_SUCCESS
        {
            return out;
        }

        let mut index: DWORD = 0;
        let mut name_buf = [0u16; MAX_PATH];

        loop {
            if !live.load(Ordering::Relaxed) {
                break;
            }

            let mut name_len = name_buf.len() as DWORD;
            let res = RegEnumKeyExW(
                h_uninstall,
                index,
                name_buf.as_mut_ptr(),
                &mut name_len,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );

            if res as u32 == ERROR_NO_MORE_ITEMS {
                break;
            }

            if res as u32 == ERROR_SUCCESS {
                let subkey_name = from_wide(&name_buf[..name_len as usize]);
                let sub_wide = to_wide(&format!("{uninstall_path}\\{subkey_name}"));
                let mut h_sub: HKEY = std::ptr::null_mut();

                if RegOpenKeyExW(root, sub_wide.as_ptr(), 0, sam | KEY_QUERY_VALUE, &mut h_sub) as u32
                    == ERROR_SUCCESS
                {
                    if let Some(app) = parse_app_entry(h_sub, &subkey_name, reg_root, &format!("{uninstall_path}\\{subkey_name}")) {
                        out.push(app);
                    }
                    RegCloseKey(h_sub);
                }
            }

            index += 1;
        }

        RegCloseKey(h_uninstall);
    }

    out
}

fn parse_app_entry(
    h_sub: HKEY,
    key_name: &str,
    reg_root: AppRegRoot,
    subpath: &str,
) -> Option<InstalledApp> {
    let name = read_reg_string(h_sub, "DisplayName")?;
    let name = name.trim().to_string();
    if name.is_empty() {
        return None;
    }

    // 过滤系统组件 / 补丁更新
    if read_reg_dword(h_sub, "SystemComponent").unwrap_or(0) == 1 {
        return None;
    }
    if let Some(parent) = read_reg_string(h_sub, "ParentKeyName") {
        if !parent.trim().is_empty() && !parent.eq_ignore_ascii_case("Steam") {
            return None;
        }
    }
    // 过滤 KB 补丁
    if name.starts_with("KB") && name.chars().nth(2).map(|c| c.is_ascii_digit()).unwrap_or(false) {
        return None;
    }

    let version = read_reg_string(h_sub, "DisplayVersion")
        .unwrap_or_default()
        .trim()
        .to_string();
    let publisher = read_reg_string(h_sub, "Publisher")
        .unwrap_or_default()
        .trim()
        .to_string();

    let (install_date, install_date_raw) = parse_install_date(read_reg_string(h_sub, "InstallDate"));

    let install_location = read_reg_string(h_sub, "InstallLocation")
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);

    let display_icon = read_reg_string(h_sub, "DisplayIcon")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let uninstall_string = read_reg_string(h_sub, "UninstallString")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let quiet_uninstall_string = read_reg_string(h_sub, "QuietUninstallString")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let est_kb = read_reg_dword(h_sub, "EstimatedSize").unwrap_or(0);
    let estimated_size = (est_kb as u64) * 1024;

    Some(InstalledApp {
        id: key_name.to_string(),
        name,
        version,
        publisher,
        last_used_date: None,
        last_used_raw: 0,
        install_date,
        install_date_raw,
        install_location,
        display_icon,
        uninstall_string,
        quiet_uninstall_string,
        estimated_size,
        registry_root: reg_root,
        registry_subpath: subpath.to_string(),
        is_system_component: false,
        // 需要先知道最终的卸载命令，统一在 dedup_and_enrich_apps 里判定
        uninstaller_missing: false,
    })
}

pub fn dir_or_file_size(path: &Path) -> u64 {
    if !path.exists() {
        return 0;
    }
    if path.is_file() {
        return std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    }
    walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

/// ROT13 编解码器（Windows UserAssist 键值加密方式）
pub fn rot13(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='m' | 'A'..='M' => (c as u8 + 13) as char,
            'n'..='z' | 'N'..='Z' => (c as u8 - 13) as char,
            _ => c,
        })
        .collect()
}

/// UserAssist 记录的反向索引。
///
/// 原始形态是「完整程序路径 -> 最后运行时间」，而匹配时我们手里只有软件名
/// 拆出来的 token。旧实现对每个软件都整表扫一遍做 `contains`，复杂度是
/// O(软件数 × UserAssist 条目数 × token 数)，几百个软件配上几千条记录就是
/// 数百万次子串搜索。这里预先把每条记录按路径分段拆成 token 建成哈希索引，
/// 匹配退化成 O(token 数) 的哈希查找。
#[derive(Default)]
pub struct UserAssistIndex {
    /// token -> 该 token 命中的最新时间戳
    by_token: std::collections::HashMap<String, u64>,
}

impl UserAssistIndex {
    fn record(&mut self, key: &str, ts: u64) {
        self.by_token
            .entry(key.to_string())
            .and_modify(|e| *e = (*e).max(ts))
            .or_insert(ts);
    }

    /// 把一条 UserAssist 记录的路径拆成可检索的 token 并全部登记。
    fn index_path(&mut self, path_lower: &str, ts: u64) {
        // 整条路径本身
        self.record(path_lower, ts);
        // 可执行文件名（含扩展名与不含扩展名两种形态）
        let p = Path::new(path_lower);
        if let Some(fname) = p.file_name().and_then(|f| f.to_str()) {
            self.record(fname, ts);
            if let Some(stem) = p.file_stem().and_then(|f| f.to_str()) {
                self.record(stem, ts);
            }
        }
        // 路径中的每一段目录名，用来匹配「安装目录叫软件名」的常见情况
        for seg in path_lower.split(|c| c == '\\' || c == '/') {
            if seg.len() >= 3 {
                self.record(seg, ts);
            }
        }
    }

    /// 查询一批 token 命中的最新时间戳。
    pub fn lookup(&self, tokens: &[String]) -> u64 {
        tokens
            .iter()
            .filter_map(|t| self.by_token.get(t.as_str()))
            .copied()
            .max()
            .unwrap_or(0)
    }

    pub fn lookup_one(&self, token: &str) -> u64 {
        self.by_token.get(token).copied().unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.by_token.is_empty()
    }
}

/// 扫描 Windows Explorer UserAssist 注册表，构建可按 token 检索的运行时间索引
fn scan_user_assist_map() -> UserAssistIndex {
    let mut map = UserAssistIndex::default();
    let ua_path = to_wide(r"Software\Microsoft\Windows\CurrentVersion\Explorer\UserAssist");
    let mut h_ua: HKEY = std::ptr::null_mut();

    unsafe {
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            ua_path.as_ptr(),
            0,
            KEY_READ | KEY_ENUMERATE_SUB_KEYS,
            &mut h_ua,
        ) as u32 != ERROR_SUCCESS
        {
            return map;
        }

        let mut guid_idx: DWORD = 0;
        let mut guid_buf = [0u16; MAX_PATH];

        loop {
            let mut guid_len = guid_buf.len() as DWORD;
            let res = RegEnumKeyExW(
                h_ua,
                guid_idx,
                guid_buf.as_mut_ptr(),
                &mut guid_len,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            if res as u32 != ERROR_SUCCESS {
                break;
            }

            let guid_str = from_wide(&guid_buf[..guid_len as usize]);
            let count_subpath = format!(
                r"Software\Microsoft\Windows\CurrentVersion\Explorer\UserAssist\{}\Count",
                guid_str
            );
            let wide_count = to_wide(&count_subpath);
            let mut h_count: HKEY = std::ptr::null_mut();

            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                wide_count.as_ptr(),
                0,
                KEY_READ | KEY_QUERY_VALUE,
                &mut h_count,
            ) as u32 == ERROR_SUCCESS
            {
                let mut val_idx: DWORD = 0;
                let mut name_buf = [0u16; 1024];
                let mut data_buf = [0u8; 512];

                loop {
                    let mut name_len = name_buf.len() as DWORD;
                    let mut data_len = data_buf.len() as DWORD;
                    let mut val_type: DWORD = 0;
                    let enum_res = RegEnumValueW(
                        h_count,
                        val_idx,
                        name_buf.as_mut_ptr(),
                        &mut name_len,
                        std::ptr::null_mut(),
                        &mut val_type,
                        data_buf.as_mut_ptr(),
                        &mut data_len,
                    );
                    if enum_res as u32 != ERROR_SUCCESS {
                        break;
                    }

                    let raw_name = from_wide(&name_buf[..name_len as usize]);
                    let decoded = rot13(&raw_name);
                    let mut clean_name = decoded.to_lowercase();
                    if let Some(pos) = clean_name.find(':') {
                        if pos > 0 && !clean_name.chars().nth(pos - 1).unwrap_or(' ').is_ascii_alphabetic() {
                            clean_name = clean_name[pos + 1..].to_string();
                        }
                    }

                    // Windows 7/10/11 结构体通常为 72 字节，FILETIME 在 offset 60..68
                    if data_len >= 68 {
                        let ft_bytes: [u8; 8] = data_buf[60..68].try_into().unwrap_or([0; 8]);
                        let ft = u64::from_le_bytes(ft_bytes);
                        const WINDOWS_EPOCH_DIFF: u64 = 116_444_736_000_000_000;
                        if ft > WINDOWS_EPOCH_DIFF {
                            let unix_secs = (ft - WINDOWS_EPOCH_DIFF) / 10_000_000;
                            if unix_secs > 0 {
                                map.index_path(&clean_name, unix_secs);
                            }
                        }
                    }

                    val_idx += 1;
                }

                RegCloseKey(h_count);
            }

            guid_idx += 1;
        }

        RegCloseKey(h_ua);
    }

    map
}

/// 提取核心关键词/Token（过滤掉 64-bit, x64, v1.0, setup 等噪声词）
pub fn extract_app_tokens(name: &str) -> Vec<String> {
    let lower = name.to_lowercase();
    let mut tokens = Vec::new();
    for part in lower.split(|c: char| !c.is_alphanumeric() && c < '\u{4e00}') {
        let trimmed = part.trim();
        if trimmed.len() >= 3
            && trimmed != "64bit"
            && trimmed != "32bit"
            && trimmed != "x64"
            && trimmed != "x86"
            && trimmed != "bit"
            && trimmed != "setup"
            && trimmed != "installer"
            && trimmed != "version"
            && trimmed != "microsoft"
            && trimmed != "corporation"
            && trimmed != "windows"
        {
            tokens.push(trimmed.to_string());
        }
    }
    tokens
}

/// 卸载命令指向的程序是不是已经没了。
///
/// 三种情况都算「能跑」：msiexec 走 MSI 数据库、不依赖单个文件；解析出的
/// 路径确实存在；命令是 winget / powershell 这类靠 PATH 解析的裸命令名。
fn uninstaller_is_missing(app: &InstalledApp) -> bool {
    let Some(cmd) = app
        .quiet_uninstall_string
        .as_ref()
        .or(app.uninstall_string.as_ref())
    else {
        return true;
    };
    if cmd.to_lowercase().contains("msiexec") {
        return false;
    }
    let (exe, _) = split_command(cmd);
    if exe.is_empty() {
        return true;
    }
    // 不含路径分隔符 = 靠 PATH 解析的命令名，不能按文件存在与否来判断
    if !exe.contains('\\') && !exe.contains('/') {
        return false;
    }
    !Path::new(&exe).exists()
}

/// 一趟遍历同时算出安装目录的总体积和「最近被访问过」的时间。
///
/// 体积要看完整棵树，而访问时间只看前两层就够了（再深的多是资源文件，
/// 对判断「这软件还在不在用」没有额外信息量），所以深层只累加体积。
fn measure_install_dir(loc: &Path) -> (u64, u64) {
    const ACCESS_PROBE_DEPTH: usize = 2;
    let mut size = 0u64;
    let mut newest = 0u64;

    for entry in walkdir::WalkDir::new(loc).into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(md) = entry.metadata() else { continue };
        size += md.len();

        if entry.depth() <= ACCESS_PROBE_DEPTH {
            if let Ok(t) = md.accessed().or_else(|_| md.modified()) {
                if let Ok(dur) = t.duration_since(std::time::UNIX_EPOCH) {
                    newest = newest.max(dur.as_secs());
                }
            }
        }
    }
    (size, newest)
}

/// 智能推断软件安装根目录（当注册表 InstallLocation 为空时通过 DisplayIcon、UninstallString 或标准安装路径推导）
fn deduce_install_location(app: &InstalledApp) -> Option<PathBuf> {
    if let Some(loc) = &app.install_location {
        if loc.exists() && !is_system_root_dir(loc) {
            return Some(loc.clone());
        }
    }

    // 1. 从 DisplayIcon 推断
    if let Some(icon) = &app.display_icon {
        let clean = icon.split(',').next().unwrap_or("").trim_matches('"').trim();
        let p = Path::new(clean);
        if p.exists() {
            if let Some(parent) = p.parent() {
                if !is_system_root_dir(parent) {
                    return Some(parent.to_path_buf());
                }
            }
        }
    }

    // 2. 从 UninstallString 推断
    if let Some(un) = &app.uninstall_string {
        if !un.to_lowercase().contains("msiexec") {
            let parts = parse_cmd_line(un);
            if let Some(first) = parts.first() {
                let p = Path::new(first);
                if p.exists() {
                    if let Some(parent) = p.parent() {
                        if !is_system_root_dir(parent) {
                            return Some(parent.to_path_buf());
                        }
                    }
                }
            }
        }
    }

    // 3. 从常见系统目录推断
    let clean_name = app.name.trim();
    let mut candidates = Vec::new();
    if let Some(local) = dirs::data_local_dir() {
        candidates.push(local.join("Programs").join(clean_name));
    }
    if let Some(roaming) = dirs::data_dir() {
        candidates.push(roaming.join(clean_name));
    }
    candidates.push(PathBuf::from(r"C:\Program Files").join(clean_name));
    candidates.push(PathBuf::from(r"C:\Program Files (x86)").join(clean_name));

    for cand in candidates {
        if cand.exists() && cand.is_dir() && !is_system_root_dir(&cand) {
            return Some(cand);
        }
    }

    None
}

/// 预先扫描桌面与开始菜单中的快捷方式 (.lnk) 活跃时间
fn scan_shortcuts_map() -> std::collections::HashMap<String, u64> {
    use std::collections::HashMap;
    let mut map = HashMap::new();

    let mut search_dirs = Vec::new();
    if let Some(user_prof) = dirs::home_dir() {
        search_dirs.push(user_prof.join("Desktop"));
    }
    search_dirs.push(PathBuf::from(r"C:\Users\Public\Desktop"));
    if let Some(appdata) = dirs::data_dir() {
        search_dirs.push(appdata.join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    search_dirs.push(PathBuf::from(r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs"));

    for base in search_dirs {
        if !base.exists() {
            continue;
        }
        for entry in walkdir::WalkDir::new(&base)
            .max_depth(3)
            .into_iter()
            .flatten()
        {
            if entry.file_type().is_file() {
                if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
                    if ext.eq_ignore_ascii_case("lnk") {
                        if let Ok(md) = entry.metadata() {
                            if let Ok(accessed) = md.accessed().or_else(|_| md.modified()) {
                                if let Ok(dur) = accessed.duration_since(std::time::UNIX_EPOCH) {
                                    let ts = dur.as_secs();
                                    if let Some(stem) = entry.path().file_stem().and_then(|s| s.to_str()) {
                                        let stem_lower = stem.to_lowercase();
                                        map.entry(stem_lower.clone())
                                            .and_modify(|e: &mut u64| *e = (*e).max(ts))
                                            .or_insert(ts);
                                        for token in extract_app_tokens(&stem_lower) {
                                            map.entry(token)
                                                .and_modify(|e: &mut u64| *e = (*e).max(ts))
                                                .or_insert(ts);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    map
}

/// 对软件列表进行去重并多线程测算真实磁盘占用与补全安装时间、最后使用时间
fn dedup_and_enrich_apps(apps: &mut Vec<InstalledApp>) {
    let mut map: std::collections::HashMap<String, InstalledApp> = std::collections::HashMap::new();
    for app in apps.drain(..) {
        let key = format!("{}_{}", app.name.to_lowercase(), app.version.to_lowercase());
        match map.entry(key) {
            std::collections::hash_map::Entry::Vacant(v) => {
                v.insert(app);
            }
            std::collections::hash_map::Entry::Occupied(mut o) => {
                let existing = o.get_mut();
                if existing.estimated_size == 0 && app.estimated_size > 0 {
                    existing.estimated_size = app.estimated_size;
                }
                if existing.uninstall_string.is_none() && app.uninstall_string.is_some() {
                    existing.uninstall_string = app.uninstall_string;
                }
                if existing.install_location.is_none() && app.install_location.is_some() {
                    existing.install_location = app.install_location;
                }
                if existing.display_icon.is_none() && app.display_icon.is_some() {
                    existing.display_icon = app.display_icon;
                }
                if existing.install_date.is_none() && app.install_date.is_some() {
                    existing.install_date = app.install_date;
                    existing.install_date_raw = app.install_date_raw;
                }
            }
        }
    }
    let mut list: Vec<InstalledApp> = map.into_values().collect();

    let ua_map = scan_user_assist_map();
    let sc_map = scan_shortcuts_map();

    // 并行精确测算目录大小与回填安装日期、最后使用时间
    list.par_iter_mut().for_each(|app| {
        let mut max_used_ts: u64 = 0;
        let tokens = extract_app_tokens(&app.name);

        // 1. 快捷方式命中
        for token in &tokens {
            if let Some(&ts) = sc_map.get(token) {
                max_used_ts = max_used_ts.max(ts);
            }
        }
        let name_lower = app.name.to_lowercase();
        if let Some(&ts) = sc_map.get(&name_lower) {
            max_used_ts = max_used_ts.max(ts);
        }

        // 2. UserAssist 命中：走反向索引做哈希查找，不再整表扫描
        max_used_ts = max_used_ts.max(ua_map.lookup(&tokens));
        max_used_ts = max_used_ts.max(ua_map.lookup_one(&name_lower));
        if let Some(icon) = &app.display_icon {
            if let Some(fname) = Path::new(icon).file_name().and_then(|f| f.to_str()) {
                max_used_ts = max_used_ts.max(ua_map.lookup_one(&fname.to_lowercase()));
            }
        }

        // 3. 智能推断或使用现有安装目录
        let deduced = deduce_install_location(app);
        if let Some(loc) = deduced {
            let loc_str = loc.to_string_lossy().to_lowercase();
            max_used_ts = max_used_ts.max(ua_map.lookup_one(&loc_str));

            // 体积测算与「最近访问时间」以前是对同一棵目录树走两趟 walkdir，
            // 这里合并成一趟。
            let (real_size, newest_access) = measure_install_dir(&loc);
            max_used_ts = max_used_ts.max(newest_access);

            if app.install_date.is_none() {
                if let Ok(md) = loc.metadata() {
                    if let Ok(created) = md.created().or_else(|_| md.modified()) {
                        let dt: chrono::DateTime<chrono::Local> = created.into();
                        app.install_date = Some(dt.format("%Y-%m-%d").to_string());
                        app.install_date_raw = dt.format("%Y%m%d").to_string().parse().unwrap_or(0);
                    }
                }
            }

            if real_size > 0 && (app.estimated_size == 0 || real_size > app.estimated_size) {
                app.estimated_size = real_size;
            }

            if app.install_location.is_none() {
                app.install_location = Some(loc);
            }
        }

        app.uninstaller_missing = uninstaller_is_missing(app);

        if max_used_ts > 0 {
            app.last_used_raw = max_used_ts;
            let dt: chrono::DateTime<chrono::Local> =
                (std::time::UNIX_EPOCH + std::time::Duration::from_secs(max_used_ts)).into();
            app.last_used_date = Some(dt.format("%Y-%m-%d").to_string());
        }
    });

    // 第二阶段：基于安装目录进行跨版本合并（例如浏览器自动更新留下的旧注册表残留项）
    let mut loc_map: std::collections::HashMap<PathBuf, InstalledApp> = std::collections::HashMap::new();
    let mut final_list: Vec<InstalledApp> = Vec::new();

    for app in list {
        if let Some(loc) = &app.install_location {
            if !is_system_root_dir(loc) {
                match loc_map.entry(loc.clone()) {
                    std::collections::hash_map::Entry::Vacant(v) => {
                        v.insert(app);
                    }
                    std::collections::hash_map::Entry::Occupied(mut o) => {
                        let existing = o.get_mut();
                        let is_newer = app.version > existing.version
                            || (existing.version.is_empty() && !app.version.is_empty());
                        if is_newer {
                            if app.estimated_size == 0 && existing.estimated_size > 0 {
                                let mut app = app;
                                app.estimated_size = existing.estimated_size;
                                *existing = app;
                            } else {
                                *existing = app;
                            }
                        } else if existing.estimated_size == 0 && app.estimated_size > 0 {
                            existing.estimated_size = app.estimated_size;
                        }
                    }
                }
                continue;
            }
        }
        final_list.push(app);
    }
    final_list.extend(loc_map.into_values());

    *apps = final_list;
}

/// 在 Windows 资源管理器中打开指定目录或定位高亮指定文件
pub fn reveal_in_explorer(path: &std::path::Path) {
    if !path.exists() {
        return;
    }
    if path.is_dir() {
        let _ = std::process::Command::new("explorer.exe")
            .arg(path)
            .spawn();
    } else {
        let _ = std::process::Command::new("explorer.exe")
            .arg(format!("/select,{}", path.display()))
            .spawn();
    }
}

/// 运行软件官方卸载向导并等待其退出
pub fn run_uninstaller_and_wait(app: &InstalledApp) -> Result<(), String> {
    let cmd = app
        .quiet_uninstall_string
        .as_ref()
        .or(app.uninstall_string.as_ref())
        .ok_or_else(|| "该软件未提供有效的卸载命令行".to_string())?;

    use std::os::windows::process::CommandExt;
    {
        let mut child = if cmd.to_lowercase().contains("msiexec") {
            std::process::Command::new("cmd")
                .raw_arg(format!("/c {cmd}"))
                .spawn()
                .map_err(|e| format!("启动卸载程序失败: {e}"))?
        } else {
            // split_command 能正确处理没加引号的带空格路径。按空格切的老办法
            // 会把 `C:\Program Files\X\unins000.exe` 截成 `C:\Program`，
            // 本机 145 款软件里有 24 款会因此直接卸载失败。
            let (exe, args) = split_command(cmd);
            if exe.is_empty() {
                return Err("无效的卸载命令".into());
            }
            let mut c = std::process::Command::new(&exe);
            for arg in &args {
                c.arg(arg);
            }
            c.spawn()
                .map_err(|e| format!("启动卸载程序失败（{exe}）: {e}"))?
        };

        let _ = child.wait().map_err(|e| format!("等待卸载程序退出失败: {e}"))?;
        Ok(())
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rot13() {
        assert_eq!(rot13("UEME_RUNPATH"), "HRZR_EHACNGU");
        assert_eq!(rot13("HRZR_EHACNGU"), "UEME_RUNPATH");
        assert_eq!(rot13("C:\\Program Files"), "P:\\Cebtenz Svyrf");
    }

    #[test]
    fn test_extract_app_tokens() {
        let tokens = extract_app_tokens("PotPlayer-64 bit");
        assert!(tokens.contains(&"potplayer".to_string()));

        let tokens2 = extract_app_tokens("PowerShell 7.6.3.0-x64");
        assert!(tokens2.contains(&"powershell".to_string()));
    }

    #[test]
    fn live_registry_scan() {
        let live = AtomicBool::new(true);
        let apps = list_installed_apps(&live);
        assert!(!apps.is_empty(), "Expected to find installed applications on Windows");

        // 验证 PotPlayer 等常见软件能够解析出最后使用时间或估算大小
        if let Some(pot) = apps.iter().find(|a| a.name.contains("PotPlayer")) {
            assert!(pot.estimated_size > 0 || pot.last_used_date.is_some() || pot.install_location.is_some());
        }
    }
}

#[cfg(test)]
mod uninstaller_probe {
    use super::*;

    /// 手动跑：打印某个软件的卸载命令及其可执行文件是否存在。
    #[test]
    #[ignore]
    fn probe_uninstaller() {
        let kw = std::env::var("QC_APP").unwrap_or_else(|_| "Kiro".into());
        let live = AtomicBool::new(true);
        for a in list_installed_apps(&live).iter().filter(|a| a.name.contains(&kw)) {
            println!("名称: {}", a.name);
            println!("  UninstallString      : {:?}", a.uninstall_string);
            println!("  QuietUninstallString : {:?}", a.quiet_uninstall_string);
            if let Some(u) = &a.uninstall_string {
                let parts = parse_cmd_line(u);
                if let Some(exe) = parts.first() {
                    println!("  解析出的可执行文件   : {exe}");
                    println!("  该文件是否存在       : {}", Path::new(exe).exists());
                }
            }
        }
    }
}

#[cfg(test)]
mod uninstaller_stats {
    use super::*;

    /// 手动跑：列出卸载器确实失效的软件。
    #[test]
    #[ignore]
    fn count_missing_uninstallers() {
        let live = AtomicBool::new(true);
        let apps = list_installed_apps(&live);
        let broken: Vec<_> = apps.iter().filter(|a| a.uninstaller_missing).collect();
        for a in &broken {
            let cmd = a
                .quiet_uninstall_string
                .as_ref()
                .or(a.uninstall_string.as_ref())
                .cloned()
                .unwrap_or_default();
            let (exe, _) = split_command(&cmd);
            println!("[卸载器失效] {} -> {}", a.name, exe);
        }
        println!("共 {} 款，其中卸载器失效 {} 款", apps.len(), broken.len());
    }
}
