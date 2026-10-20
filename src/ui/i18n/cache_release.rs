use crate::core::{cache_release::ReleasePlan, i18n::Language};

pub fn tr_release_cache(lang: Language, force: bool) -> &'static str {
    match (lang, force) {
        (Language::Zh, false) => "退出应用后清理",
        (Language::En, false) => "Quit apps and clean",
        (Language::Zh, true) => "强制结束后清理",
        (Language::En, true) => "Force quit and clean",
    }
}

pub fn tr_release_cache_detail(lang: Language, plan: &ReleasePlan, force: bool) -> String {
    let caution = match (lang, force) {
        (Language::Zh, false) => "将请求以下应用正常退出，可能出现保存提示。只有占用解除且安全复检通过后，才永久清空此缓存目录的内容。请先保存工作。",
        (Language::En, false) => "Ask these apps to quit normally; they may prompt you to save. Permanently empty this cache only after its owners release it and safety checks pass. Save your work first.",
        (Language::Zh, true) => "应用仍未释放占用。强制结束会丢失未保存的工作，也可能中断后台任务。取消可保留缓存；确认后仍需通过安全复检才会清理。",
        (Language::En, true) => "Apps still hold this cache. Force quitting can lose unsaved work and interrupt background tasks. Cancel to keep the cache; cleanup still requires fresh safety checks.",
    };
    let owners = plan
        .occupants
        .iter()
        .map(|owner| {
            format!(
                "{} · PID {}\n{}\n{}",
                owner.name,
                owner.pid,
                owner.executable.display(),
                owner.evidence.display()
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("{caution}\n\n{}\n\n{owners}", plan.item.path.display())
}

pub fn tr_release_cache_status(lang: Language, stage: &str, detail: &str) -> String {
    let label = match (lang, stage) {
        (Language::Zh, "probe") => "正在确认缓存占用者",
        (Language::En, "probe") => "Checking cache owners",
        (Language::Zh, "wait") => "正在等待应用释放缓存",
        (Language::En, "wait") => "Waiting for apps to release the cache",
        (Language::Zh, "done") => "缓存清理完成，释放",
        (Language::En, "done") => "Cache cleaned; freed",
        (Language::Zh, "busy") => "缓存仍被占用，已保留；请退出应用后重试",
        (Language::En, "busy") => "Cache remains in use and was kept; quit the apps and retry",
        (Language::Zh, _) => "未完成清理，目标已保留",
        (Language::En, _) => "Cleanup incomplete; target retained",
    };
    if detail.is_empty() {
        label.into()
    } else {
        format!("{label} · {detail}")
    }
}
