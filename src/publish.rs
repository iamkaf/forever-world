//! Prepare one release and hand the exact prepared bytes to each publisher.
//!
//! Resolution and archive creation stay local. Publisher modules only know how to
//! send files from [`PreparedRelease`]. This keeps a dry run useful and prevents
//! a platform adapter from quietly producing a different pack.

#[path = "publish_curseforge.rs"]
mod curseforge_adapter;
#[path = "publish_github.rs"]
mod github_adapter;
#[path = "publish_maven.rs"]
mod maven_adapter;
#[path = "publish_modrinth.rs"]
mod modrinth_adapter;

use crate::hash;
use crate::spec::Lockfile;
use crate::{PackRoot, Result, USER_AGENT};
use serde::{Deserialize, Deserializer, Serialize, de};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishMode {
    DryRun,
    Publish,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PublishConfig {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub changelog: Option<String>,
    #[serde(default)]
    pub modrinth: Option<ModrinthConfig>,
    #[serde(default, deserialize_with = "deserialize_curseforge")]
    pub curseforge: Option<CurseForgeConfig>,
    #[serde(default)]
    pub github: Option<GitHubConfig>,
    #[serde(default)]
    pub maven: Option<MavenConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModrinthConfig {
    pub project: String,
    #[serde(default = "default_release_type")]
    pub release_type: String,
    #[serde(default)]
    pub loaders: Vec<String>,
    #[serde(default)]
    pub game_versions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurseForgeConfig {
    pub project: u64,
    #[serde(default)]
    pub game_versions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubConfig {
    pub repository: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MavenConfig {
    pub repository: String,
}

fn default_release_type() -> String {
    "release".into()
}

fn deserialize_curseforge<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<CurseForgeConfig>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = toml::Value::deserialize(deserializer)?;
    match value {
        toml::Value::Boolean(false) => Ok(None),
        toml::Value::Table(_) => value.try_into().map(Some).map_err(de::Error::custom),
        toml::Value::Boolean(true) => Err(de::Error::custom(
            "publish.curseforge must be false or a table with a project ID",
        )),
        _ => Err(de::Error::custom(
            "publish.curseforge must be false or a table with a project ID",
        )),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactKind {
    Modrinth,
    CurseForge,
    Maven,
    MavenMetadata,
    Manifest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Artifact {
    pub name: String,
    pub path: PathBuf,
    pub checksum: PathBuf,
    pub kind: ArtifactKind,
    pub bytes: u64,
    pub sha512: String,
}

#[derive(Debug, Clone)]
pub struct PreparedRelease {
    pub lock: Lockfile,
    pub config: PublishConfig,
    pub artifacts: Vec<Artifact>,
    pub manifest: PathBuf,
}

impl PreparedRelease {
    pub fn artifact(&self, kind: ArtifactKind) -> Result<&Artifact> {
        self.artifacts
            .iter()
            .find(|artifact| artifact.kind == kind)
            .ok_or_else(|| format!("prepared release is missing a {kind:?} artifact").into())
    }

    pub fn changelog(&self, root: &PackRoot) -> Result<String> {
        let relative = self.config.changelog.as_deref().unwrap_or("CHANGELOG.md");
        crate::spec::check_pack_path(relative)?;
        let path = root.path.join(relative);
        fs::read_to_string(&path).map_err(|error| {
            format!("cannot read publish changelog {}: {error}", path.display()).into()
        })
    }
}

#[derive(Debug, Clone, Serialize)]
struct ReleaseManifest {
    schema_version: u32,
    name: String,
    slug: String,
    version: String,
    minecraft: String,
    loader: String,
    loader_version: String,
    artifacts: Vec<ManifestArtifact>,
}

#[derive(Debug, Clone, Serialize)]
struct ManifestArtifact {
    name: String,
    kind: ArtifactKind,
    bytes: u64,
    sha512: String,
}

/// Resolve all local release bytes and write the release manifest.
pub fn prepare(root: &PackRoot, mode: PublishMode) -> Result<PreparedRelease> {
    let lock = crate::load_lock(root)?;
    let spec = crate::load_spec(root)?;
    if !crate::resolve::lock_matches_spec(&spec, &lock) {
        return Err(
            "pack.toml changed since the last install; run `pack install` before publishing".into(),
        );
    }
    let config = load_config(root)?;
    fs::create_dir_all(root.dist_dir())?;

    let mrpack = crate::export::export(root)?;
    let mut artifacts = vec![artifact(&mrpack, ArtifactKind::Modrinth)?];
    if config.curseforge.is_some() {
        let curseforge = crate::curseforge::export(root)?;
        artifacts.push(artifact(&curseforge, ArtifactKind::CurseForge)?);
    }
    if let Some(maven) = &config.maven {
        if !maven.repository.starts_with("https://") {
            return Err("publish.maven.repository must use HTTPS".into());
        }
        let pom_name = format!("{}-{}.pom", lock.pack.slug, lock.pack.version);
        let pom = root.dist_dir().join(&pom_name);
        fs::write(
            &pom,
            minimal_pom(
                &lock.pack.group,
                &lock.pack.slug,
                &lock.pack.version,
                &lock.pack.name,
                config.description.as_deref(),
            ),
        )?;
        artifacts.push(artifact(&pom, ArtifactKind::Maven)?);

        let metadata = root.dist_dir().join("maven-metadata.xml");
        fs::write(
            &metadata,
            prepare_maven_metadata(&lock, &maven.repository, mode)?,
        )?;
        artifacts.push(artifact(&metadata, ArtifactKind::MavenMetadata)?);
    }
    artifacts.sort_by(|left, right| left.name.cmp(&right.name));

    let manifest = root.dist_dir().join("release-manifest.json");
    let manifest_data = ReleaseManifest {
        schema_version: 1,
        name: lock.pack.name.clone(),
        slug: lock.pack.slug.clone(),
        version: lock.pack.version.clone(),
        minecraft: lock.pack.minecraft.clone(),
        loader: lock.pack.loader.clone(),
        loader_version: lock.pack.loader_version.clone(),
        artifacts: artifacts
            .iter()
            .map(|artifact| ManifestArtifact {
                name: artifact.name.clone(),
                kind: artifact.kind,
                bytes: artifact.bytes,
                sha512: artifact.sha512.clone(),
            })
            .collect(),
    };
    let mut bytes = serde_json::to_vec_pretty(&manifest_data)?;
    bytes.push(b'\n');
    fs::write(&manifest, bytes)?;
    write_checksum(&manifest)?;

    let release = PreparedRelease {
        lock,
        config,
        artifacts,
        manifest,
    };
    crate::verify::verify_prepared_release(&release)?;
    Ok(release)
}

/// Prepare once, then publish the same artifact bytes to every configured target.
pub fn publish(root: &PackRoot, mode: PublishMode) -> Result<Vec<String>> {
    let release = prepare(root, mode.clone())?;
    let mut output = Vec::new();
    if release.config.modrinth.is_some() {
        output.extend(if mode == PublishMode::DryRun {
            modrinth_adapter::dry_run(&release)?
        } else {
            modrinth_adapter::publish(&release, root)?
        });
    }
    if release.config.curseforge.is_some() {
        output.extend(if mode == PublishMode::DryRun {
            curseforge_adapter::dry_run(&release)?
        } else {
            curseforge_adapter::publish(&release, root)?
        });
    }
    if release.config.github.is_some() {
        output.extend(if mode == PublishMode::DryRun {
            github_adapter::dry_run(&release)?
        } else {
            github_adapter::publish(&release, root)?
        });
    }
    if release.config.maven.is_some() {
        output.extend(if mode == PublishMode::DryRun {
            maven_adapter::dry_run(&release)?
        } else {
            maven_adapter::publish(&release)?
        });
    }
    if output.is_empty() {
        output.push("prepared release locally; no publish targets are configured".into());
    }
    Ok(output)
}

pub(crate) fn load_config(root: &PackRoot) -> Result<PublishConfig> {
    let text = fs::read_to_string(root.pack_toml())?;
    let value: toml::Value =
        toml::from_str(&text).map_err(|error| crate::Error::from(format!("pack.toml: {error}")))?;
    let Some(table) = value.get("publish") else {
        return Ok(PublishConfig::default());
    };
    let config: PublishConfig = table
        .clone()
        .try_into()
        .map_err(|error| crate::Error::from(format!("pack.toml [publish]: {error}")))?;
    if let Some(modrinth) = &config.modrinth
        && !matches!(modrinth.release_type.as_str(), "release" | "beta" | "alpha")
    {
        return Err(format!(
            "publish.modrinth.release_type must be release, beta, or alpha, not `{}`",
            modrinth.release_type
        )
        .into());
    }
    Ok(config)
}

pub(crate) fn artifact(path: &Path, kind: ArtifactKind) -> Result<Artifact> {
    let bytes = fs::metadata(path)?.len();
    let data = fs::read(path)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            crate::Error::from(format!("invalid artifact filename: {}", path.display()))
        })?;
    Ok(Artifact {
        name: name.into(),
        path: path.to_path_buf(),
        checksum: write_checksum(path)?,
        kind,
        bytes,
        sha512: hash::sha512_hex(&data),
    })
}

pub(crate) fn artifact_checksum(artifact: &Artifact) -> Result<Artifact> {
    let bytes = fs::read(&artifact.checksum)?;
    let name = artifact
        .checksum
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| crate::Error::from("invalid checksum filename"))?;
    Ok(Artifact {
        name: name.into(),
        path: artifact.checksum.clone(),
        checksum: artifact.checksum.clone(),
        kind: artifact.kind,
        bytes: bytes.len() as u64,
        sha512: hash::sha512_hex(&bytes),
    })
}

pub(crate) fn write_checksum(path: &Path) -> Result<PathBuf> {
    let data = fs::read(path)?;
    let checksum = path.with_file_name(format!(
        "{}.sha512",
        path.file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| crate::Error::from("invalid artifact filename"))?
    ));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| crate::Error::from("invalid artifact filename"))?;
    fs::write(&checksum, format!("{}  {name}\n", hash::sha512_hex(&data)))?;
    Ok(checksum)
}

fn minimal_pom(
    group: &str,
    artifact: &str,
    version: &str,
    name: &str,
    description: Option<&str>,
) -> String {
    let description = description.unwrap_or("Minecraft modpack (.mrpack)");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0"
         xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
         xsi:schemaLocation="http://maven.apache.org/POM/4.0.0 https://maven.apache.org/xsd/maven-4.0.0.xsd">
  <modelVersion>4.0.0</modelVersion>
  <groupId>{}</groupId>
  <artifactId>{}</artifactId>
  <version>{}</version>
  <packaging>pom</packaging>
  <name>{}</name>
  <description>{}</description>
</project>
"#,
        xml(group),
        xml(artifact),
        xml(version),
        xml(name),
        xml(description)
    )
}

pub(crate) fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub(crate) fn http_client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(300))
        .build()?)
}

pub(crate) fn artifact_bytes(artifact: &Artifact) -> Result<Vec<u8>> {
    Ok(fs::read(&artifact.path)?)
}

#[derive(Debug, Default, Deserialize)]
struct ExistingMetadata {
    #[serde(default)]
    versioning: ExistingVersioning,
}

#[derive(Debug, Default, Deserialize)]
struct ExistingVersioning {
    #[serde(default)]
    versions: ExistingVersions,
}

#[derive(Debug, Default, Deserialize)]
struct ExistingVersions {
    #[serde(default)]
    version: Vec<String>,
}

fn prepare_maven_metadata(lock: &Lockfile, repository: &str, mode: PublishMode) -> Result<String> {
    let group_path = lock.pack.group.replace('.', "/");
    let url = format!(
        "{}/{}/{}/maven-metadata.xml",
        repository.trim_end_matches('/'),
        group_path,
        lock.pack.slug
    );
    let mut versions = BTreeSet::new();
    if mode == PublishMode::Publish {
        let username = std::env::var("MAVEN_PUBLISH_USERNAME")
            .map_err(|_| crate::Error::from("set MAVEN_PUBLISH_USERNAME"))?;
        let password = std::env::var("MAVEN_PUBLISH_PASSWORD")
            .map_err(|_| crate::Error::from("set MAVEN_PUBLISH_PASSWORD"))?;
        let response = http_client()?
            .get(&url)
            .basic_auth(username, Some(password))
            .send()?;
        if response.status().is_success() {
            let existing: ExistingMetadata =
                quick_xml::de::from_str(&response.text()?).map_err(crate::Error::from_display)?;
            versions.extend(existing.versioning.versions.version);
        } else if response.status() != reqwest::StatusCode::NOT_FOUND {
            return Err(
                format!("Maven metadata lookup failed: {url}: {}", response.status()).into(),
            );
        }
    }
    versions.insert(lock.pack.version.clone());
    let latest = versions
        .iter()
        .max_by(|left, right| compare_pack_versions(left, right))
        .cloned()
        .unwrap_or_else(|| lock.pack.version.clone());
    Ok(metadata_xml(
        &lock.pack.group,
        &lock.pack.slug,
        &latest,
        &versions.into_iter().collect::<Vec<_>>(),
    ))
}

fn compare_pack_versions(left: &str, right: &str) -> std::cmp::Ordering {
    let numbers = |value: &str| {
        let mut parts = value.split('.');
        let parsed = [
            parts.next().and_then(|part| part.parse::<u64>().ok()),
            parts.next().and_then(|part| part.parse::<u64>().ok()),
            parts.next().and_then(|part| part.parse::<u64>().ok()),
        ];
        (parts.next().is_none() && parsed.iter().all(Option::is_some))
            .then(|| parsed.map(Option::unwrap))
    };
    match (numbers(left), numbers(right)) {
        (Some(left), Some(right)) => left.cmp(&right),
        _ => left.cmp(right),
    }
}

fn metadata_xml(group: &str, artifact: &str, version: &str, versions: &[String]) -> String {
    let version_rows = versions
        .iter()
        .map(|value| format!("      <version>{}</version>\n", xml(value)))
        .collect::<String>();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<metadata>\n\
  <groupId>{}</groupId>\n\
  <artifactId>{}</artifactId>\n\
  <versioning>\n\
    <latest>{}</latest>\n\
    <release>{}</release>\n\
    <versions>\n\
{}\
    </versions>\n\
  </versioning>\n\
</metadata>\n",
        xml(group),
        xml(artifact),
        xml(version),
        xml(version),
        version_rows
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pom_does_not_mention_the_server_launcher() {
        let pom = minimal_pom("com.example", "pack", "1.0.0", "Pack", None);
        assert!(pom.contains("Minecraft modpack"));
        assert!(!pom.contains("Pastel"));
    }

    #[test]
    fn maven_metadata_keeps_existing_versions() {
        let metadata = metadata_xml(
            "com.example",
            "pack",
            "1.2.0",
            &["1.1.1".into(), "1.2.0".into()],
        );
        assert!(metadata.contains("<version>1.1.1</version>"));
        assert!(metadata.contains("<latest>1.2.0</latest>"));
        assert_eq!(
            compare_pack_versions("1.10.0", "1.9.0"),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn curseforge_can_be_explicitly_unconfigured() {
        let disabled: PublishConfig =
            toml::from_str("curseforge = false\n").expect("disabled CurseForge target");
        assert!(disabled.curseforge.is_none());

        let enabled: PublishConfig =
            toml::from_str("[curseforge]\nproject = 123\n").expect("configured CurseForge target");
        assert_eq!(
            enabled.curseforge.as_ref().map(|config| config.project),
            Some(123)
        );
    }
}
