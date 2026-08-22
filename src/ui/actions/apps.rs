//! 软件管理与深度卸载动作

use crate::core::apps::{
    app_gone_after_residual_clean, residual_clean_follow_up, InstalledApp, ResidualItem,
    ResidualScanResult,
};
use crate::core::cleaner::{CleanFailure, CleanProgress};
use crate::core::i18n::{bilingual, Language};
use crate::core::model::fmt_size;
use crate::platform::{
    clean_residuals, list_installed_apps, run_uninstaller_and_wait, scan_residuals,
    verify_residuals,
};
use crate::ui::components::{ConfirmKind, ConfirmRequest};
use crate::ui::i18n::*;
use crate::ui::{UninstallPhase, UninstallProgress};
use gpui::Context;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

impl crate::ui::Root {
    /// 从内存里的已安装列表拿掉一款软件，并让虚拟列表失效重绘。
    fn drop_app_from_list(&mut self, app_id: &str) {
        let before = self.apps.list.len();
        self.apps.list.retain(|installed| installed.id != app_id);
        if self.apps.list.len() != before {
            self.apps.gen += 1;
        }
    }

    pub fn start_apps_scan(&mut self, cx: &mut Context<Self>) {
        if self.apps.scanning {
            return;
        }
        // 清空旧图标缓存——应用可能被卸载或新装，旧缓存不再可靠
        crate::ui::app_icons::clear();
        self.apps.scanning = true;
        self.apps.scanned = false;
        self.status = bilingual(|l| tr_status_apps_scanning(l).to_string());
        self.start_tick(cx);
        cx.notify();

        let live = Arc::new(AtomicBool::new(true));
        let scan = cx
            .background_executor()
            .spawn(async move { list_installed_apps(&live) });

        self.apps.task = Some(cx.spawn(async move |this, cx| {
            let result = scan.await;
            this.update(cx, |this, cx| {
                this.apps.list = result;
                this.apps.gen += 1;
                this.apps.scanned = true;
                this.apps.scanning = false;
                let total_size: u64 = this.apps.list.iter().map(|a| a.estimated_size).sum();
                let (count, size) = (this.apps.list.len(), fmt_size(total_size));
                this.status = bilingual(|l| tr_status_apps_done(l, count, &size));
                cx.notify();

                let icon_paths: Vec<std::path::PathBuf> = this
                    .apps
                    .list
                    .iter()
                    .filter_map(|app| app.icon_cache_key())
                    .collect();
                let fast = cx.background_executor().spawn({
                    let paths = icon_paths;
                    async move { crate::ui::app_icons::load_icons_from_bundle(paths) }
                });
                cx.spawn(async move |this, cx| {
                    let leftover = fast.await;
                    let leftover_n = leftover.len();
                    this.update(cx, |this, cx| {
                        this.apps.gen += 1;
                        cx.notify();
                    })
                    .ok();
                    if leftover.is_empty() {
                        crate::log!("应用图标加载完成：全部来自 bundle");
                        return;
                    }
                    let loaded = cx
                        .background_executor()
                        .spawn(async move { crate::ui::app_icons::load_icons(leftover) })
                        .await;
                    crate::log!("应用图标 AppKit 回退完成：{loaded}/{leftover_n}");
                    this.update(cx, |this, cx| {
                        this.apps.gen += 1;
                        cx.notify();
                    })
                    .ok();
                })
                .detach();
            })
            .ok();
        }));
    }

    /// 卸载软件：**先采集残留候选，再运行官方卸载程序**。
    ///
    /// 顺序很关键。安装目录、指向它的注册表值、服务的 ImagePath——这些
    /// 证据只在卸载之前存在。原先是卸载跑完才扫，那时安装目录已经没了，
    /// 所有基于路径的匹配全部落空，于是几乎每个软件都被报成「非常干净」。
    /// 现在提前扫一遍留下候选集，卸载结束后再复核哪些还在，剩下的才是
    /// 官方卸载程序没清干净的部分。
    pub fn request_uninstall_app(&mut self, app: InstalledApp, cx: &mut Context<Self>) {
        if self.residual.scanning || self.clean.running {
            return;
        }
        let lang = self.language;
        let app_name = app.name.clone();
        let size_str = if app.estimated_size > 0 {
            format!(" ({})", fmt_size(app.estimated_size))
        } else {
            String::new()
        };

        let (title, body, detail) = match lang {
            Language::Zh => (
                format!("确认卸载「{app_name}」？"),
                if cfg!(target_os = "macos") {
                    format!("将把「{app_name}」{size_str} 移入废纸篓或调用自带卸载程序，完成后扫描卸载残留并由你确认清理。")
                } else {
                    format!("将启动「{app_name}」{size_str} 官方卸载程序，完成后扫描卸载残留并由你确认清理。")
                },
                "卸载成功后会列出关联配置与缓存，仅清理你确认的项目。".to_string(),
            ),
            Language::En => (
                format!("Uninstall \"{app_name}\"?"),
                if cfg!(target_os = "macos") {
                    format!("This will move \"{app_name}\"{size_str} to Trash or run its uninstaller, then scan leftovers for your review.")
                } else {
                    format!("This will launch the official uninstaller for \"{app_name}\"{size_str}, then scan leftovers for your review.")
                },
                "After a successful uninstall, only the leftover items you confirm will be cleaned.".to_string(),
            ),
        };

        self.confirm = Some(ConfirmRequest {
            title,
            body,
            detail,
            kind: ConfirmKind::UninstallApp(Box::new(app)),
            app_data: false,
        });
        cx.notify();
    }

    pub fn execute_uninstall_app(&mut self, app: InstalledApp, cx: &mut Context<Self>) {
        let name = app.name.clone();
        let app_id = app.id.clone();
        let pre_target = app.clone();
        let uninst_target = app.clone();
        let uninstall = Arc::new(UninstallProgress::new(name.clone()));

        self.residual.scanning = true;
        self.residual.result = None;
        self.residual.uninstall = Some(uninstall.clone());
        self.status = bilingual(|l| tr_status_uninstall_waiting(l, &name));
        self.start_tick(cx);
        cx.notify();

        let work = cx.background_executor().spawn(async move {
            let shown_at = std::time::Instant::now();
            // 1. 卸载前采集候选（此时安装目录还在，证据最全）
            let pre = scan_residuals(&pre_target);
            // 2. 运行官方卸载程序并等它退出
            uninstall.set_phase(UninstallPhase::Removing);
            let result = run_uninstaller_and_wait(&uninst_target);
            // 3. 复核：只留下卸载程序没清掉的
            uninstall.set_phase(UninstallPhase::Verifying);
            let remaining = if result.is_ok() {
                verify_residuals(pre.items)
            } else {
                Vec::new()
            };
            let minimum = Duration::from_millis(900);
            if let Some(wait) = minimum.checked_sub(shown_at.elapsed()) {
                std::thread::sleep(wait);
            }
            (result, remaining)
        });

        self.residual.task = Some(cx.spawn(async move |this, cx| {
            let (result, remaining) = work.await;
            this.update(cx, |this, cx| {
                this.residual.scanning = false;
                this.residual.uninstall = None;
                if let Err(reason) = &result {
                    crate::log!("卸载「{name}」失败：{reason}");
                    this.residual.selected.clear();
                    this.residual.result = None;
                    this.status = bilingual(|l| tr_status_uninstall_failed(l, &name));
                    cx.notify();
                    return;
                }
                let total: u64 = remaining.iter().map(|i| i.size()).sum();
                let res = ResidualScanResult {
                    app_name: name.clone(),
                    app_id: app_id.clone(),
                    items: remaining,
                    total_file_size: total,
                };
                let (count, size) = (res.items.len(), fmt_size(res.total_file_size));
                this.status = bilingual(|l| {
                    let head = tr_status_uninstall_done(l, &name);
                    tr_status_uninstall_residual(l, &head, count, &size)
                });
                this.residual.selected = res.default_selection();
                this.residual.result = Some(res);

                // 卸载由外部卸载器执行，我们不知道确切删了哪些路径，
                // 无法局部更新 SizeTree。失效磁盘透镜缓存，下次打开时
                // 走 FSEvents 增量更新。
                this.drop_app_from_list(&app_id);
                this.disk.mft = None;
                #[cfg(not(windows))]
                {
                    this.macos_root_index = None;
                }

                cx.notify();
            })
            .ok();
        }));
    }

    pub fn start_residual_scan(&mut self, app: InstalledApp, cx: &mut Context<Self>) {
        if self.residual.scanning {
            return;
        }
        self.residual.scanning = true;
        self.residual.result = None;
        let scanning_name = app.name.clone();
        self.status = bilingual(|l| tr_status_residual_scanning(l, &scanning_name));
        cx.notify();

        let target = app.clone();
        let scan = cx
            .background_executor()
            .spawn(async move { scan_residuals(&target) });

        self.residual.task = Some(cx.spawn(async move |this, cx| {
            let res = scan.await;
            this.update(cx, |this, cx| {
                this.residual.scanning = false;
                let count = res.items.len();
                // 只预勾「确定」的；模糊匹配出来的交给用户自己判断
                this.residual.selected = res.default_selection();
                let (name, size) = (res.app_name.clone(), fmt_size(res.total_file_size));
                this.status = bilingual(|l| tr_status_residual_done(l, &name, count, &size));
                this.residual.result = Some(res);
                cx.notify();
            })
            .ok();
        }));
    }

    pub fn clean_selected_residuals(&mut self, cx: &mut Context<Self>) {
        let Some(res) = self.residual.result.as_ref().cloned() else {
            return;
        };
        let items_to_clean: Vec<ResidualItem> = self
            .residual
            .selected
            .iter()
            .filter_map(|&idx| res.items.get(idx).cloned())
            .collect();

        if items_to_clean.is_empty() {
            self.status = bilingual(|l| tr_status_residual_none_selected(l).to_string());
            cx.notify();
            return;
        }

        let selected_before = self.residual.selected.clone();
        self.residual.result = None;
        self.residual.scanning = true;

        let total_bytes: u64 = items_to_clean.iter().map(|it| it.size()).sum();
        let prog = Arc::new(CleanProgress::new(items_to_clean.len() as u64, total_bytes));
        // 用来读实际删掉的字节数——按预期值记账会在有删除失败时虚报释放量
        let progress = prog.clone();
        let app_name = res.app_name.clone();
        let cleaning_name = res.app_name.clone();
        let cleaning_count = items_to_clean.len();
        // 提取残留路径，用于清理后局部更新磁盘透镜
        let residual_paths: Vec<PathBuf> = items_to_clean
            .iter()
            .filter_map(|it| match &it.kind {
                crate::core::apps::ResidualKind::File(p, _)
                | crate::core::apps::ResidualKind::Directory(p, _) => Some(p.clone()),
                _ => None,
            })
            .collect();
        self.status = bilingual(|l| tr_status_residual_cleaning(l, &cleaning_name, cleaning_count));
        self.start_tick(cx);
        cx.notify();

        let clean = cx
            .background_executor()
            .spawn(async move { clean_residuals(&items_to_clean, &prog) });

        self.residual.task = Some(cx.spawn(async move |this, cx| {
            let report = clean.await;
            this.update(cx, |this, cx| {
                this.residual.scanning = false;
                let snap = progress.snapshot();
                this.clean.freed_total += snap.bytes;

                // 同步更新磁盘透镜的 SizeTree：残留文件/目录在磁盘透镜里也显示，
                // 不局部扣减的话切过去看还是旧大小。
                let deleted: Vec<PathBuf> = residual_paths
                    .iter()
                    .filter(|p| !p.exists())
                    .cloned()
                    .collect();
                this.prune_deleted_from_mft(&deleted, snap.bytes, cx);

                // 没勾选的项视为这次不处理，不再二次弹出。勾选了却没删掉的
                // 才留在对话框里方便授权后重试。
                // 手动处理项和失败项一样要留在列表里：系统扩展在用户去系统
                // 设置关掉之前一直都在，只是重试没有意义。
                let unresolved: HashSet<CleanFailure> = report
                    .failed
                    .iter()
                    .chain(report.manual.iter())
                    .cloned()
                    .collect();
                let original_items = res.items;
                let follow =
                    residual_clean_follow_up(&original_items, &selected_before, |item| match &item
                        .kind
                    {
                        crate::core::apps::ResidualKind::File(path, _)
                        | crate::core::apps::ResidualKind::Directory(path, _) => {
                            path.exists() || unresolved.contains(&CleanFailure::Path(path.clone()))
                        }
                        // 注册表键、计划任务、系统扩展没有路径，按标识串比对
                        _ => unresolved.contains(&CleanFailure::Id(item.kind.display_label())),
                    });
                this.residual.selected = follow.retry_selected;
                if app_gone_after_residual_clean(&original_items, &follow.leftover_for_app) {
                    this.drop_app_from_list(&res.app_id);
                }
                if follow.retry_items.is_empty() {
                    this.residual.result = None;
                } else {
                    let total_file_size = follow.retry_items.iter().map(ResidualItem::size).sum();
                    this.residual.result = Some(ResidualScanResult {
                        app_name: app_name.clone(),
                        app_id: res.app_id.clone(),
                        items: follow.retry_items,
                        total_file_size,
                    });
                }

                let (ok, fails, manual, size) = (
                    report.ok,
                    report.failed.len(),
                    report.manual.len(),
                    fmt_size(snap.bytes),
                );
                this.status = bilingual(|l| {
                    if fails > 0 {
                        tr_status_residual_cleaned_partial(l, &app_name, &size, fails)
                    } else if manual > 0 {
                        // 「权限不足」在这里是假话：SIP 下的系统扩展本来就
                        // 不该由我们删，重试多少次都一样。
                        tr_status_residual_cleaned_manual(l, &app_name, ok, &size, manual)
                    } else {
                        tr_status_residual_cleaned(l, &app_name, ok, &size)
                    }
                });
                cx.notify();
            })
            .ok();
        }));
    }
}
