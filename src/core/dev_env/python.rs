//! Python 解释器与虚拟环境的探测，以及 pip 包清单的本地枚举。
//!
//! # 哪些可移除、哪些只读
//!
//! 划分线是「重新安装即可恢复」的边界在哪里，以及删除后有没有第二处状态
//! 要同步：
//!
//! - 解释器**自带**的 `site-packages`（安装级）是只读的：它是那个解释器的
//!   一部分，没有「重装就能恢复」的自包含边界。一条安全事实更要紧：
//!   `core::safety` 只挡 `C:\Program Files` **本身**、不挡它之下的子目录
//!   （`safety.rs` 的断言就是这么写的），而微软商店版 Python 的安装级
//!   `site-packages` 正落在 `C:\Program Files\WindowsApps\...\Lib\` 下——
//!   这条路上没有任何路径保护兜底（P41）。
//! - 用户级 site-packages 与 venv 的 site-packages 里的**包**可以逐个卸载：
//!   每个包在 `*.dist-info` 目录里有 pip 自己写的登记（PEP 376），
//!   `pip install` 就能原样装回来。走的是 `python -m pip uninstall`
//!   （`remove::Channel::Pip`），不是直接删文件。
//! - venv **整体**可以删除：`pyvenv.cfg`（PEP 405）是 venv 生态自己写的
//!   声明，目录里没有任何外部登记会被删除落下——目录本身就是它的全部状态。
//!   走核验过的整目录删除（`remove::Channel::VenvDirectory`），出口全部过
//!   P5 的 `remove_dir_forcing`。
//!
//! 仍然没有的是「这个 venv 还有没有项目在用」的证据（同 P39 的处境，而一个
//! venv 往往就是某个项目唯一跑得起来的环境）——这个判断交给删除前的确认
//! 弹窗，而不是由代码替用户赌一个。
//!
//! 自包含且自带卸载命令的 pipx / uv 工具走 `discovery::python_tools` 那条
//! 通道，与这里的解释器无关。
//!
//! # 解释器怎么找
//!
//! 不靠 `py` 启动器：本机就没有 `py.exe`（微软商店版 Python 不带它），拿它当
//! 主来源会在很多机器上什么都查不到。走 PATH 上的 `python` / `python3` 逐个
//! 探测，且**以探测结果为准**——PATH 上的可能只是个别名（商店版的
//! `WindowsApps\...\python.exe` 就是），真实安装前缀要从 `sys.prefix` 读，
//! 去重也按它。

use super::{
    AssetSource, DevAssetItem, DevAssetKind, SiteScope, TimedEvidence, TimestampSource, VenvManager,
};
use crate::core::proc::ProcRun;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(20);

/// 解释器候选名。`py` 启动器不在这里，理由见模块头。
const INTERPRETER_NAMES: &[&str] = &["python", "python3"];

/// 一次探测问到的事实。每一项都是 `Option`：探测不到就如实留空，不拿默认值
/// 冒充（例如把「问不出包数量」显示成 0）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InterpreterFacts {
    pub version: Option<String>,
    /// `sys.prefix`——真实安装前缀。
    pub prefix: Option<PathBuf>,
    /// 解释器自带的包目录（`sysconfig` 的 `purelib`）。
    pub install_site: Option<PathBuf>,
    /// 用户级包目录（`site.getusersitepackages()`）。`pip install` 默认装在
    /// 这里——解释器安装目录只读时（商店版 / 系统 Python）更是唯一的去处。
    pub user_site: Option<PathBuf>,
}

/// 探测一个解释器。
///
/// 固定脚本，无插值。包数量走 `importlib.metadata` 而不是 `pip list`：解释器里
/// 不一定装了 pip，而「有多少个已安装发行版」是标准库就能答的问题。
pub fn probe(
    run: &mut impl FnMut(&str, &[&str], Duration) -> Option<ProcRun>,
    program: &str,
) -> InterpreterFacts {
    // 只问「是什么、在哪」，不问「装了多少」：包数量本地数 `dist-info` 目录即可
    // （见 `count_distributions`），而为它枚举一遍发行版要一秒多——那点时间
    // 全花在每次扫描都要重跑一遍 `importlib.metadata` 上。
    //
    // 包目录用 `sysconfig.get_paths()["purelib"]`：`site.getsitepackages()[0]`
    // 在商店版 CPython 上返回的是 prefix（`...\WindowsApps\PythonSoftwareFoundation...`），
    // 拿它当包目录会把整个安装目录算成包的体积。
    const SCRIPT: &str = r#"import json,sys,site,sysconfig;print(json.dumps({"version":sys.version.split()[0],"prefix":sys.prefix,"install":sysconfig.get_paths().get("purelib"),"user":site.getusersitepackages()}))"#;
    let Some(result) = run(program, &["-c", SCRIPT], TIMEOUT) else {
        return InterpreterFacts::default();
    };
    if !result.ok {
        return InterpreterFacts::default();
    }
    let text = String::from_utf8_lossy(&result.stdout).to_string();
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
        return InterpreterFacts::default();
    };
    InterpreterFacts {
        version: json
            .get("version")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        prefix: json
            .get("prefix")
            .and_then(|value| value.as_str())
            .map(PathBuf::from),
        install_site: site_path(&json, "install"),
        user_site: site_path(&json, "user"),
    }
}

/// PATH 上的 Python 解释器，按真实安装前缀去重。
///
/// 探测不了的解释器也会留一条（只带 PATH 上的路径，其余显示为空）——「这个
/// 解释器存在但我问不出它的版本」是有用信息，静默丢掉它会让页面变成空白，
/// 而空白正是这次要修的那种表象。
pub fn interpreters(
    run: &mut impl FnMut(&str, &[&str], Duration) -> Option<ProcRun>,
) -> Vec<DevAssetItem> {
    let mut items = Vec::new();
    let mut seen_interpreters: HashSet<String> = HashSet::new();
    let mut seen_sites: HashSet<String> = HashSet::new();
    for name in INTERPRETER_NAMES {
        let Some(program) = crate::platform::resolve_tool_program(name) else {
            continue;
        };
        let facts = probe(run, name);
        // 解释器自身按真实前缀去重；问不出前缀时退回 PATH 上的路径。
        let key = crate::core::safety::norm(facts.prefix.as_deref().unwrap_or(&program));
        if !seen_interpreters.insert(key) {
            continue;
        }
        items.extend(interpreter_rows(name, &program, &facts, &mut seen_sites));
    }
    items
}

/// 由一次探测结果生成行：**一个解释器有几个包目录就出几行**。
///
/// 安装级与用户级是两处独立的空间，`pip list` 把两者加起来报。混成一个数字
/// 就会出现「`pip list` 有 145 个包、界面说 1 个」——商店版 CPython 上尤其
/// 明显，它的安装目录只读，用户装的东西全在用户级目录里。
///
/// 抽成纯函数是为了可测：`interpreters` 还要先解析 PATH 上的解释器，那一步
/// 依赖本机装没装 Python。
fn interpreter_rows(
    name: &str,
    program: &Path,
    facts: &InterpreterFacts,
    seen_sites: &mut HashSet<String>,
) -> Vec<DevAssetItem> {
    let display = facts
        .version
        .clone()
        .map(|version| format!("Python {version}"))
        .unwrap_or_else(|| name.to_string());

    let mut rows: Vec<(SiteScope, Option<PathBuf>)> = Vec::new();
    rows.push((SiteScope::Install, facts.install_site.clone()));
    if facts.user_site.is_some() {
        rows.push((SiteScope::User, facts.user_site.clone()));
    } else {
        // 问不出任何包目录时仍留一行（只带 PATH 上的路径）：解释器存在但问不出
        // 细节是有用信息，静默丢掉它会让页面变成空白——而那正是要防的表象。
        rows[0].1 = None;
    }

    let mut items = Vec::new();
    for (scope, path) in rows {
        let Some(path) = path else {
            items.push(DevAssetItem {
                id: format!("python:{}", crate::core::safety::norm(program)),
                kind: DevAssetKind::PythonInterpreter {
                    name: display.clone(),
                    version: facts.version.clone(),
                    scope,
                    package_count: None,
                    site_packages: None,
                    python: program.to_path_buf(),
                },
                path: program.to_path_buf(),
                size: super::storage::AssetStorageSize::default(),
                last_change: None,
                in_use: crate::core::inuse::SpotCheck::Clear,
                is_active_env: false,
                source: AssetSource::Layout,
            });
            continue;
        };
        if !path.is_dir() || !seen_sites.insert(crate::core::safety::norm(&path)) {
            continue;
        }
        let last_change = site_packages_evidence_of(&path);
        items.push(DevAssetItem {
            id: format!("python:{}", crate::core::safety::norm(&path)),
            kind: DevAssetKind::PythonInterpreter {
                name: display.clone(),
                version: facts.version.clone(),
                scope,
                package_count: Some(count_distributions(&path)),
                site_packages: Some(path.clone()),
                python: program.to_path_buf(),
            },
            path,
            size: super::storage::AssetStorageSize::default(),
            last_change,
            in_use: crate::core::inuse::SpotCheck::Clear,
            is_active_env: false,
            source: AssetSource::Tool,
        });
    }
    items
}

/// 从探测结果里取一个包目录，空值一律当没有。
fn site_path(json: &serde_json::Value, key: &str) -> Option<PathBuf> {
    json.get(key)
        .and_then(|value| value.as_str())
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

/// 虚拟环境的候选根，`(目录, 管理者)`。
///
/// `workon_home` 由调用方从环境变量取（测试里直接传，免得改进程级环境）。
pub fn venv_roots(workon_home: Option<PathBuf>) -> Vec<(PathBuf, VenvManager)> {
    let mut roots = Vec::new();
    // virtualenvwrapper：`WORKON_HOME`，默认 `~/.virtualenvs`。
    if let Some(home) = workon_home {
        roots.push((home, VenvManager::Virtualenvwrapper));
    } else if let Some(user) = crate::platform::user_home() {
        roots.push((user.join(".virtualenvs"), VenvManager::Virtualenvwrapper));
    }
    // Poetry 的默认 virtualenvs 位置（三个平台各一份）。
    if let Some(home) = crate::platform::user_home() {
        roots.push((
            home.join(".cache/pypoetry/virtualenvs"),
            VenvManager::Poetry,
        ));
        roots.push((
            home.join("Library/Caches/pypoetry/virtualenvs"),
            VenvManager::Poetry,
        ));
    }
    if let Some(local) = crate::platform::user_cache_dir() {
        roots.push((
            local.join("pypoetry").join("Cache").join("virtualenvs"),
            VenvManager::Poetry,
        ));
    }
    roots
}

/// 候选根下的虚拟环境。`pyvenv.cfg` 不在就不算——那是 venv / virtualenv 自己
/// 写的 PEP 405 标记文件，是「这是一个虚拟环境」的权威证据；靠目录名
/// （`.venv` / `env` / `venv`）判断会把普通目录也吃进来。
pub fn virtual_envs(
    roots: &[(PathBuf, VenvManager)],
    poetry_paths: &[PathBuf],
) -> Vec<DevAssetItem> {
    let mut items = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for (root, manager) in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(item) = read_venv(&path, *manager) {
                    if seen.insert(crate::core::safety::norm(&item.path)) {
                        items.push(item);
                    }
                }
            }
        }
    }

    // `poetry env list --full-path` 报出来的路径可能在默认位置之外。
    for path in poetry_paths {
        if let Some(item) = read_venv(path, VenvManager::Poetry) {
            if seen.insert(crate::core::safety::norm(&item.path)) {
                items.push(item);
            }
        }
    }

    items.sort_by(|a, b| a.display_name().cmp(b.display_name()));
    items
}

/// 读一个虚拟环境；没有 `pyvenv.cfg` 就不是虚拟环境。
fn read_venv(dir: &Path, manager: VenvManager) -> Option<DevAssetItem> {
    let identity = super::VenvIdentity::capture(dir)?;
    let cfg = std::fs::read_to_string(dir.join("pyvenv.cfg")).ok()?;
    let name = dir.file_name()?.to_string_lossy().to_string();
    let site_packages = venv_site_packages(dir);
    Some(DevAssetItem {
        id: format!("venv:{}", crate::core::safety::norm(dir)),
        kind: DevAssetKind::VirtualEnv {
            name,
            version: venv_cfg_value(&cfg, "version"),
            base_python: venv_cfg_value(&cfg, "home"),
            package_count: site_packages.as_deref().map(count_distributions),
            manager,
            identity: Some(identity),
        },
        path: dir.to_path_buf(),
        size: super::storage::AssetStorageSize::default(),
        // 时间的含义是「最后一次装/卸包」，来源如实标注，不冒充「最后使用」。
        last_change: site_packages.as_deref().and_then(site_packages_evidence_of),
        in_use: crate::core::inuse::SpotCheck::Clear,
        is_active_env: false,
        source: AssetSource::Tool,
    })
}

/// `site-packages` 目录的 mtime = 最后一次装/卸包的时间。
///
/// 解释器与虚拟环境共用这一条：两者都没有更细的自证记录，而这个目录只在
/// 增删包时变动，语义是确定的。
fn site_packages_evidence_of(site_packages: &Path) -> Option<TimedEvidence> {
    std::fs::metadata(site_packages)
        .and_then(|md| md.modified())
        .ok()
        .map(|at| TimedEvidence {
            at,
            source: TimestampSource::SitePackages,
        })
}

/// 虚拟环境的 site-packages：Windows 是 `Lib\site-packages`，其余平台是
/// `lib/pythonX.Y/site-packages`。
pub fn venv_site_packages(dir: &Path) -> Option<PathBuf> {
    let windows = dir.join("Lib").join("site-packages");
    if windows.is_dir() {
        return Some(windows);
    }
    let lib = dir.join("lib");
    let mut found: Option<PathBuf> = None;
    if let Ok(entries) = std::fs::read_dir(&lib) {
        for entry in entries.flatten() {
            let candidate = entry.path().join("site-packages");
            if candidate.is_dir() {
                found = Some(candidate);
            }
        }
    }
    found
}

/// `site-packages` 下的已安装发行版数量：`*.dist-info` 目录，每个发行版一个。
///
/// 不去跑这个 venv 自己的解释器——它可能已经被移动或删掉了基础解释器，
/// 那正是用户想清理它的原因。
fn count_distributions(site_packages: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(site_packages) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| {
            entry.path().is_dir() && entry.file_name().to_string_lossy().ends_with(".dist-info")
        })
        .count()
}

/// 从 `pyvenv.cfg` 取一个键的值。格式是 `key = value`，值里可能有空格。
fn venv_cfg_value(cfg: &str, key: &str) -> Option<String> {
    cfg.lines()
        .filter_map(|line| line.split_once('='))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case(key))
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// venv 自己的解释器。Windows 是 `Scripts\python.exe`，其余平台是 `bin/python`。
///
/// 按包卸载的 argv 靠它定死；不在了（基础解释器被移走之外，venv 自带的
/// python 也不会单独消失，但被挪动过的 venv 可能真的没有）就没有按包卸载
/// 的通道。venv 整体删除不依赖它。
pub fn venv_python(venv_dir: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    let candidate = venv_dir.join("Scripts").join("python.exe");
    #[cfg(not(windows))]
    let candidate = venv_dir.join("bin").join("python");
    candidate.is_file().then_some(candidate)
}

/// 枚举一个 site-packages 里的 pip 包：每个 `*.dist-info` 一个条目。
///
/// dist-info 目录（PEP 376）是 pip 写的登记，所以枚举它是读生态自己的
/// 账本，不是按目录猜安装——与 P38 禁止的那类「看到目录名就当装了」的
/// 降级扫描是两回事。名字取 `METADATA` 的 `Name:`：目录名是
/// `name-version` 无分隔拼接，`python-dateutil-2.9.0.dist-info` 里哪个
/// 连字符是分隔符没法确定，登记文件里的 `Name` 才是权威值。`METADATA`
/// 读不出、名字缺失或含非法字符的包**不产出条目**——卸载 argv 要用它，
/// 信不过就不给入口，也不猜。venv 一样适用：可能不在的只是基础解释器，
/// 登记始终在磁盘上。
pub fn pip_packages(site_packages: &Path, python: &Path) -> Vec<DevAssetItem> {
    let Ok(entries) = std::fs::read_dir(site_packages) else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().ends_with(".dist-info") {
            continue;
        }
        let dist_info = entry.path();
        if !dist_info.is_dir() {
            continue;
        }
        let Ok(metadata) = std::fs::read_to_string(dist_info.join("METADATA")) else {
            continue;
        };
        let Some(name) =
            metadata_field(&metadata, "Name").filter(|name| is_valid_distribution_name(name))
        else {
            continue;
        };
        let version = metadata_field(&metadata, "Version");
        // dist-info 目录的 mtime：装上当前版本的时间。语义与 npm 的
        // package.json 同档，如实标注来源，不冒充「最后使用」。
        let last_change = std::fs::metadata(&dist_info)
            .and_then(|md| md.modified())
            .ok()
            .map(|at| TimedEvidence {
                at,
                source: TimestampSource::PackageManifest,
            });
        items.push(DevAssetItem {
            id: format!("pip:{}", crate::core::safety::norm(&dist_info)),
            kind: DevAssetKind::PipPackage {
                python: python.to_path_buf(),
                name,
                version,
            },
            path: dist_info,
            size: super::storage::AssetStorageSize::default(),
            last_change,
            in_use: crate::core::inuse::SpotCheck::Clear,
            is_active_env: false,
            source: AssetSource::Tool,
        });
    }
    items.sort_by(|a, b| a.display_name().cmp(b.display_name()));
    items
}

/// 从 dist-info `METADATA`（RFC 822 形式）的头部取一个字段值。
///
/// 只看头部：第一个空行之后是包的描述正文，不再往里找。字段名大小写
/// 不敏感，值两侧的空白不算内容。
pub fn metadata_field(metadata: &str, key: &str) -> Option<String> {
    for line in metadata.lines() {
        if line.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case(key) {
                let value = value.trim();
                return (!value.is_empty()).then(|| value.to_string());
            }
        }
    }
    None
}

/// 发行版名字的合法字符：PEP 503 归一化形式允许的字母、数字、`.`、`-`、`_`。
///
/// 名字要进卸载 argv，任何越界字符（空格、引号、cmd 元字符……）都不收——
/// 宁可这条不给入口，也不把登记里读来的字符串拼进命令。
pub fn is_valid_distribution_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// `poetry env list --full-path` 的输出：一行一个绝对路径。
///
/// 只在 poetry 在 PATH 上时才问；不在就不问——「没装 poetry」与「poetry 说没有
/// 环境」都要走默认目录那条路，不该让前者报错。
pub fn poetry_env_paths(
    run: &mut impl FnMut(&str, &[&str], Duration) -> Option<ProcRun>,
) -> Vec<PathBuf> {
    if crate::platform::resolve_tool_program("poetry").is_none() {
        return Vec::new();
    }
    let Some(result) = run("poetry", &["env", "list", "--full-path"], TIMEOUT) else {
        return Vec::new();
    };
    if !result.ok {
        return Vec::new();
    }
    String::from_utf8_lossy(&result.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| Path::new(line).is_absolute())
        .map(PathBuf::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc_run(stdout: &str, ok: bool) -> ProcRun {
        ProcRun {
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
            exit_code: Some(if ok { 0 } else { 1 }),
            ok,
        }
    }

    fn proc_run_from(r: &ProcRun) -> ProcRun {
        ProcRun {
            stdout: r.stdout.clone(),
            stderr: r.stderr.clone(),
            exit_code: r.exit_code,
            ok: r.ok,
        }
    }

    /// 按程序名回答的假执行器。探测脚本很长，测试里只关心「问了哪个解释器」。
    fn fake(
        answers: &[(&str, Option<ProcRun>)],
    ) -> impl FnMut(&str, &[&str], Duration) -> Option<ProcRun> {
        let mut table: Vec<(String, Option<ProcRun>)> = answers
            .iter()
            .map(|(key, value)| (key.to_string(), value.as_ref().map(proc_run_from)))
            .collect();
        move |program, args, _timeout| {
            let full = format!("{program} {}", args.join(" "));
            // 用 `position` 取下标再 `remove`，避免同时不可变借用（找）与
            // 可变借用（删）。
            let index = table
                .iter()
                .position(|(key, _)| *key == full)
                .or_else(|| table.iter().position(|(key, _)| *key == program));
            index.and_then(|index| table.remove(index).1)
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    /// 造一个 dist-info：目录 + 带指定 `Name`/`Version` 的 `METADATA`。
    fn write_dist_info(site: &Path, dir_name: &str, name: &str, version: &str) {
        let dir = site.join(dir_name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("METADATA"),
            format!("Metadata-Version: 2.1\nName: {name}\nVersion: {version}\n"),
        )
        .unwrap();
    }

    // 测试里的路径一律用正斜杠：只需 `PathBuf::from` 与比较，语义与反斜杠
    // 相同，而 JSON 与 Rust 两层转义都不用写。

    #[test]
    fn probe_reads_version_prefix_and_both_package_dirs() {
        let stdout = r#"{"version":"3.13.14","prefix":"C:/Python313","install":"C:/Python313/Lib/site-packages","user":"C:/Users/u/AppData/Roaming/Python/Python313/site-packages"}"#;
        let mut runner = fake(&[("python", Some(proc_run(stdout, true)))]);
        let facts = probe(&mut runner, "python");
        assert_eq!(facts.version.as_deref(), Some("3.13.14"));
        assert_eq!(facts.prefix, Some(PathBuf::from("C:/Python313")));
        assert_eq!(
            facts.install_site,
            Some(PathBuf::from("C:/Python313/Lib/site-packages"))
        );
        assert_eq!(
            facts.user_site,
            Some(PathBuf::from(
                "C:/Users/u/AppData/Roaming/Python/Python313/site-packages"
            )),
            "用户级包目录必须一并拿到：商店版与系统 Python 上用户的包基本都在那里"
        );
    }

    /// `site.getsitepackages()` 返回的是**列表**，且商店版 CPython 上它的第 0
    /// 个元素是 `prefix`（整个安装目录），不是包目录——照着 `[0]` 取会把几百
    /// MB 的安装目录算成包的体积。这里锁住「列表形态一律不认」，而不是退回
    /// 取第一个元素。
    #[test]
    fn an_array_shaped_package_dir_is_rejected_rather_than_indexed() {
        let stdout = r#"{"version":"3.13.14","prefix":"C:/Python313","install":["C:/Python313","C:/Python313/Lib/site-packages"]}"#;
        let mut runner = fake(&[("python", Some(proc_run(stdout, true)))]);
        let facts = probe(&mut runner, "python");
        assert_eq!(
            facts.install_site, None,
            "列表形态必须留空，不能取 [0]——那在商店版上就是安装目录"
        );
        assert_eq!(
            facts.version.as_deref(),
            Some("3.13.14"),
            "其余字段照常可用"
        );
    }

    /// 问不出就留空，不能拿默认值冒充：把「问不出」显示成「装了 0 个包」正是
    /// 这次要防的那类表象。
    #[test]
    fn a_broken_probe_leaves_every_fact_unknown() {
        let mut runner = fake(&[("python", Some(proc_run("", false)))]);
        assert_eq!(probe(&mut runner, "python"), InterpreterFacts::default());

        let mut runner = fake(&[("python", Some(proc_run("not json", true)))]);
        assert_eq!(probe(&mut runner, "python"), InterpreterFacts::default());

        let mut runner = fake(&[]);
        assert_eq!(probe(&mut runner, "python"), InterpreterFacts::default());
    }

    /// 用户可见的那个 bug：安装级与用户级包目录必须各成一行。
    ///
    /// 商店版 CPython 的安装目录只读，用户的 145 个包里 144 个在用户级目录；
    /// 只报安装目录就会出现「`pip list` 明明有东西，界面说 1 个包」。
    #[test]
    fn one_row_per_package_directory_so_the_count_matches_pip_list() {
        let install = temp_dir("qc_py_install_site");
        let user = temp_dir("qc_py_user_site");
        std::fs::create_dir_all(install.join("pip-26.1.2.dist-info")).unwrap();
        for name in ["numpy-1.26.0.dist-info", "requests-2.32.0.dist-info"] {
            std::fs::create_dir_all(user.join(name)).unwrap();
        }

        let facts = InterpreterFacts {
            version: Some("3.13.14".into()),
            prefix: Some(PathBuf::from("C:/Python313")),
            install_site: Some(install.clone()),
            user_site: Some(user.clone()),
        };
        let items = interpreter_rows(
            "python",
            &PathBuf::from("C:/Python313/python.exe"),
            &facts,
            &mut HashSet::new(),
        );

        assert_eq!(items.len(), 2, "两个包目录应当各成一行：{items:?}");
        let scopes: Vec<SiteScope> = items
            .iter()
            .map(|item| match &item.kind {
                DevAssetKind::PythonInterpreter { scope, .. } => *scope,
                other => panic!("期望 PythonInterpreter，实际 {other:?}"),
            })
            .collect();
        assert!(scopes.contains(&SiteScope::Install) && scopes.contains(&SiteScope::User));

        let counts: Vec<Option<usize>> = items
            .iter()
            .map(|item| match &item.kind {
                DevAssetKind::PythonInterpreter { package_count, .. } => *package_count,
                _ => unreachable!(),
            })
            .collect();
        assert!(counts.contains(&Some(1)), "安装目录 1 个包：{counts:?}");
        assert!(counts.contains(&Some(2)), "用户目录 2 个包：{counts:?}");
        assert_eq!(
            counts.iter().flatten().sum::<usize>(),
            3,
            "两行加起来才是 pip list 报的总数"
        );
        // 两行本身都没有移除入口：可移除的是用户级目录**里面**的 pip 包
        // 条目（展开后逐个卸载），安装级则整个只读（P41）。行不是删除单位。
        for item in &items {
            assert!(!crate::core::dev_env::remove::can_remove(item));
        }

        let _ = std::fs::remove_dir_all(&install);
        let _ = std::fs::remove_dir_all(&user);
    }

    #[test]
    fn pyvenv_cfg_is_the_only_proof_of_a_virtualenv() {
        let root = temp_dir("qc_venv_proof");
        let real = root.join("real-env");
        std::fs::create_dir_all(real.join("Lib").join("site-packages")).unwrap();
        std::fs::write(
            real.join("pyvenv.cfg"),
            "home = C:/Python313\nversion = 3.13.14\n",
        )
        .unwrap();
        // 名字很像但没有任何标记的目录不该被当成环境。
        let impostor = root.join("looks-like-a-venv");
        std::fs::create_dir_all(&impostor).unwrap();

        let items = virtual_envs(&[(root.clone(), VenvManager::Virtualenvwrapper)], &[]);
        assert_eq!(items.len(), 1, "只有带 pyvenv.cfg 的才算");
        assert_eq!(items[0].display_name(), "real-env");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_virtualenv_reports_its_base_interpreter_and_package_count() {
        let root = temp_dir("qc_venv_facts");
        let env = root.join("proj");
        let packages = env.join("Lib").join("site-packages");
        std::fs::create_dir_all(packages.join("numpy-1.26.0.dist-info")).unwrap();
        std::fs::create_dir_all(packages.join("pip-26.1.2.dist-info")).unwrap();
        std::fs::create_dir_all(packages.join("numpy")).unwrap();
        std::fs::write(
            env.join("pyvenv.cfg"),
            "home = C:/Python313\ninclude-system-site-packages = false\nversion = 3.13.14\n",
        )
        .unwrap();

        let items = virtual_envs(&[(root.clone(), VenvManager::Poetry)], &[]);
        assert_eq!(items.len(), 1);
        match &items[0].kind {
            DevAssetKind::VirtualEnv {
                version,
                base_python,
                package_count,
                manager,
                ..
            } => {
                assert_eq!(version.as_deref(), Some("3.13.14"));
                assert_eq!(base_python.as_deref(), Some("C:/Python313"));
                assert_eq!(*package_count, Some(2), "只数 dist-info");
                assert_eq!(*manager, VenvManager::Poetry);
            }
            other => panic!("期望 VirtualEnv，实际 {other:?}"),
        }
        assert_eq!(
            items[0].last_change.map(|evidence| evidence.source),
            Some(TimestampSource::SitePackages),
            "时间来源必须标明是「包目录」，不能冒充使用时间"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn poetry_reported_paths_are_included_and_deduplicated() {
        let root = temp_dir("qc_venv_poetry");
        let external = temp_dir("qc_venv_external");
        std::fs::create_dir_all(external.join("Lib").join("site-packages")).unwrap();
        std::fs::write(external.join("pyvenv.cfg"), "home = /usr/bin\n").unwrap();

        let items = virtual_envs(
            &[(root.clone(), VenvManager::Poetry)],
            &[external.clone(), external.clone()],
        );
        assert_eq!(items.len(), 1, "同一路径报两次只算一条");
        assert_eq!(items[0].path, external);

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&external);
    }

    #[test]
    fn workon_home_overrides_the_default_root() {
        let custom = PathBuf::from("C:/custom-workon");
        let roots = venv_roots(Some(custom.clone()));
        assert!(
            roots.iter().any(
                |(path, manager)| *path == custom && *manager == VenvManager::Virtualenvwrapper
            ),
            "WORKON_HOME 指定的根必须在列表里：{roots:?}"
        );
        assert!(
            !roots.iter().any(|(path, _)| path.ends_with(".virtualenvs")),
            "显式给了 WORKON_HOME 就不该再退回默认位置"
        );
    }

    #[test]
    fn venv_cfg_values_tolerate_spacing_and_case() {
        let cfg = "HOME = C:/Python313\nVersion=3.13.14\nempty =\n";
        assert_eq!(venv_cfg_value(cfg, "home").as_deref(), Some("C:/Python313"));
        assert_eq!(venv_cfg_value(cfg, "VERSION").as_deref(), Some("3.13.14"));
        assert_eq!(venv_cfg_value(cfg, "empty"), None, "空值等于没有");
        assert_eq!(venv_cfg_value(cfg, "missing"), None);
    }

    #[test]
    fn poetry_is_not_queried_when_it_is_not_installed() {
        // 本机没有 poetry；这条断言「不在 PATH 上就一次命令都不跑」。
        if crate::platform::resolve_tool_program("poetry").is_some() {
            return;
        }
        let mut ran = false;
        let mut runner = |_: &str, _: &[&str], _: Duration| {
            ran = true;
            None
        };
        assert!(poetry_env_paths(&mut runner).is_empty());
        assert!(!ran, "poetry 不在 PATH 上时不该跑命令");
    }

    /// 包名来自 `METADATA` 的 `Name:`，不是从目录名猜的：dist-info 目录名是
    /// `name-version` 无分隔拼接，`python-dateutil-2.9.0` 里哪个连字符是
    /// 分隔符没法从目录名确定。
    #[test]
    fn pip_packages_are_named_by_the_dist_info_metadata() {
        let site = temp_dir("qc_pip_site");
        write_dist_info(
            &site,
            "python_dateutil-2.9.0.dist-info",
            "python-dateutil",
            "2.9.0",
        );
        write_dist_info(&site, "numpy-1.26.0.dist-info", "numpy", "1.26.0");
        // 只有一个普通目录，不产条目。
        std::fs::create_dir_all(site.join("numpy")).unwrap();

        let packages = pip_packages(&site, Path::new("C:/Python313/python.exe"));
        assert_eq!(packages.len(), 2, "METADATA 读不出的不算：{packages:?}");
        let names: Vec<&str> = packages.iter().map(|item| item.display_name()).collect();
        assert!(
            names.contains(&"python-dateutil"),
            "名字要取登记里的规范值：{names:?}"
        );
        assert!(names.contains(&"numpy"));

        for package in &packages {
            assert_eq!(
                package.source,
                AssetSource::Tool,
                "dist-info 是 pip 自己写的登记，来源是 Tool 而不是按目录的 Layout"
            );
            assert!(
                package
                    .path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().ends_with(".dist-info")),
                "条目路径是 dist-info 目录本身，事后核验就验它消失"
            );
            match &package.kind {
                DevAssetKind::PipPackage {
                    python, version, ..
                } => {
                    assert_eq!(python, &PathBuf::from("C:/Python313/python.exe"));
                    assert!(!version.as_deref().unwrap_or("").is_empty());
                }
                other => panic!("期望 PipPackage，实际 {other:?}"),
            }
        }

        let _ = std::fs::remove_dir_all(&site);
    }

    /// `METADATA` 缺失或名字信不过的 dist-info 不产条目：卸载 argv 要用
    /// 登记里的名字，宁缺也不猜。
    #[test]
    fn a_dist_info_without_a_trustworthy_name_is_not_listed() {
        let site = temp_dir("qc_pip_unreadable");
        std::fs::create_dir_all(site.join("broken-1.0.dist-info")).unwrap();
        write_dist_info(&site, "no-name-1.0.dist-info", "", "1.0");
        write_dist_info(&site, "evil-1.0.dist-info", "bad name; & whoami", "1.0");

        assert!(
            pip_packages(&site, Path::new("C:/Python313/python.exe")).is_empty(),
            "没有 METADATA、没有 Name、名字含非法字符的都不该出现"
        );

        let _ = std::fs::remove_dir_all(&site);
    }

    #[test]
    fn metadata_field_reads_only_the_header_and_ignores_case() {
        let metadata =
            "Metadata-Version: 2.1\nNAME: python-dateutil\nVersion:  2.9.0 \n\nName-in-body: no\n";
        assert_eq!(
            metadata_field(metadata, "name").as_deref(),
            Some("python-dateutil")
        );
        assert_eq!(
            metadata_field(metadata, "Version").as_deref(),
            Some("2.9.0")
        );
        assert_eq!(
            metadata_field(metadata, "Name-in-body"),
            None,
            "第一个空行之后是正文，不再往里找"
        );
        assert_eq!(metadata_field(metadata, "missing"), None);
    }

    /// venv 的按包卸载用 venv 自己的 python；它在哪是 PEP 405 的既定布局，
    /// 不是猜的。
    #[test]
    fn a_virtualenv_exposes_its_own_python() {
        let root = temp_dir("qc_venv_python");
        #[cfg(windows)]
        let python = root.join("Scripts").join("python.exe");
        #[cfg(not(windows))]
        let python = root.join("bin").join("python");
        std::fs::create_dir_all(python.parent().unwrap()).unwrap();

        assert_eq!(venv_python(&root), None, "python 还不在，先不给按包卸载");
        std::fs::write(&python, b"").unwrap();
        assert_eq!(venv_python(&root), Some(python));

        let _ = std::fs::remove_dir_all(&root);
    }
}
