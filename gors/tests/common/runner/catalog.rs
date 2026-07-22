use super::program_name;
use crate::common::TestConfig;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FixtureManifest {
    #[serde(default)]
    expected_program_count: Option<usize>,
    #[serde(default)]
    fixtures: BTreeMap<String, FixtureDirective>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureDirective {
    status: FixtureStatus,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    reason_file: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FixtureStatus {
    Run,
    Unsupported,
    CompileError,
}

#[derive(Debug)]
pub(super) struct FixtureCatalog {
    pub(super) runnable_dirs: Vec<PathBuf>,
    pub(super) all_program_names: BTreeSet<String>,
    pub(super) excluded_names: BTreeSet<String>,
}

pub(super) fn discover_program_dirs(
    fixture_root: &Path,
    config: &TestConfig,
) -> Result<FixtureCatalog, String> {
    let mut dirs = Vec::new();
    collect_program_dirs_recursive(fixture_root, &mut dirs)?;
    let manifest = load_fixture_manifest(fixture_root)?;
    let all_program_names = dirs
        .iter()
        .map(|path| program_name(fixture_root, path))
        .collect::<BTreeSet<_>>();
    validate_fixture_manifest(fixture_root, &manifest, &all_program_names)?;
    let excluded_names = manifest
        .fixtures
        .iter()
        .filter(|(name, directive)| {
            directive.status != FixtureStatus::Run && all_program_names.contains(*name)
        })
        .map(|(name, _)| name.clone())
        .collect::<BTreeSet<_>>();
    dirs.retain(|path| {
        let name = program_name(fixture_root, path);
        match manifest
            .fixtures
            .get(&name)
            .map(|directive| directive.status)
        {
            None => true,
            Some(FixtureStatus::Run) => true,
            Some(FixtureStatus::Unsupported) => config.include_unsupported,
            Some(FixtureStatus::CompileError) => false,
        }
    });
    dirs.retain(|path| program_matches_filter(fixture_root, path, config.filter.as_deref()));
    dirs.sort();
    if let Some(limit) = config.limit {
        dirs.truncate(limit);
    }
    Ok(FixtureCatalog {
        runnable_dirs: dirs,
        all_program_names,
        excluded_names,
    })
}

fn collect_program_dirs_recursive(dir: &Path, dirs: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("{}: {error}", dir.display()))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path.join("main.go").exists() {
            dirs.push(path.clone());
        }
        collect_program_dirs_recursive(&path, dirs)?;
    }
    Ok(())
}

fn load_fixture_manifest(fixture_root: &Path) -> Result<FixtureManifest, String> {
    let path = fixture_root.join("fixtures.json");
    if !path.exists() {
        return Ok(FixtureManifest::default());
    }
    serde_json::from_str(
        &fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?,
    )
    .map_err(|error| format!("{}: {error}", path.display()))
}

fn validate_fixture_manifest(
    fixture_root: &Path,
    manifest: &FixtureManifest,
    program_names: &BTreeSet<String>,
) -> Result<(), String> {
    if let Some(expected) = manifest.expected_program_count
        && program_names.len() != expected
    {
        return Err(format!(
            "fixtures.json expected {expected} program directories containing main.go, discovered {}; update the inventory only when adding or removing an intentional fixture",
            program_names.len()
        ));
    }
    for name in program_names {
        let has_private_component = Path::new(name).components().any(|component| {
            component
                .as_os_str()
                .to_str()
                .is_some_and(|part| part.starts_with('_'))
        });
        if has_private_component && !manifest.fixtures.contains_key(name) {
            return Err(format!(
                "{name}: underscore-prefixed fixtures require an explicit fixtures.json status"
            ));
        }
    }
    for (name, directive) in &manifest.fixtures {
        if !program_names.contains(name) {
            return Err(format!(
                "{name}: fixtures.json entry does not reference a directory containing main.go"
            ));
        }
        if directive.status != FixtureStatus::Run {
            let reason = if let Some(reason_file) = &directive.reason_file {
                let path = fixture_root.join(reason_file);
                fs::read_to_string(&path).map_err(|error| {
                    format!("cannot read reason file {}: {error}", path.display())
                })?
            } else {
                directive.reason.clone()
            };
            if reason.trim().is_empty() {
                return Err(format!(
                    "{name}: non-running fixture status {:?} requires a non-empty reason or reasonFile",
                    directive.status
                ));
            }
        }
    }
    Ok(())
}

fn program_matches_filter(fixture_root: &Path, path: &Path, filter: Option<&str>) -> bool {
    filter.is_none_or(|filter| {
        path.strip_prefix(fixture_root)
            .ok()
            .and_then(|relative| relative.to_str())
            .or_else(|| path.file_name().and_then(|name| name.to_str()))
            .is_some_and(|name| name.contains(filter))
    })
}
