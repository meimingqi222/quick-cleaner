//! MFT 扫描引擎：scan_volume / build_tree / resolve_path

use super::mft_parser::*;
use super::mft_types::*;
use crate::core::disk::{DirUsage, ScanError, VolumeId};
use rayon::prelude::*;
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub fn scan_volume(vol_id: &VolumeId, top_n: usize) -> Result<ScanResult, ScanError> {
    let letter = vol_id
        .drive_letter()
        .ok_or(ScanError::UnsupportedFilesystem("NTFS"))?;
    let started = Instant::now();

    // 对系统卷发起原始卷句柄读取是杀软重点盯防的行为（rawcopy / 勒索软件
    // 都走这条路），实测 C: 比 D: 在这一步多花近 1 秒，怀疑是实时监控的钩子。
    // 单独计时，免得这段时间被含混地算进「解析」里。
    let t_open = Instant::now();
    let vol = Volume::open(letter)?;
    let open_time = t_open.elapsed();
    let t_vd = Instant::now();
    let vd = vol.volume_data()?;
    let vd_time = t_vd.elapsed();

    let bytes_per_cluster = vd.bytes_per_cluster as u64;
    let bytes_per_sector = vd.bytes_per_sector as usize;
    let rec_size = vd.bytes_per_file_record_segment as usize;
    let mft_offset = vd.mft_start_lcn as u64 * bytes_per_cluster;

    let t_first = Instant::now();
    let mut first = vec![0u8; rec_size.max(bytes_per_sector)];
    vol.read_at(mft_offset, &mut first)?;
    let first_time = t_first.elapsed();
    if !apply_fixup(&mut first, bytes_per_sector) {
        return Err(ScanError::UnsupportedFilesystem("NTFS"));
    }
    let mut frags: Vec<DataFragment> = Vec::new();
    collect_data_fragments(&first, &mut frags);

    let mut ext_records = 0usize;
    let t_prep = Instant::now();
    if let Some(list) = read_attribute_list(&first, &vol, bytes_per_cluster) {
        let partial = flatten_fragments(frags.clone());
        for rec_no in attribute_list_data_records(&list) {
            if rec_no == 0 {
                continue;
            }
            if let Some(ext) = read_mft_record(
                &vol,
                &partial,
                rec_no,
                rec_size,
                bytes_per_cluster,
                bytes_per_sector,
            ) {
                let before = frags.len();
                collect_data_fragments(&ext, &mut frags);
                if frags.len() > before {
                    ext_records += 1;
                }
            }
        }
    }

    let runs = flatten_fragments(frags);
    if runs.is_empty() {
        return Err(ScanError::UnsupportedFilesystem("NTFS"));
    }
    let run_clusters: u64 = runs.iter().map(|&(_, c)| c).sum();

    let mft_valid = vd.mft_valid_data_length.max(0) as u64;
    let est_records = (mft_valid / rec_size as u64) as usize;
    let mut entries: Vec<Entry> = Vec::with_capacity(est_records + 1024);

    let prep_time = t_prep.elapsed();

    let mut buf = vec![0u8; CHUNK_BYTES];
    let mut consumed: u64 = 0;
    // 读盘与解析在同一个循环里交织，只看总时长分不出谁是瓶颈。分开计时：
    // 读盘占大头就该在 IO 上想办法（更大的块、异步预读），解析占大头就该
    // 把 parse_record 并行化。两条路的代价完全不同，不测清楚不该动手。
    let (mut io_time, mut parse_time) = (Duration::ZERO, Duration::ZERO);
    let mut hard_links: Vec<(u32, u32)> = Vec::new();

    'outer: for (lcn, clusters) in runs {
        let run_bytes = clusters * bytes_per_cluster;
        let base = lcn as u64 * bytes_per_cluster;
        let mut done: u64 = 0;

        while done < run_bytes {
            if consumed >= mft_valid {
                break 'outer;
            }
            let remain = run_bytes - done;
            // 按 MFT 记录（1024 字节）对齐读取。标准 NTFS 簇大小 ≥4KB，
            // 每一个 run 的字节数必然是簇大小的整数倍，因而也是 1024 的整数倍；
            // 单条记录不会跨越物理 run 边界，entries.len() 可安全作为连续记录号。
            let want = (CHUNK_BYTES as u64).min(remain) / rec_size as u64 * rec_size as u64;
            if want == 0 {
                break;
            }

            let t_io = Instant::now();
            let got = vol.read_at(base + done, &mut buf[..want as usize])?;
            io_time += t_io.elapsed();
            let full = got / rec_size;
            if full == 0 {
                break 'outer;
            }

            // 块内并行解析。每条记录 1024 字节、彼此独立，`apply_fixup` 只改
            // 本记录的缓冲区，是天然可并行的纯计算——实测这一步单线程要占掉
            // 整个 MFT 解析的三成（C 盘 1.22 秒），而读盘已经贴着硬件上限。
            //
            // **顺序必须保持**：`entries` 的下标就是 MFT 记录号，而 `Entry::parent`
            // 存的正是记录号。错一位整棵目录树就全乱了。`par_chunks_mut` 是
            // 索引并行迭代器，经 `enumerate`/`map_init` 后 `collect` 到 Vec 仍
            // 保序，因此记录号 = `base_rec + k` 成立。
            //
            // `links` 用 `map_init` 挂在每个 rayon 线程上复用：改成每条记录新建
            // 就是三百多万次小分配，省下的解析时间会被分配器吃回去。
            let t_parse = Instant::now();
            let base_rec = entries.len() as u32;
            let parsed: Vec<(Entry, Vec<(u32, u32)>)> = buf[..full * rec_size]
                .par_chunks_mut(rec_size)
                .enumerate()
                .map_init(
                    || Vec::<(u32, u8)>::with_capacity(8),
                    |links, (k, rec)| {
                        let rec_no = base_rec + k as u32;
                        let mut entry = Entry::default();
                        links.clear();
                        if apply_fixup(rec, bytes_per_sector) {
                            parse_record(rec, &mut entry, links);
                        }

                        // 硬链接是少数派，绝大多数记录这里返回空 Vec，不分配。
                        let mut extra: Vec<(u32, u32)> = Vec::new();
                        if entry.base_ref != 0 {
                            for &(pa, _) in links.iter() {
                                extra.push((entry.base_ref, pa));
                            }
                        } else if entry.used && !entry.is_dir && links.len() > 1 {
                            for &(pa, _) in links.iter() {
                                if pa != entry.parent {
                                    extra.push((rec_no, pa));
                                }
                            }
                        }
                        (entry, extra)
                    },
                )
                .collect();

            for (entry, extra) in parsed {
                entries.push(entry);
                hard_links.extend(extra);
            }
            parse_time += t_parse.elapsed();

            let advanced = (full * rec_size) as u64;
            done += advanced;
            consumed += advanced;
        }
    }

    if entries.len() <= ROOT_RECORD as usize {
        return Err(ScanError::UnsupportedFilesystem("NTFS"));
    }

    let n = entries.len();

    let t_merge = Instant::now();
    let mut merged_from_ext = 0u64;
    for i in 0..n {
        let (base, size) = (entries[i].base_ref as usize, entries[i].size);
        if base == 0 || size == 0 || base >= n {
            continue;
        }
        if entries[base].used && !entries[base].is_dir {
            entries[base].size += size;
            merged_from_ext += 1;
        }
    }

    let merge_time = t_merge.elapsed();

    let t_agg = Instant::now();
    let mut dir_size = vec![0u64; n];
    let mut dir_files = vec![0u64; n];
    let mut total_size = 0u64;
    let mut file_count = 0u64;
    let mut dir_count = 0u64;

    for i in 0..n {
        let e = &entries[i];
        if !e.used {
            continue;
        }
        if e.is_dir {
            dir_count += 1;
            continue;
        }
        file_count += 1;
        total_size += e.size;

        add_to_ancestors(&entries, &mut dir_size, &mut dir_files, e.parent, e.size);
    }

    hard_links.sort_unstable();
    hard_links.dedup();
    hard_links.retain(|&(rec_no, parent)| {
        let i = rec_no as usize;
        i < n && entries[i].used && !entries[i].is_dir && entries[i].parent != parent
    });

    let mut hard_link_size = 0u64;
    for &(rec_no, parent) in &hard_links {
        let size = entries[rec_no as usize].size;
        hard_link_size += size;
        add_to_ancestors(&entries, &mut dir_size, &mut dir_files, parent, size);
    }
    let agg_time = t_agg.elapsed();
    let unique_size = total_size;
    let unique_files = file_count;
    total_size += hard_link_size;
    file_count += hard_links.len() as u64;

    // 目录体积排行榜只有命令行工具 mftscan 用得上；GUI 走的是 SizeTree
    // 逐层下钻，不需要这份榜单。top_n 为 0 时直接跳过全盘排序与路径解析。
    let dirs: Vec<DirUsage> = if top_n == 0 {
        Vec::new()
    } else {
        let mut ranked: Vec<u32> = (0..n as u32)
            .filter(|&i| {
                entries[i as usize].used && entries[i as usize].is_dir && dir_size[i as usize] > 0
            })
            .collect();
        ranked.sort_unstable_by(|&a, &b| dir_size[b as usize].cmp(&dir_size[a as usize]));
        ranked.truncate(top_n);

        let mut cache: HashMap<u32, String> = HashMap::new();
        ranked
            .iter()
            .map(|&i| DirUsage {
                path: resolve_path(&entries, &[], i, vol_id, &mut cache),
                size: dir_size[i as usize],
                file_count: dir_files[i as usize],
            })
            .collect()
    };

    let t_tree = Instant::now();
    let tree = build_tree(vol_id.clone(), entries, dir_size, dir_files);
    let tree_time = t_tree.elapsed();

    crate::log!(
        "MFT 解析 {letter}: 完成 {:?}（开卷 {:?} / 卷信息 {:?} / 首记录 {:?} / run list {:?} / 读盘 {:?} / 解析 {:?} / 扩展并回 {:?} / 聚合 {:?} / 建树 {:?}），扩展记录 {}，读入 {}，记录 {}/{}，{} 文件 / {} 目录，占用 {}",
        started.elapsed(),
        open_time,
        vd_time,
        first_time,
        prep_time,
        io_time,
        parse_time,
        merge_time,
        agg_time,
        tree_time,
        ext_records,
        crate::core::model::fmt_size(consumed),
        n,
        mft_valid / rec_size as u64,
        file_count,
        dir_count,
        crate::core::model::fmt_size(total_size)
    );

    Ok(ScanResult {
        volume: vol_id.clone(),
        tree,
        total_size,
        file_count,
        dir_count,
        dirs,
        elapsed_ms: started.elapsed().as_millis() as u64,
        records_read: n as u64,
        records_expected: mft_valid / rec_size as u64,
        mft_run_bytes: run_clusters * bytes_per_cluster,
        ext_records: ext_records as u64,
        ext_data_merged: merged_from_ext,
        hard_links: hard_links.len() as u64,
        unique_size,
        unique_files,
    })
}

pub(super) fn build_tree(
    volume: VolumeId,
    mut entries: Vec<Entry>,
    dir_size: Vec<u64>,
    dir_files: Vec<u64>,
) -> SizeTree {
    let n = entries.len();

    let mut counts = vec![0u32; n];
    for i in 0..n {
        let e = &entries[i];
        if !e.used || i as u32 == ROOT_RECORD {
            continue;
        }
        let p = e.parent as usize;
        if p < n && entries[p].used && entries[p].is_dir {
            counts[p] += 1;
        }
    }

    let mut child_start = vec![0u32; n + 1];
    for i in 0..n {
        child_start[i + 1] = child_start[i] + counts[i];
    }

    let mut cursor: Vec<u32> = child_start[..n].to_vec();
    let mut child_at = vec![0u32; child_start[n] as usize];
    for i in 0..n {
        let e = &entries[i];
        if !e.used || i as u32 == ROOT_RECORD {
            continue;
        }
        let p = e.parent as usize;
        if p < n && entries[p].used && entries[p].is_dir {
            child_at[cursor[p] as usize] = i as u32;
            cursor[p] += 1;
        }
    }

    // 把名字灌入连续 name_pool，entry 只存偏移。
    // 解析阶段每条 Entry.name 是独立堆分配；这里一次性释放掉，
    // 之后 Entry 就只剩定长字段，省掉 6.3M × ~40 字节 allocator overhead。
    let mut name_pool = Vec::with_capacity(n * 16);
    for e in &mut entries {
        let off = name_pool.len() as u32;
        name_pool.extend_from_slice(e.name.as_bytes());
        e.name_off = off;
        e.name_len = e.name.len() as u16;
        e.name.clear();
        e.name.shrink_to_fit();
    }

    SizeTree {
        volume,
        entries,
        name_pool,
        dir_size,
        dir_files,
        child_start,
        child_at,
    }
}

pub(super) fn add_to_ancestors(
    entries: &[Entry],
    dir_size: &mut [u64],
    dir_files: &mut [u64],
    start: u32,
    size: u64,
) {
    let n = entries.len();
    let mut cur = start;
    let mut depth = 0;
    loop {
        let idx = cur as usize;
        if idx >= n || depth > MAX_DEPTH {
            break;
        }
        dir_size[idx] += size;
        dir_files[idx] += 1;
        if cur == ROOT_RECORD {
            break;
        }
        let next = entries[idx].parent;
        if next == cur {
            break;
        }
        cur = next;
        depth += 1;
    }
}

pub(super) fn resolve_path(
    entries: &[Entry],
    name_pool: &[u8],
    idx: u32,
    volume: &VolumeId,
    cache: &mut HashMap<u32, String>,
) -> String {
    let label = volume.display();
    if idx == ROOT_RECORD {
        return label.to_string();
    }
    if let Some(hit) = cache.get(&idx) {
        return hit.clone();
    }

    let mut chain: Vec<u32> = Vec::new();
    let mut cur = idx;
    let mut base = label.to_string();
    let mut depth = 0;

    loop {
        if cur == ROOT_RECORD || depth > MAX_DEPTH {
            break;
        }
        if let Some(hit) = cache.get(&cur) {
            base = hit.clone();
            break;
        }
        let i = cur as usize;
        if i >= entries.len() || !entries[i].used {
            break;
        }
        chain.push(cur);
        let next = entries[i].parent;
        if next == cur {
            break;
        }
        cur = next;
        depth += 1;
    }

    let mut path = base;
    for &c in chain.iter().rev() {
        let e = &entries[c as usize];
        // name_pool 为空时（build_tree 之前的 dirs 排行榜路径）回退到 entry.name
        let name: &str = if name_pool.is_empty() {
            &e.name
        } else {
            let off = e.name_off as usize;
            let end = off + e.name_len as usize;
            std::str::from_utf8(&name_pool[off..end]).unwrap_or("")
        };
        path.push('\\');
        path.push_str(name);
        cache.insert(c, path.clone());
    }
    path
}
