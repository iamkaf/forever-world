use crate::spec::{ContentKind, ContentSide};
use crate::{PackRoot, Result, curseforge, fetch, load_lock, load_spec, overlay, resolve};
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddOptions {
    pub kind: ContentKind,
    pub side: Option<ContentSide>,
}

impl Default for AddOptions {
    fn default() -> Self {
        Self {
            kind: ContentKind::Mod,
            side: None,
        }
    }
}

pub fn add(
    root: &PackRoot,
    query: &str,
    requested_version: Option<&str>,
    options: AddOptions,
) -> Result<String> {
    let spec = load_spec(root)?;
    if spec
        .content()
        .any(|content| content.id.eq_ignore_ascii_case(query))
    {
        return Err(format!("{} is already in pack.toml", query.trim()).into());
    }

    let resolver = resolve::Resolver::new()?;
    let project = resolver.find_project(query)?;
    if spec.content().any(|content| content.id == project) {
        return Err(format!("{project} is already in pack.toml").into());
    }
    let version = match requested_version {
        Some(version) if !version.trim().is_empty() => version.to_string(),
        _ => resolver.latest_version(&spec.pack, options.kind, &project)?,
    };
    let detected_side = resolver.project_side(&project, options.kind)?;
    let side = if options.kind == ContentKind::Shader {
        ContentSide::Client
    } else {
        options.side.unwrap_or(detected_side)
    };
    append_modrinth(root, options.kind, &project, &version, side)?;
    Ok(project)
}

pub fn remove(root: &PackRoot, query: &str) -> Result<()> {
    let text = fs::read_to_string(root.pack_toml())?;
    let (updated, removed) = remove_entry(&text, query)?;
    if !removed {
        return Err(format!("{query} is not in pack.toml").into());
    }
    crate::spec::PackSpec::parse(&updated)?;
    fs::write(root.pack_toml(), updated)?;
    Ok(())
}

pub fn install(root: &PackRoot) -> Result<InstallReport> {
    let spec = load_spec(root)?;
    let lock = match load_lock(root) {
        Ok(lock) if resolve::lock_matches_spec(&spec, &lock) => lock,
        Ok(_) | Err(_) => resolve::resolve_pack(root)?,
    };
    fetch::ensure_all(root, &lock.file)?;
    curseforge::ensure_mappings(root)?;
    let generated = overlay::overlay(root)?;
    Ok(InstallReport {
        files: lock.file.len(),
        generated,
    })
}

#[derive(Debug, Clone)]
pub struct InstallReport {
    pub files: usize,
    pub generated: std::path::PathBuf,
}

pub fn run(root: &PackRoot, target: RunTarget) -> Result<()> {
    let lock = installed_lock(root)?;
    for file in &lock.file {
        fetch::check_cached(root, file)?;
    }
    let generated = overlay::overlay(root)?;
    match target {
        RunTarget::Client => modstage(
            root,
            &generated,
            "client",
            &format!("{}-client", lock.pack.slug),
        ),
        RunTarget::Server => modstage(
            root,
            &generated,
            "server",
            &format!("{}-server", lock.pack.slug),
        ),
        RunTarget::Pair => teakit_pair(root, &lock),
    }
}

fn teakit_pair(root: &PackRoot, lock: &crate::spec::Lockfile) -> Result<()> {
    let node = format!("{}-{}", lock.pack.minecraft, lock.pack.loader);
    let instance = format!("{}-pair", lock.pack.slug);
    let status = Command::new("./teakitw")
        .args([
            "pair",
            "--no-sync-sdk",
            "--node",
            &node,
            "--modstage-config",
            "generated/modstage.toml",
            "--modstage-instance",
            &instance,
            "--test-file",
            "tests/teakit/startup.test.ts",
            "--timeout",
            "360",
            "--report",
            "build/teakit/startup.json",
        ])
        .current_dir(&root.path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|error| format!("could not start TeaKit pair runner: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("TeaKit pair runner exited with {status}").into())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunTarget {
    Client,
    Server,
    Pair,
}

fn installed_lock(root: &PackRoot) -> Result<crate::spec::Lockfile> {
    let spec = load_spec(root)?;
    let lock = load_lock(root).map_err(|error| {
        crate::Error::from(format!(
            "{error}; run `pack install` before running the pack"
        ))
    })?;
    if !resolve::lock_matches_spec(&spec, &lock) {
        return Err("pack.toml changed since the last install; run `pack install`".into());
    }
    Ok(lock)
}

fn modstage(root: &PackRoot, config: &Path, side: &str, instance: &str) -> Result<()> {
    let status = Command::new("modstage")
        .args([
            "--config",
            config.to_str().unwrap_or("generated/modstage.toml"),
            "run",
            side,
            instance,
            "--timeout",
            "180s",
        ])
        .current_dir(&root.path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|error| format!("could not start modstage: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("modstage exited with {status}").into())
    }
}

fn append_modrinth(
    root: &PackRoot,
    kind: ContentKind,
    project: &str,
    version: &str,
    side: ContentSide,
) -> Result<()> {
    let mut text = fs::read_to_string(root.pack_toml())?;
    let section = match kind {
        ContentKind::Mod if side == ContentSide::Client => "client_mods",
        ContentKind::Mod => "mods",
        ContentKind::Shader => "shaders",
    };
    let header = format!("[{section}]\n");
    let line = format!("{} = {}\n", project, toml_string(version));
    if let Some(header_start) = text.find(&header) {
        let content_start = header_start + header.len();
        let insertion = text[content_start..]
            .find("\n[")
            .map(|offset| content_start + offset + 1)
            .unwrap_or(text.len());
        text.insert_str(insertion, &line);
    } else {
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push('\n');
        text.push_str(&header);
        text.push_str(&line);
    }
    crate::spec::PackSpec::parse(&text)?;
    fs::write(root.pack_toml(), text)?;
    Ok(())
}

fn remove_entry(text: &str, query: &str) -> Result<(String, bool)> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    for (index, line) in lines.iter().enumerate() {
        if !matches!(line.trim(), "[mods]" | "[client_mods]" | "[shaders]") {
            continue;
        }
        let end = lines[index + 1..]
            .iter()
            .position(|line| line.trim_start().starts_with('['))
            .map(|offset| index + 1 + offset)
            .unwrap_or(lines.len());
        for (entry, candidate) in lines[index + 1..end].iter().enumerate() {
            let Some((id, _)) = candidate.split_once('=') else {
                continue;
            };
            if id
                .trim()
                .trim_matches('"')
                .eq_ignore_ascii_case(query.trim())
            {
                let mut updated = lines[..index + 1].concat();
                updated.push_str(&lines[index + 1..index + 1 + entry].concat());
                updated.push_str(&lines[index + 2 + entry..].concat());
                return Ok((updated, true));
            }
        }
    }

    Ok((text.to_string(), false))
}

fn toml_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('\"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_unknown_content_when_removing_missing_project() {
        let text = "format = 1\n\n[mods]\nsodium = \"1\"\n";
        let (updated, removed) = remove_entry(text, "iris").expect("remove");
        assert!(!removed);
        assert_eq!(updated, text);
    }

    #[test]
    fn removes_a_project_from_the_keyed_manifest() {
        let text = "format = 1\n\n[mods]\ncreate = \"1\"\n\n[client_mods]\nsodium = \"2\"\n\n[shaders]\ncomplementary-unbound = \"3\"\n";
        let (updated, removed) = remove_entry(text, "sodium").expect("remove");
        assert!(removed);
        assert!(!updated.contains("sodium"));
        assert!(updated.contains("create = \"1\""));
        assert!(updated.contains("complementary-unbound"));
    }
}
