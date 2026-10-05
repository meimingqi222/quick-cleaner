//! A bounded dependency graph extracts data; it never evaluates source code.
use super::facts::{self, Evidence};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Variable {
    Path { path: String },
    CanonicalHash { path: String, length: usize },
    JsonField { path: String, pointer: String },
    JsonSet { path: String, pointer: String },
    Directories { path: String, limit: usize },
}
#[derive(Clone, Debug, Serialize)]
pub struct Resolution {
    pub evidence: Evidence,
    pub value: Option<Value>,
}
pub type Values = BTreeMap<String, Resolution>;

fn names(template: &str) -> Result<Vec<String>, String> {
    let mut rest = template;
    let mut names = Vec::new();
    while let Some(start) = rest.find("${") {
        rest = &rest[start + 2..];
        let end = rest.find('}').ok_or("Unclosed variable")?;
        let name = &rest[..end];
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err("Invalid variable reference".into());
        }
        names.push(name.into());
        rest = &rest[end + 1..];
    }
    Ok(names)
}
pub fn validate_template(
    template: &str,
    variables: &BTreeMap<String, Variable>,
) -> Result<(), String> {
    if !super::relative(template) {
        return Err("Invalid relative template".into());
    }
    for name in names(template)? {
        if !variables.contains_key(&name) {
            return Err("Unknown variable reference".into());
        }
    }
    Ok(())
}
impl Variable {
    fn path(&self) -> &str {
        match self {
            Self::Path { path }
            | Self::CanonicalHash { path, .. }
            | Self::JsonField { path, .. }
            | Self::JsonSet { path, .. }
            | Self::Directories { path, .. } => path,
        }
    }
}
pub fn validate(variables: &BTreeMap<String, Variable>) -> Result<Vec<String>, String> {
    if variables.len() > 256 {
        return Err("Variable resource limit".into());
    }
    for (name, variable) in variables {
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err("Invalid variable name".into());
        }
        validate_template(variable.path(), variables)?;
        match variable {
            Variable::CanonicalHash { length, .. } if !(8..=64).contains(length) => {
                return Err("Invalid digest length".into())
            }
            Variable::Directories { limit, .. } if !(1..=256).contains(limit) => {
                return Err("Directory enumeration limit".into())
            }
            Variable::JsonField { pointer, .. } | Variable::JsonSet { pointer, .. }
                if !pointer.starts_with('/') || pointer.len() > 1024 =>
            {
                return Err("Invalid JSON pointer".into())
            }
            _ => {}
        }
    }
    fn visit(
        name: &str,
        variables: &BTreeMap<String, Variable>,
        active: &mut BTreeSet<String>,
        done: &mut BTreeSet<String>,
        order: &mut Vec<String>,
    ) -> Result<(), String> {
        if done.contains(name) {
            return Ok(());
        }
        if active.len() >= 32 || !active.insert(name.into()) {
            return Err("Variable cycle or depth limit".into());
        }
        for dependency in names(variables[name].path())? {
            visit(&dependency, variables, active, done, order)?;
        }
        active.remove(name);
        done.insert(name.into());
        order.push(name.into());
        Ok(())
    }
    let mut order = Vec::new();
    let mut active = BTreeSet::new();
    let mut done = BTreeSet::new();
    for name in variables.keys() {
        visit(name, variables, &mut active, &mut done, &mut order)?;
    }
    Ok(order)
}

pub fn paths(template: &str, values: &Values) -> Result<Vec<String>, String> {
    let mut results = vec![template.to_owned()];
    for name in names(template)? {
        let resolution = values.get(&name).ok_or("Missing variable")?;
        if resolution.evidence != Evidence::Confirmed {
            return Err("Unconfirmed variable".into());
        }
        let value = resolution.value.as_ref().ok_or("Missing variable value")?;
        let replacements: Vec<&str> = match value {
            Value::String(value) => vec![value],
            Value::Array(values) => values
                .iter()
                .map(|value| value.as_str().ok_or("Non-string path member"))
                .collect::<Result<_, _>>()?,
            _ => return Err("Invalid path variable type".into()),
        };
        let mut expanded = BTreeSet::new();
        for path in &results {
            for replacement in &replacements {
                if !super::relative(replacement) || replacement.contains("${") {
                    return Err("Variable path escape".into());
                }
                let path = path.replace(&format!("${{{name}}}"), replacement);
                if !super::relative(&path) || expanded.len() >= 256 {
                    return Err("Path expansion limit or escape".into());
                }
                expanded.insert(path);
            }
        }
        results = expanded.into_iter().collect();
    }
    if results.iter().any(|path| !super::relative(path)) {
        return Err("Invalid resolved path".into());
    }
    Ok(results)
}

pub fn evaluate(variables: &BTreeMap<String, Variable>, root: &Path) -> Values {
    let Ok(order) = validate(variables) else {
        return Values::new();
    };
    let mut values = Values::new();
    let mut documents = BTreeMap::new();
    let mut remaining_bytes = 8 * 1024 * 1024;
    for name in order {
        let variable = &variables[&name];
        let result = (|| -> Result<Option<Value>, String> {
            let paths = paths(variable.path(), &values)?;
            if paths.len() != 1 {
                return Err("Extraction requires one path".into());
            }
            let relative = &paths[0];
            let target = root.join(relative.replace('\\', "/"));
            if !facts::confined_path(root, &target)? {
                return Ok(None);
            }
            let value = match variable {
                Variable::Path { .. } => Value::String(relative.clone()),
                Variable::CanonicalHash { length, .. } => {
                    let canonical = std::fs::canonicalize(&target).map_err(|e| e.to_string())?;
                    Value::String(
                        format!(
                            "{:x}",
                            Sha256::digest(crate::core::safety::norm(&canonical).as_bytes())
                        )[..*length]
                            .into(),
                    )
                }
                Variable::JsonField { pointer, .. } | Variable::JsonSet { pointer, .. } => {
                    if !facts::confined_file(root, &target)? {
                        return Err("JSON input is not a file".into());
                    }
                    let json: &Value = documents
                        .entry(target.clone())
                        .or_insert_with(|| {
                            let length =
                                std::fs::metadata(&target).map_err(|e| e.to_string())?.len();
                            if length > remaining_bytes as u64 {
                                return Err("Extraction byte budget exhausted".into());
                            }
                            let bytes = facts::read(&target)?;
                            if bytes.len() > remaining_bytes {
                                return Err("Extraction byte budget exhausted".into());
                            }
                            remaining_bytes -= bytes.len();
                            serde_json::from_slice(&bytes).map_err(|e| e.to_string())
                        })
                        .as_ref()
                        .map_err(Clone::clone)?;
                    let Some(value) = json.pointer(pointer) else {
                        return Ok(None);
                    };
                    if matches!(variable, Variable::JsonSet { .. }) {
                        let array = value.as_array().ok_or("Invalid JSON set")?;
                        if array.len() > 256
                            || array
                                .iter()
                                .any(|v| v.as_str().is_none_or(|s| s.len() > 1024))
                        {
                            return Err("JSON set limit or type".into());
                        }
                        let members: BTreeSet<_> = array.iter().filter_map(Value::as_str).collect();
                        Value::Array(
                            members
                                .into_iter()
                                .map(|s| Value::String(s.into()))
                                .collect(),
                        )
                    } else {
                        if value.as_str().is_none_or(|s| s.len() > 1024) {
                            return Err("JSON field limit or type".into());
                        }
                        value.clone()
                    }
                }
                Variable::Directories { limit, .. } => {
                    let mut members = Vec::new();
                    for (count, entry) in std::fs::read_dir(&target)
                        .map_err(|e| e.to_string())?
                        .enumerate()
                    {
                        if count >= *limit {
                            return Err("Directory enumeration exhausted".into());
                        }
                        let entry = entry.map_err(|e| e.to_string())?;
                        if facts::confined_path(root, &entry.path())?
                            && entry.file_type().map_err(|e| e.to_string())?.is_dir()
                        {
                            let name = entry
                                .file_name()
                                .into_string()
                                .map_err(|_| "Invalid directory name")?;
                            let member = format!("{relative}/{name}");
                            if !super::relative(&member) {
                                return Err("Invalid directory member".into());
                            }
                            members.push(Value::String(member));
                        }
                    }
                    members.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
                    Value::Array(members)
                }
            };
            Ok(Some(value))
        })();
        let resolution = match result {
            Ok(Some(value)) => Resolution {
                evidence: Evidence::Confirmed,
                value: Some(value),
            },
            Ok(None) => Resolution {
                evidence: Evidence::Absent,
                value: None,
            },
            Err(_) => Resolution {
                evidence: Evidence::Unknown,
                value: None,
            },
        };
        values.insert(name, resolution);
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extraction_graph_expands_only_confined_manifest_members() {
        let root = crate::core::testing::fixture("variable_graph");
        std::fs::create_dir_all(root.join("app/cache")).unwrap();
        std::fs::write(
            root.join("app/manifest.json"),
            br#"{"members":["cache","cache"],"version":"1.2"}"#,
        )
        .unwrap();
        let variables = BTreeMap::from([
            ("app".into(), Variable::Path { path: "app".into() }),
            (
                "members".into(),
                Variable::JsonSet {
                    path: "${app}/manifest.json".into(),
                    pointer: "/members".into(),
                },
            ),
            (
                "key".into(),
                Variable::CanonicalHash {
                    path: "${app}".into(),
                    length: 16,
                },
            ),
            (
                "dirs".into(),
                Variable::Directories {
                    path: "app".into(),
                    limit: 8,
                },
            ),
        ]);
        let values = evaluate(&variables, &root);
        assert_eq!(paths("${app}/${members}", &values).unwrap(), ["app/cache"]);
        assert_eq!(
            values["key"]
                .value
                .as_ref()
                .unwrap()
                .as_str()
                .unwrap()
                .len(),
            16
        );
        assert_eq!(paths("${dirs}", &values).unwrap(), ["app/cache"]);
        std::fs::write(
            root.join("app/manifest.json"),
            br#"{"members":["../outside"]}"#,
        )
        .unwrap();
        assert!(paths("${app}/${members}", &evaluate(&variables, &root)).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn cycles_unknown_inputs_and_partial_enumeration_never_authorize() {
        let cyclic = BTreeMap::from([
            (
                "a".into(),
                Variable::Path {
                    path: "${b}".into(),
                },
            ),
            (
                "b".into(),
                Variable::Path {
                    path: "${a}".into(),
                },
            ),
        ]);
        assert!(validate(&cyclic).is_err());
        assert!(validate(&BTreeMap::from([(
            "a".into(),
            Variable::Path {
                path: "${missing}".into()
            }
        )]))
        .is_err());
        let root = crate::core::testing::fixture("variable_limits");
        std::fs::create_dir_all(root.join("apps/a")).unwrap();
        std::fs::create_dir_all(root.join("apps/b")).unwrap();
        let variables = BTreeMap::from([(
            "apps".into(),
            Variable::Directories {
                path: "apps".into(),
                limit: 1,
            },
        )]);
        let values = evaluate(&variables, &root);
        assert_eq!(values["apps"].evidence, Evidence::Unknown);
        assert!(paths("${apps}", &values).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
