//! 开发环境与全局包资产管理模块。
//!
//! 支持 Conda/Mamba 虚拟环境、Node/Bun/pnpm 全局包、uv/pipx 独立 CLI 工具的
//! 资产发现、存储度量（逻辑与独占可释放空间）及受限生命周期管理。
//!
//! 分层：`discovery` 只读探测 → `inventory` 生态命令的唯一解析入口 →
//! `remove` 唯一移除通道。发现与移除共用 `inventory`，所以「界面上列出的」
//! 与「预检认得的」永远是同一份清单。

pub mod discovery;
pub mod inventory;
pub mod python;
pub mod remove;
pub mod storage;

use crate::core::inuse::SpotCheck;
use std::path::PathBuf;
use std::time::SystemTime;
pub use storage::AssetStorageSize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DevAssetKind {
    CondaEnv {
        name: String,
        is_base: bool,
        python_version: Option<String>,
        package_count: Option<usize>,
    },
    NodeGlobalPackage {
        manager: String,
        name: String,
        version: String,
        bin_shims: Vec<String>,
    },
    PythonTool {
        tool_kind: PythonToolKind,
        name: String,
        executables: Vec<String>,
    },
    /// 一个 Python 解释器的**某一个包目录**。行本身没有移除入口——可移除的
    /// 是它里面的 [`DevAssetKind::PipPackage`] 条目，划分见 `python` 模块头。
    ///
    /// 一个解释器通常有两个包目录（安装级的与用户级的），所以它是两行。商店版
    /// CPython 尤其明显：安装目录只读，用户装的东西全在用户级目录里——把两者
    /// 混成一个数字，就会出现「`pip list` 有 145 个包，界面说只有 1 个」。
    PythonInterpreter {
        name: String,
        version: Option<String>,
        scope: SiteScope,
        package_count: Option<usize>,
        site_packages: Option<PathBuf>,
        /// 拥有这个包目录的解释器（探测期解析出的绝对路径）。用户级目录展开
        /// 后按包卸载要用它跑 `python -m pip uninstall`。
        python: PathBuf,
    },
    /// 一个虚拟环境。行本身可整体删除（`pyvenv.cfg` 自证，见
    /// `remove::Channel::VenvDirectory`），也可以展开逐包卸载；仍然没有的
    /// 是「还有没有项目在用」的证据（P39 的处境），那由确认弹窗交给用户判断。
    VirtualEnv {
        name: String,
        version: Option<String>,
        /// `pyvenv.cfg` 的 `home`，指向基础解释器。
        base_python: Option<String>,
        package_count: Option<usize>,
        manager: VenvManager,
        /// 扫描期的目录对象身份；未知身份不能获得树删除授权。
        identity: Option<VenvIdentity>,
    },
    /// 一个 pip 安装的发行版。`*.dist-info` 目录（PEP 376）是 pip 自己写的
    /// 登记，`pip install` 就能原样装回来——「重新安装即可恢复」的边界在
    /// 这里成立，所以走 pip 通道卸载（`remove::Channel::Pip`）。
    PipPackage {
        /// 拥有这个包所在 site-packages 的解释器；venv 里的包是 venv 自己
        /// 的 python。卸载 argv 的 program 在预检时由它定死。
        python: PathBuf,
        /// `METADATA` 的 `Name:`——规范名，卸载命令用它。
        name: String,
        version: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VenvIdentity((u64, u64));

impl VenvIdentity {
    pub(crate) fn capture(path: &std::path::Path) -> Option<Self> {
        if !path.is_absolute() || path.ancestors().count() > 128 {
            return None;
        }
        for ancestor in path.ancestors() {
            let md = std::fs::symlink_metadata(ancestor).ok()?;
            if !md.is_dir() || md.file_type().is_symlink() {
                return None;
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if md.file_attributes() & winapi::um::winnt::FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                    return None;
                }
            }
        }
        #[cfg(windows)]
        {
            crate::platform::windows::identity::object_id(path).map(Self)
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let md = std::fs::symlink_metadata(path).ok()?;
            Some(Self((md.dev(), md.ino())))
        }
    }

    pub(crate) fn recheck(self, path: &std::path::Path) -> bool {
        Self::capture(path) == Some(self)
    }
}

/// 包目录是哪一级的。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SiteScope {
    /// 解释器自带的 `Lib/site-packages`（商店版 / 系统 Python 上通常是只读的）。
    Install,
    /// 用户级 site-packages（`site.getusersitepackages()`）。用户 `pip install`
    /// 装的东西默认落在这里——安装目录只读时更是唯一的去处。
    User,
}

/// 谁管的虚拟环境。只影响展示归类——两者的目录结构与可删性没有区别，
/// 而本轮都不提供移除入口。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VenvManager {
    /// virtualenvwrapper（`$WORKON_HOME` / `~/.virtualenvs`）。
    Virtualenvwrapper,
    /// Poetry 的 virtualenvs。
    Poetry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PythonToolKind {
    Uv,
    Pipx,
}

/// 这个条目是怎么来的。
///
/// 只有 [`AssetSource::Tool`] 的条目才提供移除入口：`Layout` 来源的路径
/// 没有被生态确认过，拿它去执行删除等于用猜的路径动刀。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetSource {
    /// 生态自己的清单命令报告的。
    Tool,
    /// 命令不可用或输出读不懂时，按已知安装布局扫出来的降级结果，只展示。
    Layout,
}

/// 这个时间点是从哪儿来的。
///
/// 措辞不是小事：`CondaTransaction` 是「环境最后一次装/卸包」，
/// `PackageManifest` 是「装到当前版本的时间」，`DirectoryEntry` 是「目录项
/// 最后增删」——**没有一个**等于「最后一次使用」。界面必须按来源措辞，
/// 不能笼统写成「最近使用」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimestampSource {
    /// conda 事务记录（`conda-meta/history`，conda 每次事务重写）。
    CondaTransaction,
    /// 包自己的 `package.json`：安装或升到当前版本的时间。
    PackageManifest,
    /// 工具自己的元数据（pipx 的 `pipx_metadata.json`、uv 的 `uv-receipt.toml`）。
    ToolMetadata,
    /// 只拿得到目录 mtime：目录直接子项最后增删的时间。拿不到上面任何一种
    /// 时的最后手段。
    DirectoryEntry,
    /// 虚拟环境的 `site-packages` 目录 mtime：最后一次装/卸包。venv 没有别的
    /// 自证记录，这个语义是确定的，所以单独一档而不是混进 `DirectoryEntry`。
    SitePackages,
}

/// 生态写下的一个时间点，连同它的含义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimedEvidence {
    pub at: SystemTime,
    pub source: TimestampSource,
}

impl TimedEvidence {
    /// 距 `now` 多少天。
    ///
    /// 时间点在将来（系统时钟被回拨过、或文件来自别的时间基准）时按 0 计，
    /// 不返回负数——界面上不该出现「-3 天前」。
    pub fn age_days(&self, now: SystemTime) -> u64 {
        now.duration_since(self.at)
            .map(|elapsed| elapsed.as_secs() / 86_400)
            .unwrap_or(0)
    }
}

#[derive(Clone, Debug)]
pub struct DevAssetItem {
    pub id: String,
    pub kind: DevAssetKind,
    pub path: PathBuf,
    pub size: AssetStorageSize,
    /// 生态自证的时间点。`None` = 拿不到任何依据，界面显示「—」而不是
    /// 拿别的字段凑一个。
    pub last_change: Option<TimedEvidence>,
    pub in_use: SpotCheck,
    pub is_active_env: bool,
    pub source: AssetSource,
}

impl DevAssetItem {
    pub fn display_name(&self) -> &str {
        match &self.kind {
            DevAssetKind::CondaEnv { name, .. } => name,
            DevAssetKind::NodeGlobalPackage { name, .. } => name,
            DevAssetKind::PythonTool { name, .. } => name,
            DevAssetKind::PythonInterpreter { name, .. } => name,
            DevAssetKind::VirtualEnv { name, .. } => name,
            DevAssetKind::PipPackage { name, .. } => name,
        }
    }

    /// base 环境与当前激活环境：生态语义上的不可删，不是路径黑名单。
    pub fn is_protected(&self) -> bool {
        match &self.kind {
            DevAssetKind::CondaEnv { is_base, .. } => *is_base || self.is_active_env,
            _ => self.is_active_env,
        }
    }
}
