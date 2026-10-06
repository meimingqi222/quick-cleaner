use crate::core::i18n::Language;
/// 卸载确认的规则说明。
///
/// 只报规则 **id**：规则随程序版本发布，没有独立版本可供用户对照；快照序号
/// 是「计划必须来自同一份规则」的内部安全机制（`plan.rs` 的 observation 校验），
/// 不是用户概念。
pub fn tr_source_rule_plan(lang: Language, id: &str, targets: usize, preserve: &str) -> String {
    match lang {
        Language::Zh => format!("规则 {id}，{targets} 个程序与依赖目标。复核 → 官方卸载 → 补充清理 → 核验产物和登记 → 删除恢复记录。保留：{preserve}。共享依赖会再次复核。"),
        Language::En => format!("Rule {id}, {targets} program and dependency targets. Recheck → official uninstall → supplemental cleanup → verify artifacts and registrations → remove recovery records. Preserve: {preserve}. Shared dependencies are rechecked."),
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
