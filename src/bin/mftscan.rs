//! MFT 扫描的命令行验证工具。
//!
//! 用法：
//!   mftscan [盘符] [top_n]                     只跑 MFT 扫描并打印结果
//!   mftscan [盘符] [top_n] --csv <WizTree.csv> 跑完再和 WizTree 导出对比
//!
//! WizTree CSV 是正确性基准：目录行的“大小”列是递归真实大小，和我们
//! 聚合出来的 dir_size 应当一致（活动系统上会有少量漂移，属正常）。

use quick_cleaner::core::model::fmt_size;
use quick_cleaner::platform::{is_elevated, mft};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let letter = args
        .get(1)
        .and_then(|s| s.chars().next())
        .unwrap_or('C')
        .to_ascii_uppercase();
    let top_n: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(30);
    let csv = args
        .iter()
        .position(|a| a == "--csv")
        .and_then(|i| args.get(i + 1))
        .cloned();

    println!("管理员权限: {}", if is_elevated() { "是" } else { "否" });
    println!("正在扫描 {letter}: 的 $MFT …\n");

    // 对比模式下需要拿到很多目录，否则只取用户要的 top_n
    let want = if csv.is_some() { 200_000 } else { top_n };

    let scan = match mft::scan_volume(letter, want) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("扫描失败：{e}");
            if matches!(e, mft::MftError::AccessDenied) {
                eprintln!("请以管理员身份运行。");
            }
            std::process::exit(1);
        }
    };

    println!("耗时      : {} ms", scan.elapsed_ms);
    println!("文件总数  : {}", scan.file_count);
    println!("目录总数  : {}", scan.dir_count);
    println!("占用总量  : {} ({} 字节)", fmt_size(scan.total_size), scan.total_size);

    println!("\n--- MFT 读取诊断 ---");
    println!("解析记录槽 : {}", scan.records_read);
    println!("应有记录数 : {}", scan.records_expected);
    println!("run 覆盖   : {}", fmt_size(scan.mft_run_bytes));
    println!("扩展记录   : {}", scan.ext_records);
    println!("扩展 $DATA 并回 : {} 个文件", scan.ext_data_merged);
    println!("硬链接额外归属 : {} 条", scan.hard_links);
    println!(
        "去重后实际占用 : {} / {} 文件（硬链接只算一次）",
        fmt_size(scan.unique_size),
        scan.unique_files
    );
    if scan.records_read + 16 < scan.records_expected {
        println!(
            "⚠ MFT 没读全！只读到 {:.1}%，run list 可能仍有段没跟到。",
            scan.records_read as f64 * 100.0 / scan.records_expected.max(1) as f64
        );
    } else {
        println!("✓ MFT 记录槽数与 MftValidDataLength 推算一致");
    }
    println!("\n占用最大的 {top_n} 个目录：");
    for (i, d) in scan.dirs.iter().take(top_n).enumerate() {
        println!(
            "{:>3}. {:>10}  {:>9} 文件  {}",
            i + 1,
            fmt_size(d.size),
            d.file_count,
            d.path
        );
    }

    if let Some(path) = csv {
        compare_with_wiztree(&scan, &path);
    }
}

/// 解析 WizTree 导出的 CSV，和 MFT 扫描结果逐目录对比。
fn compare_with_wiztree(scan: &mft::MftScan, csv_path: &str) {
    println!("\n================ 与 WizTree 基准对比 ================");
    let file = match std::fs::File::open(csv_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("打不开 CSV：{e}");
            return;
        }
    };

    // WizTree: 文件名称,大小,分配,修改时间,属性,文件,文件夹
    let mut baseline: HashMap<String, (u64, u64)> = HashMap::new();
    let mut root_size = 0u64;
    let mut root_files = 0u64;

    let reader = BufReader::new(file);
    for line in reader.lines().map_while(Result::ok) {
        let Some(row) = parse_csv_row(&line) else {
            continue;
        };
        if row.len() < 7 {
            continue;
        }
        let name = &row[0];
        // 只看目录行（以反斜杠结尾）
        if !name.ends_with('\\') {
            continue;
        }
        let (Ok(size), Ok(files)) = (row[1].parse::<u64>(), row[5].parse::<u64>()) else {
            continue;
        };

        let key = normalize(name);
        // "C:" 是根，单独记
        if key.len() <= 2 {
            root_size = size;
            root_files = files;
            continue;
        }
        baseline.insert(key, (size, files));
    }

    println!("WizTree 基准目录数 : {}", baseline.len());
    println!(
        "根目录 (C:)  基准 {} / {} 文件",
        fmt_size(root_size), root_files
    );
    println!(
        "根目录 (C:)  本次 {} / {} 文件",
        fmt_size(scan.total_size), scan.file_count
    );
    if root_size > 0 {
        let d = pct_diff(scan.total_size, root_size);
        let df = pct_diff(scan.file_count, root_files);
        println!("总量偏差 {d:+.3}% ，文件数偏差 {df:+.3}%");
    }

    // 逐目录比对
    let mut matched = 0usize;
    let mut missing = 0usize;
    let mut exact = 0usize;
    let mut within_1pct = 0usize;
    let mut worst: Vec<(f64, String, u64, u64)> = Vec::new();

    for d in &scan.dirs {
        let key = normalize(&d.path);
        match baseline.get(&key) {
            Some(&(bsize, _)) => {
                matched += 1;
                if bsize == d.size {
                    exact += 1;
                }
                let diff = pct_diff(d.size, bsize);
                if diff.abs() <= 1.0 {
                    within_1pct += 1;
                } else {
                    worst.push((diff, d.path.clone(), d.size, bsize));
                }
            }
            None => missing += 1,
        }
    }

    println!("\n对比了 {} 个目录（{} 个在基准中找不到）", matched, missing);
    if matched > 0 {
        println!(
            "完全一致 : {} ({:.1}%)",
            exact,
            exact as f64 * 100.0 / matched as f64
        );
        println!(
            "偏差 ≤1% : {} ({:.1}%)",
            within_1pct,
            within_1pct as f64 * 100.0 / matched as f64
        );
    }

    if !worst.is_empty() {
        worst.sort_by(|a, b| b.0.abs().partial_cmp(&a.0.abs()).unwrap());
        println!("\n偏差最大的 15 个目录（本次 vs 基准）：");
        for (diff, path, mine, base) in worst.iter().take(15) {
            println!(
                "  {:+8.2}%  本次 {:>10}  基准 {:>10}  {}",
                diff,
                fmt_size(*mine),
                fmt_size(*base),
                path
            );
        }
    }
}

fn pct_diff(mine: u64, base: u64) -> f64 {
    if base == 0 {
        return 0.0;
    }
    (mine as f64 - base as f64) * 100.0 / base as f64
}

/// 统一成小写、去掉结尾反斜杠，方便两边比对。
fn normalize(p: &str) -> String {
    p.trim_end_matches('\\').to_ascii_lowercase()
}

/// 极简 CSV 解析：只需处理 WizTree 的双引号包裹字段。
fn parse_csv_row(line: &str) -> Option<Vec<String>> {
    if line.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '"' if in_quotes => {
                if chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            }
            '"' => in_quotes = true,
            ',' if !in_quotes => {
                out.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    out.push(cur);
    Some(out)
}
