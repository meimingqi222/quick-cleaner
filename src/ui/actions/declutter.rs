//! 文件整理扫描动作

use crate::core::i18n::{bilingual, Language};
use crate::core::model::fmt_size;
use gpui::Context;

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
}
