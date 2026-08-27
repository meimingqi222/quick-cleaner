//! 清理执行动作：垃圾清理、路径清理、取消清理

use crate::core::cleaner::{
    clean_arbitrary, clean_targets, CleanProgress, CleanReport, CleanSnapshot,
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
            title: tr_confirm_delete_title(lang).to_string(),
            body: tr_confirm_delete_msg(lang, count, &fmt_size(self.selected_size())),
            detail: tr_confirm_no_recycle(lang).to_string(),
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
        let dropped_busy = if busy.is_empty() {
            0
        } else {
            let dropped = self.junk.selected.intersection(&busy).count();
            for p in &busy {
                self.junk.selected.remove(p);
            }
            dropped
        };

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
                // 虚拟路径（快照/Docker 镜像）不在文件系统上，exists() 恒为
                // false，只能按清理报告判定成败——否则 rmi/tmutil 失败会被
                // 误报成成功，条目从界面上消失，用户下次重扫才发现没删掉。
                let reported_failed: Vec<&std::path::Path> =
                    report.failed.iter().filter_map(|f| f.as_path()).collect();
                let failed: Vec<PathBuf> = completed_targets
                    .iter()
                    .filter(|target| {
                        if is_virtual_path(&target.path) {
                            return reported_failed.contains(&target.path.as_path());
                        }
                        if target.remove_dir {
                            target.path.exists()
                        } else {
                            std::fs::read_dir(&target.path)
                                .map(|mut entries| entries.next().is_some())
                                .unwrap_or_else(|_| target.path.exists())
                        }
                    })
                    .map(|target| target.path.clone())
                    .collect();
                this.clean.last_failed = failed.clone();
                this.clean.last_failed_files = snap.failed;

                // 就地更新，不再触发整轮复扫（开发垃圾扫描要几十秒）
                this.apply_clean_result(&attempted, &failed);

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
                let (files, size) = (snap.files, fmt_size(snap.bytes));
                this.status = bilingual(|l| {
                    let base = if fails > 0 {
                        tr_status_clean_done_partial(l, files, &size, fails)
                    } else {
                        tr_status_clean_done(l, files, &size)
                    };
                    if dropped_busy > 0 {
                        format!("{base} · {}", tr_busy_skipped(l, dropped_busy))
                    } else {
                        base
                    }
                });
            },
            cx,
        );
    }

    /// 磁盘透镜：删掉单个用户点名的路径。
    pub fn start_clean_path(&mut self, path: PathBuf, size: u64, cx: &mut Context<Self>) {
        if self.clean.running {
            return;
        }
        let target = path.clone();
        let shown = path.display().to_string();
        let disposal = self.disposal();

        self.spawn_clean(
            (0, size),
            bilingual(|l| tr_status_deleting_path(l, &shown)),
            move |p| clean_arbitrary(std::slice::from_ref(&target), disposal, p),
            move |this, _report, snap, cx| {
                let shown = path.display().to_string();
                if path.exists() {
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
