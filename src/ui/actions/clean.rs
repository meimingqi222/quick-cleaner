//! 清理执行动作：垃圾清理、路径清理、取消清理

use crate::core::cleaner::{
    clean_arbitrary_items, clean_targets, ArbitraryTarget, CleanFailure, CleanProgress,
    CleanReport, CleanSnapshot, CleanTarget,
};
use crate::core::i18n::{bilingual, Text};
use crate::core::model::{fmt_size, is_virtual_path};
use crate::core::safety::is_protected;
use crate::ui::components::{ConfirmKind, ConfirmRequest};
use crate::ui::i18n::*;
use gpui::Context;
use std::path::PathBuf;
use std::sync::Arc;

impl crate::ui::Root {
    pub fn request_clean_selected(&mut self, cx: &mut Context<Self>) {
        let count = self.selected_count();
        if count == 0 || self.clean.running || !self.junk.scanned {
            return;
        }
        let lang = self.language;
        self.confirm = Some(ConfirmRequest {
            title: tr_confirm_clean_selected_title(lang).to_string(),
            body: tr_confirm_delete_msg(lang, count, &fmt_size(self.selected_size())),
            detail: tr_confirm_clean_selected_detail(lang).to_string(),
            kind: ConfirmKind::CleanSelected,
            app_data: false,
        });
        cx.notify();
    }

    pub fn request_clean_path(&mut self, path: PathBuf, size: u64, cx: &mut Context<Self>) {
        if self.clean.running {
            return;
        }
        let lang = self.language;
        if is_protected(&path) {
            let shown = path.display().to_string();
            self.status = bilingual(|l| tr_protected_path(l, &shown));
            cx.notify();
            return;
        }

        let app_data = crate::core::safety::under_home_app_support(&path);
        self.confirm = Some(ConfirmRequest {
            title: tr_confirm_delete_title(lang).to_string(),
            body: tr_confirm_delete_path_msg(lang, &path.display().to_string(), &fmt_size(size)),
            detail: tr_confirm_no_recycle_check_running(lang).to_string(),
            kind: ConfirmKind::CleanPath(path, size),
            app_data,
        });
        cx.notify();
    }

    pub fn confirm_accept(&mut self, cx: &mut Context<Self>) {
        let Some(req) = self.confirm.take() else {
            return;
        };
        match req.kind {
            ConfirmKind::CleanSelected => self.start_clean(cx),
            ConfirmKind::CleanPath(p, size) => self.start_clean_path(p, size, cx),
            ConfirmKind::CleanDiskSelected => self.start_clean_disk_selected(cx),
            ConfirmKind::UninstallApp(app) => self.execute_uninstall_app(*app, cx),
            ConfirmKind::KillProcess {
                pid,
                start_time,
                unique_id,
                name,
            } => self.kill_process(pid, start_time, unique_id, name, cx),
            ConfirmKind::InstallFanHelper(mode) => self.install_fan_helper_and_apply(mode, cx),
        }
    }

    pub fn clean_snapshot(&self) -> Option<CleanSnapshot> {
        self.clean.progress.as_ref().map(|p| p.snapshot())
    }

    pub fn cancel_clean(&mut self, cx: &mut Context<Self>) {
        if let Some(p) = &self.clean.progress {
            p.request_cancel();
            self.status = bilingual(|l| tr_status_stopping(l).to_string());
            cx.notify();
        }
    }

    /// 三个清理入口共用的编排。
    ///
    /// 「置 cleaning → 建进度 → 写状态栏 → 起心跳 → 后台删 → 回主线程收尾」
    /// 这套仪式以前在 `start_clean` / `start_clean_path` /
    /// `start_clean_disk_selected` 里各抄了一遍，连「从内存 MFT 树剔除已删
    /// 路径」那段都是逐行重复的。三份实现漂移过一次（清理任务曾经借用
    /// scan_task 的槽位），收敛掉才不会有第二次。
    ///
    /// `work` 在后台线程上跑，`finish` 回到主线程收尾——差异全在这两个闭包里。
    pub(crate) fn spawn_clean(
        &mut self,
        totals: (u64, u64),
        status: Text,
        work: impl FnOnce(&CleanProgress) -> CleanReport + Send + 'static,
        finish: impl FnOnce(&mut Self, CleanReport, CleanSnapshot, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let (total_files, total_bytes) = totals;
        self.clean.running = true;
        let progress = Arc::new(CleanProgress::new(total_files, total_bytes));
        self.clean.progress = Some(progress.clone());
        self.status = status;
        self.start_tick(cx);
        cx.notify();

        let clean = cx
            .background_executor()
            .spawn(async move { work(&progress) });

        self.clean.task = Some(cx.spawn(async move |this, cx| {
            let report: CleanReport = clean.await;
            this.update(cx, |this, cx| {
                this.clean.running = false;
                let snap = this.clean_snapshot().unwrap_or_default();
                this.clean.freed_total += snap.bytes;
                finish(this, report, snap, cx);
                // 所有清理入口共用这一层；在这里刷新可避免磁盘卡片和健康分
                // 继续显示清理前的可用空间。状态页轮询也会持续刷新当前卷。
                this.disk.refresh_volume_spaces();
                cx.notify();
            })
            .ok();
        }));
    }

    /// 删除后局部更新 SizeTree，无需全量重扫。
    ///
    /// 用 `Arc::make_mut` 获取可变引用，对每个已删路径调用
    /// `ScanResult::remove_path`：标记子树为 unused，沿父链扣减聚合大小。
    /// UI 立即看到目录消失和容量变化，当前位置保留不变。macOS 主卷还要
    /// 同步替换整盘索引缓存，否则重新分析会重新挂回删除前的旧 `Arc`。
    pub fn start_clean(&mut self, cx: &mut Context<Self>) {
        if self.clean.running || !self.junk.scanned {
            return;
        }
        // 被占用的目标一律跳过并取消勾选：勾选可能是在占用检测结果
        // 合并之前就做下的（那时条目还是「推荐」），不能只依赖勾选态。
        // macOS 允许删除正被打开的文件，不拦的话应用会在已消失的路径上
        // 继续写，删了等于没删干净还埋雷。
        let busy: std::collections::HashSet<PathBuf> = self
            .junk
            .items()
            .filter(|i| i.busy.is_some())
            .map(|i| i.path.clone())
            .collect();
        let dropped_busy = drop_busy_from_selection(&mut self.junk.selected, &busy);

        let attempted = self.selected_paths();
        if attempted.is_empty() {
            self.status = bilingual(|l| {
                if dropped_busy > 0 {
                    tr_status_all_busy(l, dropped_busy)
                } else {
                    tr_status_nothing_selected(l).to_string()
                }
            });
            cx.notify();
            return;
        }

        let total_files: u64 = self
            .items()
            .filter(|i| self.junk.selected.contains(&i.path))
            .map(|i| i.file_count)
            .sum();
        let totals = (total_files, self.selected_size());

        self.clean.last_failed.clear();
        let n = attempted.len();
        let targets = self.selected_targets();
        let completed_targets = targets.clone();

        self.spawn_clean(
            totals,
            bilingual(|l| tr_status_deleting_n(l, n)),
            move |p| clean_targets(&targets, p),
            move |this, report, snap, cx| {
                // 以本次清理报告为准，不再用“目录清理后仍非空”倒推失败。
                // 缓存目录通常会被正在运行的应用瞬间重建；那是清理之后产生
                // 的新内容，不代表刚才没删掉。反过来，remove_dir=false 的
                // 目标会把失败记在具体孩子上，要映射回列表中的父目标。
                let failed = reported_targets(&completed_targets, &report.failed);
                let mut still_there = failed.clone();
                append_unique(
                    &mut still_there,
                    reported_targets(&completed_targets, &report.skipped_items),
                );
                append_unique(
                    &mut still_there,
                    reported_targets(&completed_targets, &report.manual),
                );

                // 用户主动停止时，循环后半段的目标没有进入 CleanReport；只有
                // 这个场景保留磁盘后置检查，避免把尚未处理的条目从列表移除。
                if snap.cancelled {
                    append_unique(
                        &mut still_there,
                        completed_targets
                            .iter()
                            .filter(|target| target_has_cleanable_contents(target))
                            .map(|target| target.path.clone()),
                    );
                }
                this.clean.last_failed = failed.clone();
                this.clean.last_failed_files = snap.failed;

                // brew owner command 在后台线程里持久化节流时间；同步回 Root
                // 已持有的设置，保证用户不重启应用直接重扫时也不会再次出现。
                if completed_targets
                    .iter()
                    .any(|target| crate::core::brew::is_brew_virtual(&target.path))
                {
                    this.settings = crate::core::settings::Settings::load();
                }

                // 就地更新，不再触发整轮复扫（开发垃圾扫描要几十秒）
                this.apply_clean_result(&attempted, &still_there);

                // 同步更新磁盘透镜的 SizeTree：垃圾清理删掉的路径
                //（缓存、临时文件、构建产物）在磁盘透镜里也会显示，
                // 不局部扣减的话切过去看还是旧大小。虚拟路径不在树里，跳过。
                let deleted: Vec<PathBuf> = completed_targets
                    .iter()
                    .filter(|target| {
                        target.remove_dir && !is_virtual_path(&target.path) && !target.path.exists()
                    })
                    .map(|target| target.path.clone())
                    .collect();
                this.prune_deleted_from_mft(&deleted, snap.bytes, cx);

                // “只清空内容”的目录本身仍存在，不能把整个目录从树里摘掉；
                // 子项变化也无法靠 remove_path 精确表达，失效索引后重新加载。
                if completed_targets.iter().any(|target| !target.remove_dir) {
                    this.disk.mft = None;
                    #[cfg(not(windows))]
                    {
                        this.macos_root_index = None;
                    }
                }

                let fails = this.clean.last_failed.len();
                let unresolved = fails + dropped_busy;
                let (files, size) = (snap.files, fmt_size(snap.bytes));
                this.status = bilingual(|l| {
                    if unresolved > 0 {
                        tr_status_clean_done_partial(l, files, &size, unresolved)
                    } else {
                        tr_status_clean_done(l, files, &size)
                    }
                });

                // 持久化审计：分类清理按类目决定永久删除或移入回收站，这行
                // JSONL 是用户事后翻旧账的记录。目标清单取用户勾选的那一层，
                // 与 cleaner::audit 的取舍一致。
                crate::core::history::record(
                    "category_clean",
                    &attempted,
                    report.ok,
                    report.skipped,
                    report.failed.len() + report.manual.len(),
                    snap.bytes,
                );
            },
            cx,
        );
    }

    /// 磁盘透镜：删掉单个用户点名的路径。
    pub fn start_clean_path(&mut self, path: PathBuf, size: u64, cx: &mut Context<Self>) {
        if self.clean.running {
            return;
        }
        let item = ArbitraryTarget::capture(path.clone());
        let shown = path.display().to_string();
        let disposal = self.disposal();

        self.spawn_clean(
            (0, size),
            bilingual(|l| tr_status_deleting_path(l, &shown)),
            move |p| clean_arbitrary_items(std::slice::from_ref(&item), disposal, p),
            move |this, report, snap, cx| {
                let shown = path.display().to_string();
                // 手选路径可能走回收站：用户自己的选择也留痕，且 disposal
                // 影响成败判定（回收站不计 bytes），审计里要如实反映。
                crate::core::history::record(
                    "arbitrary_clean",
                    std::slice::from_ref(&path),
                    report.ok,
                    report.skipped,
                    report.failed.len(),
                    snap.bytes,
                );
                if report.was_skipped(&path) {
                    this.status = bilingual(|l| tr_protected_path(l, &shown));
                    return;
                }
                if !report.failed.is_empty() || path.exists() {
                    this.status = bilingual(|l| tr_status_delete_failed(l, &shown));
                    return;
                }
                let (files, size) = (snap.files, fmt_size(snap.bytes));
                this.status = bilingual(|l| tr_status_deleted_path(l, &shown, files, &size));
                this.prune_deleted_from_mft(std::slice::from_ref(&path), snap.bytes, cx);
            },
            cx,
        );
    }
}

/// 从已勾选集合里剔除全部被占用目标，返回被剔除的数量。
///
/// 抽成不依赖 `gpui::Context` 的纯函数，是为了能在不搭建 UI 测试脚手架
/// 的情况下单测这条"老行为不能回归"的安全规则：勾选可能是在占用检测结果
/// 合并之前就做下的（那时条目还是「推荐」），清理入口必须在执行前再筛
/// 一遍，不能只信任勾选态。
fn drop_busy_from_selection(
    selected: &mut std::collections::HashSet<PathBuf>,
    busy: &std::collections::HashSet<PathBuf>,
) -> usize {
    if busy.is_empty() {
        return 0;
    }
    let dropped = selected.intersection(busy).count();
    for p in busy {
        selected.remove(p);
    }
    dropped
}

/// 把清理器报告的具体失败叶子映射回 UI 中用户勾选的顶层目标。
fn reported_targets(targets: &[CleanTarget], items: &[CleanFailure]) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    targets
        .iter()
        .filter(|target| {
            items.iter().filter_map(CleanFailure::as_path).any(|path| {
                path == target.path
                    || (!target.remove_dir && path.starts_with(target.path.as_path()))
            })
        })
        .filter(|target| seen.insert(target.path.clone()))
        .map(|target| target.path.clone())
        .collect()
}

fn append_unique(paths: &mut Vec<PathBuf>, more: impl IntoIterator<Item = PathBuf>) {
    let mut seen: std::collections::HashSet<PathBuf> = paths.iter().cloned().collect();
    paths.extend(more.into_iter().filter(|path| seen.insert(path.clone())));
}

fn target_has_cleanable_contents(target: &CleanTarget) -> bool {
    if is_virtual_path(&target.path) {
        return true;
    }
    if target.remove_dir {
        return target.path.exists();
    }
    std::fs::read_dir(&target.path)
        .map(|mut entries| entries.next().is_some())
        .unwrap_or_else(|_| target.path.exists())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cleaner::Disposal;
    use std::collections::HashSet;

    fn clean_target(path: &str, remove_dir: bool) -> CleanTarget {
        CleanTarget {
            path: PathBuf::from(path),
            remove_dir,
            size_hint: None,
            disposal: Disposal::Permanent,
            identity: None,
        }
    }

    #[test]
    fn drop_busy_from_selection_removes_and_counts() {
        let mut selected: HashSet<PathBuf> = [
            PathBuf::from("/a"),
            PathBuf::from("/b"),
            PathBuf::from("/c"),
        ]
        .into_iter()
        .collect();
        // /z 被占用但压根没被勾选过：不该被计进 dropped
        let busy: HashSet<PathBuf> = [PathBuf::from("/a"), PathBuf::from("/z")]
            .into_iter()
            .collect();

        let dropped = drop_busy_from_selection(&mut selected, &busy);

        assert_eq!(dropped, 1);
        assert!(!selected.contains(&PathBuf::from("/a")));
        assert!(selected.contains(&PathBuf::from("/b")));
        assert!(selected.contains(&PathBuf::from("/c")));
    }

    #[test]
    fn drop_busy_from_selection_empty_busy_is_noop() {
        let mut selected: HashSet<PathBuf> = [PathBuf::from("/a")].into_iter().collect();
        let dropped = drop_busy_from_selection(&mut selected, &HashSet::new());
        assert_eq!(dropped, 0);
        assert!(selected.contains(&PathBuf::from("/a")));
    }

    #[test]
    fn child_failure_maps_to_content_only_parent_once() {
        let targets = vec![
            clean_target("/cache", false),
            clean_target("/cache", false),
            clean_target("/whole", true),
        ];
        let failures = vec![CleanFailure::Path(PathBuf::from("/cache/nested/file"))];

        assert_eq!(
            reported_targets(&targets, &failures),
            vec![PathBuf::from("/cache")]
        );
    }

    #[test]
    fn successful_but_recreated_target_is_not_reported_as_failed() {
        let targets = vec![clean_target("/cache", false)];

        assert!(reported_targets(&targets, &[]).is_empty());
    }
}
