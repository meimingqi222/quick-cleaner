//! 软件管理与深度卸载动作

use crate::core::apps::{
    app_gone_after_residual_clean, discovered_program_files_absent, residual_clean_follow_up,
    InstalledApp, ResidualItem, ResidualOccupancy, ResidualScanResult, ResidualScope,
};
use crate::core::cleaner::{CleanFailure, CleanProgress};
use crate::core::i18n::{bilingual, Language};
use crate::core::model::fmt_size;
use crate::platform::{
    clean_residuals, detect_occupancy, list_installed_apps, run_uninstaller_reported,
    scan_residuals, verify_residuals,
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

fn status_with_uninstall_steps(
    lang: Language,
    mut status: String,
    executions: &[crate::core::rules::flow::PlanExecution],
) -> String {
    let steps: Vec<_> = executions
        .iter()
        .flat_map(|execution| &execution.steps)
        .collect();
    if !steps.is_empty() {
        let succeeded = steps
            .iter()
            .filter(|step| step.status == crate::core::rules::flow::StepStatus::Succeeded)
            .count();
        status.push('\n');
        status.push_str(&tr_source_execution_result(lang, succeeded, steps.len()));
    }
    status
}

impl crate::ui::Root {
    /// 从内存里的已安装列表拿掉一款软件，并让虚拟列表失效重绘。
    fn drop_app_from_list(&mut self, app_id: &str) {
        let before = self.apps.list.len();
        self.apps.list.retain(|installed| installed.id != app_id);
        if self.apps.list.len() != before {
            self.apps.gen += 1;
        }
    }

    /// 请求移除一个开发环境资产：先走应用内确认。
    ///
    /// 这里只负责把「要干什么、谁来干」讲清楚；真正的判定在
    /// `core::dev_env::remove` 里独立做，界面上的 `can_remove` 只是提前告知。
    pub fn request_remove_dev_asset(
        &mut self,
        item: crate::core::dev_env::DevAssetItem,
        cx: &mut Context<Self>,
    ) {
        if self.apps.dev.removing.is_some() {
            return;
        }
        if !crate::core::dev_env::remove::can_remove(&item) {
            return;
        }
        let lang = self.language;
        let name = item.display_name().to_string();
        let what = tr_dev_what(lang, &item.kind);
        // venv 走的是整目录删除，措辞必须照实说；其余条目仍是生态卸载命令。
        let body = match &item.kind {
            crate::core::dev_env::DevAssetKind::VirtualEnv { .. } => {
                tr_dev_confirm_body_directory(lang, &what)
            }
            _ => tr_dev_confirm_body(lang, &what),
        };
        // pip 包的体积扫描期没有单独测算，详情如实给版本号，不拿 0 冒充。
        let detail = match &item.kind {
            crate::core::dev_env::DevAssetKind::PipPackage { version, .. } => {
                tr_dev_confirm_detail_version(
                    lang,
                    &item.path.display().to_string(),
                    version.as_deref().unwrap_or("-"),
                )
            }
            _ => tr_dev_confirm_detail(
                lang,
                &item.path.display().to_string(),
                &fmt_size(item.size.exclusive_reclaimable_bytes),
            ),
        };
        self.confirm = Some(ConfirmRequest {
            title: tr_dev_confirm_title(lang, &name),
            body,
            detail,
            kind: ConfirmKind::RemoveDevAsset(Box::new(item)),
            // **不能**借 `app_data` 表达「这条很重要」：那个标记的含义是
            // 「目标位于 macOS `~/Library/Application Support`」，会渲染出
            // 「这里存放应用数据（聊天记录、密码库…）」那段 Danger 提示。
            // 一个 Windows 上的 npm 全局包套上它，弹窗就会指着
            // `C:\nvm4w\nodejs\node_modules\...` 说它在 `~/Library` 下——
            // 真机核对时正是这么撞上的。严重性已经写在 body 里。
            app_data: false,
        });
        cx.notify();
    }

    /// 执行移除：生态命令 → 核验 → 按结果重扫与报告。
    ///
    /// 结果分三类，必须分开报：已移除、**未执行**（拒绝，目标未被触碰）、
    /// **执行了但没通过核验**（状态未知，需要用户手工确认）。后两类混为一谈
    /// 会让用户在「其实还在」和「可能删了一半」之间失去判断依据。
    pub fn execute_remove_dev_asset(
        &mut self,
        item: crate::core::dev_env::DevAssetItem,
        cx: &mut Context<Self>,
    ) {
        let lang = self.language;
        let name = item.display_name().to_string();
        let id = item.id.clone();
        // pip 包不占主列表的行，摘除走的是父行的就地更新而不是 drop_asset。
        let is_pip_package = matches!(
            item.kind,
            crate::core::dev_env::DevAssetKind::PipPackage { .. }
        );
        self.apps.dev.removing = Some(name.clone());
        self.status = crate::core::i18n::bilingual(|l| tr_dev_status_removing(l, &name));
        cx.notify();

        let path = item.path.clone();
        self.apps.dev.removal_task = Some(cx.spawn(async move |this, cx| {
            let work = item.clone();
            let outcome = cx
                .background_executor()
                .spawn(async move { crate::core::dev_env::remove::remove_asset(&work) })
                .await;

            let (ok, failed, status) = match &outcome {
                crate::core::dev_env::remove::RemovalOutcome::Removed => {
                    (1usize, 0usize, tr_dev_status_removed(lang, &name))
                }
                crate::core::dev_env::remove::RemovalOutcome::Refused(reason) => {
                    (0, 0, tr_dev_status_refused(lang, &name, *reason))
                }
                crate::core::dev_env::remove::RemovalOutcome::Failed(failure) => {
                    (0, 1, tr_dev_status_failed(lang, &name, &failure.detail))
                }
            };
            crate::core::history::record("dev_asset_remove", &[path], ok, 0, failed, 0);

            // 移除成功后**不重扫**：我们确切知道那一条没了，重扫一遍要再走
            // 几万个文件（一次十几秒），而结论不会变。就地摘掉这一行并扣掉
            // 它记录过的体积即可——这是 `drop_app_from_list` 对已安装软件用
            // 的同一套做法。
            //
            // 只有「状态未知」才值得重扫：命令可能动了一半，清单已经不可信。
            let rescan = matches!(
                outcome,
                crate::core::dev_env::remove::RemovalOutcome::Failed(_)
            );
            let removed = matches!(
                outcome,
                crate::core::dev_env::remove::RemovalOutcome::Removed
            );
            let _ = this.update(cx, |this, cx| {
                this.apps.dev.removing = None;
                if removed {
                    if is_pip_package {
                        this.apps.dev.drop_pip_package(&id);
                    } else {
                        this.apps.dev.drop_asset(&id);
                    }
                }
                // 状态按当前语言重新渲染，而不是把这一轮的语种固定下来。
                this.status = crate::core::i18n::bilingual(move |_| status.clone());
                cx.notify();
                if rescan && !this.apps.dev.scanning {
                    this.start_dev_env_scan(cx);
                }
            });
        }));
    }

    /// 请求批量移除当前所有已勾选的开发环境资产。
    pub fn request_remove_selected_dev_assets(&mut self, cx: &mut Context<Self>) {
        if self.apps.dev.removing.is_some() {
            return;
        }
        let selected_ids = self.apps.dev.selected.clone();
        if selected_ids.is_empty() {
            return;
        }

        // 收集所有匹配且可移除的条目对象
        let mut targets: Vec<crate::core::dev_env::DevAssetItem> = Vec::new();
        for group in [
            &self.apps.dev.conda_envs,
            &self.apps.dev.python_envs,
            &self.apps.dev.node_packages,
            &self.apps.dev.python_tools,
        ] {
            for item in group {
                if selected_ids.contains(&item.id) && crate::core::dev_env::remove::can_remove(item)
                {
                    targets.push(item.clone());
                }
            }
        }
        for packages in self.apps.dev.site_packages.values() {
            for item in packages {
                if selected_ids.contains(&item.id) && crate::core::dev_env::remove::can_remove(item)
                {
                    targets.push(item.clone());
                }
            }
        }

        if targets.is_empty() {
            return;
        }

        if targets.len() == 1 {
            self.request_remove_dev_asset(targets.remove(0), cx);
            return;
        }

        let lang = self.language;
        let count = targets.len();
        let total_size: u64 = targets
            .iter()
            .map(|t| t.size.exclusive_reclaimable_bytes)
            .sum();
        let size_str = fmt_size(total_size);

        let mut preview_lines: Vec<String> = targets
            .iter()
            .take(6)
            .map(|t| format!("· {}", t.display_name()))
            .collect();
        if count > 6 {
            preview_lines.push(format!("...等共 {count} 项"));
        }
        let detail = format!(
            "预计可释放：{size_str}\n\n待移除清单：\n{}",
            preview_lines.join("\n")
        );

        self.confirm = Some(ConfirmRequest {
            title: tr_dev_batch_confirm_title(lang, count),
            body: tr_dev_batch_confirm_body(lang).to_string(),
            detail,
            kind: ConfirmKind::BatchRemoveDevAssets(targets),
            app_data: false,
        });
        cx.notify();
    }

    /// 执行批量移除：串行调用各生态官方卸载命令并核验。
    pub fn execute_batch_remove_dev_assets(
        &mut self,
        items: Vec<crate::core::dev_env::DevAssetItem>,
        cx: &mut Context<Self>,
    ) {
        if items.is_empty() {
            return;
        }
        let total_count = items.len();
        self.apps.dev.removing = Some(format!("{total_count} 项"));
        self.status =
            crate::core::i18n::bilingual(move |_| format!("正在批量移除 {total_count} 项…"));
        cx.notify();

        self.apps.dev.removal_task = Some(cx.spawn(async move |this, cx| {
            let mut removed_count = 0usize;
            let mut failed_count = 0usize;
            let mut refused_count = 0usize;
            let mut freed_bytes = 0u64;

            for (idx, item) in items.into_iter().enumerate() {
                let name = item.display_name().to_string();
                let status_name = name.clone();
                let current_idx = idx + 1;
                let _ = this.update(cx, |this, cx| {
                    this.status = crate::core::i18n::bilingual(move |_| {
                        format!("正在移除 ({current_idx}/{total_count}): {status_name}")
                    });
                    cx.notify();
                });

                let work = item.clone();
                let outcome = cx
                    .background_executor()
                    .spawn(async move { crate::core::dev_env::remove::remove_asset(&work) })
                    .await;

                let id = item.id.clone();
                let is_pip_package = matches!(
                    item.kind,
                    crate::core::dev_env::DevAssetKind::PipPackage { .. }
                );

                match &outcome {
                    crate::core::dev_env::remove::RemovalOutcome::Removed => {
                        removed_count += 1;
                        freed_bytes = freed_bytes
                            .saturating_add(item.size.exclusive_reclaimable_bytes);
                        crate::core::history::record(
                            "dev_asset_remove",
                            std::slice::from_ref(&item.path),
                            1,
                            0,
                            0,
                            item.size.exclusive_reclaimable_bytes,
                        );
                        let _ = this.update(cx, |this, cx| {
                            if is_pip_package {
                                this.apps.dev.drop_pip_package(&id);
                            } else {
                                this.apps.dev.drop_asset(&id);
                            }
                            cx.notify();
                        });
                    }
                    crate::core::dev_env::remove::RemovalOutcome::Refused(reason) => {
                        refused_count += 1;
                        crate::log!("批量移除跳过「{name}」：{reason:?}");
                        crate::core::history::record(
                            "dev_asset_remove",
                            std::slice::from_ref(&item.path),
                            0,
                            0,
                            0,
                            0,
                        );
                    }
                    crate::core::dev_env::remove::RemovalOutcome::Failed(failure) => {
                        failed_count += 1;
                        crate::log!("批量移除失败「{name}」：{}", failure.detail);
                        crate::core::history::record(
                            "dev_asset_remove",
                            std::slice::from_ref(&item.path),
                            0,
                            0,
                            1,
                            0,
                        );
                    }
                }
            }

            let _ = this.update(cx, |this, cx| {
                this.apps.dev.removing = None;
                let freed_str = fmt_size(freed_bytes);
                this.status = crate::core::i18n::bilingual(move |l| {
                    match l {
                        Language::Zh => {
                            if failed_count > 0 {
                                format!("批量移除完成：已移除 {removed_count} 项，释放 {freed_str}，失败 {failed_count} 项")
                            } else if refused_count > 0 {
                                format!("批量移除完成：已移除 {removed_count} 项，释放 {freed_str}，跳过 {refused_count} 项")
                            } else {
                                format!("批量移除完成：已成功移除 {removed_count} 项，释放 {freed_str}")
                            }
                        }
                        Language::En => {
                            if failed_count > 0 {
                                format!("Batch remove finished: {removed_count} removed ({freed_str}), {failed_count} failed")
                            } else {
                                format!("Batch remove finished: {removed_count} removed ({freed_str})")
                            }
                        }
                    }
                });
                cx.notify();
                if failed_count > 0 && !this.apps.dev.scanning {
                    this.start_dev_env_scan(cx);
                }
            });
        }));
    }

    /// 展开/收起一个 site 行（用户级包目录行或虚拟环境行）的 pip 包列表。
    ///
    /// 包列表是展开时从磁盘枚举的（读 dist-info 登记，无子进程），收起只
    /// 折叠视图、缓存留着，再展开不重读。重扫后整份缓存过期，随结果清空。
    pub fn select_active_dev_env(&mut self, env_id: String, cx: &mut Context<Self>) {
        self.apps.dev.active_env_id = Some(env_id.clone());
        self.load_dev_env_packages_if_needed(&env_id, cx);
        cx.notify();
    }

    pub fn load_dev_env_packages_if_needed(&mut self, env_id: &str, cx: &mut Context<Self>) {
        let dev = &mut self.apps.dev;
        if dev.scanning || dev.removing.is_some() || dev.loading_site.is_some() {
            return;
        }
        if dev.site_packages.contains_key(env_id) {
            return;
        }
        let Some(row) = dev.python_envs.iter().find(|item| item.id == env_id) else {
            return;
        };
        let target = match &row.kind {
            crate::core::dev_env::DevAssetKind::PythonInterpreter {
                scope,
                site_packages: Some(site),
                python,
                ..
            } if *scope == crate::core::dev_env::SiteScope::User => {
                Some((site.clone(), python.clone()))
            }
            crate::core::dev_env::DevAssetKind::VirtualEnv { .. } => {
                let site = crate::core::dev_env::python::venv_site_packages(&row.path);
                let python = crate::core::dev_env::python::venv_python(&row.path);
                match (site, python) {
                    (Some(site), Some(python)) => Some((site, python)),
                    _ => None,
                }
            }
            _ => None,
        };
        let Some((site_dir, python)) = target else {
            return;
        };
        let site_id = env_id.to_string();
        dev.loading_site = Some(site_id.clone());
        cx.notify();

        let read = cx
            .background_executor()
            .spawn(async move { crate::core::dev_env::python::pip_packages(&site_dir, &python) });
        dev.packages_task = Some(cx.spawn(async move |this, cx| {
            let packages = read.await;
            let _ = this.update(cx, |this, cx| {
                let dev = &mut this.apps.dev;
                dev.loading_site = None;
                dev.site_packages.insert(site_id, packages);
                cx.notify();
            });
        }));
    }

    pub fn toggle_dev_site_packages(&mut self, site_id: String, cx: &mut Context<Self>) {
        let dev = &mut self.apps.dev;
        if dev.scanning || dev.removing.is_some() || dev.loading_site.is_some() {
            return;
        }
        if !dev.expanded_sites.remove(&site_id) {
            let Some(row) = dev.python_envs.iter().find(|item| item.id == site_id) else {
                return;
            };
            // 展开需要两样东西：包目录在哪、用哪个解释器卸载。安装级包目录
            // 行不在可展开之列（P41：那是解释器的一部分，整行只读）。
            let target = match &row.kind {
                crate::core::dev_env::DevAssetKind::PythonInterpreter {
                    scope,
                    site_packages: Some(site),
                    python,
                    ..
                } if *scope == crate::core::dev_env::SiteScope::User => {
                    Some((site.clone(), python.clone()))
                }
                crate::core::dev_env::DevAssetKind::VirtualEnv { .. } => {
                    let site = crate::core::dev_env::python::venv_site_packages(&row.path);
                    let python = crate::core::dev_env::python::venv_python(&row.path);
                    match (site, python) {
                        (Some(site), Some(python)) => Some((site, python)),
                        // 缺 site 或缺 python 就没有按包卸载的通道；整行删除
                        // 不受影响。
                        _ => None,
                    }
                }
                _ => None,
            };
            let Some((site_dir, python)) = target else {
                return;
            };
            dev.expanded_sites.insert(site_id.clone());
            dev.loading_site = Some(site_id.clone());
            cx.notify();

            let read = cx.background_executor().spawn(async move {
                crate::core::dev_env::python::pip_packages(&site_dir, &python)
            });
            dev.packages_task = Some(cx.spawn(async move |this, cx| {
                let packages = read.await;
                let _ = this.update(cx, |this, cx| {
                    let dev = &mut this.apps.dev;
                    dev.loading_site = None;
                    dev.site_packages.insert(site_id, packages);
                    cx.notify();
                });
            }));
            return;
        }
        cx.notify();
    }

    pub fn start_dev_env_scan(&mut self, cx: &mut Context<Self>) {
        if self.apps.dev.scanning {
            return;
        }
        self.apps.dev.scanning = true;
        self.apps.dev.scanned = false;
        self.start_tick(cx);
        cx.notify();
        let scan_fut = cx
            .background_executor()
            .spawn(async move { crate::core::dev_env::discovery::discover_all() });
        self.apps.dev.task = Some(cx.spawn(async move |this, cx| {
            let result = scan_fut.await;
            let _ = this.update(cx, |this, cx| {
                // 总计直接取批量测定的结果，不在界面层逐项相加：conda 的
                // `pkgs` 缓存与环境之间大量使用硬链接，逐项相加会把同一份
                // 数据算两遍以上。
                let total_logical = result.total.logical_bytes;
                // 「可释放」只算进得了移除通道的部分，见 `removable_exclusive`。
                let total_exclusive = result.removable_exclusive;
                this.apps.dev.conda_envs = result.conda_envs;
                this.apps.dev.python_envs = result.python_envs;
                this.apps.dev.node_packages = result.node_packages;
                this.apps.dev.python_tools = result.python_tools;
                this.apps.dev.total_logical_size = total_logical;
                this.apps.dev.total_exclusive_size = total_exclusive;
                // 展开着的包列表是上一轮扫描间隙从磁盘枚举的，重扫后全部
                // 过期：整份清掉，收起所有展开行。
                this.apps.dev.expanded_sites.clear();
                this.apps.dev.site_packages.clear();
                this.apps.dev.loading_site = None;
                this.apps.dev.scanned = true;
                this.apps.dev.scanning = false;
                this.apps.dev.ensure_active_env();
                if let Some(active_id) = this.apps.dev.active_env_id.clone() {
                    this.load_dev_env_packages_if_needed(&active_id, cx);
                }
                cx.notify();
            });
        }));
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
        if self.residual.scanning || self.clean.running || !app.can_uninstall() {
            return;
        }
        let lang = self.language;
        let app_name = app.name.clone();
        let size_str = if app.estimated_size > 0 {
            format!(" ({})", fmt_size(app.estimated_size))
        } else {
            String::new()
        };

        let (title, body, mut detail) = match lang {
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
        if app.discovery.is_some() {
            detail = tr_discovered_uninstall_detail(lang).to_owned();
        }
        if let Some(discovery) = &app.discovery {
            if let Some(reference) = discovery
                .plan
                .as_ref()
                .map(|plan| &plan.rule)
                .or(discovery.rule.as_ref())
            {
                let definition = reference.snapshot.definition(&reference.id);
                if let Some(layout) = &definition.app {
                    detail.push('\n');
                    detail.push_str(&tr_source_rule_plan(
                        lang,
                        &reference.id,
                        discovery
                            .plan
                            .as_ref()
                            .and_then(|plan| plan.installation.as_ref())
                            .map_or(discovery.program_paths.len(), |instance| {
                                instance.artifact_count()
                            }),
                        &layout.preserve.join(", "),
                    ));
                }
            }
        }

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
        self.residual.uninstall_executions.clear();
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
            let outcome = run_uninstaller_reported(&uninst_target);
            let result = outcome.result;
            // 3. 复核：只留下卸载程序没清掉的；占用证据按「此刻」采集，
            //    不能用卸载前的快照——官方卸载器可能顺手杀掉了代理进程，
            //    拿旧证据弹「仍在运行」会把用户吓唬错。
            uninstall.set_phase(UninstallPhase::Verifying);
            let (remaining, occupancy) = if result.is_ok() {
                (
                    verify_residuals(pre.items),
                    detect_occupancy(&uninst_target),
                )
            } else {
                (Vec::new(), ResidualOccupancy::default())
            };
            let minimum = Duration::from_millis(900);
            if let Some(wait) = minimum.checked_sub(shown_at.elapsed()) {
                std::thread::sleep(wait);
            }
            (result, remaining, occupancy, outcome.plan_executions)
        });

        self.residual.task = Some(cx.spawn(async move |this, cx| {
            let (result, remaining, occupancy, executions) = work.await;
            this.update(cx, |this, cx| {
                this.residual.scanning = false;
                this.residual.uninstall_executions = executions;
                this.residual.uninstall = None;
                if let Err(reason) = &result {
                    crate::log!("卸载「{name}」失败：{reason}");
                    this.residual.selected.clear();
                    this.residual.result = None;
                    this.status = bilingual(|l| {
                        status_with_uninstall_steps(
                            l,
                            tr_status_uninstall_failed_reason(l, &name, reason),
                            &this.residual.uninstall_executions,
                        )
                    });
                    cx.notify();
                    return;
                }
                let total: u64 = remaining.iter().map(|i| i.size()).sum();
                let res = ResidualScanResult {
                    app_name: name.clone(),
                    scope: ResidualScope::App,
                    app_id: app_id.clone(),
                    items: remaining,
                    total_file_size: total,
                    occupancy,
                };
                let (count, size) = (res.items.len(), fmt_size(res.total_file_size));
                this.status = bilingual(|l| {
                    let head = tr_status_uninstall_done(l, &name);
                    status_with_uninstall_steps(
                        l,
                        tr_status_uninstall_residual(l, &head, count, &size),
                        &this.residual.uninstall_executions,
                    )
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

    /// 扫描机器上所有「本体已经不在、用户目录里还留着东西」的软件（macOS）。
    ///
    /// 这是唯一一个不依赖已安装列表的残留入口。软件一旦被用户自己删掉
    /// （拖进废纸篓、用别家的清理工具、跑厂商自己的卸载器），上面那套按应用
    /// 的扫描就再也够不到它——`/Applications` 里没有 `.app` 就没有
    /// [`InstalledApp`]，没有 [`InstalledApp`] 就没有残留扫描的起点。
    ///
    /// 与按应用扫描的两处不同，都在弹窗和状态文案里如实体现：
    /// - 一次覆盖多款软件，没有单一的「主人」，所以没有占用探测（`ps` /
    ///   `launchctl` 的判据是单个 Bundle ID 或应用名）。
    /// - 测不出就是测不出：Spotlight 索引不可用时如实告知，不回退成「很干净」。
    #[cfg(target_os = "macos")]
    pub fn start_orphan_leftover_scan(&mut self, cx: &mut Context<Self>) {
        if self.residual.scanning || self.clean.running {
            return;
        }
        self.residual.scanning = true;
        self.residual.result = None;
        self.status = bilingual(|l| tr_status_orphan_scanning(l).to_string());
        cx.notify();

        let scan = cx
            .background_executor()
            .spawn(async move { crate::platform::macos::residuals::scan_orphan_residuals() });

        self.residual.task = Some(cx.spawn(async move |this, cx| {
            let scanned = scan.await;
            this.update(cx, |this, cx| {
                this.residual.scanning = false;
                let Some(res) = scanned else {
                    this.status = bilingual(|l| tr_status_orphan_unknown(l).to_string());
                    cx.notify();
                    return;
                };
                let count = res.items.len();
                let size = fmt_size(res.total_file_size);
                this.status = bilingual(|l| tr_status_orphan_done(l, count, &size));
                this.residual.selected = res.default_selection();
                this.residual.result = Some(res);
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
        let scan = cx.background_executor().spawn(async move {
            // 占用探测要 fork `ps`/`launchctl`，卸载流程的预扫描用不上
            // 它（那边卸载完会重新采集），所以由调用方按需补上，不塞进
            // `scan_residuals` 让每个调用方都白跑一遍。
            let mut res = scan_residuals(&target);
            res.occupancy = detect_occupancy(&target);
            res
        });

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
        let app_name_for_abort = res.app_name.clone();
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
        self.status = bilingual(|l| match res.scope {
            ResidualScope::App => tr_status_residual_cleaning(l, &cleaning_name, cleaning_count),
            ResidualScope::OrphanLeftovers => tr_status_orphan_cleaning(l, cleaning_count),
        });
        self.start_tick(cx);
        cx.notify();

        // 中止时要把列表和勾选原样放回去，所以先留一份。
        let restore = res.clone();
        let restore_selected = selected_before.clone();
        let app_id_for_check = res.app_id.clone();
        let discovery_for_check = self
            .apps
            .list
            .iter()
            .find(|app| app.id == res.app_id)
            .and_then(|app| app.discovery.clone());
        let is_discovered = app_id_for_check.starts_with("discovered:");
        let scope_for_check = res.scope;

        let clean = cx.background_executor().spawn(async move {
            // 删除边界的最后一道判据：这个 bundle 真的已经不在机器上了吗？
            //
            // 残留扫描的整个前提是「应用已被卸载」。前提错了——用户把应用
            // 挪去了别的目录、卸载器其实没卸干净、或者同一个 bundle id 还
            // 有第二份安装——那么这些"残留"就是活应用的配置、登录态和许可
            // 证。这一道刻意放在真正删除之前的最后一跳，而不是扫描时：扫描
            // 到用户点确认之间可能过去很久，期间应用完全可能被重新装上。
            //
            // macOS 专属：判据是 Spotlight 反查，Windows 没有等价物（它的
            // 残留判定走注册表卸载登记项，是另一套证据），所以这里按仓库
            // 约定写成显式的平台分支，而不是塞进 `platform` 门面契约。
            //
            // 孤儿扫描没有单一的 app_id：它一次覆盖多款软件，所以按每条残留
            // 自己记着的主人（Bundle ID）逐项复核，集合是此刻现取的。任意一条
            // 验不了、或者主人又被装回来，整批不删。
            #[cfg(target_os = "macos")]
            {
                let owners_gone = match scope_for_check {
                    ResidualScope::App => matches!(
                        crate::platform::macos::apps::bundle_is_still_installed(&app_id_for_check),
                        Some(false)
                    ),
                    ResidualScope::OrphanLeftovers => {
                        crate::platform::macos::residuals::orphan_owners_all_gone(&items_to_clean)
                    }
                };
                // 明确还装着，或者根本测不出（超时/命令失败）——两种都
                // 不许删。`None` 必须 fail closed：Spotlight 索引不全时
                // 的空结果和"确实没装"长得一模一样。
                if !owners_gone {
                    return None;
                }
            }
            #[cfg(not(target_os = "macos"))]
            let _ = &app_id_for_check;

            #[cfg(windows)]
            if let Err(reason) = crate::platform::windows::apps::validate_discovered_residual_clean(
                &app_id_for_check,
                &items_to_clean,
            ) {
                crate::log!("Residual cleanup blocked: {reason}");
                return None;
            }

            let report = clean_residuals(&items_to_clean, &prog);
            let discovered_gone = discovery_for_check
                .as_ref()
                .map(discovered_program_files_absent);
            Some((report, discovered_gone))
        });

        self.residual.task = Some(cx.spawn(async move |this, cx| {
            let Some((report, discovered_gone)) = clean.await else {
                this.update(cx, |this, cx| {
                    this.residual.scanning = false;
                    // 原样还原：一个字节都没删，列表和勾选不该丢。
                    this.residual.result = Some(restore);
                    this.residual.selected = restore_selected;
                    this.status = bilingual(|l| match scope_for_check {
                        ResidualScope::App => {
                            if cfg!(windows) && is_discovered {
                                tr_status_discovered_cleanup_blocked(l).to_owned()
                            } else {
                                tr_status_residual_still_installed(l, &app_name_for_abort)
                            }
                        }
                        ResidualScope::OrphanLeftovers => {
                            tr_status_orphan_still_installed(l).to_string()
                        }
                    });
                    cx.notify();
                })
                .ok();
                return;
            };
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

                // 不自动重开残留对话框：删不掉的多半会一直删不掉，重开变成
                // 点一次弹一次。状态栏报失败计数；残留扫描入口可再扫。
                // `leftover_for_app` 仍要算——没清掉的项不能当成已卸干净。
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
                this.residual.selected.clear();
                this.residual.result = None;
                // 只覆盖「残留扫描」入口：主卸载成功路径在弹残留框之前
                // 就已经 drop 了应用。这里失败项非空时保留列表项，避免
                // 残留扫描流程里 InstallDir 清掉、AppData 失败却再也找不到入口。
                if follow.retry_items.is_empty()
                    && discovered_gone.unwrap_or_else(|| {
                        !is_discovered
                            && app_gone_after_residual_clean(
                                &original_items,
                                &follow.leftover_for_app,
                            )
                    })
                {
                    this.drop_app_from_list(&res.app_id);
                }

                let (ok, fails, manual, size) = (
                    report.ok,
                    report.failed.len(),
                    report.manual.len(),
                    fmt_size(snap.bytes),
                );
                this.status = bilingual(|l| match scope_for_check {
                    ResidualScope::App => {
                        if fails > 0 {
                            tr_status_residual_cleaned_partial(l, &app_name, &size, fails)
                        } else if manual > 0 {
                            // 「权限不足」在这里是假话：SIP 下的系统扩展本来就
                            // 不该由我们删，重试多少次都一样。
                            tr_status_residual_cleaned_manual(l, &app_name, ok, &size, manual)
                        } else {
                            tr_status_residual_cleaned(l, &app_name, ok, &size)
                        }
                    }
                    ResidualScope::OrphanLeftovers => {
                        if fails > 0 {
                            tr_status_orphan_cleaned_partial(l, &size, fails)
                        } else if manual > 0 {
                            tr_status_orphan_cleaned_manual(l, ok, &size, manual)
                        } else {
                            tr_status_orphan_cleaned(l, ok, &size)
                        }
                    }
                });

                // 同分类清理：残留（走废纸篓，但仍可能永久失败的授权路径）
                // 也要留持久化审计，出问题时能对着应用名翻账。
                crate::core::history::record(
                    "residual_clean",
                    &residual_paths,
                    ok,
                    report.skipped,
                    fails + manual,
                    snap.bytes,
                );
                cx.notify();
            })
            .ok();
        }));
    }
}
