//! 磁盘透镜动作：扫描、卷切换、磁盘清理

use crate::core::cleaner::clean_arbitrary;
use crate::core::disk::VolumeId;
use crate::core::i18n::bilingual;
use crate::core::model::fmt_size;
use crate::platform::scan_volume;
use crate::ui::components::{ConfirmKind, ConfirmRequest};
use crate::ui::i18n::*;
use gpui::Context;
use std::path::PathBuf;

impl crate::ui::Root {
    pub fn enter_disk_node(&mut self, idx: u32, cx: &mut Context<Self>) {
        self.disk.path.push(idx);
        cx.notify();
    }

    pub fn switch_disk_volume(&mut self, vol: VolumeId, cx: &mut Context<Self>) {
        self.disk.volume_menu_open = false;
        if self.disk.volume == vol && (self.disk.mft.is_some() || self.disk.scanning) {
            return;
        }
        self.disk.volume = vol;
        self.disk.mft = None;
        self.disk.rows.clear();
        self.disk.rows_key = None;
        self.disk.path = vec![crate::core::disk::ROOT_NODE];
        self.disk.sel.clear();
        self.start_mft_scan(cx);
    }

    pub fn start_mft_scan(&mut self, cx: &mut Context<Self>) {
        if self.disk.scanning {
            return;
        }
        self.disk.scanning = true;
        self.disk.error = None;
        let vol = self.disk.volume.clone();
        self.disk.refresh_volume_spaces();
        self.disk.sel.clear();
        let saved_path = self.current_disk_full_path();
        self.status = bilingual(|l| tr_status_disk_scanning(l, &vol));
        self.start_tick(cx);
        cx.notify();

        // macOS：磁盘透镜根据所选卷加载不同索引。
        // 主卷 `/`：加载/构建整盘索引，首次可能需要 1-2 分钟。
        // 其他卷：直接扫描。
        // Windows：仍然走 scan_volume 解析 $MFT。
        #[cfg(not(windows))]
        let cached_root_index = self.macos_root_index.clone();
        let scan_t0 = std::time::Instant::now();
        let scan = cx.background_executor().spawn(async move {
            #[cfg(windows)]
            {
                scan_volume(&vol, 0).map(std::sync::Arc::new)
            }
            #[cfg(not(windows))]
            {
                let is_root = vol.mount_point() == std::path::Path::new("/");
                if is_root {
                    if let Some(scan) = cached_root_index {
                        crate::log!("磁盘透镜复用已缓存整盘索引：{} 条记录", scan.records_read);
                        Ok(scan)
                    } else {
                        let t0 = std::time::Instant::now();
                        let live = std::sync::atomic::AtomicBool::new(true);
                        let result =
                            match crate::core::devscan::load_or_build_macos_root_index(&live) {
                                Some(scan) => {
                                    crate::log!("磁盘透镜加载整盘索引：{:?}", t0.elapsed());
                                    Ok(scan)
                                }
                                None => Err(crate::core::disk::ScanError::Io(
                                    "无法加载或构建整盘索引".into(),
                                )),
                            };
                        result
                    }
                } else {
                    let t0 = std::time::Instant::now();
                    let result = scan_volume(&vol, 0).map(std::sync::Arc::new);
                    crate::log!("磁盘透镜扫描外接卷 {}：{:?}", vol.display(), t0.elapsed());
                    result
                }
            }
        });

        let vol_for_task = self.disk.volume.clone();
        self.disk.task = Some(cx.spawn(async move |this, cx| {
            let result = scan.await;
            this.update(cx, |this, cx| {
                // 扫描期间用户可能切换卷。旧任务的结果绝不能挂到新卷 UI
                // 上；切换当时因 scanning=true 未能启动的新扫描在这里补上。
                if this.disk.volume != vol_for_task {
                    this.disk.scanning = false;
                    this.start_mft_scan(cx);
                    return;
                }
                this.disk.scanning = false;
                match result {
                    Ok(s) => {
                        // 磁盘总占用用 statfs 的「总量-空闲」，不用 SizeTree 累加。
                        // APFS 快照/克隆/硬链接会导致「所有文件大小相加」超过物理容量。
                        let used = this
                            .disk
                            .space
                            .map(|(total, free)| fmt_size(total - free))
                            .unwrap_or_else(|| fmt_size(s.total_size));
                        let files = s.file_count;
                        let elapsed = scan_t0.elapsed().as_secs_f64();
                        this.status = bilingual(|l| tr_status_disk_done(l, files, &used, elapsed));
                        // 仅当 saved_path 确实属于当前卷时才尝试恢复层级；跨盘切换时直接回到新盘根目录
                        let is_same_vol = saved_path.as_ref().is_some_and(|p| {
                            p.to_string_lossy().starts_with(vol_for_task.display())
                        });
                        if is_same_vol {
                            if let Some(target_path) = saved_path {
                                let resolved = s.tree.find_path(&target_path);
                                this.disk.path = if resolved.is_empty() {
                                    vec![s.tree.root()]
                                } else {
                                    resolved
                                };
                            } else {
                                this.disk.path = vec![s.tree.root()];
                            }
                        } else {
                            this.disk.path = vec![s.tree.root()];
                        }
                        // 主卷 `/` 的整盘索引缓存起来，避免下次打开磁盘透镜重扫
                        #[cfg(not(windows))]
                        if vol_for_task.mount_point() == std::path::Path::new("/") {
                            this.macos_root_index = Some(s.clone());
                        }
                        this.disk.mft = Some(s.clone());
                        // 磁盘透镜扫完的 MFT 也存到搜索索引，搜索可复用
                        #[cfg(windows)]
                        {
                            if !this
                                .search
                                .indices
                                .iter()
                                .any(|existing| existing.volume == s.volume)
                            {
                                this.search.indices.push(s);
                            }
                        }
                        this.disk.gen += 1;
                    }
                    Err(e) => {
                        this.status =
                            bilingual(|l| tr_status_disk_failed(l, &tr_scan_error(l, &e)));
                        this.disk.error = Some(e);
                        this.disk.mft = None;
                        this.disk.gen += 1;
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    // ---- 文件快速检索 ----

    pub fn request_clean_disk_selected(&mut self, cx: &mut Context<Self>) {
        if self.disk.sel.is_empty() || self.clean.running {
            return;
        }
        let total_size = self.disk_selected_size();
        let count = self.disk.sel.len();
        let lang = self.language;

        self.confirm = Some(ConfirmRequest {
            title: tr_confirm_delete_selected_title(lang).to_string(),
            body: tr_confirm_delete_selected_msg(lang, count, &fmt_size(total_size)),
            detail: tr_confirm_no_recycle_check_data(lang).to_string(),
            kind: ConfirmKind::CleanDiskSelected,
        });
        cx.notify();
    }

    pub(crate) fn prune_deleted_from_mft(
        &mut self,
        deleted: &[PathBuf],
        freed_bytes: u64,
        _cx: &mut Context<Self>,
    ) {
        #[cfg(not(windows))]
        let mut updated_root_index = false;
        if let Some(mft) = &mut self.disk.mft {
            // 主卷索引同时由 macos_root_index 持有。mmap 主体经 Arc 共享且
            // 只读，Clone 复制的是显式 delta（追加节点 + 覆盖表），修改
            // 完成后必须让缓存也指向这份新数据。
            let mft_mut = std::sync::Arc::make_mut(mft);
            for path in deleted {
                mft_mut.remove_path(path);
            }
            #[cfg(not(windows))]
            {
                updated_root_index = mft_mut.volume.mount_point() == std::path::Path::new("/");
            }
            self.disk.gen += 1;

            // 当前所在目录可能已被删除，沿 path 栈往回退到有效节点
            while self.disk.path.len() > 1 {
                let cur = *self.disk.path.last().unwrap();
                if mft_mut.tree.valid(cur) {
                    break;
                }
                self.disk.path.pop();
            }
        }
        #[cfg(not(windows))]
        if updated_root_index {
            self.macos_root_index = self.disk.mft.clone();
            if let Some(scan) = self.macos_root_index.clone() {
                crate::core::devscan::remember_macos_root_index(scan);
            }
        }
        if let Some((_, free)) = &mut self.disk.space {
            *free += freed_bytes;
        }
    }

    /// 智能清理页：删掉当前勾选的所有分类项。
    /// 磁盘透镜：删掉当前勾选的一批路径。
    pub fn start_clean_disk_selected(&mut self, cx: &mut Context<Self>) {
        if self.clean.running || self.disk.sel.is_empty() {
            return;
        }
        // 展开成实际删除目标：勾选目录里若埋着被排除的子孙，会自动下钻绕开。
        let targets = self.disk.sel.resolve_targets();
        if targets.is_empty() {
            return;
        }
        let total_size = self.disk_selected_size();
        let n = targets.len();
        let to_clean = targets.clone();
        let disposal = self.disposal();

        self.spawn_clean(
            (0, total_size),
            bilingual(|l| tr_status_batch_deleting(l, n)),
            move |p| clean_arbitrary(&to_clean, disposal, p),
            move |this, _report, snap, cx| {
                this.clear_disk_selection();

                let deleted: Vec<PathBuf> = targets
                    .iter()
                    .filter(|target| !target.exists())
                    .cloned()
                    .collect();
                let fails = targets.len().saturating_sub(deleted.len());
                let (files, size) = (snap.files, fmt_size(snap.bytes));
                this.status = bilingual(|l| {
                    if fails == 0 {
                        tr_status_batch_done(l, files, &size)
                    } else {
                        tr_status_batch_done_partial(l, &size, fails)
                    }
                });

                this.prune_deleted_from_mft(&deleted, snap.bytes, cx);
            },
            cx,
        );
    }
}
