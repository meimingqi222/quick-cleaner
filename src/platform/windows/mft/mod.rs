//! Windows NTFS MFT 解析与磁盘空间扫描

mod mft_parser;
mod mft_scanner;
mod mft_types;

pub use mft_scanner::scan_volume;
pub use mft_types::{DirUsage, Node, ScanError, ScanResult, SizeTree, ROOT_RECORD};

#[cfg(test)]
mod tests {
    use super::mft_parser::{
        apply_fixup, attribute_list_data_records, collect_data_fragments, flatten_fragments,
        parse_record, parse_runs, u16_at, DataFragment, Entry,
    };
    use super::mft_scanner::build_tree;
    use super::*;
    use crate::core::disk::VolumeId;
    use rayon::prelude::*;

    fn synthetic_tree() -> SizeTree {
        let mut entries = vec![Entry::default(); 12];
        let mut mk = |i: usize, parent: u32, name: &str, is_dir: bool, size: u64| {
            entries[i] = Entry {
                parent,
                name_off: 0,
                name_len: 0,
                is_dir,
                size,
                used: true,
                base_ref: 0,
                mtime: 0,
                name: name.to_string(),
            };
        };
        mk(5, 5, "", true, 0);
        mk(6, 5, "Windows", true, 0);
        mk(7, 5, "Users", true, 0);
        mk(8, 6, "a.dll", false, 100);
        mk(9, 7, "me", true, 0);
        mk(10, 9, "big.iso", false, 5000);
        mk(11, 7, "readme.txt", false, 50);

        let mut dir_size = vec![0u64; 12];
        let mut dir_files = vec![0u64; 12];
        for (i, sz) in [(5, 5150u64), (6, 100), (7, 5050), (9, 5000)] {
            dir_size[i] = sz;
        }
        for (i, fc) in [(5, 3u64), (6, 1), (7, 2), (9, 1)] {
            dir_files[i] = fc;
        }
        build_tree(
            VolumeId::from_drive_letter('C'),
            entries,
            dir_size,
            dir_files,
        )
    }

    #[test]
    fn children_are_sorted_by_size_desc() {
        let t = synthetic_tree();
        let kids = t.children(t.root());
        let names: Vec<&str> = kids.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["Users", "Windows"]);
        assert_eq!(kids[0].size, 5050);
        assert_eq!(kids[1].size, 100);
    }

    #[test]
    fn children_mix_dirs_and_files() {
        let t = synthetic_tree();
        let kids = t.children(7);
        let names: Vec<&str> = kids.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["me", "readme.txt"]);
        assert!(kids[0].is_dir);
        assert!(!kids[1].is_dir);
        assert_eq!(kids[1].size, 50);
    }

    #[test]
    fn own_size_excludes_subdirectories() {
        let t = synthetic_tree();
        let kids = t.children(t.root());
        let users = kids.iter().find(|c| c.name == "Users").unwrap();
        assert_eq!(users.size, 5050);
        assert_eq!(users.own_size, 50);

        let me = t.children(7).into_iter().find(|c| c.name == "me").unwrap();
        assert_eq!(me.own_size, 5000);
    }

    #[test]
    fn resolves_full_paths() {
        let t = synthetic_tree();
        assert_eq!(t.path_of(10), r"C:\Users\me\big.iso");
        assert_eq!(t.path_of(6), r"C:\Windows");
        assert_eq!(t.path_of(t.root()), "C:");
    }

    /// 阶段一的查表路径。放在这里是因为树夹具在这个模块里，
    /// 被测函数本身属于 `core::scanner`。
    #[test]
    fn measure_via_tree_matches_the_aggregated_size() {
        use crate::core::scanner::measure_via_tree;
        use std::path::Path;
        let t = synthetic_tree();

        // 目录取递归聚合值，文件取自身大小
        assert_eq!(
            measure_via_tree(&t, Path::new(r"C:\Users")),
            Some((5050, 2))
        );
        assert_eq!(
            measure_via_tree(&t, Path::new(r"C:\Users\me")),
            Some((5000, 1))
        );
        assert_eq!(
            measure_via_tree(&t, Path::new(r"C:\Users\me\big.iso")),
            Some((5000, 1))
        );
        // 大小写不敏感，和 NTFS 一致
        assert_eq!(
            measure_via_tree(&t, Path::new(r"c:\users")),
            Some((5050, 2))
        );
    }

    /// 查不到就必须返回 None 让调用方退回遍历，绝不能把「走到一半」的
    /// 那个祖先目录的体积当成结果——那会凭空多报几个 GB。
    #[test]
    fn measure_via_tree_refuses_partial_matches() {
        use crate::core::scanner::measure_via_tree;
        use std::path::Path;
        let t = synthetic_tree();

        // MFT 快照之后才建出来的目录：Users 有，nope 没有
        assert_eq!(measure_via_tree(&t, Path::new(r"C:\Users\nope")), None);
        assert_eq!(
            measure_via_tree(&t, Path::new(r"C:\Users\me\deep\er")),
            None
        );
        // 不是这个卷
        assert_eq!(measure_via_tree(&t, Path::new(r"D:\Users")), None);
        // 没有盘符
        assert_eq!(measure_via_tree(&t, Path::new(r"\\server\Users")), None);
    }

    /// 并行解析的正确性全押在「rayon 的索引并行迭代器保序」这一条上：
    /// `entries` 的下标就是 MFT 记录号，而 `Entry::parent` 存的也是记录号，
    /// 错一位整棵目录树就废了。这里把那条流水线原样跑一遍，钉住这个假设——
    /// 万一将来换了迭代器组合或 rayon 改了行为，这个测试先红。
    #[test]
    fn parallel_chunk_mapping_preserves_order() {
        const REC: usize = 16;
        let mut buf: Vec<u8> = (0u32..4096).map(|i| (i % 251) as u8).collect();
        let out: Vec<(usize, u8)> = buf
            .par_chunks_mut(REC)
            .enumerate()
            .map_init(
                Vec::<u8>::new,
                |scratch: &mut Vec<u8>, (k, c): (usize, &mut [u8])| {
                    scratch.clear();
                    scratch.push(c[0]);
                    (k, c[0])
                },
            )
            .collect();

        assert_eq!(out.len(), 4096 / REC);
        for (i, (k, b)) in out.iter().enumerate() {
            assert_eq!(*k, i, "enumerate 的序号必须与 collect 后的下标一致");
            assert_eq!(*b, ((i * REC) % 251) as u8, "第 {i} 块的内容错位了");
        }
    }

    #[test]
    fn parent_walks_up_and_stops_at_root() {
        let t = synthetic_tree();
        assert_eq!(t.parent_of(10), Some(9));
        assert_eq!(t.parent_of(9), Some(7));
        assert_eq!(t.parent_of(7), Some(ROOT_RECORD));
        assert_eq!(t.parent_of(ROOT_RECORD), None);
    }

    #[test]
    fn largest_files_ignores_directories() {
        let t = synthetic_tree();
        let files = t.largest_files(10);
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["big.iso", "a.dll", "readme.txt"]);
        assert!(files.iter().all(|f| !f.is_dir));
    }

    #[test]
    fn largest_files_respects_limit() {
        let t = synthetic_tree();
        let files = t.largest_files(2);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].name, "big.iso");
        assert_eq!(files[1].name, "a.dll");
        assert!(t.largest_files(0).is_empty());
    }

    #[test]
    fn empty_directory_has_no_children() {
        let t = synthetic_tree();
        assert!(t.children(8).is_empty());
    }

    #[test]
    fn ntfs_metadata_is_filtered_from_children_and_largest_files() {
        let mut entries = vec![Entry::default(); 16];
        let mut mk = |i: usize, parent: u32, name: &str, is_dir: bool, size: u64| {
            entries[i] = Entry {
                parent,
                name_off: 0,
                name_len: 0,
                is_dir,
                size,
                used: true,
                base_ref: 0,
                mtime: 0,
                name: name.to_string(),
            };
        };
        mk(0, 5, "$MFT", false, 3_000_000_000);
        mk(2, 5, "$LogFile", false, 64_000_000);
        mk(5, 5, "", true, 0);
        mk(6, 5, "MyData", true, 0);
        mk(11, 5, "$Extend", true, 100_000);
        mk(14, 6, "video.mp4", false, 1_000_000_000);

        let mut dir_size = vec![0u64; 16];
        let mut dir_files = vec![0u64; 16];
        dir_size[5] = 4_064_100_000;
        dir_files[5] = 4;
        let t = build_tree(
            VolumeId::from_drive_letter('C'),
            entries,
            dir_size,
            dir_files,
        );

        // 验证根目录下过滤掉了 $MFT, $LogFile, $Extend，只保留 MyData
        let kids = t.children(t.root());
        assert_eq!(kids.len(), 1);
        assert_eq!(kids[0].name, "MyData");

        // 验证全盘大文件排除了 $MFT 和 $LogFile，只排入普通文件 video.mp4
        let files = t.largest_files(10);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "video.mp4");
    }

    // -----------------------------------------------------------------
    // 畸形记录的防御性解析
    //
    // $MFT 是从**裸卷**上读来的，而且是在系统正在写盘的时候读。fixup 校验
    // 拦得住整扇区撕裂，拦不住所有字节组合。下面这组用例把「磁盘上的长度
    // 字段在撒谎」的各种形态都固定下来：要求一律安静地放弃，绝不 panic。
    // -----------------------------------------------------------------

    /// 造一条最小可用的 FILE 记录：`len` 字节，属性表从 `attrs_off` 开始。
    fn skeleton_record(len: usize, attrs_off: u16) -> Vec<u8> {
        let mut rec = vec![0u8; len];
        rec[0..4].copy_from_slice(b"FILE");
        rec[0x14..0x16].copy_from_slice(&attrs_off.to_le_bytes());
        rec[0x16..0x18].copy_from_slice(&0x01u16.to_le_bytes()); // in_use
        rec
    }

    fn parse(rec: &[u8]) -> bool {
        let mut e = Entry::default();
        let mut links = Vec::new();
        parse_record(rec, &mut e, &mut links)
    }

    /// 声明 alen = 0x10 的非常驻 $DATA：属性头还没读完记录就到头了。
    /// 老代码在 `u64_at(rec, pos + 0x10)` 上直接越界 panic。
    #[test]
    fn truncated_non_resident_data_attr_is_ignored() {
        let mut rec = skeleton_record(0x40, 0x30);
        rec[0x30..0x34].copy_from_slice(&0x80u32.to_le_bytes()); // $DATA
        rec[0x34..0x38].copy_from_slice(&0x10u32.to_le_bytes()); // alen 谎报成 0x10
        rec[0x38] = 1; // non_resident
        assert!(!parse(&rec), "属性头装不下就该整条放弃");
    }

    /// 常驻属性同理：alen 小于常驻头长度 0x18 时不能去读 0x10/0x14 偏移。
    #[test]
    fn truncated_resident_attr_is_ignored() {
        let mut rec = skeleton_record(0x40, 0x30);
        rec[0x30..0x34].copy_from_slice(&0x30u32.to_le_bytes()); // $FILE_NAME
        rec[0x34..0x38].copy_from_slice(&0x10u32.to_le_bytes());
        assert!(!parse(&rec));
    }

    /// alen 超出记录长度：不能继续往下走。
    #[test]
    fn attr_longer_than_record_stops_parsing() {
        let mut rec = skeleton_record(0x80, 0x30);
        rec[0x30..0x34].copy_from_slice(&0x80u32.to_le_bytes());
        rec[0x34..0x38].copy_from_slice(&0xffff_ffffu32.to_le_bytes());
        assert!(!parse(&rec));
    }

    /// alen = 0 会让 `pos += alen` 原地踏步。属性头下限保证了它必然被拒。
    #[test]
    fn zero_length_attr_cannot_spin_forever() {
        let mut rec = skeleton_record(0x80, 0x30);
        rec[0x30..0x34].copy_from_slice(&0x80u32.to_le_bytes());
        rec[0x34..0x38].copy_from_slice(&0u32.to_le_bytes());
        assert!(!parse(&rec));
    }

    /// attrs_off 指到记录外面。
    #[test]
    fn attrs_offset_past_end_is_ignored() {
        let rec = skeleton_record(0x40, 0x9999);
        assert!(!parse(&rec));
    }

    /// run list 的字段宽度是 4 bit，最大 15，但 u64 只装得下 8 字节。
    /// 老代码在 `1i64 << (15 * 8 - 1)` 上移位溢出。
    #[test]
    fn runlist_rejects_oversized_field_widths() {
        // header 0xF1 => len_size=1, off_size=15
        let mut runs = vec![0u8; 17];
        runs[0] = 0xF1;
        runs[1] = 0x08;
        assert!(parse_runs(&runs).is_empty());

        // header 0x1F => len_size=15, off_size=1
        let mut runs = vec![0u8; 17];
        runs[0] = 0x1F;
        assert!(parse_runs(&runs).is_empty());
    }

    /// off_size == 8 且偏移为负——**这在大卷上是完全合法的数据**。
    /// 老代码的符号扩展写成「减去 1 << (off_size * 8)」，这里就是 `1i64 << 64`。
    #[test]
    fn runlist_handles_full_width_negative_offset() {
        let mut runs = vec![0x11u8, 0x10, 0x64]; // +100 处 16 簇
        runs.push(0x81); // len_size=1, off_size=8
        runs.push(0x08); // 8 簇
        runs.extend_from_slice(&(-2i64).to_le_bytes()); // 回退 2 簇
        runs.push(0x00); // 终止符

        assert_eq!(parse_runs(&runs), vec![(100, 16), (98, 8)]);
    }

    /// off_size == 0 是稀疏段：跳过这一段，但后面的段还要继续解析。
    #[test]
    fn runlist_skips_sparse_segment_and_continues() {
        let runs = [
            0x11u8, 0x10, 0x64, // +100 处 16 簇
            0x01, 0x08, // 稀疏：8 簇，无偏移
            0x11, 0x04, 0x0a, // 再 +10 处 4 簇
            0x00,
        ];
        assert_eq!(parse_runs(&runs), vec![(100, 16), (110, 4)]);
    }

    /// 兜底：任意字节流喂进解析器都不能 panic。
    ///
    /// 用固定种子的 LCG 而不是随机数，保证失败可复现，也不用引入依赖。
    #[test]
    fn arbitrary_bytes_never_panic() {
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };

        for case in 0..2000 {
            let len = 0x30 + (next() as usize % 0x400);
            let mut rec: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            // 一半的用例保留合法的 FILE 头，好让解析真的走进属性循环
            if case % 2 == 0 {
                rec[0..4].copy_from_slice(b"FILE");
                rec[0x16] |= 0x01;
            }
            let mut e = Entry::default();
            let mut links = Vec::new();
            let _ = parse_record(&rec, &mut e, &mut links);

            let mut frags = Vec::new();
            collect_data_fragments(&rec, &mut frags);
            let _ = attribute_list_data_records(&rec);
            let _ = parse_runs(&rec);
        }
    }

    #[test]
    fn parse_runs_single() {
        let runs = [0x21u8, 0x18, 0x34, 0x02, 0x00];
        assert_eq!(parse_runs(&runs), vec![(0x0234, 0x18)]);
    }

    #[test]
    fn parse_runs_negative_offset() {
        let runs = [0x11u8, 0x10, 0x20, 0x11, 0x10, 0xe0, 0x00];
        let got = parse_runs(&runs);
        assert_eq!(got, vec![(0x20, 0x10), (0x00, 0x10)]);
    }

    #[test]
    fn parse_runs_stops_at_terminator() {
        let runs = [0x11u8, 0x08, 0x10, 0x00, 0x11, 0x08, 0x10];
        assert_eq!(parse_runs(&runs), vec![(0x10, 0x08)]);
    }

    fn attr_list_entry(atype: u32, start_vcn: u64, rec_no: u64, name_len: u8) -> Vec<u8> {
        let mut e = vec![0u8; 0x18];
        e[0x00..0x04].copy_from_slice(&atype.to_le_bytes());
        e[0x04..0x06].copy_from_slice(&0x18u16.to_le_bytes());
        e[0x06] = name_len;
        e[0x08..0x10].copy_from_slice(&start_vcn.to_le_bytes());
        e[0x10..0x18].copy_from_slice(&rec_no.to_le_bytes());
        e
    }

    #[test]
    fn attribute_list_picks_unnamed_data_records() {
        let mut list = Vec::new();
        list.extend(attr_list_entry(0x10, 0, 0, 0));
        list.extend(attr_list_entry(0x80, 0, 0, 0));
        list.extend(attr_list_entry(0x80, 100, 42, 0));
        list.extend(attr_list_entry(0x80, 200, 77, 0));
        list.extend(attr_list_entry(0x80, 300, 99, 4));
        list.extend(attr_list_entry(0x80, 400, 42, 0));

        assert_eq!(attribute_list_data_records(&list), vec![42, 77]);
    }

    #[test]
    fn flatten_orders_fragments_by_vcn() {
        let frags = vec![
            DataFragment {
                start_vcn: 100,
                runs: vec![(0x50, 4)],
            },
            DataFragment {
                start_vcn: 0,
                runs: vec![(0x10, 2), (0x30, 3)],
            },
            DataFragment {
                start_vcn: 300,
                runs: vec![(0x90, 1)],
            },
        ];
        assert_eq!(
            flatten_fragments(frags),
            vec![(0x10, 2), (0x30, 3), (0x50, 4), (0x90, 1)]
        );
    }

    #[test]
    fn fixup_rejects_corrupt_record() {
        let mut rec = vec![0u8; 1024];
        rec[0..4].copy_from_slice(b"FILE");
        rec[0x04] = 0x30;
        rec[0x06] = 3;
        rec[0x30] = 0xaa;
        rec[0x31] = 0xbb;
        assert!(!apply_fixup(&mut rec, 512));
    }

    #[test]
    fn fixup_restores_sector_tails() {
        let mut rec = vec![0u8; 1024];
        rec[0..4].copy_from_slice(b"FILE");
        rec[0x04] = 0x30;
        rec[0x06] = 3;
        rec[0x30] = 0xaa;
        rec[0x31] = 0xbb;
        rec[0x32] = 0x22;
        rec[0x33] = 0x11;
        rec[0x34] = 0x44;
        rec[0x35] = 0x33;
        rec[510] = 0xaa;
        rec[511] = 0xbb;
        rec[1022] = 0xaa;
        rec[1023] = 0xbb;

        assert!(apply_fixup(&mut rec, 512));
        assert_eq!(u16_at(&rec, 510), 0x1122);
        assert_eq!(u16_at(&rec, 1022), 0x3344);
    }
}
