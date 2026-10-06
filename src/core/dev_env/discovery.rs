//! 开发环境与全局包探测。
//!
//! # 清单的权威性分两级，条目自己带着这个事实
//!
//! - [`AssetSource::Tool`]：生态自己的清单命令报告的
//!   （`conda info --json`、`npm ls --global`、`pnpm root --global`、
//!   `uv tool dir`、`pipx list --json`）。只有这一级提供移除入口。
//! - [`AssetSource::Layout`]：命令不可用或输出读不懂时，按已知安装布局扫出来的
//!   降级结果。**只展示**——它的路径没有被生态确认过，拿它去执行删除等于用
//!   猜的路径动刀。
//!
//! 降级不是「静默少一块」：条目照常进表、体积照常算，只是没有删除入口。
//!
//! # 时间只有生态自己写下的那一个
//!
//! 这里**不使用 atime，也不把目录 mtime 当使用时间**：目录的 mtime 只在增删
//! 直接子项时变化，天天用的环境照样是很旧的 mtime；atime 在 Windows 上默认
//! 不更新，本仓库也没有任何地方验证过它。所以每个时间点都带来源
//! （[`TimestampSource`]），界面上按来源措辞——「最后变更」不能写成「最近使用」。
//! 更重要的：时间**不参与预选**，没有「很久没用就默认勾上」这条路径。
//!
//! # 体积
//!
//! 由 [`discover_all`] 一次性批量测定：单个目录的逻辑体积各算各的，总计按文件
//! 唯一身份去重。conda 的 `pkgs` 缓存与环境之间大量使用硬链接，逐项相加会把
//! 同一份数据算两遍以上。

use super::inventory;
use super::storage::{measure_batch_storage, AssetStorageSize};
use super::{
    AssetSource, DevAssetItem, DevAssetKind, PythonToolKind, TimedEvidence, TimestampSource,
};
use crate::core::inuse::{spot_check, SpotCheck};
use crate::core::proc::ProcRun;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub struct DevDiscoveryResult {
    pub conda_envs: Vec<DevAssetItem>,
    /// Python 解释器与虚拟环境，**只读**（见 `super::python` 模块头）。
    pub python_envs: Vec<DevAssetItem>,
    pub node_packages: Vec<DevAssetItem>,
    pub python_tools: Vec<DevAssetItem>,
    /// 全部资产合并、按文件唯一身份去重后的总计。
    pub total: AssetStorageSize,
    /// 其中**进得了移除通道**的那部分的独占体积。
    ///
    /// 页面上的「预估可释放」只能报这个数：只读资产（Python 解释器与虚拟
    /// 环境、bun、以及一切 `Layout` 来源的条目）这个页面根本不会去删，把它们
    /// 算进「可释放」就是在报一个做不到的数。逐项独立体积相加是安全的——
    /// `measure_batch_storage` 只在 `nlink <= 1` 时计入独占，而单链接的文件
    /// 不可能同时属于另一个条目。
    pub removable_exclusive: u64,
}

/// 扫描发现本机所有开发环境与全局包资产。
pub fn discover_all() -> DevDiscoveryResult {
    // 直接把生产 runner 传进去，不再包一层闭包：包一层就多一个「哪个 runner」
    // 的决定点，而探针里传错一个 runner 会让本机核查看到假象。
    let mut run = crate::core::proc::run_tool_with_timeout;
    discover_all_with(&mut run)
}

pub(crate) fn discover_all_with(
    run: &mut impl FnMut(&str, &[&str], Duration) -> Option<ProcRun>,
) -> DevDiscoveryResult {
    let mut conda_envs = conda_environments(run);
    let mut python_envs = python_environments(run);
    let mut node_packages = node_global_packages(run);
    let mut python_tools = python_tools(run);

    // 跨通道按规范化路径去重：同一个目录从两个来源被列出来（大小写混写的
    // 安装根、`environments.txt` 与命令清单各报一次）会让体积算两遍，
    // 混录进来的那一条还会漏掉 base/激活标记。
    let mut seen: HashSet<String> = HashSet::new();
    for group in [
        &mut conda_envs,
        &mut python_envs,
        &mut node_packages,
        &mut python_tools,
    ] {
        group.retain(|item| seen.insert(crate::core::safety::norm(&item.path)));
    }

    // 一次批量测定：逐项体积 + 去重后的总计。
    let dirs: Vec<PathBuf> = conda_envs
        .iter()
        .chain(python_envs.iter())
        .chain(node_packages.iter())
        .chain(python_tools.iter())
        .map(|item| item.path.clone())
        .collect();
    let (total, per_dir) = measure_batch_storage(&dirs);
    let mut sizes = per_dir.into_iter();
    for group in [
        &mut conda_envs,
        &mut python_envs,
        &mut node_packages,
        &mut python_tools,
    ] {
        for item in group.iter_mut() {
            item.size = sizes.next().unwrap_or_default();
        }
    }

    // 占用复检一次跑完，结果回填到各组。
    let busy = spot_check(&dirs);
    for group in [
        &mut conda_envs,
        &mut python_envs,
        &mut node_packages,
        &mut python_tools,
    ] {
        for item in group.iter_mut() {
            if let Some(status) = busy.get(&item.path) {
                item.in_use = *status;
            }
        }
    }

    let removable_exclusive =
        reclaimable_bytes(&[&conda_envs, &python_envs, &node_packages, &python_tools]);

    DevDiscoveryResult {
        conda_envs,
        python_envs,
        node_packages,
        python_tools,
        total,
        removable_exclusive,
    }
}

/// conda / mamba 的环境。不含体积，体积由 [`discover_all_with`] 批量测定。
pub fn conda_environments(
    run: &mut impl FnMut(&str, &[&str], Duration) -> Option<ProcRun>,
) -> Vec<DevAssetItem> {
    // conda 与 mamba 是同一套 CLI 的两个前端；只装了 mamba 的机器上
    // `conda` 不存在，但环境照样在。
    let inventory = match inventory::conda(run) {
        Ok(inventory) => Some((inventory, AssetSource::Tool)),
        Err(_) => inventory::mamba(run).ok().map(|i| (i, AssetSource::Tool)),
    };

    let active = active_conda_prefix();
    match inventory {
        Some((inventory, source)) => {
            let mut seen = HashSet::new();
            let mut items: Vec<DevAssetItem> = inventory
                .envs
                .into_iter()
                .filter(|path| seen.insert(crate::core::safety::norm(path)))
                .map(|path| {
                    let is_base = inventory.root_prefix.as_ref().is_some_and(|root| {
                        crate::core::safety::norm(root) == crate::core::safety::norm(&path)
                    });
                    conda_item(&path, is_base, source, active.as_deref())
                })
                .collect();
            sort_conda(&mut items);
            items
        }
        // 命令不可用：按已知安装布局扫，但只展示。`~/.conda/environments.txt`
        // 虽然是 conda 写的，却是它不保证新鲜的缓存，同样只算 Layout——真正
        // 的移除授权在执行前还会再问一次命令，那时一样会被拒。
        None => {
            let mut items: Vec<DevAssetItem> = layout_conda_roots()
                .into_iter()
                .map(|(path, is_base)| {
                    conda_item(&path, is_base, AssetSource::Layout, active.as_deref())
                })
                .collect();
            sort_conda(&mut items);
            items
        }
    }
}

fn sort_conda(items: &mut [DevAssetItem]) {
    items.sort_by(|a, b| {
        let a_base = matches!(&a.kind, DevAssetKind::CondaEnv { is_base: true, .. });
        let b_base = matches!(&b.kind, DevAssetKind::CondaEnv { is_base: true, .. });
        b_base
            .cmp(&a_base)
            .then_with(|| a.display_name().cmp(b.display_name()))
    });
}

fn conda_item(
    path: &Path,
    is_base: bool,
    source: AssetSource,
    active: Option<&str>,
) -> DevAssetItem {
    let normalized = crate::core::safety::norm(path);
    let name = if is_base {
        String::from("base")
    } else {
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown")
            .to_string()
    };
    let (python_version, package_count, last_change) = inspect_conda_meta(path);
    DevAssetItem {
        id: format!("conda:{normalized}"),
        kind: DevAssetKind::CondaEnv {
            name,
            is_base,
            python_version,
            package_count,
        },
        path: path.to_path_buf(),
        size: AssetStorageSize::default(),
        last_change,
        in_use: SpotCheck::Clear,
        is_active_env: active == Some(normalized.as_str()),
        source,
    }
}

/// 当前激活环境的前缀。`CONDA_PREFIX` 由 shell 的 `conda activate` 注入。
fn active_conda_prefix() -> Option<String> {
    std::env::var_os("CONDA_PREFIX").map(|prefix| crate::core::safety::norm(&PathBuf::from(prefix)))
}

/// 命令不可用时的 conda 环境候选：已知安装根 + `environments.txt`。
///
/// `~/.conda` **不在其中**：那是 conda 存配置与 `environments.txt` 的目录，
/// 不是环境。把它当安装根会凭空多出一个名叫 base 的假环境，体积还是配置
/// 目录的体积。
fn layout_conda_roots() -> Vec<(PathBuf, bool)> {
    let mut found: Vec<(PathBuf, bool)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut push = |path: PathBuf, is_base: bool, found: &mut Vec<(PathBuf, bool)>| {
        if path.is_dir() && seen.insert(crate::core::safety::norm(&path)) {
            found.push((path, is_base));
        }
    };

    for root in common_conda_roots() {
        if root.is_dir() {
            push(root.clone(), true, &mut found);
            if let Ok(entries) = std::fs::read_dir(root.join("envs")) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        push(path, false, &mut found);
                    }
                }
            }
        }
    }

    // conda 自己记的环境清单，含 `.condarc` 指到别的盘上的那些。
    if let Some(home) = crate::platform::user_home() {
        if let Ok(content) = std::fs::read_to_string(home.join(".conda").join("environments.txt")) {
            for line in content.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    push(PathBuf::from(trimmed), false, &mut found);
                }
            }
        }
    }

    found
}

/// 常见的 conda / mamba 安装根。
///
/// 每一项都必须是**安装根**（它自己就是 base 环境、且底下有 `envs/`）。
fn common_conda_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = crate::platform::user_home() {
        for name in [
            "miniconda3",
            "anaconda3",
            "miniforge3",
            "mambaforge",
            "micromamba",
        ] {
            roots.push(home.join(name));
        }
    }
    // `%LOCALAPPDATA%\miniconda3`：Windows 上的每用户常用位置。取
    // `user_cache_dir()` 本身，不是它的 parent——parent 得到的是 `AppData`，
    // 底下并没有 miniconda3。
    if let Some(local) = crate::platform::user_cache_dir() {
        for name in ["miniconda3", "anaconda3", "miniforge3"] {
            roots.push(local.join(name));
        }
    }
    #[cfg(windows)]
    {
        // 全机安装位置。
        if let Some(program_data) = std::env::var_os("ProgramData") {
            let program_data = PathBuf::from(program_data);
            roots.push(program_data.join("miniconda3"));
            roots.push(program_data.join("anaconda3"));
        }
    }
    #[cfg(not(windows))]
    {
        for root in [
            "/opt/conda",
            "/opt/miniconda3",
            "/usr/local/miniconda3",
            "/usr/local/anaconda3",
        ] {
            roots.push(PathBuf::from(root));
        }
        if let Some(home) = crate::platform::user_home() {
            for name in ["opt/miniconda3", "opt/anaconda3"] {
                roots.push(home.join(name));
            }
        }
    }
    roots
}

/// `conda-meta` 里的 Python 版本、包数量，以及环境最后一次事务的时间。
///
/// 时间取 `conda-meta/history` 的 mtime：那个文件由 conda 每次事务重写，
/// 所以它是「环境最后一次装/卸包」，**不是**「最后一次使用」。
fn inspect_conda_meta(env_path: &Path) -> (Option<String>, Option<usize>, Option<TimedEvidence>) {
    let meta_dir = env_path.join("conda-meta");
    if !meta_dir.is_dir() {
        return (None, None, None);
    }

    let mut python_version = None;
    let mut package_count = 0usize;

    if let Ok(entries) = std::fs::read_dir(&meta_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                if name.ends_with(".json") {
                    package_count += 1;
                    if python_version.is_none() && name.starts_with("python-") {
                        // `python-3.11.8-h1234_0.json` → `3.11.8`
                        if let Some(version) = name.split('-').nth(1) {
                            python_version = Some(version.to_string());
                        }
                    }
                }
            }
        }
    }

    let last_change = std::fs::metadata(meta_dir.join("history"))
        .and_then(|md| md.modified())
        .ok()
        .map(|at| TimedEvidence {
            at,
            source: TimestampSource::CondaTransaction,
        });

    (python_version, Some(package_count), last_change)
}

/// 进得了移除通道的那部分资产的独占体积。
///
/// 页面上的「可释放」只能报这个数：只读资产（Python 解释器与虚拟环境、bun、
/// 以及一切降级扫描来源的条目）这个页面根本不会去删，把它们算进「可释放」
/// 就是在报一个做不到的数。
///
/// 逐项独立体积相加是安全的：`measure_batch_storage` 只在 `nlink <= 1` 时计入
/// 独占，而单链接的文件不可能同时属于另一个条目。
fn reclaimable_bytes(groups: &[&[DevAssetItem]]) -> u64 {
    groups
        .iter()
        .flat_map(|group| group.iter())
        .filter(|item| super::remove::can_remove(item))
        .map(|item| item.size.exclusive_reclaimable_bytes)
        .sum()
}

/// Python 解释器与虚拟环境，**只读**。
///
/// 两者都不参与移除：解释器的 `site-packages` 不是自包含资产，虚拟环境没有
/// 「还有没有项目在用」的证据。理由与两条安全事实见 `super::python` 模块头。
pub fn python_environments(
    run: &mut impl FnMut(&str, &[&str], Duration) -> Option<ProcRun>,
) -> Vec<DevAssetItem> {
    let mut items = super::python::interpreters(run);
    let workon_home = std::env::var_os("WORKON_HOME").map(PathBuf::from);
    let roots = super::python::venv_roots(workon_home);
    let poetry_paths = super::python::poetry_env_paths(run);
    items.extend(super::python::virtual_envs(&roots, &poetry_paths));
    items.sort_by(|a, b| a.display_name().cmp(b.display_name()));
    items
}

/// Node 全局包（npm / pnpm / bun）。不含体积。
pub fn node_global_packages(
    run: &mut impl FnMut(&str, &[&str], Duration) -> Option<ProcRun>,
) -> Vec<DevAssetItem> {
    let mut items = Vec::new();

    // npm：权威清单是 `--depth=0` 的顶级依赖。它顺带排除了「是另一个全局包
    // 的依赖」的包——那些包不在这一层，因而拿不到删除授权。
    match inventory::npm_top_level(run) {
        Ok(packages) => {
            for package in packages {
                items.push(node_package_item(
                    "npm",
                    &package.name,
                    package.version.as_deref(),
                    &package.path,
                    AssetSource::Tool,
                ));
            }
        }
        Err(_) => items.extend(node_modules_from_roots("npm", &layout_npm_dirs())),
    }

    // pnpm：根由 `pnpm root --global` 给出，子目录即各包（它的既定布局）。
    match inventory::pnpm_global_root(run) {
        Ok(root) => items.extend(node_modules_from_roots("pnpm", &[root])),
        Err(_) => items.extend(node_modules_from_roots("pnpm", &layout_pnpm_dirs())),
    }

    // bun：清单输出格式未核实，一律 Layout（只展示）。它的布局本身是 bun
    // 文档写明的，用作展示没有问题。
    items.extend(node_modules_from_roots("bun", &layout_bun_dirs()));

    items.sort_by(|a, b| a.display_name().cmp(b.display_name()));
    items
}

/// 列出若干个 `node_modules` 根里的包。
fn node_modules_from_roots(manager: &str, roots: &[PathBuf]) -> Vec<DevAssetItem> {
    roots
        .iter()
        .flat_map(|root| node_modules_items(manager, root))
        .collect()
}

/// 列出一个 `node_modules` 根里的包。
///
/// 顶层成员优先取**生态自己写的清单**（`node_modules` 旁边的 `package.json`）：
/// 那才是「用户装了什么」。目录的直接子项不是——本机实测 bun 声明 3 个全局包、
/// 目录里躺着 132 个，另外 129 个是那 3 个的传递依赖，列出来既没法看也没法用，
/// 还给「删除」提供了错误的候选。
///
/// 清单问不出来时才退回按目录列，并标成 [`AssetSource::Layout`]（只展示）。
fn node_modules_items(manager: &str, root: &Path) -> Vec<DevAssetItem> {
    match inventory::declared_top_level(root) {
        Some(declared) => declared
            .into_iter()
            .filter_map(|name| {
                read_node_package(manager, &root.join(&name), Some(&name), AssetSource::Tool)
            })
            .collect(),
        None => list_node_modules(manager, root),
    }
}

/// 按目录列 `node_modules` 的直接子项。清单问不出来时的降级路径。
fn list_node_modules(manager: &str, root: &Path) -> Vec<DevAssetItem> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        // 作用域包（`@scope/name`）真正的东西在下一层。
        if name.starts_with('@') {
            if let Ok(scoped) = std::fs::read_dir(&path) {
                for scoped_entry in scoped.flatten() {
                    let scoped_path = scoped_entry.path();
                    if scoped_path.is_dir() {
                        let scoped_name = scoped_entry.file_name().to_string_lossy().to_string();
                        items.extend(read_node_package(
                            manager,
                            &scoped_path,
                            Some(&scoped_name),
                            AssetSource::Layout,
                        ));
                    }
                }
            }
            continue;
        }
        // `.pnpm`、`.modules.yaml` 这类是包管理器的内部件，不是包。
        if name.starts_with('.') {
            continue;
        }
        items.extend(read_node_package(
            manager,
            &path,
            Some(name),
            AssetSource::Layout,
        ));
    }
    items
}

/// 读一个包目录。`fallback_name` 在 `package.json` 缺失或读不懂时兜底：
/// 生态清单已经说了它装在哪儿，那就不能因为读不到清单文件而把它藏起来。
fn read_node_package(
    manager: &str,
    package_dir: &Path,
    fallback_name: Option<&str>,
    source: AssetSource,
) -> Option<DevAssetItem> {
    let manifest = package_dir.join("package.json");
    let json = std::fs::read_to_string(&manifest)
        .ok()
        .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok());

    let name = json
        .as_ref()
        .and_then(|json| json.get("name"))
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .or_else(|| fallback_name.map(str::to_string))?;
    if !package_dir.is_dir() {
        return None;
    }
    let version = json
        .as_ref()
        .and_then(|json| json.get("version"))
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");

    Some(node_package_item(
        manager,
        &name,
        Some(version),
        package_dir,
        source,
    ))
}

fn node_package_item(
    manager: &str,
    name: &str,
    version: Option<&str>,
    path: &Path,
    source: AssetSource,
) -> DevAssetItem {
    DevAssetItem {
        id: format!("{manager}:{name}"),
        kind: DevAssetKind::NodeGlobalPackage {
            manager: manager.to_string(),
            name: name.to_string(),
            version: version.unwrap_or("unknown").to_string(),
            bin_shims: bin_shims(path),
        },
        path: path.to_path_buf(),
        size: AssetStorageSize::default(),
        last_change: package_manifest_time(path),
        in_use: SpotCheck::Clear,
        is_active_env: false,
        source,
    }
}

/// `package.json` 的 mtime：这个包被安装或升到当前版本的时间。
fn package_manifest_time(package_dir: &Path) -> Option<TimedEvidence> {
    std::fs::metadata(package_dir.join("package.json"))
        .and_then(|md| md.modified())
        .ok()
        .map(|at| TimedEvidence {
            at,
            source: TimestampSource::PackageManifest,
        })
}

fn bin_shims(package_dir: &Path) -> Vec<String> {
    let Ok(content) = std::fs::read_to_string(package_dir.join("package.json")) else {
        return Vec::new();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Vec::new();
    };
    let Some(bin) = json.get("bin") else {
        return Vec::new();
    };
    if let Some(single) = bin.as_str() {
        return Path::new(single)
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| vec![name.to_string()])
            .unwrap_or_default();
    }
    bin.as_object()
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default()
}

fn layout_npm_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = crate::platform::user_home() {
        dirs.push(home.join("node_modules"));
        dirs.push(home.join(".npm-global").join("lib").join("node_modules"));
    }
    #[cfg(windows)]
    {
        if let Some(roaming) = crate::platform::user_data_dir() {
            dirs.push(roaming.join("npm").join("node_modules"));
        }
    }
    #[cfg(not(windows))]
    {
        dirs.push(PathBuf::from("/usr/local/lib/node_modules"));
    }
    dirs
}

fn layout_pnpm_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = crate::platform::user_home() {
        dirs.push(home.join(".local/share/pnpm/global"));
    }
    if let Some(local) = crate::platform::user_cache_dir() {
        dirs.push(local.join("pnpm").join("global"));
    }
    // pnpm 的全局根下面是 store 版本号一层（`global/<n>/node_modules`），
    // 版本号无法预知，所以枚举这一层再进 node_modules。
    let mut roots = Vec::new();
    for dir in dirs {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let candidate = entry.path().join("node_modules");
                if candidate.is_dir() {
                    roots.push(candidate);
                }
            }
        }
    }
    roots
}

fn layout_bun_dirs() -> Vec<PathBuf> {
    crate::platform::user_home()
        .map(|home| vec![home.join(".bun/install/global/node_modules")])
        .unwrap_or_default()
}

/// 独立 CLI 工具（uv tool / pipx）。不含体积。
pub fn python_tools(
    run: &mut impl FnMut(&str, &[&str], Duration) -> Option<ProcRun>,
) -> Vec<DevAssetItem> {
    let mut items = Vec::new();

    match inventory::uv_tool_root(run) {
        Ok(root) => {
            for (path, name) in tool_dirs(&root) {
                items.push(tool_item(
                    PythonToolKind::Uv,
                    &name,
                    &path,
                    AssetSource::Tool,
                ));
            }
        }
        Err(_) => {
            for root in layout_uv_tool_dirs() {
                for (path, name) in tool_dirs(&root) {
                    items.push(tool_item(
                        PythonToolKind::Uv,
                        &name,
                        &path,
                        AssetSource::Layout,
                    ));
                }
            }
        }
    }

    match inventory::pipx_venvs(run) {
        Ok(tools) => {
            for tool in tools {
                // 老版本 pipx 不报 `environment`，退回它自己的 `venvs/<name>` 约定。
                let path = tool.venv.clone().unwrap_or_else(|| {
                    layout_pipx_venvs_dirs()
                        .first()
                        .map(|root| root.join(&tool.name))
                        .unwrap_or_else(|| PathBuf::from(&tool.name))
                });
                items.push(tool_item(
                    PythonToolKind::Pipx,
                    &tool.name,
                    &path,
                    AssetSource::Tool,
                ));
            }
        }
        Err(_) => {
            for root in layout_pipx_venvs_dirs() {
                for (path, name) in tool_dirs(&root) {
                    items.push(tool_item(
                        PythonToolKind::Pipx,
                        &name,
                        &path,
                        AssetSource::Layout,
                    ));
                }
            }
        }
    }

    items.sort_by(|a, b| a.display_name().cmp(b.display_name()));
    items
}

/// 工具根下的直接子目录，`(路径, 名字)`。
fn tool_dirs(root: &Path) -> Vec<(PathBuf, String)> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            (!name.starts_with('.')).then(|| (entry.path(), name))
        })
        .collect()
}

fn tool_item(
    tool_kind: PythonToolKind,
    name: &str,
    path: &Path,
    source: AssetSource,
) -> DevAssetItem {
    let prefix = match tool_kind {
        PythonToolKind::Uv => "uv",
        PythonToolKind::Pipx => "pipx",
    };
    DevAssetItem {
        id: format!("{prefix}:{name}"),
        kind: DevAssetKind::PythonTool {
            tool_kind,
            name: name.to_string(),
            executables: vec![name.to_string()],
        },
        path: path.to_path_buf(),
        size: AssetStorageSize::default(),
        last_change: tool_metadata_time(path),
        in_use: SpotCheck::Clear,
        is_active_env: false,
        source,
    }
}

/// 工具自己写的安装记录 / venv 目录的时间。
///
/// pipx 的 `pipx_metadata.json`、uv 的 `uv-receipt.toml` 是工具写的；两者都
/// 没有时退回目录 mtime，并如实标成 [`TimestampSource::DirectoryEntry`]——
/// 那是「目录项最后增删」，不是「最后使用」。
fn tool_metadata_time(venv_dir: &Path) -> Option<TimedEvidence> {
    for name in ["pipx_metadata.json", "uv-receipt.toml"] {
        if let Ok(md) = std::fs::metadata(venv_dir.join(name)) {
            if let Ok(at) = md.modified() {
                return Some(TimedEvidence {
                    at,
                    source: TimestampSource::ToolMetadata,
                });
            }
        }
    }
    std::fs::metadata(venv_dir)
        .and_then(|md| md.modified())
        .ok()
        .map(|at| TimedEvidence {
            at,
            source: TimestampSource::DirectoryEntry,
        })
}

fn layout_uv_tool_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(local) = crate::platform::user_cache_dir() {
        dirs.push(local.join("uv").join("tools"));
    }
    if let Some(home) = crate::platform::user_home() {
        dirs.push(home.join(".local/share/uv/tools"));
        dirs.push(home.join(".local/share/uv/tools".to_lowercase()));
    }
    dirs
}

fn layout_pipx_venvs_dirs() -> Vec<PathBuf> {
    crate::platform::user_home()
        .map(|home| vec![home.join(".local").join("pipx").join("venvs")])
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn proc_run(stdout: &str, ok: bool) -> ProcRun {
        ProcRun {
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
            exit_code: Some(if ok { 0 } else { 1 }),
            ok,
        }
    }

    /// 按 (程序, 参数) 查表回答的假执行器；未登记的调用返回 `None`
    /// （= 命令不存在），用来测降级路径。
    struct FakeRunner {
        answers: HashMap<String, Option<ProcRun>>,
    }

    impl FakeRunner {
        fn new() -> Self {
            Self {
                answers: HashMap::new(),
            }
        }
        fn answers(mut self, key: &str, value: Option<ProcRun>) -> Self {
            self.answers.insert(key.to_string(), value);
            self
        }
        fn run(&mut self, program: &str, args: &[&str], _timeout: Duration) -> Option<ProcRun> {
            self.answers
                .remove(&format!("{program} {}", args.join(" ")))
                .unwrap_or(None)
        }
    }

    fn conda_env(name: &str, path: &str) -> DevAssetItem {
        DevAssetItem {
            id: format!("conda:{path}"),
            kind: DevAssetKind::CondaEnv {
                name: name.into(),
                is_base: false,
                python_version: None,
                package_count: None,
            },
            path: PathBuf::from(path),
            size: super::AssetStorageSize::default(),
            last_change: None,
            in_use: SpotCheck::Clear,
            is_active_env: false,
            source: AssetSource::Tool,
        }
    }

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    /// 本机实探：列清单、标出来源与可否移除，但**不测体积**——体积要走进
    /// 每个环境的整棵树（几 GB、几十万文件），那是 `discover_all` 的活，
    /// 不该拖慢测试。
    ///
    /// 这条在 CI 上通常什么也扫不到（没有 conda / npm 全局包），它的用途是
    /// 在真机上人工核对「哪些条目被认得、哪些只展示」。
    #[test]
    fn live_probe_lists_the_assets_on_this_machine() {
        let mut run = crate::core::proc::run_tool_with_timeout;
        let t = std::time::Instant::now();
        let conda = conda_environments(&mut run);
        let t_conda = t.elapsed();
        let t = std::time::Instant::now();
        let python = python_environments(&mut run);
        let t_python = t.elapsed();
        let t = std::time::Instant::now();
        let node = node_global_packages(&mut run);
        let t_node = t.elapsed();
        let t = std::time::Instant::now();
        let tools = python_tools(&mut run);
        let t_tools = t.elapsed();
        println!(
            "phases: conda {:?}, python {:?}, node {:?}, tools {:?}",
            t_conda, t_python, t_node, t_tools
        );
        // 体积测定是唯一要走遍每个文件的一步，单独计时。
        let dirs: Vec<PathBuf> = conda
            .iter()
            .chain(python.iter())
            .chain(node.iter())
            .chain(tools.iter())
            .map(|item| item.path.clone())
            .collect();
        let t = std::time::Instant::now();
        let (total, _) = measure_batch_storage(&dirs);
        println!(
            "measure: {:?} for {} dirs, {} files, logical {}",
            t.elapsed(),
            dirs.len(),
            total.file_count,
            total.logical_bytes
        );
        println!(
            "live probe: {} conda, {} python, {} node, {} tools",
            conda.len(),
            python.len(),
            node.len(),
            tools.len()
        );
        for item in conda
            .iter()
            .chain(python.iter())
            .chain(node.iter())
            .chain(tools.iter())
        {
            println!(
                "  [{:?}] {} at {} removable={}",
                item.source,
                item.display_name(),
                item.path.display(),
                crate::core::dev_env::remove::can_remove(item)
            );
        }
    }

    #[test]
    fn conda_inventory_is_preferred_and_marks_the_base_environment() {
        let envs = temp_root("qc_conda_tool_envs");
        let base = envs.join("miniconda3");
        let derived = base.join("envs").join("data");
        std::fs::create_dir_all(&derived).unwrap();

        let info = format!(
            r#"{{"envs":["{}","{}"],"root_prefix":"{}"}}"#,
            base.to_string_lossy().replace('\\', "/"),
            derived.to_string_lossy().replace('\\', "/"),
            base.to_string_lossy().replace('\\', "/")
        );
        let mut runner =
            FakeRunner::new().answers("conda info --json", Some(proc_run(&info, true)));

        let items = conda_environments(&mut |p, a, t| runner.run(p, a, t));
        assert_eq!(items.len(), 2);
        let base_item = items
            .iter()
            .find(|item| item.display_name() == "base")
            .expect("root_prefix 指名的那个必须是 base");
        assert!(
            matches!(base_item.kind, DevAssetKind::CondaEnv { is_base: true, .. }),
            "base 由 conda 自己的 root_prefix 认定，不由安装根猜测"
        );
        assert!(items.iter().all(|item| item.source == AssetSource::Tool));

        let _ = std::fs::remove_dir_all(&envs);
    }

    #[test]
    fn without_the_tool_conda_falls_back_to_layout_and_is_not_removable() {
        let root = temp_root("qc_conda_layout");
        let base = root.join("miniconda3");
        std::fs::create_dir_all(base.join("envs").join("data")).unwrap();

        // 命令全部不可用 → 降级。这里不依赖本机真的装了 conda：把所有命令
        // 都回答「跑不起来」，降级路径走的就是 `common_conda_roots`。
        let mut runner = FakeRunner::new();
        let items = conda_environments(&mut |p, a, t| runner.run(p, a, t));
        // 降级扫描看的是真实 home / 常见安装根，这里不断言它一定扫到了
        // 我们刚造的临时目录，只断言降级条目一律不可移除。
        for item in &items {
            assert_eq!(item.source, AssetSource::Layout);
            assert!(
                !crate::core::dev_env::remove::can_remove(item),
                "降级条目不得提供移除入口"
            );
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dot_conda_directory_is_never_listed_as_an_environment() {
        // `~/.conda` 存的是配置与 environments.txt，不是环境。把它当安装根
        // 会凭空多出一个名叫 base 的假环境。
        let roots = common_conda_roots();
        assert!(
            !roots.iter().any(|root| root.ends_with(".conda")),
            "安装根名单里不得出现 ~/.conda，实际 {:?}",
            roots
        );
    }

    #[test]
    fn npm_top_level_packages_are_tool_sourced_and_named_by_the_command() {
        let prefix = temp_root("qc_npm_tool_prefix");
        let mut runner = FakeRunner::new()
            .answers(
                "npm prefix --global",
                Some(proc_run(&format!("{}\n", prefix.display()), true)),
            )
            .answers(
                "npm ls --global --depth=0 --json",
                Some(proc_run(
                    r#"{"dependencies":{"typescript":{"version":"5.4.2"}}}"#,
                    true,
                )),
            );

        let items = node_global_packages(&mut |p, a, t| runner.run(p, a, t));
        let typescript = items
            .iter()
            .find(|item| item.display_name() == "typescript")
            .expect("npm 顶级依赖应入表");
        assert_eq!(typescript.source, AssetSource::Tool);
        assert!(crate::core::dev_env::remove::can_remove(typescript));
        match &typescript.kind {
            DevAssetKind::NodeGlobalPackage {
                manager, version, ..
            } => {
                assert_eq!(manager, "npm");
                assert_eq!(version, "5.4.2");
            }
            other => panic!("期望 NodeGlobalPackage，实际 {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&prefix);
    }

    #[test]
    fn bun_items_are_never_removable_even_when_its_manifest_declares_them() {
        // bun 的顶层清单是它自己写的（`<global>/package.json`），所以来源是
        // 「生态声明」；但 bun 没有核实过的卸载语义，仍然不给删除入口。
        // 「清单可信」与「能删」是两件事，不能用一个字段表达。
        let global = temp_root("qc_bun_global");
        let node_modules = global.join("node_modules");
        std::fs::create_dir_all(node_modules.join("typescript")).unwrap();
        std::fs::write(
            node_modules.join("typescript").join("package.json"),
            br#"{"name":"typescript","version":"5.4.2"}"#,
        )
        .unwrap();
        std::fs::write(
            global.join("package.json"),
            br#"{"dependencies":{"typescript":"5.4.2"}}"#,
        )
        .unwrap();

        let items = node_modules_items("bun", &node_modules);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].source, AssetSource::Tool, "清单是生态自己写的");
        assert!(
            !crate::core::dev_env::remove::can_remove(&items[0]),
            "bun 没有核实过的卸载通道，任何来源的条目都不能删"
        );

        let _ = std::fs::remove_dir_all(&global);
    }

    #[test]
    fn a_transitive_dependency_is_not_mistaken_for_an_installed_package() {
        // 本机实测的形态：bun 声明 3 个全局包，目录里躺着 132 个目录。
        // 按目录列会把 129 个传递依赖当成用户装的包展示，还给删除提供错误候选。
        let global = temp_root("qc_bun_transitive");
        let node_modules = global.join("node_modules");
        for name in ["tool", "its-dep"] {
            std::fs::create_dir_all(node_modules.join(name)).unwrap();
            std::fs::write(
                node_modules.join(name).join("package.json"),
                format!(r#"{{"name":"{name}","version":"1.0.0"}}"#),
            )
            .unwrap();
        }
        std::fs::write(
            global.join("package.json"),
            br#"{"dependencies":{"tool":"1.0.0"}}"#,
        )
        .unwrap();

        let items = node_modules_items("bun", &node_modules);
        let names: Vec<&str> = items.iter().map(|item| item.display_name()).collect();
        assert_eq!(names, vec!["tool"], "its-dep 是传递依赖，不该入表");

        let _ = std::fs::remove_dir_all(&global);
    }

    #[test]
    fn without_a_manifest_the_listing_falls_back_to_the_directory() {
        let global = temp_root("qc_bun_no_manifest");
        let node_modules = global.join("node_modules");
        std::fs::create_dir_all(node_modules.join("thing")).unwrap();
        std::fs::write(
            node_modules.join("thing").join("package.json"),
            br#"{"name":"thing","version":"2.0.0"}"#,
        )
        .unwrap();

        let items = node_modules_items("bun", &node_modules);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].source, AssetSource::Layout, "问不出清单就是降级");
        assert!(!crate::core::dev_env::remove::can_remove(&items[0]));

        let _ = std::fs::remove_dir_all(&global);
    }

    #[test]
    fn reclaimable_bytes_excludes_read_only_assets() {
        // 「可释放」必须只算这个页面真能删的部分。只读资产算进去，用户会看到
        // 一个永远兑现不了的数字。
        let mut removable = conda_env("old", "/h/miniconda3/envs/old");
        removable.size = super::AssetStorageSize {
            logical_bytes: 100,
            exclusive_reclaimable_bytes: 100,
            file_count: 1,
        };
        let mut read_only = conda_env("base", "/h/miniconda3");
        read_only.source = AssetSource::Layout;
        read_only.size = super::AssetStorageSize {
            logical_bytes: 900,
            exclusive_reclaimable_bytes: 900,
            file_count: 9,
        };

        assert_eq!(reclaimable_bytes(&[&[removable, read_only]]), 100);
    }

    #[test]
    fn the_same_directory_never_enters_the_table_twice() {
        // 同一路径两种写法（大小写不同）只能算一条；否则体积翻倍，
        // 且其中一条会漏掉 base/激活标记。
        let root = temp_root("qc_dedupe");
        let env_a = root.join("envs").join("Data");
        let env_b = root.join("envs").join("data");
        std::fs::create_dir_all(&env_a).unwrap();
        // Windows 文件系统不区分大小写，第二次 create 会命中同一个目录；
        // macOS 默认也不区分。这里只要求结果里 path 规范化后唯一。
        let _ = std::fs::create_dir_all(&env_b);

        let mut seen = HashSet::new();
        let items = conda_environments(&mut |_, _, _| None);
        for item in &items {
            assert!(
                seen.insert(crate::core::safety::norm(&item.path)),
                "规范化路径重复：{}",
                item.path.display()
            );
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn timestamps_only_come_from_tool_authored_sources() {
        // 目录 mtime 只能作为最后手段出现，且必须如实标成 DirectoryEntry；
        // 绝不允许冒用「使用时间」。
        let dir = temp_root("qc_timestamp_source");
        let evidence = tool_metadata_time(&dir).expect("目录存在就应给出时间");
        assert_eq!(evidence.source, TimestampSource::DirectoryEntry);
        assert!(!matches!(
            evidence.source,
            TimestampSource::CondaTransaction | TimestampSource::PackageManifest
        ));

        // 工具自己写了元数据文件时，来源必须升到 ToolMetadata。
        std::fs::write(dir.join("pipx_metadata.json"), b"{}").unwrap();
        let evidence = tool_metadata_time(&dir).expect("元数据文件存在");
        assert_eq!(evidence.source, TimestampSource::ToolMetadata);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn inspect_conda_meta_reads_version_count_and_transaction_time() {
        let dir = temp_root("qc_conda_meta");
        let meta = dir.join("conda-meta");
        std::fs::create_dir_all(&meta).unwrap();
        std::fs::write(meta.join("history"), "==> 2026-10-06 12:00:00 <==").unwrap();
        std::fs::write(meta.join("python-3.11.8-h123.json"), "{}").unwrap();
        std::fs::write(meta.join("numpy-1.26.0-h456.json"), "{}").unwrap();

        let (python, count, last_change) = inspect_conda_meta(&dir);
        assert_eq!(python.as_deref(), Some("3.11.8"));
        assert_eq!(count, Some(2));
        assert_eq!(
            last_change.map(|evidence| evidence.source),
            Some(TimestampSource::CondaTransaction)
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scoped_packages_and_manager_internals_are_handled() {
        let root = temp_root("qc_scope/node_modules");
        std::fs::create_dir_all(root.join("@scope").join("thing")).unwrap();
        std::fs::create_dir_all(root.join(".pnpm")).unwrap();
        std::fs::write(
            root.join("@scope").join("thing").join("package.json"),
            br#"{"name":"@scope/thing","version":"1.0.0"}"#,
        )
        .unwrap();
        std::fs::write(root.join(".modules.yaml"), b"x").unwrap();

        // 这个根没有旁边的清单文件 → 走「按目录列」的降级路径。
        let items = list_node_modules("npm", &root);
        assert_eq!(items.len(), 1, "作用域包算一个，内部件不算：{items:?}");
        assert_eq!(items[0].display_name(), "@scope/thing");
        assert_eq!(items[0].source, AssetSource::Layout);

        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("qc_scope"));
    }
}
