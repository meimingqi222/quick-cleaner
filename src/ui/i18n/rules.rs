use crate::core::i18n::Language;
pub fn tr_source_rule_plan(
    lang: Language,
    id: &str,
    version: u32,
    sequence: u64,
    targets: usize,
    preserve: &str,
) -> String {
    match lang {
        Language::Zh => format!("规则 {id} r{version}（规则包 v{sequence}），{targets} 个程序与依赖目标。复核 → 官方卸载 → 补充清理 → 核验产物和登记 → 删除恢复记录。保留：{preserve}。共享依赖会再次复核。"),
        Language::En => format!("Rule {id} r{version} (bundle v{sequence}), {targets} program and dependency targets. Recheck → official uninstall → supplemental cleanup → verify artifacts and registrations → remove recovery records. Preserve: {preserve}. Shared dependencies are rechecked."),
    }
}
pub fn tr_rules_conflict(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "规则约束冲突，已阻止清理",
        Language::En => "Conflicting rule constraints; cleanup blocked",
    }
}
pub fn tr_source_execution_result(lang: Language, succeeded: usize, total: usize) -> String {
    match lang {
        Language::Zh => format!("卸载步骤：{succeeded}/{total} 已完成"),
        Language::En => format!("Uninstall steps: {succeeded}/{total} completed"),
    }
}
pub fn tr_rules_version(lang: Language, sequence: u64) -> String {
    match lang {
        Language::Zh => format!("清理规则 v{sequence}"),
        Language::En => format!("Cleanup rules v{sequence}"),
    }
}
pub fn tr_rules_bundled(lang: Language) -> &'static str {
    match lang {
        Language::Zh => "规则随程序更新，扫描与清理使用内置规则",
        Language::En => "Rules ship with app updates; scans and cleanup use bundled rules",
    }
}
