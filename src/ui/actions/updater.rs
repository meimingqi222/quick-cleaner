//! 自动更新动作：检查、下载、校验、安装交接
//!
//! 更新源为 GitHub Releases；开发态（cargo target）与用户关闭自动检查时
//! 不发起任何网络请求。发现新版本**不会静默重启**，必须由用户点安装。

use crate::core::updater::{
    self, candidate_asset_names, current_target, download_to_file, evaluate_release,
    extract_update_zip, fetch_latest_release, fetch_text, is_skipped, parse_checksum_sidecar,
    sha256_file, UpdateStatus, CHECK_INTERVAL_MS, FIRST_CHECK_DELAY_MS,
};
use crate::ui::i18n::*;
use gpui::Context;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

impl crate::ui::Root {
    /// 启动延迟检查 + 周期重查。幂等。
    pub fn start_update_scheduler(&mut self, cx: &mut Context<Self>) {
        // 即使用户关了自动检查，发行安装也要清理上次更新留下的 `.old`。
        if crate::platform::is_packaged_install() {
            crate::platform::cleanup_previous_update_leftovers();
        }
        if self.update.task.is_some() || !crate::platform::is_packaged_install() {
            return;
        }
        if !self.settings.auto_check_updates {
            return;
        }
        self.update.task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(FIRST_CHECK_DELAY_MS))
                .await;
            loop {
                let keep = this
                    .update(cx, |this, cx| {
                        this.check_for_updates_internal(false, cx);
                        this.settings.auto_check_updates
                    })
                    .unwrap_or(false);
                if !keep {
                    return;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(CHECK_INTERVAL_MS))
                    .await;
            }
        }));
    }

    /// 用户手动「检查更新」：跳过节流，但仍 join 在飞请求。
    pub fn check_for_updates_manual(&mut self, cx: &mut Context<Self>) {
        self.check_for_updates_internal(true, cx);
    }

    fn check_for_updates_internal(&mut self, manual: bool, cx: &mut Context<Self>) {
        // 门禁无条件：开发构建永远不碰 GitHub，手动也不例外。
        // `manual` 只用来决定是否跳过自动检查开关与错误后是否弹窗。
        if !crate::platform::is_packaged_install() {
            return;
        }
        if !manual && !self.settings.auto_check_updates {
            return;
        }
        if self.update.checking {
            return;
        }
        if matches!(
            self.update.status,
            UpdateStatus::Downloading { .. }
                | UpdateStatus::Verifying { .. }
                | UpdateStatus::Downloaded { .. }
                | UpdateStatus::Installing { .. }
        ) {
            return;
        }

        let current_version = env!("CARGO_PKG_VERSION").to_string();
        let skipped = self.settings.skipped_update_version.clone();
        self.update.checking = true;
        self.update.status = UpdateStatus::Checking {
            current_version: current_version.clone(),
        };
        cx.notify();

        let work = cx.background_executor().spawn(async move {
            let release = fetch_latest_release(15_000)?;
            let candidates = candidate_asset_names(current_target());
            let status = evaluate_release(&current_version, &release, candidates)?;
            Ok::<UpdateStatus, String>(status)
        });

        cx.spawn(async move |this, cx| {
            let result = work.await;
            let applied = this.update(cx, |this, cx| {
                this.update.checking = false;
                this.settings.last_update_check_at = Some(chrono::Utc::now().timestamp());
                this.settings.save();
                match result {
                    Ok(status) => {
                        if let UpdateStatus::Available { latest_version, .. } = &status {
                            if is_skipped(latest_version, skipped.as_deref()) {
                                this.update.status = UpdateStatus::NotAvailable {
                                    current_version: env!("CARGO_PKG_VERSION").to_string(),
                                };
                            } else {
                                this.update.status = status;
                                // maka 同款 autoDownload：发现即后台下载，侧栏出现
                                // 「重启安装」入口；不弹窗打断。
                                this.start_update_download(cx);
                            }
                        } else {
                            this.update.status = status;
                        }
                    }
                    Err(message) => {
                        crate::log!("更新检查失败: {message}");
                        this.update.status = UpdateStatus::Error {
                            current_version: env!("CARGO_PKG_VERSION").to_string(),
                            operation: updater::UpdateOperation::Check,
                            message: message.clone(),
                        };
                        // 自动静默检查失败不弹窗；侧栏会出现「更新失败」可点。
                        // 手动检查失败写状态栏，同样不强制弹窗。
                        if manual {
                            this.status = crate::core::i18n::bilingual(|l| {
                                format!("{}: {message}", tr_update_failed(l))
                            });
                        }
                    }
                }
                cx.notify();
            });
            let _ = applied;
        })
        .detach();
    }

    pub fn open_update_dialog(&mut self, cx: &mut Context<Self>) {
        // 空状态不弹「检查更新」对话框——没有可执行的更新动作。
        if !self.update.status.wants_attention() {
            return;
        }
        self.update.show_dialog = true;
        cx.notify();
    }

    pub fn dismiss_update_dialog(&mut self, cx: &mut Context<Self>) {
        self.update.show_dialog = false;
        cx.notify();
    }

    pub fn skip_this_update_version(&mut self, cx: &mut Context<Self>) {
        if let Some(v) = self.update.status.latest_version().map(|s| s.to_string()) {
            self.settings.skipped_update_version = Some(v);
            self.settings.save();
        }
        self.update.show_dialog = false;
        if matches!(self.update.status, UpdateStatus::Available { .. }) {
            self.update.status = UpdateStatus::NotAvailable {
                current_version: env!("CARGO_PKG_VERSION").to_string(),
            };
        }
        cx.notify();
    }

    /// 开始下载（Available → Downloading → Verifying → Downloaded）。
    pub fn start_update_download(&mut self, cx: &mut Context<Self>) {
        let UpdateStatus::Available {
            current_version,
            latest_version,
            asset_name,
            asset_url,
            checksum_url,
            release_url,
            notes,
            ..
        } = self.update.status.clone()
        else {
            return;
        };
        let cache = match crate::platform::update_cache_dir() {
            Some(c) => c,
            None => {
                self.update.status = UpdateStatus::Error {
                    current_version,
                    operation: updater::UpdateOperation::Download,
                    message: "update cache dir unavailable".into(),
                };
                cx.notify();
                return;
            }
        };

        self.update.status = UpdateStatus::Downloading {
            current_version: current_version.clone(),
            latest_version: latest_version.clone(),
            progress: Default::default(),
            asset_name: asset_name.clone(),
            asset_url: asset_url.clone(),
            checksum_url: checksum_url.clone(),
            release_url: release_url.clone(),
            notes: notes.clone(),
        };
        let progress = Arc::new(Mutex::new(updater::DownloadProgress::default()));
        self.update.live_progress = Some(progress.clone());
        cx.notify();

        // 后台线程只改 Arc，不碰 UI；这里短轮询 notify，让对话框上的百分比走起来。
        cx.spawn(async move |this, cx| loop {
            let downloading = this
                .update(cx, |this, _| {
                    matches!(this.update.status, UpdateStatus::Downloading { .. })
                })
                .unwrap_or(false);
            if !downloading {
                return;
            }
            let _ = this.update(cx, |_, cx| cx.notify());
            cx.background_executor()
                .timer(Duration::from_millis(250))
                .await;
        })
        .detach();

        let live = self.live.clone();
        let work = cx.background_executor().spawn(async move {
            let zip_path = cache.join(&asset_name);
            let sidecar_path = cache.join(format!("{asset_name}.sha256"));
            {
                let progress = progress.clone();
                download_to_file(&asset_url, &zip_path, &mut |transferred, total| {
                    if let Ok(mut p) = progress.lock() {
                        p.transferred = transferred;
                        p.total = total;
                        p.percent = match total {
                            Some(t) if t > 0 => {
                                (transferred as f32 / t as f32 * 100.0).clamp(0.0, 100.0)
                            }
                            _ => 0.0,
                        };
                    }
                })?;
            }
            let sidecar = fetch_text(&checksum_url, 20_000)?;
            std::fs::write(&sidecar_path, &sidecar).map_err(|e| e.to_string())?;
            let expected = parse_checksum_sidecar(&sidecar)
                .ok_or_else(|| "checksum sidecar is malformed".to_string())?;
            let actual = sha256_file(&zip_path).map_err(|e| e.to_string())?;
            if actual != expected {
                return Err(format!(
                    "checksum mismatch: expected {expected}, got {actual}"
                ));
            }
            if !live.load(std::sync::atomic::Ordering::Relaxed) {
                return Err("app is shutting down".into());
            }
            let extracted = cache.join("extracted");
            let payload = extract_update_zip(&zip_path, &extracted)?;
            Ok::<PathBuf, String>(payload)
        });

        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.update.live_progress = None;
                match result {
                    Ok(payload) => {
                        this.update.status = UpdateStatus::Downloaded {
                            current_version: env!("CARGO_PKG_VERSION").to_string(),
                            latest_version,
                            payload,
                        };
                    }
                    Err(message) => {
                        crate::log!("更新下载失败: {message}");
                        this.update.status = UpdateStatus::Error {
                            current_version: env!("CARGO_PKG_VERSION").to_string(),
                            operation: updater::UpdateOperation::Download,
                            message,
                        };
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 下载完成后交给平台 helper 替换并重启。有进行中的扫描/清理时先确认。
    pub fn install_update_now(&mut self, cx: &mut Context<Self>) {
        let UpdateStatus::Downloaded {
            payload,
            latest_version,
            ..
        } = self.update.status.clone()
        else {
            return;
        };
        let busy = self.junk.scanning
            || self.clean.running
            || self.apps.scanning
            || self.disk.scanning
            || self.declutter.clean_task.is_some();
        if busy && !self.update.install_confirmed {
            self.confirm = Some(crate::ui::components::ConfirmRequest {
                kind: crate::ui::components::ConfirmKind::InstallUpdate,
                title: tr_update_install_title(self.language).to_string(),
                body: tr_update_install_busy_body(self.language).to_string(),
                detail: String::new(),
                app_data: false,
            });
            cx.notify();
            return;
        }
        self.apply_downloaded_update(payload, latest_version, cx);
    }

    pub fn apply_downloaded_update(
        &mut self,
        payload: PathBuf,
        latest_version: String,
        cx: &mut Context<Self>,
    ) {
        self.update.install_confirmed = false;
        self.update.status = UpdateStatus::Installing {
            current_version: env!("CARGO_PKG_VERSION").to_string(),
            latest_version,
        };
        cx.notify();
        let work = cx
            .background_executor()
            .spawn(async move { crate::platform::apply_update_and_restart(&payload) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(()) => {
                    // helper 已拉起，主动退出让替换发生。
                    crate::log!("更新已交接给 helper，准备退出");
                    cx.quit();
                }
                Err(message) => {
                    crate::log!("更新安装失败: {message}");
                    this.update.status = UpdateStatus::Error {
                        current_version: env!("CARGO_PKG_VERSION").to_string(),
                        operation: updater::UpdateOperation::Install,
                        message,
                    };
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// 重试：按当前 error 的 operation 回退到检查或重新下载。
    pub fn retry_update(&mut self, cx: &mut Context<Self>) {
        let op = match &self.update.status {
            UpdateStatus::Error { operation, .. } => *operation,
            _ => return,
        };
        match op {
            updater::UpdateOperation::Check => {
                self.update.status = UpdateStatus::Idle {
                    current_version: env!("CARGO_PKG_VERSION").to_string(),
                };
                self.check_for_updates_manual(cx);
            }
            updater::UpdateOperation::Download => {
                // 重新检查以拿到 Available，再由用户/自动继续。
                self.update.status = UpdateStatus::Idle {
                    current_version: env!("CARGO_PKG_VERSION").to_string(),
                };
                self.check_for_updates_manual(cx);
            }
            updater::UpdateOperation::Install => {
                if let UpdateStatus::Error { .. } = self.update.status {
                    // 无法从 error 恢复到 downloaded（payload 可能还在），重查。
                    self.update.status = UpdateStatus::Idle {
                        current_version: env!("CARGO_PKG_VERSION").to_string(),
                    };
                    self.check_for_updates_manual(cx);
                }
            }
        }
    }

    /// 打开 GitHub Release 页面（失败时的兜底手动下载）。
    pub fn open_update_release_page(&mut self, url: String) {
        crate::platform::open_url(&url);
    }
}
