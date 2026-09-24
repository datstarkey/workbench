//! Root `package.json` inspection for the Scripts sidebar: which package
//! manager a project uses and which scripts it defines, in file order.

use std::path::Path;

use anyhow::{Context, Result};
use serde::de::{Deserializer, MapAccess, Visitor};
use serde::Deserialize;

use crate::types::{PackageInfo, PackageScript};

const MANAGERS: [&str; 4] = ["bun", "pnpm", "yarn", "npm"];

/// Checked in order; the first lockfile present wins.
const LOCKFILES: [(&str, &str); 5] = [
    ("bun.lock", "bun"),
    ("bun.lockb", "bun"),
    ("pnpm-lock.yaml", "pnpm"),
    ("yarn.lock", "yarn"),
    ("package-lock.json", "npm"),
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackageJson {
    #[serde(default)]
    package_manager: Option<String>,
    #[serde(default, deserialize_with = "ordered_scripts")]
    scripts: Vec<PackageScript>,
}

/// Reads `<dir>/package.json`. `None` when the directory has none.
pub fn read(dir: &Path) -> Result<Option<PackageInfo>> {
    let file = dir.join("package.json");
    let raw = match std::fs::read_to_string(&file) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("reading {}", file.display())),
    };
    let pkg: PackageJson =
        serde_json::from_str(&raw).with_context(|| format!("parsing {}", file.display()))?;
    let (manager, manager_version, detected_from) =
        detect_manager(dir, pkg.package_manager.as_deref());
    Ok(Some(PackageInfo {
        manager: manager.to_string(),
        manager_version,
        detected_from,
        scripts: pkg.scripts,
    }))
}

/// `packageManager` field first, then lockfiles, then npm.
fn detect_manager(dir: &Path, field: Option<&str>) -> (&'static str, Option<String>, String) {
    if let Some((name, version)) = field.and_then(parse_package_manager) {
        return (name, version, "packageManager".to_string());
    }
    for (lockfile, name) in LOCKFILES {
        if dir.join(lockfile).is_file() {
            return (name, None, lockfile.to_string());
        }
    }
    ("npm", None, "default".to_string())
}

/// `"pnpm@9.1.0+sha512.abc"` -> `("pnpm", Some("9.1.0"))`. Unknown managers yield `None`.
fn parse_package_manager(field: &str) -> Option<(&'static str, Option<String>)> {
    let (name, version) = field.split_once('@').unwrap_or((field, ""));
    let name = MANAGERS.into_iter().find(|m| *m == name.trim())?;
    let version = version.split('+').next().unwrap_or_default().trim();
    Some((name, (!version.is_empty()).then(|| version.to_string())))
}

/// Script names are typed into the user's shell as `<manager> run <name>`
/// (sh, zsh, PowerShell or cmd), so only names no shell treats specially are
/// listed. A leading `-` would be parsed as a flag.
fn is_safe_script_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '-' | '_' | '.' | '/'))
}

/// `serde_json` maps are sorted, so read `scripts` entry by entry to keep the
/// order the author wrote them in.
fn ordered_scripts<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<PackageScript>, D::Error> {
    struct ScriptsVisitor;

    impl<'de> Visitor<'de> for ScriptsVisitor {
        type Value = Vec<PackageScript>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a map of script names to commands")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut scripts = Vec::new();
            while let Some((name, command)) = map.next_entry::<String, serde_json::Value>()? {
                if let (true, Some(command)) = (is_safe_script_name(&name), command.as_str()) {
                    scripts.push(PackageScript {
                        name,
                        command: command.to_string(),
                    });
                }
            }
            Ok(scripts)
        }
    }

    d.deserialize_map(ScriptsVisitor)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(package_json: &str, lockfiles: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), package_json).unwrap();
        for lockfile in lockfiles {
            std::fs::write(dir.path().join(lockfile), "").unwrap();
        }
        dir
    }

    #[test]
    fn missing_package_json_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read(dir.path()).unwrap(), None);
    }

    #[test]
    fn package_manager_field_wins_over_lockfile() {
        let dir = project(
            r#"{"packageManager":"pnpm@9.1.0+sha512.abc"}"#,
            &["bun.lock"],
        );
        let info = read(dir.path()).unwrap().unwrap();
        assert_eq!(info.manager, "pnpm");
        assert_eq!(info.manager_version.as_deref(), Some("9.1.0"));
        assert_eq!(info.detected_from, "packageManager");
    }

    #[test]
    fn falls_back_to_lockfile_then_npm() {
        let dir = project("{}", &["yarn.lock"]);
        let info = read(dir.path()).unwrap().unwrap();
        assert_eq!(
            (info.manager.as_str(), info.detected_from.as_str()),
            ("yarn", "yarn.lock")
        );

        let dir = project(r#"{"packageManager":"deno@2"}"#, &[]);
        let info = read(dir.path()).unwrap().unwrap();
        assert_eq!(
            (info.manager.as_str(), info.detected_from.as_str()),
            ("npm", "default")
        );
    }

    #[test]
    fn scripts_keep_file_order_and_drop_unsafe_names() {
        let dir = project(
            r#"{"scripts":{"dev":"vite","build":"vite build","test:unit":"vitest",
                "x; rm -rf ~":"echo","--flag":"echo","bad":7}}"#,
            &[],
        );
        let names: Vec<_> = read(dir.path())
            .unwrap()
            .unwrap()
            .scripts
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, ["dev", "build", "test:unit"]);
    }

    #[test]
    fn invalid_json_is_an_error() {
        let dir = project("{", &[]);
        assert!(read(dir.path()).is_err());
    }
}
