//! 文件整理的扫描与清理动作

use crate::core::cleaner::{clean_arbitrary_items, ArbitraryTarget, CleanProgress, Disposal};
use crate::core::i18n::{bilingual, Language};
use crate::core::model::fmt_size;
use crate::ui::views::DeclutterTab;
use gpui::Context;
use std::path::{Path, PathBuf};

impl crate::ui::Root {
    pub fn start_declutter_scan(&mut self, cx: &mut Context<Self>) {
        if self.declutter.scanning {
            return;
        }
        self.declutter.scanning = true;
        self.status = bilingual(|l| match l {
            Language::Zh => "正在利用索引与多线程深度扫描大文件、重复文件与相似图片...".to_string(),
            Language::En => {
                "Scanning for large files, duplicates and similar photos (indexed)...".to_string()
            }
        });
        cx.notify();

        let live = self.live.clone();
        let mft_tree = self.disk.mft.clone();
        #[cfg(not(windows))]
        let macos_idx = self.macos_root_index.clone();

        cx.spawn(async move |this, cx| {
            let scan_data = cx
                .background_executor()
                .spawn(async move {
                    let t_start = std::time::Instant::now();
                    #[cfg(windows)]
                    let tree_ref = mft_tree.as_ref().map(|s| &s.tree);
                    #[cfg(not(windows))]
                    let tree_ref = mft_tree
                        .as_ref()
                        .map(|s| &s.tree)
                        .or_else(|| macos_idx.as_ref().map(|s| &s.tree));

                    let (downloads, (large_files, (duplicates, photos))) = rayon::join(
                        || crate::core::declutter::scan_downloads_folder(&live, tree_ref),
                        || {
                            rayon::join(
                                || {
                                    crate::core::declutter::scan_large_old_files(
                                        &live, 50_000_000, tree_ref,
                                    )
                                },
                                || {
                                    rayon::join(
                                        || {
                                            crate::core::declutter::scan_duplicate_files(
                                                &live, tree_ref,
                                            )
                                        },
                                        || {
                                            crate::core::declutter::scan_similar_photos(
                                                &live, tree_ref,
                                            )
                                        },
                                    )
                                },
                            )
                        },
                    );

                    (
                        downloads,
                        large_files,
                        duplicates,
                        photos,
                        t_start.elapsed(),
                    )
                })
                .await;

            this.update(cx, |this, cx| {
                this.declutter.scanning = false;
                this.declutter.scanned = true;
                this.declutter.scan_elapsed_secs = Some(scan_data.4.as_secs_f64());
                this.declutter.download_items = scan_data.0;
                this.declutter.large_files = scan_data.1;
                this.declutter.duplicate_groups = scan_data.2;
                this.declutter.photo_groups = scan_data.3;

                let savings = this.declutter.total_potential_savings();
                crate::log!(
                    "[Declutter] 全盘智能整理扫描完成: 总耗时 {:?}, 发现可优化空间 {}",
                    scan_data.4,
                    fmt_size(savings)
                );
                this.status = bilingual(move |l| match l {
                    Language::Zh => {
                        format!("文件整理扫描完成，发现可优化空间 {}", fmt_size(savings))
                    }
                    Language::En => format!("Declutter scan complete, found {}", fmt_size(savings)),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 冗余整理四个页签共用的「清理所选项」。
    ///
    /// 委托给 `core::cleaner::clean_arbitrary(Disposal::RecycleBin)`——它本来就是
    /// 为「用户手选的任意路径」设计的：循环 → `is_protected` → 处置 → 报表，
    /// 和这里需要的形状完全一致。以前 declutter 自带一份 `clean_declutter_items`
    /// 平行实现，代价是三份 trash 原语、两套保护检查、两套计数口径各自演化，
    /// 符号链接和目录大小两个 bug 就是各自补课补出来的。
    ///
    /// 计数口径也随之对齐 core 的不变量：**移入废纸篓不释放空间**，所以只报
    /// 条目数，不再说「释放 X」。
    pub fn clean_declutter_selected(&mut self, tab: DeclutterTab, cx: &mut Context<Self>) {
        if self.declutter.cleaning {
            return;
        }
        let paths = self.selected_declutter_paths(tab);
        if paths.is_empty() {
            return;
        }

        let n = paths.len();
        self.declutter.cleaning = true;
        self.status = bilingual(move |l| match l {
            Language::Zh => format!("正在把 {n} 项移入废纸篓..."),
            Language::En => format!("Moving {n} items to Trash..."),
        });
        cx.notify();

        let items: Vec<ArbitraryTarget> = paths.into_iter().map(ArbitraryTarget::capture).collect();
        let work = cx.background_executor().spawn(async move {
            // 废纸篓不释放空间，字节总量填 0：进度条上的「已释放」必须是真的。
            let progress = CleanProgress::new(items.len() as u64, 0);
            let report = clean_arbitrary_items(&items, Disposal::RecycleBin, &progress);
            let failed: Vec<PathBuf> = report
                .failed
                .iter()
                .chain(report.manual.iter())
                .filter_map(|f| f.as_path().map(Path::to_path_buf))
                .collect();
            (report.ok, failed)
        });

        self.declutter.clean_task = Some(cx.spawn(async move |this, cx| {
            let (moved, failed) = work.await;
            this.update(cx, |this, cx| {
                this.declutter.cleaning = false;
                // 只摘掉真正移走的条目：失败的留在列表里，用户还能看见和重试。
                this.prune_cleaned_declutter_items(tab, &failed);

                let (zh_noun, en_noun) = declutter_item_noun(tab);
                let n_failed = failed.len();
                this.status = bilingual(move |l| match l {
                    Language::Zh => {
                        let mut msg = format!("已把 {moved} 个{zh_noun}移入废纸篓");
                        if n_failed > 0 {
                            msg.push_str(&format!("；{n_failed} 个失败"));
                        }
                        msg
                    }
                    Language::En => {
                        let mut msg = format!("Moved {moved} {en_noun} to Trash");
                        if n_failed > 0 {
                            msg.push_str(&format!("; {n_failed} failed"));
                        }
                        msg
                    }
                });
                cx.notify();
            })
            .ok();
        }));
    }

    /// 清空当前页签的全部勾选。
    ///
    /// 只有「大型与旧文件」页有这个入口（相似图片页对应的是「智能全选」），
    /// 所以操作条按参数决定显不显示——但选择状态怎么清是各页签自己的事，
    /// 收在这里和 `selected_declutter_paths` / `prune_cleaned_declutter_items`
    /// 放在一起，三个「按页签分派」的地方形状保持一致。
    pub fn clear_declutter_selection(&mut self, tab: DeclutterTab, cx: &mut Context<Self>) {
        let d = &mut self.declutter;
        match tab {
            DeclutterTab::Downloads => d.download_items.iter_mut().for_each(|f| f.selected = false),
            DeclutterTab::LargeFiles => d.large_files.iter_mut().for_each(|f| f.selected = false),
            DeclutterTab::SimilarPhotos => d
                .photo_groups
                .iter_mut()
                .flat_map(|g| &mut g.photos)
                .for_each(|p| p.selected = false),
            DeclutterTab::Duplicates => d
                .duplicate_groups
                .iter_mut()
                .flat_map(|g| &mut g.files)
                .for_each(|f| f.selected = false),
            DeclutterTab::Overview => {}
        }
        cx.notify();
    }

    /// 当前页签下被勾选的路径。
    fn selected_declutter_paths(&self, tab: DeclutterTab) -> Vec<PathBuf> {
        let d = &self.declutter;
        match tab {
            DeclutterTab::Downloads => d
                .download_items
                .iter()
                .filter(|f| f.selected)
                .map(|f| f.path.clone())
                .collect(),
            DeclutterTab::LargeFiles => d
                .large_files
                .iter()
                .filter(|f| f.selected)
                .map(|f| f.path.clone())
                .collect(),
            DeclutterTab::SimilarPhotos => d
                .photo_groups
                .iter()
                .flat_map(|g| &g.photos)
                .filter(|p| p.selected)
                .map(|p| p.path.clone())
                .collect(),
            DeclutterTab::Duplicates => d
                .duplicate_groups
                .iter()
                .flat_map(|g| &g.files)
                .filter(|f| f.selected)
                .map(|f| f.path.clone())
                .collect(),
            DeclutterTab::Overview => Vec::new(),
        }
    }

    /// 删除完成后把已清掉的条目从列表里摘掉。
    ///
    /// 分组页签还要丢掉只剩一个成员的组：一张照片谈不上「相似」，
    /// 一个文件谈不上「重复」。
    fn prune_cleaned_declutter_items(&mut self, tab: DeclutterTab, failed: &[PathBuf]) {
        // 勾上了但没移走的要留下：以前无条件按 selected 摘除，失败的条目会从
        // 界面上消失，用户以为清掉了，其实文件还在盘上。
        let gone = |path: &PathBuf, selected: bool| selected && !failed.contains(path);
        let d = &mut self.declutter;
        match tab {
            DeclutterTab::Downloads => d.download_items.retain(|f| !gone(&f.path, f.selected)),
            DeclutterTab::LargeFiles => d.large_files.retain(|f| !gone(&f.path, f.selected)),
            DeclutterTab::SimilarPhotos => {
                for g in &mut d.photo_groups {
                    g.photos.retain(|p| !gone(&p.path, p.selected));
                }
                d.photo_groups.retain(|g| g.photos.len() >= 2);
            }
            DeclutterTab::Duplicates => {
                for g in &mut d.duplicate_groups {
                    g.files.retain(|f| !gone(&f.path, f.selected));
                }
                d.duplicate_groups.retain(|g| g.files.len() >= 2);
            }
            DeclutterTab::Overview => {}
        }
    }
}

/// 状态栏里对这批条目的称呼。
fn declutter_item_noun(tab: DeclutterTab) -> (&'static str, &'static str) {
    match tab {
        DeclutterTab::Downloads => ("下载项", "downloads"),
        DeclutterTab::LargeFiles => ("大文件", "large files"),
        DeclutterTab::SimilarPhotos => ("相似照片", "similar photos"),
        DeclutterTab::Duplicates => ("重复文件", "duplicate files"),
        DeclutterTab::Overview => ("项目", "items"),
    }
}
