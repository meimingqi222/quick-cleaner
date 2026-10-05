//! Read-only worktree timing: cargo run --example worktrees -- [--measure] <container>
use quick_cleaner::core::worktrees;

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let measure = args.iter().any(|arg| arg == "--measure");
    for container in args.into_iter().filter(|arg| arg != "--measure") {
        let path = std::path::PathBuf::from(container);
        let start = std::time::Instant::now();
        let found = worktrees::discover(&path);
        println!(
            "{}: {} worktrees, {:?}",
            path.display(),
            found.len(),
            start.elapsed()
        );
        for tree in &found {
            println!("  {}", tree.display());
        }
        let start = std::time::Instant::now();
        for _ in 0..10 {
            std::hint::black_box(worktrees::discover(&path));
        }
        println!("  mean over 10 warm runs: {:?}", start.elapsed() / 10);
        if measure {
            use quick_cleaner::core::{
                categories::ScanTarget,
                cleaner::Disposal,
                rules::{Operation, RuleRef},
                CategoryId,
            };
            let targets: Vec<_> = found
                .into_iter()
                .map(|path| ScanTarget {
                    operation: Operation::classify(&path, true),
                    disposal: Disposal::Permanent,
                    rule: RuleRef::engine(),
                    label: path.to_string_lossy().into_owned().into(),
                    path,
                    category: CategoryId::DevWorktrees,
                    recommended: false,
                    size_hint: None,
                })
                .collect();
            let start = std::time::Instant::now();
            let live = std::sync::atomic::AtomicBool::new(true);
            let categories = quick_cleaner::core::scanner::scan_fixed(&targets, &live);
            let bytes: u64 = categories.iter().map(|category| category.total_size).sum();
            println!(
                "  scan including directory sizes: {:?}, {}",
                start.elapsed(),
                quick_cleaner::core::model::fmt_size(bytes)
            );
        }
    }
}
