//! 文件搜索与索引动作

use crate::core::i18n::bilingual;
#[cfg(windows)]
use crate::core::disk::VolumeId;
#[cfg(windows)]
use crate::platform::is_elevated;
use crate::ui::i18n::*;
use crate::ui::text_input::clamp_to_boundary;
use crate::ui::SearchSortCol;
use gpui::Context;
use std::time::Duration;

impl crate::ui::Root {
    pub(crate) fn search_index_ready(&self) -> bool {
        #[cfg(windows)]
        {
            !self.search.indices.is_empty()
        }
        #[cfg(not(windows))]
        {
            self.macos_root_index.is_some()
        }
    }

    /// 启动后台构建搜索索引。Windows 逐卷解析 $MFT；macOS 加载整盘索引。
    ///
    /// Windows 上的关键约束：垃圾扫描的预扫描和磁盘透镜都只会解析**当前卷**
    /// 的 $MFT，并把那一份索引塞进 `search.indices`。早期实现看到
    /// `indices` 非空就直接复用返回，导致多盘机器上 D:/E: 等其他盘的文件
    /// 永远搜不到——必须点「重新扫描」清空索引后才会重扫所有卷。
    /// 现在改为：先吸收已有的单卷索引（预扫描 / 磁盘透镜），再对照
    /// `list_volumes()` 补扫缺失的卷，追加进 `indices`。
    pub fn start_search_index(&mut self, cx: &mut Context<Self>) {
        if self.search.indexing {
            return;
        }

        // macOS：如果已经有整盘索引，直接复用
        #[cfg(not(windows))]
        {
            if self.macos_root_index.is_some() {
                self.run_search();
                cx.notify();
                return;
            }
        }

        // Windows：先把磁盘透镜已扫的 MFT 吸收进搜索索引（去重），
        // 再对照全卷列表计算还需要补扫哪些卷。预扫描留下的单卷索引
        // 也已经在 `indices` 里，会一并被覆盖检查命中。
        #[cfg(windows)]
        {
            if let Some(mft) = &self.disk.mft {
                if !self
                    .search
                    .indices
                    .iter()
                    .any(|existing| existing.volume == mft.volume)
                {
                    crate::log!(
                        "文件搜索：复用磁盘透镜已扫描的 MFT 索引（{} 条记录）",
                        mft.records_read
                    );
                    self.search.indices.push(mft.clone());
                }
            }

            let all_vols = crate::platform::list_volumes();
            let missing: Vec<VolumeId> = all_vols
                .into_iter()
                .filter(|v| !self.search.indices.iter().any(|idx| idx.volume == *v))
                .collect();

            if missing.is_empty() {
                // 所有卷都已索引，无需再扫
                self.status = bilingual(|l| tr_search_ready(l).to_string());
                self.run_search();
                cx.notify();
                return;
            }

            // 未提权时无法读 $MFT。如果至少已经有一些索引（来自预扫描），
            // 仍然让用户搜索已有部分；否则提示需要管理员权限。
            if !is_elevated() {
                crate::log!("文件搜索：未提权，无法读取 $MFT");
                if !self.search.indices.is_empty() {
                    self.status = bilingual(|l| tr_search_ready(l).to_string());
                    self.run_search();
                    cx.notify();
                    return;
                }
                self.status = bilingual(|l| tr_search_need_admin(l).to_string());
                cx.notify();
                return;
            }

            self.search.indexing = true;
            self.status = bilingual(|l| tr_search_indexing(l).to_string());
            self.start_tick(cx);
            cx.notify();

            let live = self.live.clone();
            let scan = cx.background_executor().spawn(async move {
                use crate::platform::scan_volume;
                crate::log!("文件搜索：开始补扫 {} 个卷的 $MFT", missing.len());
                let mut indices = Vec::new();
                for vol in &missing {
                    if !live.load(std::sync::atomic::Ordering::Relaxed) {
                        crate::log!("文件搜索：扫描被取消");
                        break;
                    }
                    crate::log!("文件搜索：扫描卷 {vol}");
                    match scan_volume(vol, 0) {
                        Ok(s) => {
                            crate::log!(
                                "文件搜索：卷 {vol} 扫描完成，{} 条记录",
                                s.records_read
                            );
                            indices.push(std::sync::Arc::new(s));
                        }
                        Err(e) => {
                            crate::log!("文件搜索：卷 {vol} 扫描失败：{e:?}");
                        }
                    }
                }
                crate::log!("文件搜索：补扫完成，新增 {} 个索引", indices.len());
                indices
            });

            self.search.index_task = Some(cx.spawn(async move |this, cx| {
                let result = scan.await;
                this.update(cx, |this, cx| {
                    this.search.indexing = false;
                    this.search.indices.extend(result);
                    if this.search_index_ready() {
                        this.status = bilingual(|l| tr_search_ready(l).to_string());
                        this.run_search();
                    } else {
                        this.status = bilingual(|l| tr_search_no_index(l).to_string());
                    }
                    cx.notify();
                })
                .ok();
            }));
        }

        // macOS：走到这里说明还没有整盘索引，需要后台构建
        #[cfg(not(windows))]
        {
            self.search.indexing = true;
            self.status = bilingual(|l| tr_search_indexing(l).to_string());
            self.start_tick(cx);
            cx.notify();

            let live = self.live.clone();
            let cached = self.macos_root_index.clone();

            let scan = cx.background_executor().spawn(async move {
                if let Some(s) = cached {
                    vec![s]
                } else {
                    match crate::core::devscan::load_or_build_macos_root_index(&live) {
                        Some(s) => vec![s],
                        None => vec![],
                    }
                }
            });

            self.search.index_task = Some(cx.spawn(async move |this, cx| {
                let result = scan.await;
                this.update(cx, |this, cx| {
                    this.search.indexing = false;
                    if let Some(s) = result.first() {
                        this.macos_root_index = Some(s.clone());
                    }
                    if this.search_index_ready() {
                        this.status = bilingual(|l| tr_search_ready(l).to_string());
                        this.run_search();
                    } else {
                        this.status = bilingual(|l| tr_search_no_index(l).to_string());
                    }
                    cx.notify();
                })
                .ok();
            }));
        }
    }

    /// 执行搜索（在主线程，索引已就绪时调用）。
    ///
    /// 空查询不再清空结果——而是返回全树按大小降序的前 `max_results` 项
    /// （类似 Everything 不输关键字时列出全部）。搜索树内部已对空查询
    /// 做了优化，只回溯最终选出的 N 条路径。
    pub fn run_search(&mut self) {
        let query = self.search.query.trim();

        let max_results = 500;
        let mut all_hits = Vec::new();

        #[cfg(windows)]
        {
            for idx in &self.search.indices {
                all_hits.extend(idx.tree.search(query, max_results));
            }
        }
        #[cfg(not(windows))]
        {
            if let Some(idx) = &self.macos_root_index {
                all_hits.extend(idx.tree.search(query, max_results));
            }
        }

        // 合并后按大小降序，截断
        all_hits.sort_unstable_by_key(|b| std::cmp::Reverse(b.size));
        all_hits.truncate(max_results);
        self.search.results = all_hits;
        self.search.gen += 1;
    }

    /// 搜索框文本变化时触发搜索（带 120ms 防抖与后台异步检索，避免主线程打字卡顿）。
    ///
    /// 空查询也走搜索路径——返回全树最大的 N 项，不再清空结果列表。
    pub fn search_input_changed(&mut self, cx: &mut Context<Self>) {
        if !self.search_index_ready() {
            return;
        }
        let query = self.search.query.trim().to_string();

        self.search.is_searching = true;
        cx.notify();

        #[cfg(windows)]
        let indices = self.search.indices.clone();
        #[cfg(not(windows))]
        let macos_idx = self.macos_root_index.clone();

        self.search.search_task = Some(cx.spawn(async move |this, cx| {
            // 防抖延迟：120ms 内再次输入会由新任务替换
            cx.background_executor()
                .timer(Duration::from_millis(120))
                .await;

            let hits = cx
                .background_executor()
                .spawn(async move {
                    let max_results = 500;
                    let mut all_hits = Vec::new();
                    #[cfg(windows)]
                    {
                        for idx in &indices {
                            all_hits.extend(idx.tree.search(&query, max_results));
                        }
                    }
                    #[cfg(not(windows))]
                    {
                        if let Some(idx) = &macos_idx {
                            all_hits.extend(idx.tree.search(&query, max_results));
                        }
                    }
                    all_hits.sort_unstable_by_key(|b| std::cmp::Reverse(b.size));
                    all_hits.truncate(max_results);
                    all_hits
                })
                .await;

            this.update(cx, |this, cx| {
                this.search.results = hits;
                this.apply_search_sort();
                this.search.is_searching = false;
                cx.notify();
            })
            .ok();
        }));
    }

    /// 执行文件搜索结果排序（一级：同类型文件聚合；二级：表头列排序与升降序）
    pub fn apply_search_sort(&mut self) {
        let group_by_kind = self.search.group_by_kind;
        let col = self.search.sort_col;
        let asc = self.search.sort_asc;

        self.search.results.sort_by(|a, b| {
            // 一级排序：按文件类型分类聚合
            let kind_cmp = if group_by_kind {
                let ka = crate::ui::components::icons::FileVisualKind::from_name(&a.name, a.is_dir)
                    as u8;
                let kb = crate::ui::components::icons::FileVisualKind::from_name(&b.name, b.is_dir)
                    as u8;
                ka.cmp(&kb)
            } else {
                std::cmp::Ordering::Equal
            };

            // 二级排序：按表头配置（名称 / 路径 / 大小）
            let col_cmp = match col {
                SearchSortCol::Name => {
                    let cmp = a.name.to_lowercase().cmp(&b.name.to_lowercase());
                    if asc {
                        cmp
                    } else {
                        cmp.reverse()
                    }
                }
                SearchSortCol::Path => {
                    let cmp = a.path.to_lowercase().cmp(&b.path.to_lowercase());
                    if asc {
                        cmp
                    } else {
                        cmp.reverse()
                    }
                }
                SearchSortCol::Size => {
                    let cmp = a.size.cmp(&b.size);
                    if asc {
                        cmp
                    } else {
                        cmp.reverse()
                    }
                }
            };

            kind_cmp
                .then(col_cmp)
                .then_with(|| b.size.cmp(&a.size))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        self.search.gen = self.search.gen.wrapping_add(1);
    }

    /// 切换一级排序开关：是否按同类型文件聚合
    pub fn search_toggle_group_by_kind(&mut self, cx: &mut Context<Self>) {
        self.search.group_by_kind = !self.search.group_by_kind;
        self.apply_search_sort();
        cx.notify();
    }

    /// 点击表头切换二级排序列与升降序
    pub fn search_toggle_sort(&mut self, col: SearchSortCol, cx: &mut Context<Self>) {
        if self.search.sort_col == col {
            self.search.sort_asc = !self.search.sort_asc;
        } else {
            self.search.sort_col = col;
            // 切换到新列时的自然默认方向：大小默认降序（大文件优先），其他列默认升序
            self.search.sort_asc = !matches!(col, SearchSortCol::Size);
        }
        self.apply_search_sort();
        cx.notify();
    }

    /// 搜索框退格键
    pub fn file_search_backspace(&mut self, cx: &mut Context<Self>) {
        let sel = clamp_to_boundary(&self.search.query, self.search.sel.clone());
        if sel.start != sel.end {
            self.search.query.replace_range(sel.clone(), "");
            self.search.sel = sel.start..sel.start;
        } else if sel.start > 0 {
            let prev = self.search.query[..sel.start]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.search.query.replace_range(prev..sel.start, "");
            self.search.sel = prev..prev;
        }
        self.search.marked = None;
        self.search_input_changed(cx);
    }

    /// 清空搜索框
    ///
    /// 清空后不再让结果列表空白——空查询会返回全树最大的 N 项，
    /// 与刚进入搜索页时的状态一致。
    pub fn file_search_clear(&mut self, cx: &mut Context<Self>) {
        self.search.search_task = None;
        self.search.query.clear();
        self.search.sel = 0..0;
        self.search.marked = None;
        // 不再 clear results，而是走空查询搜索（top N）
        self.search_input_changed(cx);
    }

    /// 用户主动点击“重新分析”时不能直接复用进程内索引；清空它后让加载器
    /// 回放 FSEvents 并核对文件系统。页面首次打开仍走快速内存缓存。
    pub fn restart_mft_scan(&mut self, cx: &mut Context<Self>) {
        #[cfg(not(windows))]
        if self.disk.volume.mount_point() == std::path::Path::new("/") {
            self.macos_root_index = None;
        }
        self.start_mft_scan(cx);
    }
}
