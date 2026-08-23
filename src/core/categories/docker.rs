//! Docker 冗余镜像的发现端：机制与选择规则在 [`crate::core::docker`]。

use super::{target_with_size, CategoryId, ScanTarget};
use crate::core::docker::{self, JunkKind};
use crate::core::i18n::Text;
use crate::core::model::docker_image_path;

/// 枚举冗余镜像并生成虚拟路径目标。
///
/// docker CLI 没装或守护进程没跑时 `list_images` 返回空，这里直接返回
/// ——类别在界面上静默消失，与查不到 APFS 快照时的行为一致。
pub(super) fn push_docker_targets(t: &mut Vec<ScanTarget>) {
    let images = docker::list_images();
    if images.is_empty() {
        return;
    }
    let container_refs = docker::list_container_refs();
    for junk in docker::select_docker_junk(&images, &container_refs) {
        let label = match junk.kind {
            JunkKind::Dangling => Text::new(
                format!("悬空镜像 {}（构建残留）", &junk.image.id[..junk.image.id.len().min(12)]),
                format!(
                    "Dangling image {} (build residue)",
                    &junk.image.id[..junk.image.id.len().min(12)]
                ),
            ),
            JunkKind::OldVersion => Text::new(
                format!("{}:{}（旧版本）", junk.image.repository, junk.image.tag),
                format!("{}:{} (old version)", junk.image.repository, junk.image.tag),
            ),
            JunkKind::Unreferenced => Text::new(
                format!("{}:{}（未被容器使用）", junk.image.repository, junk.image.tag),
                format!(
                    "{}:{} (unused by any container)",
                    junk.image.repository, junk.image.tag
                ),
            ),
        };
        let path = docker_image_path(&junk.rmi_ref());
        t.push(target_with_size(
            path,
            label,
            CategoryId::DockerImages,
            junk.image.size,
        ));
    }
}
