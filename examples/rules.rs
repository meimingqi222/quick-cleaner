//! Validate, package and explain rules shipped with the application.
use quick_cleaner::core::rules::{self, RuleBundle};
use std::path::Path;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let mode = args.first().map(String::as_str).unwrap_or("check");
    let sequence = args
        .get(1)
        .map(|s| s.parse::<u64>())
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or(1);
    let bundle = RuleBundle::from_directory(Path::new("rules"), sequence)?;
    let bytes = serde_json::to_vec(&bundle).map_err(|e| e.to_string())?;
    match mode {
        "check" => println!(
            "schema={} sequence={} rules={} bytes={}",
            bundle.schema,
            bundle.sequence,
            bundle.rules.len(),
            bytes.len()
        ),
        "pack" => {
            let output = Path::new(
                args.get(2)
                    .ok_or("usage: cargo run --example rules -- pack SEQUENCE OUTPUT")?,
            );
            std::fs::create_dir_all(output).map_err(|e| e.to_string())?;
            std::fs::write(output.join("rules.json"), bytes).map_err(|e| e.to_string())?;
        }
        "explain" => {
            let source = Path::new(
                args.get(2)
                    .ok_or("explain SEQUENCE RULE_TOML FIXTURE_ROOT")?,
            );
            let root = Path::new(args.get(3).ok_or("Missing isolated fixture root")?);
            let rule: rules::RuleDefinition =
                toml::from_str(&std::fs::read_to_string(source).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            let mut validation = bundle.clone();
            validation.rules.retain(|r| r.id != rule.id);
            validation.rules.push(rule.clone());
            validation.validate()?;
            let snapshot = std::sync::Arc::new(rules::RuleSnapshot { bundle: validation });
            let reference = rules::RuleRef {
                snapshot: snapshot.clone(),
                id: rule.id.clone(),
                scope: Some(root.to_path_buf()),
                contributors: Vec::new(),
                blocked: None,
                observation: None,
            }
            .observed();
            let observation = reference
                .observation
                .as_ref()
                .ok_or("Missing scan observation")?;
            let variables = observation.variables.clone();
            let facts = observation.facts.clone();
            let detected = observation.detected;
            let mut blocked = Vec::new();
            if detected != rules::facts::Evidence::Confirmed {
                blocked.push("Ownership evidence absent or unknown".into());
            }
            let preserve: Vec<_> = rule
                .preserve
                .iter()
                .chain(rule.app.iter().flat_map(|app| &app.preserve))
                .flat_map(|path| match rules::variables::paths(path, &variables) {
                    Ok(paths) => paths
                        .into_iter()
                        .map(|path| root.join(path))
                        .collect::<Vec<_>>(),
                    Err(reason) => {
                        blocked.push(reason);
                        vec![root.to_path_buf()]
                    }
                })
                .collect();
            let mut planned = Vec::new();
            let mut recommendations = Vec::new();
            for entry in &rule.entries {
                match rules::variables::paths(&entry.path, &variables) {
                    Ok(members) => {
                        for relative in members {
                            let path = root.join(relative);
                            planned.push(rules::PlannedTarget {
                                operation: entry.operation.operation().for_scanned_path(&path),
                                identity: quick_cleaner::core::model::capture_identity(&path),
                                path,
                                disposal: entry.disposal,
                            });
                            recommendations.push(entry.recommended);
                        }
                    }
                    Err(reason) => blocked.push(reason),
                }
            }
            let directory_discovery = rules::directories::explain_at(&snapshot, &rule.id, root);
            let version_discovery = rules::versions::explain_at(&snapshot, &rule.id, root);
            let mut plan = rules::CleanupPlan::new(reference, planned);
            plan.blocked.extend(blocked.iter().cloned());
            if let Err(reason) = plan.validate() {
                blocked.push(reason);
            }
            let mut cleanup_plan = plan.explanation();
            let mut targets = cleanup_plan["targets"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            for (target, recommended) in targets.iter_mut().zip(recommendations) {
                target["recommended"] = serde_json::json!(recommended);
                target["identity_confirmed"] = target["identity_captured"].clone();
            }
            #[cfg(windows)]
            let installation = if rule.app.is_some() {
                rules::with_snapshot(snapshot, || {
                    match quick_cleaner::platform::windows::explain_source_install(root, &rule.id) {
                        Ok(plan) => Some(plan),
                        Err(reason) => {
                            blocked.push(reason);
                            None
                        }
                    }
                })
            } else {
                None
            };
            #[cfg(not(windows))]
            let installation: Option<serde_json::Value> = None;
            cleanup_plan["blocked"] = serde_json::json!(blocked);
            let result = serde_json::json!({"rule":rule.id,"sequence":sequence,"variables":variables,"facts":facts,"detected":detected,"targets":targets,"directory_discovery":directory_discovery,"version_discovery":version_discovery,"preserve":preserve,"cleanup_plan":cleanup_plan,"installation_plan":installation,"blocked":blocked,"installation_layout":rule.app});
            println!(
                "{}",
                serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?
            );
        }
        _ => return Err("Expected check, pack or explain".into()),
    }
    Ok(())
}
