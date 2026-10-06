//! macOS 命令行输出解析器（`launchctl` / `codesign` / `systemextensionsctl`）。
//!
//! 这些解析是**纯字符串逻辑**，与 macOS API 无关。放在 `core` 而不是
//! `platform::macos` 是为了让它们的单元测试在 Windows 上也编译、执行——
//! `platform::macos` 整体被 `#[cfg(target_os = "macos")]` 门控，纯逻辑
//! 不该被平台门一起挡在 Windows 测试之外。

/// 从带引号的 `"标签" => …` 行里取标签。
pub(crate) fn quoted_label(line: &str) -> Option<String> {
    let start = line.find('"')?;
    let rest = &line[start + 1..];
    let end = rest.find('"')?;
    let label = &rest[..end];
    (!label.is_empty()).then(|| label.to_string())
}

/// 不分配的 ASCII 大小写不敏感包含判断；`needle_lower` 必须已是小写。
pub(crate) fn contains_ignore_ascii_case(haystack: &str, needle_lower: &str) -> bool {
    let (hay, needle) = (haystack.as_bytes(), needle_lower.as_bytes());
    !needle.is_empty()
        && needle.len() <= hay.len()
        && hay
            .windows(needle.len())
            .any(|w| w.eq_ignore_ascii_case(needle))
}

/// 名字是不是形态合法的 Bundle ID / 反向域名（长度、含点、无分隔符、字符集）。
pub(crate) fn valid_bundle_id(id: &str) -> bool {
    id.len() >= 3
        && id.contains('.')
        && !id.contains('/')
        && !id.contains('\\')
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

/// 从 `launchctl print gui/<uid>` 的输出里解析这款软件在 launchd 的驻留
/// 证据，返回仍登记**且未被禁用**的任务标签。`needle` 必须已是小写。
///
/// 输出有两个相关段落，形状不同，取标签的方式必须分开：
///
/// - `services = {` 摘要段（域内全部已登记任务）：行形如
///   `<pid> <上次退出状态> <标签>`，**不带引号**。首 token 是 pid（数字）
///   或 `-`，标签取最后一个空白分隔 token。本机实测这个段有 488 个任务，
///   旧解析器因要求引号整段漏掉。
/// - 末尾 `disabled services = {` 段：行形如 `"标签" => enabled|disabled`。
///   只对 `=> enabled` 报警——disabled 意味着不会自启，把它报成「后台
///   仍在运行」会得出与事实相反的结论（iStat Menus 实测：两个任务均已
///   disable、ps 零进程，旧解析器照样亮警示）。
///
/// 已登记但此刻 pid 为 0（未在跑）的也报：开机自启随时会把它拉起来，
/// 用户该知道「清了还会回来」。
pub(crate) fn parse_launchd_registered(output: &str, needle: &str) -> Vec<String> {
    if needle.is_empty() {
        return Vec::new();
    }
    let mut labels: Vec<String> = Vec::new();
    // `None` = 不在关心的段落里；`Some(true)` = disabled 段。三种状态用一个
    // 枚举值表示，免得两个 bool 之间出现「都为真」这种不存在的组合。
    let mut section: Option<bool> = None;
    for line in output.lines() {
        let trimmed = line.trim_start();
        match trimmed {
            "services = {" => {
                section = Some(false);
                continue;
            }
            "disabled services = {" => {
                section = Some(true);
                continue;
            }
            "}" => {
                section = None;
                continue;
            }
            _ => {}
        }
        // 两个分支只负责「把标签抠出来」，命中判定与去重共用下面一条尾巴。
        let label = match section {
            // `"标签" => enabled|disabled`
            Some(true) => trimmed
                .contains("=> enabled")
                .then(|| quoted_label(trimmed))
                .flatten(),
            // `<pid|-> <状态> <标签>`
            Some(false) => {
                let mut tokens = trimmed.split_whitespace();
                let first = tokens.next().unwrap_or_default();
                let is_pid = first == "-" || first.bytes().all(|b| b.is_ascii_digit());
                // 至少三段才是一条记录：少了说明是表头或分隔行。
                (is_pid && !first.is_empty() && tokens.clone().count() >= 2)
                    .then(|| tokens.next_back().map(str::to_string))
                    .flatten()
            }
            None => None,
        };
        let Some(label) = label else { continue };
        if contains_ignore_ascii_case(&label, needle) && !labels.contains(&label) {
            labels.push(label);
        }
    }
    labels
}

/// 从 `codesign -d --entitlements :-` 的输出里取签名的 App Group（只保留
/// 形态合法的反向域名，去重）。
pub(crate) fn parse_application_groups(entitlements: &str) -> Vec<String> {
    let Some((_, after_key)) =
        entitlements.split_once("<key>com.apple.security.application-groups</key>")
    else {
        return Vec::new();
    };
    let Some((array, _)) = after_key.split_once("</array>") else {
        return Vec::new();
    };

    let mut groups = Vec::new();
    let mut rest = array;
    while let Some((_, after_open)) = rest.split_once("<string>") {
        let Some((value, after_close)) = after_open.split_once("</string>") else {
            break;
        };
        if valid_bundle_id(value) && !groups.iter().any(|known| known == value) {
            groups.push(value.to_string());
        }
        rest = after_close;
    }
    groups
}

/// 从 `systemextensionsctl list` 的输出里取已激活的系统扩展
/// `(teamID, bundleID)`。只收 `[activated …]` 的行——已经
/// terminated/uninstalling 的不是残留。
pub(crate) fn parse_system_extensions(output: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for line in output.lines() {
        if !line.contains("[activated") {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        // 表头之外的数据行至少有 enabled/active/teamID/bundleID 四列
        if fields.len() < 4 {
            continue;
        }
        let team_id = fields[2].trim();
        // bundleID 那列后面跟着 " (版本)"，切掉
        let bundle_id = fields[3].split_whitespace().next().unwrap_or_default();
        if team_id.is_empty() || !valid_bundle_id(bundle_id) {
            continue;
        }
        found.push((team_id.to_string(), bundle_id.to_string()));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_activated_system_extensions_only() {
        // 取自真机 `systemextensionsctl list` 的输出形状
        let output = "1 extension(s)\n--- com.apple.system_extension.driver_extension\nenabled\tactive\tteamID\tbundleID (version)\tname\t[state]\n*\t*\tG43BCU2T37\torg.pqrs.Karabiner-DriverKit-VirtualHIDDevice (1.6.0/1.6.0)\torg.pqrs.Karabiner-DriverKit-VirtualHIDDevice\t[activated enabled]\n*\t*\tXXXXXXXXXX\tcom.other.gone (1.0/1.0)\tcom.other.gone\t[terminated waiting to uninstall on reboot]\n";

        let found = parse_system_extensions(output);
        assert_eq!(
            found,
            [(
                "G43BCU2T37".to_string(),
                "org.pqrs.Karabiner-DriverKit-VirtualHIDDevice".to_string()
            )],
            "只应收 activated 的扩展，表头和 terminated 行都要跳过"
        );
    }

    #[test]
    fn parses_only_signed_application_group_values() {
        let entitlements = r#"<plist><dict>
<key>com.apple.security.application-groups</key><array>
<string>group.com.example.app</string>
<string>group.com.example.shared</string>
</array><key>other</key><array><string>com.unrelated.value</string></array>
</dict></plist>"#;

        assert_eq!(
            parse_application_groups(entitlements),
            ["group.com.example.app", "group.com.example.shared"],
            "只应取 application-groups 下的合法值，other 键下的不算"
        );
    }

    #[test]
    fn launchd_parse_covers_services_section_and_ignores_disabled() {
        // 形状取自真机 `launchctl print gui/<uid>`：services 摘要段不带
        // 引号（<pid> <状态> <标签>），末尾 disabled 段才带引号。
        let output = "\
	services = {
	   61288      - 	application.com.quickcleaner.app.269898235.269898241
	       0      0 	com.bjango.istatmenus.helper
	     727      - 	com.apple.syncdefaultsd
	}
	some other section = {
	   1      - 	com.bjango.istatmenus.outside
	}
	disabled services = {
		\"com.bjango.istatmenus.agent\" => disabled
		\"com.bjango.istatmenus.status\" => disabled
		\"com.bjango.istatmenus.updater\" => enabled
	}
";
        let found = parse_launchd_registered(output, "com.bjango.istatmenus");
        // services 段：登记即报（pid 0 也是登记着、随时会被拉起）
        assert!(found.contains(&"com.bjango.istatmenus.helper".to_string()));
        // 别的段落不误收
        assert!(!found.contains(&"com.bjango.istatmenus.outside".to_string()));
        // disabled 段：只有 => enabled 才报——把已禁用报成「仍在运行」
        // 会得出与事实相反的结论
        assert!(!found.contains(&"com.bjango.istatmenus.agent".to_string()));
        assert!(!found.contains(&"com.bjango.istatmenus.status".to_string()));
        assert!(found.contains(&"com.bjango.istatmenus.updater".to_string()));
        assert_eq!(found.len(), 2);
    }
}
