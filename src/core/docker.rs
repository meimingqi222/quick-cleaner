//! Docker 镜像清理的机制层：CLI 封装 + 纯解析/选择逻辑。
//!
//! 桌面版 Docker 把所有镜像装在一个虚拟机磁盘文件里（macOS 的
//! `Docker.raw`、Windows 的 `ext4.vhdx`），宿主文件系统只能看到一整个
//! 大文件，文件级的扫描-清理管线看不见单个镜像。所以这个类别不走路径：
//! 发现靠 `docker images`，清理靠 `docker image rm`，条目用
//! `docker://image/<ref>` 虚拟路径表示（见 [`crate::core::model`]）。
//!
//! docker CLI 在两个平台上的命令面完全一致，机制只此一份，不进平台
//! 契约；唯一的平台差异是 Windows 下子进程要抑制控制台窗口闪烁。

use std::collections::{HashMap, HashSet};

/// `docker images` 里悬空镜像的显示名。
const NONE: &str = "<none>";

/// 用 `|` 分隔而不是官方文档惯用的 `\t`：Go 模板里的转义序列依赖 docker
/// 的预处理，管道符没有这层不确定性，镜像名/标签也不可能包含它。
const IMAGES_FORMAT: &str = "{{.ID}}|{{.Repository}}|{{.Tag}}|{{.Size}}";

/// `docker images` 的一行。
#[derive(Clone, Debug, PartialEq)]
pub struct DockerImage {
    /// 归一化后的镜像 ID（剥掉 `sha256:` 前缀的十六进制串）。
    pub id: String,
    pub repository: String,
    pub tag: String,
    /// 从 "187MB" 这类人读大小解析出的字节。
    pub size: u64,
}

/// 镜像被判定为冗余的原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JunkKind {
    /// 悬空镜像（`<none>`）：构建过程留下的无标签层。
    Dangling,
    /// 同仓库存在更新版本号标签的旧版本。
    OldVersion,
    /// 未被任何容器（含已停止容器）引用。
    Unreferenced,
}

/// 一个可清理的镜像条目。
#[derive(Clone, Debug, PartialEq)]
pub struct DockerJunk {
    pub image: DockerImage,
    pub kind: JunkKind,
}

impl DockerJunk {
    /// 交给 `docker image rm` 的引用参数。
    ///
    /// 带标签的用 `repo:tag` 而不是镜像 ID：同一镜像 ID 常挂多个标签
    /// （`nginx:stable` 与 `nginx:1.24` 往往同 ID），按 ID 删要么报错、
    /// 要么 `--force` 连用户没勾选的标签一起摘掉。悬空镜像没有可用的
    /// 名称引用，只能按 ID。
    pub fn rmi_ref(&self) -> String {
        if self.image.repository == NONE {
            self.image.id.clone()
        } else {
            format!("{}:{}", self.image.repository, self.image.tag)
        }
    }
}

fn docker_command(args: &[&str]) -> std::process::Command {
    let mut cmd = std::process::Command::new("docker");
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // GUI 子系统下不抑制的话，每次调用都会闪一个控制台黑框
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

/// 枚举本机镜像。CLI 没装、守护进程没跑都返回空——类别静默消失，
/// 与 LocalSnapshots 查不到快照时的行为一致。
pub fn list_images() -> Vec<DockerImage> {
    let Ok(out) = docker_command(&["images", "--format", IMAGES_FORMAT]).output() else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    parse_images(&String::from_utf8_lossy(&out.stdout))
}

/// 所有容器（含已停止）引用的镜像名，用于防误删预过滤。
pub fn list_container_refs() -> Vec<String> {
    let Ok(out) = docker_command(&["container", "ls", "-a", "--format", "{{.Image}}"]).output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// `docker image rm`。`Ok(true)` 表示有镜像层被真删（stdout 出现
/// `Deleted:` 行，磁盘空间实际释放）；`Ok(false)` 表示只摘了标签
/// （仅有 `Untagged:` 行，空间未释放）。
pub fn remove_image(rmi_ref: &str) -> Result<bool, String> {
    match docker_command(&["image", "rm", rmi_ref]).output() {
        Ok(out) if out.status.success() => {
            Ok(String::from_utf8_lossy(&out.stdout).contains("Deleted:"))
        }
        Ok(out) => Err(String::from_utf8_lossy(&out.stderr).trim().to_string()),
        Err(e) => Err(format!("无法启动 docker：{e}")),
    }
}

/// 解析 `docker images --format` 的输出，一行一镜像，坏行跳过。
pub fn parse_images(stdout: &str) -> Vec<DockerImage> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let mut fields = line.split('|');
        let (Some(id), Some(repository), Some(tag), Some(size)) = (
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
        ) else {
            continue;
        };
        if id.is_empty() || repository.is_empty() {
            continue;
        }
        out.push(DockerImage {
            id: id.trim_start_matches("sha256:").to_string(),
            repository: repository.to_string(),
            tag: tag.to_string(),
            size: parse_size(size),
        });
    }
    out
}

/// 解析 docker 的人读大小（"187MB"、"1.24GB"、"823kB"）为字节。
/// docker 用 SI 十进制（kB = 1000）。失败返回 0——体积只影响展示与
/// 进度条，不值得让整个类别失败。
pub fn parse_size(s: &str) -> u64 {
    let s = s.trim();
    let unit_start = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(s.len());
    let (num, unit) = s.split_at(unit_start);
    let value: f64 = num.parse().unwrap_or(0.0);
    let mult = match unit.trim() {
        "B" => 1.0,
        "kB" | "KB" => 1e3,
        "MB" => 1e6,
        "GB" => 1e9,
        "TB" => 1e12,
        _ => 1.0,
    };
    (value * mult) as u64
}

/// 版本号形态的标签：整段都是点分数字（"1.27"、"22.04"）。
/// "1.27-alpine"、"latest" 这类变体/别名不算——变体是用户的主动选择，
/// 不是能推断新旧的版本号。
fn version_tag(tag: &str) -> Option<Vec<u64>> {
    if tag.is_empty() {
        return None;
    }
    tag.split('.').map(|seg| seg.parse::<u64>().ok()).collect()
}

/// 从镜像清单里挑出冗余项，优先级 悬空 > 旧版本 > 未引用。
///
/// 旧版本与未引用都只对「未被任何容器引用」的镜像生效——被停止容器
/// 占着的镜像删不掉（rmi 会报错），在发现阶段就拦掉，不放进列表。
/// 按 `rmi_ref` 去重：同一个标签只能生成一个条目，否则虚拟路径重复
/// 会让按路径去重的选中集互相串。
pub fn select_docker_junk(images: &[DockerImage], container_refs: &[String]) -> Vec<DockerJunk> {
    let mut junk: Vec<DockerJunk> = Vec::new();
    let mut taken: HashSet<String> = HashSet::new();

    for image in images {
        if image.repository != NONE || is_referenced(image, container_refs) {
            continue;
        }
        let item = DockerJunk {
            image: image.clone(),
            kind: JunkKind::Dangling,
        };
        taken.insert(item.rmi_ref());
        junk.push(item);
    }

    // 同仓库分组，版本号标签多于一个时保留最新的，其余未引用的报旧版本。
    let mut by_repo: HashMap<&str, Vec<&DockerImage>> = HashMap::new();
    for image in images {
        if image.repository != NONE {
            by_repo
                .entry(image.repository.as_str())
                .or_default()
                .push(image);
        }
    }
    for images_of_repo in by_repo.values() {
        let mut versioned: Vec<(&DockerImage, Vec<u64>)> = images_of_repo
            .iter()
            .filter_map(|img| version_tag(&img.tag).map(|v| (*img, v)))
            .collect();
        if versioned.len() < 2 {
            continue;
        }
        versioned.sort_by_key(|(_, v)| v.clone());
        for (image, _) in &versioned[..versioned.len() - 1] {
            if is_referenced(image, container_refs) {
                continue;
            }
            let item = DockerJunk {
                image: (*image).clone(),
                kind: JunkKind::OldVersion,
            };
            if taken.insert(item.rmi_ref()) {
                junk.push(item);
            }
        }
    }

    // 剩下的未引用标签（含各仓库的最新版、latest、alpine 这类变体）。
    for image in images {
        if image.repository == NONE || is_referenced(image, container_refs) {
            continue;
        }
        let item = DockerJunk {
            image: image.clone(),
            kind: JunkKind::Unreferenced,
        };
        if taken.insert(item.rmi_ref()) {
            junk.push(item);
        }
    }
    junk
}

/// 容器引用匹配。容器列表的 IMAGE 列可能是 `repo:tag`、省略了
/// `:latest` 的裸仓库名，或镜像名被摘掉后退化的镜像 ID（长短皆可能）。
/// 宁可多匹配（少报可清理）也不误删正在使用的镜像。
fn is_referenced(image: &DockerImage, container_refs: &[String]) -> bool {
    container_refs.iter().any(|r| {
        let r = r.trim();
        if r.is_empty() {
            return false;
        }
        if r == format!("{}:{}", image.repository, image.tag) {
            return true;
        }
        if image.tag == "latest" && r == image.repository {
            return true;
        }
        let r_hex = r.strip_prefix("sha256:").unwrap_or(r);
        r_hex.len() >= 6
            && image.id.len() >= 6
            && (image.id.starts_with(r_hex) || r_hex.starts_with(image.id.as_str()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(id: &str, repo: &str, tag: &str, size: u64) -> DockerImage {
        DockerImage {
            id: id.to_string(),
            repository: repo.to_string(),
            tag: tag.to_string(),
            size,
        }
    }

    /// docker 实际输出的形态（管道分隔 + <none> 悬空行 + 注册表长仓库名）。
    #[test]
    fn parse_images_handles_real_output() {
        let stdout = "a1b2c3d4e5f6|nginx|1.27|187MB\n\
                      f6e5d4c3b2a1|<none>|<none>|823kB\n\
                      001122334455|ghcr.io/owner/img|1.0|1.24GB\n\
                      \n\
                      garbage line without pipes\n";
        let images = parse_images(stdout);
        assert_eq!(images.len(), 3);
        assert_eq!(images[1].repository, "<none>");
        assert_eq!(images[1].size, 823_000);
        assert_eq!(images[2].repository, "ghcr.io/owner/img");
        assert_eq!(images[2].size, 1_240_000_000);
    }

    #[test]
    fn parse_size_units() {
        assert_eq!(parse_size("0B"), 0);
        assert_eq!(parse_size("823kB"), 823_000);
        assert_eq!(parse_size("187MB"), 187_000_000);
        assert_eq!(parse_size("1.24GB"), 1_240_000_000);
        assert_eq!(parse_size("2.5TB"), 2_500_000_000_000);
        // 解析失败按 0 处理，不 panic
        assert_eq!(parse_size("n/a"), 0);
    }

    #[test]
    fn dangling_images_are_selected_by_id() {
        let images = [img("f6e5d4c3b2a1", "<none>", "<none>", 823_000)];
        let junk = select_docker_junk(&images, &[]);
        assert_eq!(junk.len(), 1);
        assert_eq!(junk[0].kind, JunkKind::Dangling);
        // 悬空镜像没有名称引用，rmi 参数就是 ID
        assert_eq!(junk[0].rmi_ref(), "f6e5d4c3b2a1");
    }

    /// 被容器（哪怕已停止）引用的镜像一律不报，包括悬空镜像。
    /// ID 用真实形态的 12 位十六进制（is_referenced 对过短的 ID 前缀
    /// 有防误报护栏）。
    #[test]
    fn referenced_images_are_excluded() {
        let images = [
            img("aaa1aaa1aaa1", "nginx", "1.25", 100),
            img("bbb2bbb2bbb2", "<none>", "<none>", 50),
        ];
        let refs = vec!["nginx:1.25".to_string(), "bbb2bbb2bbb2".to_string()];
        assert!(select_docker_junk(&images, &refs).is_empty());
    }

    /// 同仓库多版本只保留最新，其余报旧版本；最新版未引用时归入
    /// Unreferenced 而不是漏掉。
    #[test]
    fn old_versions_keep_newest_per_repo() {
        let images = [
            img("aaa1", "nginx", "1.25", 100),
            img("aaa2", "nginx", "1.26", 101),
            img("aaa3", "nginx", "1.27", 102),
        ];
        let junk = select_docker_junk(&images, &[]);
        let old: Vec<_> = junk
            .iter()
            .filter(|j| j.kind == JunkKind::OldVersion)
            .collect();
        assert_eq!(old.len(), 2);
        // rmi 参数按 repo:tag，不按 ID——同 ID 多标签时不能误伤别的标签
        assert_eq!(old[0].rmi_ref(), "nginx:1.25");
        assert_eq!(old[1].rmi_ref(), "nginx:1.26");
        // 最新版未被引用，报为 Unreferenced
        assert!(junk
            .iter()
            .any(|j| j.kind == JunkKind::Unreferenced && j.rmi_ref() == "nginx:1.27"));
    }

    /// 版本号比较是逐段数值比较：1.9 > 1.27 是错的，1.10 > 1.9 才对。
    #[test]
    fn version_compare_is_numeric_per_segment() {
        let images = [img("aaa1", "app", "1.9", 1), img("aaa2", "app", "1.10", 1)];
        let junk = select_docker_junk(&images, &[]);
        let old: Vec<_> = junk
            .iter()
            .filter(|j| j.kind == JunkKind::OldVersion)
            .collect();
        assert_eq!(old.len(), 1);
        assert_eq!(old[0].rmi_ref(), "app:1.9");
    }

    /// 变体/别名标签不参与新旧推断：只剩一个版本号标签时没有"旧版本"。
    #[test]
    fn variant_tags_never_marked_old() {
        let images = [
            img("aaa1", "nginx", "1.26", 1),
            img("aaa2", "nginx", "1.27-alpine", 1),
            img("aaa3", "nginx", "latest", 1),
        ];
        let junk = select_docker_junk(&images, &[]);
        assert!(junk.iter().all(|j| j.kind == JunkKind::Unreferenced));
    }

    /// 未引用的镜像报 Unreferenced；容器用裸仓库名引用（省略 :latest）
    /// 也要命中。
    #[test]
    fn unreferenced_reported_and_bare_repo_ref_counts() {
        let images = [
            img("aaa1", "redis", "latest", 1),
            img("bbb2", "postgres", "16", 1),
        ];
        let junk = select_docker_junk(&images, &["redis".to_string()]);
        let refs: Vec<String> = junk.iter().map(|j| j.rmi_ref()).collect();
        assert_eq!(refs, vec!["postgres:16"]);
        assert_eq!(junk[0].kind, JunkKind::Unreferenced);
    }

    /// 容器的 IMAGE 列退化为镜像 ID（长短前缀皆可）时按前缀匹配。
    #[test]
    fn container_shows_image_id_prefix() {
        let full = "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c";
        let images = [img(full, "app", "1.0", 1)];
        let short = &full[..12];
        let junk = select_docker_junk(&images, &[short.to_string()]);
        assert!(junk.is_empty());
    }

    /// 每个条目的 rmi_ref 唯一——虚拟路径按它构造，重复会让选中集串项。
    #[test]
    fn rmi_refs_are_unique() {
        let images = [
            img("aaa1", "nginx", "1.25", 1),
            img("aaa2", "nginx", "1.26", 1),
            img("aaa3", "app", "2.0", 1),
            img("ddd4", "<none>", "<none>", 1),
        ];
        let junk = select_docker_junk(&images, &[]);
        let refs: HashSet<String> = junk.iter().map(|j| j.rmi_ref()).collect();
        assert_eq!(refs.len(), junk.len());
    }
}
