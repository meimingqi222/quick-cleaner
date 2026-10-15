//! 应用自动更新：版本检测、下载校验、安装交接
//!
//! 分层约束：本模块只做与 OS 无关的解析/比较/文件校验与解压，纯逻辑来自
//! `gpui-app-update-core`，这里保留资产名、ureq 2 请求、sidecar 发现策略与文案；
//! 「替换正在运行的二进制并重启」走 `platform` 门面
//! [`crate::platform::apply_update_and_restart`]。
//!
//! 更新源是 GitHub Releases（`meimingqi222/quick-cleaner`），**没有独立
//! 更新服务器**。正式通道只认 `releases/latest` 里非 draft / 非 prerelease
//! 的那一条。

use std::io::Read;
use std::path::{Path, PathBuf};

pub const GITHUB_OWNER: &str = "meimingqi222";
pub const GITHUB_REPO: &str = "quick-cleaner";
/// 无 token 时 GitHub API 要求 UA，否则 403。
const USER_AGENT: &str = concat!("QuickCleaner-Updater/", env!("CARGO_PKG_VERSION"));

/// 启动后首次检查延迟。
pub const FIRST_CHECK_DELAY_MS: u64 = 10_000;
/// 周期检查间隔（前一次检查结束后再计时）。
pub const CHECK_INTERVAL_MS: u64 = 4 * 60 * 60 * 1000;

/// 更新进度。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DownloadProgress {
    pub percent: f32,
    pub transferred: u64,
    pub total: Option<u64>,
}

/// 更新状态机。UI 与调度都只认这个枚举。
#[derive(Clone, Debug, PartialEq)]
pub enum UpdateStatus {
    Idle {
        current_version: String,
    },
    Checking {
        current_version: String,
    },
    NotAvailable {
        current_version: String,
    },
    Available {
        current_version: String,
        latest_version: String,
        release_url: String,
        notes: String,
        asset_name: String,
        asset_url: String,
        checksum_url: String,
    },
    Downloading {
        current_version: String,
        latest_version: String,
        progress: DownloadProgress,
        asset_name: String,
        asset_url: String,
        checksum_url: String,
        release_url: String,
        notes: String,
    },
    Verifying {
        current_version: String,
        latest_version: String,
    },
    Downloaded {
        current_version: String,
        latest_version: String,
        /// 解压后待安装的路径：Windows 为 exe，macOS 为 `.app`。
        payload: PathBuf,
    },
    Installing {
        current_version: String,
        latest_version: String,
    },
    Error {
        current_version: String,
        operation: UpdateOperation,
        message: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateOperation {
    Check,
    Download,
    Install,
}

impl UpdateStatus {
    pub fn current_version(&self) -> &str {
        match self {
            UpdateStatus::Idle { current_version }
            | UpdateStatus::Checking { current_version }
            | UpdateStatus::NotAvailable { current_version }
            | UpdateStatus::Available {
                current_version, ..
            }
            | UpdateStatus::Downloading {
                current_version, ..
            }
            | UpdateStatus::Verifying {
                current_version, ..
            }
            | UpdateStatus::Downloaded {
                current_version, ..
            }
            | UpdateStatus::Installing {
                current_version, ..
            }
            | UpdateStatus::Error {
                current_version, ..
            } => current_version,
        }
    }

    pub fn latest_version(&self) -> Option<&str> {
        match self {
            UpdateStatus::Available { latest_version, .. }
            | UpdateStatus::Downloading { latest_version, .. }
            | UpdateStatus::Verifying { latest_version, .. }
            | UpdateStatus::Downloaded { latest_version, .. }
            | UpdateStatus::Installing { latest_version, .. } => Some(latest_version),
            _ => None,
        }
    }

    /// 侧栏是否应显示更新入口。
    pub fn wants_attention(&self) -> bool {
        matches!(
            self,
            UpdateStatus::Available { .. }
                | UpdateStatus::Downloading { .. }
                | UpdateStatus::Verifying { .. }
                | UpdateStatus::Downloaded { .. }
                | UpdateStatus::Installing { .. }
                | UpdateStatus::Error { .. }
        )
    }
}

pub use gpui_app_update_core::{
    is_skipped, is_version_newer, looks_like_dev_build, normalize_version, parse_checksum_sidecar,
    select_asset_by_candidates, sha256_file, GithubAsset, GithubRelease,
};

/// 从 latest release 里选出本平台发行包。
///
/// 名称与 `.github/workflows/release.yml` 的 artifact 命名钉死；
/// 改 CI 产物名必须同步改这里和单测。
pub fn select_asset(release: &GithubRelease, target: UpdateTarget) -> Option<&GithubAsset> {
    let wanted = match target {
        UpdateTarget::WindowsX64 => "quick-cleaner-windows-x86_64.zip",
        UpdateTarget::MacosUniversal => "quick-cleaner-macos-universal.zip",
        UpdateTarget::MacosAarch64 => "quick-cleaner-macos-aarch64.zip",
        UpdateTarget::MacosX86_64 => "quick-cleaner-macos-x86_64.zip",
    };
    select_asset_by_candidates(release, &[wanted])
}

/// 当前构建应订阅的发行包目标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateTarget {
    WindowsX64,
    MacosUniversal,
    MacosAarch64,
    MacosX86_64,
}

pub fn current_target() -> UpdateTarget {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        UpdateTarget::WindowsX64
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        UpdateTarget::MacosAarch64
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        UpdateTarget::MacosX86_64
    }
    #[cfg(not(any(
        all(target_os = "windows", target_arch = "x86_64"),
        target_os = "macos"
    )))]
    {
        // 其它平台没有发行包；调用方应在 is_packaged 门禁处拦下。
        UpdateTarget::WindowsX64
    }
}

/// 本平台的候选资产名列表（优先序）。Universal 缺失时只回退**本架构**包，
/// 禁止把另一个 CPU 的 zip 放进来——Intel 装上 aarch64 会打不开。
pub fn candidate_asset_names(target: UpdateTarget) -> &'static [&'static str] {
    match target {
        UpdateTarget::WindowsX64 => &["quick-cleaner-windows-x86_64.zip"],
        UpdateTarget::MacosUniversal => &["quick-cleaner-macos-universal.zip"],
        UpdateTarget::MacosAarch64 => &[
            "quick-cleaner-macos-universal.zip",
            "quick-cleaner-macos-aarch64.zip",
        ],
        UpdateTarget::MacosX86_64 => &[
            "quick-cleaner-macos-universal.zip",
            "quick-cleaner-macos-x86_64.zip",
        ],
    }
}

/// 校验下载包与 sidecar；消息沿用原有英文文案。
///
/// SHA-256 sidecar 与安装包来自同一发布源，只能发现损坏或截断，不是发布者认证。
pub fn verify_checksum(zip_path: &Path, sidecar: &str) -> Result<(), String> {
    gpui_app_update_core::verify_sha256_sidecar(zip_path, sidecar).map_err(|e| e.to_string())
}

/// 在 `cache_dir` 下新建独占 staging 目录并解压，返回「主载荷」路径：
/// - Windows：`quick-cleaner.exe`（根目录或多一层目录）
/// - macOS：`QuickCleaner.app` bundle
///
/// 整个归档先校验（逃逸、绝对路径、重复项、符号链接、数量和体积上限）再写入；
/// 只清理本产品缓存里旧的 staging 与旧版固定的 `extracted` 目录。
pub fn extract_update_zip(zip_path: &Path, cache_dir: &Path) -> Result<PathBuf, String> {
    let _ = std::fs::remove_dir_all(cache_dir.join("extracted"));
    let staging = gpui_app_update_core::create_staging_dir(cache_dir)
        .map_err(|e| format!("create extract dir: {e}"))?;
    gpui_app_update_core::extract_zip(
        zip_path,
        &staging,
        gpui_app_update_core::ExtractLimits::default(),
    )
    .map_err(|e| e.to_string())?;
    match payload_spec() {
        Some(spec) => gpui_app_update_core::find_payload(&staging, spec).map_err(|e| e.to_string()),
        None => Err("unsupported platform for app update".into()),
    }
}

fn payload_spec() -> Option<gpui_app_update_core::PayloadSpec<'static>> {
    #[cfg(windows)]
    {
        Some(gpui_app_update_core::PayloadSpec::WindowsExe(
            "quick-cleaner.exe",
        ))
    }
    #[cfg(target_os = "macos")]
    {
        Some(gpui_app_update_core::PayloadSpec::MacApp(
            "QuickCleaner.app",
        ))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        None
    }
}

/// 阻塞拉取 latest release JSON。
pub fn fetch_latest_release(timeout_ms: u64) -> Result<GithubRelease, String> {
    let url = format!("https://api.github.com/repos/{GITHUB_OWNER}/{GITHUB_REPO}/releases/latest");
    let resp = ureq::get(&url)
        .set("User-Agent", USER_AGENT)
        .set("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_millis(timeout_ms))
        .call()
        .map_err(|e| format!("github api: {e}"))?;
    resp.into_json::<GithubRelease>()
        .map_err(|e| format!("decode release json: {e}"))
}

/// 下载 URL 到文件；`on_progress` 为 (transferred, total)。
pub fn download_to_file(
    url: &str,
    dest: &Path,
    on_progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create download dir: {e}"))?;
    }
    let resp = ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .timeout(std::time::Duration::from_secs(300))
        .call()
        .map_err(|e| format!("download: {e}"))?;
    let total = resp
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok());
    let mut reader = resp.into_reader();
    let mut file =
        std::fs::File::create(dest).map_err(|e| format!("create {}: {e}", dest.display()))?;
    let mut buf = [0u8; 64 * 1024];
    let mut transferred = 0u64;
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("read download: {e}"))?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buf[..n])
            .map_err(|e| format!("write download: {e}"))?;
        transferred += n as u64;
        on_progress(transferred, total);
    }
    Ok(())
}

pub fn fetch_text(url: &str, timeout_ms: u64) -> Result<String, String> {
    let resp = ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .timeout(std::time::Duration::from_millis(timeout_ms))
        .call()
        .map_err(|e| format!("fetch: {e}"))?;
    resp.into_string().map_err(|e| format!("read body: {e}"))
}

/// 完整的「检查 → 有新版本则产出 Available」纯逻辑；网络由调用方注入。
///
/// sidecar 按「资产 URL + `.sha256`」拼接，检查阶段不要求它出现在资产列表里。
pub fn evaluate_release(
    current_version: &str,
    release: &GithubRelease,
    candidates: &[&str],
) -> Result<UpdateStatus, String> {
    let candidate = gpui_app_update_core::evaluate_release(
        current_version,
        release,
        candidates,
        gpui_app_update_core::ChecksumSource::UrlSuffix,
    )
    .map_err(|e| e.to_string())?;
    Ok(match candidate {
        None => UpdateStatus::NotAvailable {
            current_version: current_version.to_string(),
        },
        Some(c) => UpdateStatus::Available {
            current_version: current_version.to_string(),
            latest_version: c.latest_version,
            release_url: c.release_url,
            notes: c.notes,
            asset_name: c.asset_name,
            asset_url: c.asset_url,
            checksum_url: c.checksum_url,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_semver() {
        assert!(is_version_newer("0.0.8", "0.0.7"));
        assert!(is_version_newer("v0.1.0", "0.0.9"));
        assert!(is_version_newer("1.0.0", "0.9.9"));
        assert!(!is_version_newer("0.0.7", "0.0.7"));
        assert!(!is_version_newer("0.0.6", "0.0.7"));
        assert!(!is_version_newer("v0.0.7", "0.0.7"));
        assert!(is_version_newer("0.0.10", "0.0.9"));
    }

    #[test]
    fn missing_patch_treated_as_zero() {
        assert!(is_version_newer("1.2", "1.1.9"));
        assert!(!is_version_newer("1.2", "1.2.0"));
    }

    #[test]
    fn parse_sidecar() {
        assert_eq!(
            parse_checksum_sidecar("abc123...  file.zip\n"),
            None,
            "too short rejected"
        );
        let hex = "a".repeat(64);
        assert_eq!(
            parse_checksum_sidecar(&format!("{hex}  quick-cleaner-windows-x86_64.zip\r\n")),
            Some(hex.clone())
        );
        assert_eq!(parse_checksum_sidecar(""), None);
    }

    #[test]
    fn draft_and_prerelease_are_ignored() {
        let release = GithubRelease {
            tag_name: "v9.9.9".into(),
            html_url: "https://example".into(),
            name: None,
            body: None,
            prerelease: true,
            draft: false,
            assets: vec![GithubAsset {
                name: "quick-cleaner-windows-x86_64.zip".into(),
                browser_download_url: "https://example/a.zip".into(),
                size: 1,
            }],
        };
        let status =
            evaluate_release("0.0.1", &release, &["quick-cleaner-windows-x86_64.zip"]).unwrap();
        assert!(matches!(status, UpdateStatus::NotAvailable { .. }));

        let mut draft = release;
        draft.prerelease = false;
        draft.draft = true;
        let status =
            evaluate_release("0.0.1", &draft, &["quick-cleaner-windows-x86_64.zip"]).unwrap();
        assert!(matches!(status, UpdateStatus::NotAvailable { .. }));
    }

    #[test]
    fn evaluate_picks_matching_asset() {
        let release = GithubRelease {
            tag_name: "v0.0.8".into(),
            html_url: "https://example/rel".into(),
            name: Some("0.0.8".into()),
            body: Some("notes".into()),
            prerelease: false,
            draft: false,
            assets: vec![
                GithubAsset {
                    name: "quick-cleaner-macos-universal.zip".into(),
                    browser_download_url: "https://example/mac.zip".into(),
                    size: 2,
                },
                GithubAsset {
                    name: "quick-cleaner-windows-x86_64.zip".into(),
                    browser_download_url: "https://example/win.zip".into(),
                    size: 1,
                },
            ],
        };
        let status = evaluate_release(
            "0.0.7",
            &release,
            &[
                "quick-cleaner-macos-universal.zip",
                "quick-cleaner-macos-aarch64.zip",
            ],
        )
        .unwrap();
        match status {
            UpdateStatus::Available {
                latest_version,
                asset_name,
                asset_url,
                checksum_url,
                notes,
                ..
            } => {
                assert_eq!(latest_version, "0.0.8");
                assert_eq!(asset_name, "quick-cleaner-macos-universal.zip");
                assert_eq!(asset_url, "https://example/mac.zip");
                assert_eq!(checksum_url, "https://example/mac.zip.sha256");
                assert_eq!(notes, "notes");
            }
            other => panic!("expected Available, got {other:?}"),
        }
    }

    #[test]
    fn no_newer_version_is_not_available() {
        let release = GithubRelease {
            tag_name: "v0.0.7".into(),
            html_url: "https://example".into(),
            name: None,
            body: None,
            prerelease: false,
            draft: false,
            assets: vec![],
        };
        let status = evaluate_release("0.0.7", &release, &["a.zip"]).unwrap();
        assert!(matches!(status, UpdateStatus::NotAvailable { .. }));
    }

    #[test]
    fn dev_build_paths_are_detected() {
        assert!(looks_like_dev_build(Path::new(
            r"D:\code\quick-cleaner\target\debug\quick-cleaner.exe"
        )));
        assert!(looks_like_dev_build(Path::new(
            "/tmp/qc/target/release/quick-cleaner"
        )));
        assert!(!looks_like_dev_build(Path::new(
            r"C:\Users\me\AppData\Local\QuickCleaner\quick-cleaner.exe"
        )));
    }

    #[test]
    fn skipped_version_gate() {
        assert!(is_skipped("0.0.8", Some("v0.0.8")));
        assert!(!is_skipped("0.0.8", Some("0.0.7")));
        assert!(!is_skipped("0.0.8", None));
    }

    #[test]
    fn mac_fallback_never_crosses_architecture() {
        let aarch = candidate_asset_names(UpdateTarget::MacosAarch64);
        assert_eq!(aarch[0], "quick-cleaner-macos-universal.zip");
        assert!(aarch.contains(&"quick-cleaner-macos-aarch64.zip"));
        assert!(!aarch.iter().any(|n| n.contains("x86_64")));

        let intel = candidate_asset_names(UpdateTarget::MacosX86_64);
        assert_eq!(intel[0], "quick-cleaner-macos-universal.zip");
        assert!(intel.contains(&"quick-cleaner-macos-x86_64.zip"));
        assert!(!intel.iter().any(|n| n.contains("aarch64")));
    }

    #[test]
    fn intel_mac_does_not_pick_aarch64_when_universal_missing() {
        let release = GithubRelease {
            tag_name: "v0.0.8".into(),
            html_url: "https://example".into(),
            name: None,
            body: None,
            prerelease: false,
            draft: false,
            assets: vec![
                GithubAsset {
                    name: "quick-cleaner-macos-aarch64.zip".into(),
                    browser_download_url: "https://example/arm.zip".into(),
                    size: 1,
                },
                GithubAsset {
                    name: "quick-cleaner-macos-x86_64.zip".into(),
                    browser_download_url: "https://example/intel.zip".into(),
                    size: 1,
                },
            ],
        };
        let status = evaluate_release(
            "0.0.7",
            &release,
            candidate_asset_names(UpdateTarget::MacosX86_64),
        )
        .unwrap();
        match status {
            UpdateStatus::Available { asset_name, .. } => {
                assert_eq!(asset_name, "quick-cleaner-macos-x86_64.zip");
            }
            other => panic!("expected x86_64 asset, got {other:?}"),
        }
    }

    #[test]
    fn parse_sidecar_strips_bom() {
        let hex = "b".repeat(64);
        let with_bom = format!("\u{feff}{hex}  file.zip\n");
        assert_eq!(parse_checksum_sidecar(&with_bom), Some(hex));
    }

    #[test]
    fn extract_rejects_zip_slip() {
        let dir = crate::core::testing::fixture("qc_updater_zip_slip");
        let zip_path = dir.join("evil.zip");
        {
            use std::io::Write;
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("../escape.txt", opts).unwrap();
            zip.write_all(b"nope").unwrap();
            zip.finish().unwrap();
        }
        let cache = dir.join("cache");
        let err = extract_update_zip(&zip_path, &cache).unwrap_err();
        assert!(err.contains("escape") || err.contains("absolute"), "{err}");
        assert!(!dir.join("escape.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 发行包布局与 `.github/workflows/release.yml` 一致：Windows 根目录 exe + 说明文件，
    /// macOS 为 `zip -r -y` 打出的 `QuickCleaner.app` bundle。
    #[test]
    fn extract_roundtrip_and_find_exe_or_app() {
        let dir = crate::core::testing::fixture("qc_updater_extract_ok");
        let zip_path = dir.join("pack.zip");
        {
            use std::io::Write;
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let opts = zip::write::SimpleFileOptions::default();
            for name in [
                "quick-cleaner.exe",
                "README.md",
                "QuickCleaner.app/Contents/Info.plist",
                "QuickCleaner.app/Contents/MacOS/quick-cleaner",
            ] {
                zip.start_file(name, opts).unwrap();
                zip.write_all(b"binary").unwrap();
            }
            zip.finish().unwrap();
        }
        let cache = dir.join("cache");
        std::fs::create_dir_all(cache.join("extracted")).unwrap();
        let result = extract_update_zip(&zip_path, &cache);
        #[cfg(windows)]
        assert!(result.unwrap().ends_with("quick-cleaner.exe"));
        #[cfg(target_os = "macos")]
        assert!(result.unwrap().ends_with("QuickCleaner.app"));
        #[cfg(not(any(windows, target_os = "macos")))]
        assert_eq!(result.unwrap_err(), "unsupported platform for app update");
        assert!(
            !cache.join("extracted").exists(),
            "legacy fixed dir is retired"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn checksum_verification_keeps_messages() {
        let dir = crate::core::testing::fixture("qc_updater_checksum");
        let file = dir.join("a.zip");
        std::fs::write(&file, b"abc").unwrap();
        let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        verify_checksum(&file, &format!("{abc}  a.zip\n")).unwrap();
        assert_eq!(
            verify_checksum(&file, "junk").unwrap_err(),
            "checksum sidecar is malformed"
        );
        let zero = "0".repeat(64);
        assert_eq!(
            verify_checksum(&file, &zero).unwrap_err(),
            format!("checksum mismatch: expected {zero}, got {abc}")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
