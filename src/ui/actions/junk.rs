//! 垃圾扫描相关动作：start_scan / start_discovery / start_discovery_arc

use crate::core::categories::all_targets;
use crate::core::disk::VolumeId;
use crate::core::i18n::bilingual;
use crate::core::model::fmt_size;
#[cfg(windows)]
use crate::core::scanner::dominant_volume;
#[cfg(windows)]
use crate::core::scanner::scan_discovered;
#[cfg(not(windows))]
use crate::core::scanner::scan_discovered_arc;
use crate::core::scanner::{merge_discovered, scan_fixed, scan_fixed_with_tree};
#[cfg(windows)]
use crate::platform::is_elevated;
use crate::ui::i18n::*;
use gpui::Context;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

impl crate::ui::Root {
    /// 发起一轮扫描。**分两个阶段**，界面不必等最慢的那条通道。
    ///
    /// 第一阶段扫固定路径表（`%TEMP%`、各种缓存目录），本机约 1 秒就能出结果，
    /// 界面立刻可用；第二阶段才去全盘检索构建产物，那是整轮里最贵的一步
    /// （本机 25 秒量级），跑完再把结果并进列表。
    ///
    /// 之所以值得拆：耗时几乎全在第二阶段，而它对应的「项目构建产物」类目
    /// **默认根本不勾选**——让用户为一个默认不清的类目干等半分钟，代价和收益
    /// 完全不成比例。
    pub fn start_scan(&mut self, cx: &mut Context<Self>) {
        if self.junk.scanning {
            return;
        }
        // 通知上一轮（可能还在跑的第二阶段）停下
        self.live.store(false, Ordering::Relaxed);
        self.junk.scan_task.take();
        self.junk.discover_task.take();

        self.junk.gen += 1;
        let gen = self.junk.gen;
        self.junk.scanning = true;
        self.junk.scanned = false;
        self.junk.discovering = false;
        self.status = bilingual(|l| tr_status_scanning(l).to_string());
        let live = Arc::new(AtomicBool::new(true));
        self.live = live.clone();
        self.start_tick(cx);
        cx.notify();

        let targets = all_targets();
        // 提权时先解析目标最集中的那个卷的 $MFT，阶段一在树上查表而不是
        // 遍历目录。看着是给首屏多加了一步，实测反而更快：本机 MFT 解析
        // 3.3 秒，而遍历要 4.1~4.9 秒——阶段一的瓶颈是 `go\pkg\mod`、
        // `npm-cache` 这类几十万个小文件的目录，每一个都要几秒，而它们的
        // 递归体积在 MFT 树里查一次表就有。
        //
        // 解析出来的树随后原样交给阶段二，一次解析两个阶段用，省掉第二次
        // 全盘解析。内存峰值不变——阶段二本来也要在内存里放一棵树。
        // Windows 上读 $MFT 需要管理员权限，未提权时跳过预扫描。
        //
        // macOS：先加载/构建用户目录索引，阶段一在树上查表（毫秒级），
        // 阶段二在树上 DFS。索引复用后首次启动和后续启动都受益。
        #[cfg(windows)]
        let prescan_volume = if is_elevated() {
            dominant_volume(&targets)
        } else {
            None
        };
        #[cfg(not(windows))]
        let prescan_volume: Option<VolumeId> = None;
        let scan = cx.background_executor().spawn(async move {
            #[cfg(windows)]
            let pre = prescan_volume.and_then(|v| scan_volume(&v, 0).ok());
            #[cfg(not(windows))]
            let pre = {
                let _ = prescan_volume; // 消除未使用变量警告
                                        // 直接用整盘索引——它包含用户目录，省掉单独构建/持有
                                        // 用户目录索引的 ~700MB 内存。有缓存时加载同样快。
                crate::core::devscan::load_or_build_macos_root_index(&live)
            };
            let cats = match &pre {
                Some(s) => scan_fixed_with_tree(&targets, &live, &s.tree),
                None => scan_fixed(&targets, &live),
            };
            (cats, pre)
        });
        self.junk.scan_task = Some(cx.spawn(async move |this, cx| {
            let (result, prescanned) = scan.await;
            this.update(cx, |this, cx| {
                this.junk.categories = result;
                this.junk.scanned = true;
                this.junk.scanning = false;
                this.select_recommended();
                let total_str = fmt_size(this.total_cleanable());
                this.status = bilingual(|l| tr_status_scan_fixed_done(l, &total_str));
                // macOS：prescanned 现在是整盘索引，直接作为 macos_root_index
                // 给磁盘透镜复用。不再单独持有 macos_index（用户目录索引）——
                // 整盘索引已包含用户目录，省掉 ~700MB 重复内存。
                #[cfg(not(windows))]
                {
                    this.macos_root_index = prescanned.clone();
                    this.start_discovery_arc(gen, prescanned, cx);
                }
                #[cfg(windows)]
                {
                    // 把垃圾扫描阶段解析的 MFT 存到搜索索引里，搜索功能
                    // 可以直接复用，不必再扫一遍同一个卷。
                    if let Some(s) = &prescanned {
                        let arc = std::sync::Arc::new(s.clone());
                        if !this
                            .search
                            .indices
                            .iter()
                            .any(|existing| existing.volume == arc.volume)
                        {
                            this.search.indices.push(arc);
                        }
                    }
                    this.start_discovery(gen, prescanned, cx);
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// 第二阶段：全盘检索构建产物，跑完并进已有分类。
    ///
    /// `gen` 是发起这轮扫描时的 `scan_gen`。回来时如果对不上，说明用户已经
    /// 点了「重新扫描」，这份结果属于上一轮，直接丢掉——否则会把过期数据
    /// （甚至是被取消后只跑了一半的数据）并进新列表。
    #[cfg(windows)]
    fn start_discovery(
        &mut self,
        gen: u64,
        prescanned: Option<crate::core::disk::ScanResult>,
        cx: &mut Context<Self>,
    ) {
        self.junk.discovering = true;
        let live = self.live.clone();
        let discover = cx
            .background_executor()
            .spawn(async move { scan_discovered(&live, prescanned) });

        self.junk.discover_task = Some(cx.spawn(async move |this, cx| {
            let items = discover.await;
            this.update(cx, |this, cx| {
                if this.junk.gen != gen {
                    return;
                }
                this.junk.discovering = false;
                let was_recommended = this.junk.selection_is_recommended();
                merge_discovered(&mut this.junk.categories, items);
                if was_recommended {
                    this.junk.select_recommended();
                }
                let total_str = fmt_size(this.total_cleanable());
                this.status = bilingual(|l| tr_status_scan_done(l, &total_str));
                cx.notify();
            })
            .ok();
        }));
    }

    /// macOS 专用：接受 `Arc<ScanResult>` 的 start_discovery 变体。
    /// 避免从 prescanned 中 clone 6.6M 条目的 ScanResult。
    #[cfg(not(windows))]
    fn start_discovery_arc(
        &mut self,
        gen: u64,
        prescanned: Option<std::sync::Arc<crate::core::disk::ScanResult>>,
        cx: &mut Context<Self>,
    ) {
        self.junk.discovering = true;
        let live = self.live.clone();
        let discover = cx
            .background_executor()
            .spawn(async move { scan_discovered_arc(&live, prescanned) });

        self.junk.discover_task = Some(cx.spawn(async move |this, cx| {
            let items = discover.await;
            this.update(cx, |this, cx| {
                if this.junk.gen != gen {
                    return;
                }
                this.junk.discovering = false;
                let was_recommended = this.junk.selection_is_recommended();
                merge_discovered(&mut this.junk.categories, items);
                if was_recommended {
                    this.junk.select_recommended();
                }
                let total_str = fmt_size(this.total_cleanable());
                this.status = bilingual(|l| tr_status_scan_done(l, &total_str));
                cx.notify();
            })
            .ok();
        }));
    }

    // ---- 智能清理：转发给 JunkState，逻辑与测试都在那边 ----
}
