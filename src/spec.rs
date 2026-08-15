use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SideRequirement {
    Required,
    Optional,
    Unsupported,
}

impl SideRequirement {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Optional => "optional",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvSpec {
    pub client: SideRequirement,
    pub server: SideRequirement,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackMeta {
    pub name: String,
    pub slug: String,
    pub version: String,
    #[serde(default = "default_group")]
    pub group: String,
    pub minecraft: String,
    pub loader: String,
    pub loader_version: String,
}

fn default_group() -> String {
    "com.iamkaf.modpacks".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSpec {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub provider: SourceProvider,
    #[serde(default)]
    pub requested_version: String,
    pub path: String,
    pub file_size: u64,
    pub sha1: String,
    pub sha512: String,
    pub env: EnvSpec,
    pub downloads: Vec<String>,
}

impl FileSpec {
    pub fn validate(&self) -> Result<()> {
        validate_locked_file(self, 2)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SourceProvider {
    Modrinth,
    #[default]
    Direct,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ContentSide {
    #[default]
    Both,
    Client,
    Server,
}

impl ContentSide {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Both => "both",
            Self::Client => "client",
            Self::Server => "server",
        }
    }

    pub fn env(self) -> EnvSpec {
        match self {
            Self::Both => EnvSpec {
                client: SideRequirement::Required,
                server: SideRequirement::Required,
            },
            Self::Client => EnvSpec {
                client: SideRequirement::Required,
                server: SideRequirement::Unsupported,
            },
            Self::Server => EnvSpec {
                client: SideRequirement::Unsupported,
                server: SideRequirement::Required,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentKind {
    Mod,
    Shader,
}

impl ContentKind {
    pub fn section(self) -> &'static str {
        match self {
            Self::Mod => "mods",
            Self::Shader => "shaders",
        }
    }

    pub fn folder(self) -> &'static str {
        match self {
            Self::Mod => "mods",
            Self::Shader => "shaderpacks",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContentSource {
    Modrinth {
        modrinth: String,
        version: String,
    },
    Direct {
        id: String,
        version: String,
        filename: String,
        url: String,
    },
}

impl ContentSource {
    pub fn id(&self) -> &str {
        match self {
            Self::Modrinth { modrinth, .. } => modrinth,
            Self::Direct { id, .. } => id,
        }
    }

    pub fn version(&self) -> &str {
        match self {
            Self::Modrinth { version, .. } | Self::Direct { version, .. } => version,
        }
    }

    pub fn provider(&self) -> SourceProvider {
        match self {
            Self::Modrinth { .. } => SourceProvider::Modrinth,
            Self::Direct { .. } => SourceProvider::Direct,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentSpec {
    #[serde(default)]
    pub side: ContentSide,
    #[serde(flatten)]
    pub source: ContentSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackSpec {
    pub format: u32,
    pub pack: PackMeta,
    #[serde(default, rename = "mod")]
    pub mods: Vec<ContentSpec>,
    #[serde(default)]
    pub shader: Vec<ContentSpec>,
}

impl PackSpec {
    pub fn parse(text: &str) -> Result<Self> {
        let value: toml::Value =
            toml::from_str(text).map_err(|error| Error::from(format!("pack.toml: {error}")))?;
        validate_source_keys(&value)?;
        let mut legacy_value = value.clone();
        let mods = legacy_value
            .as_table_mut()
            .and_then(|root| root.remove("mods"));
        let client_mods = legacy_value
            .as_table_mut()
            .and_then(|root| root.remove("client_mods"));
        let shaders = legacy_value
            .as_table_mut()
            .and_then(|root| root.remove("shaders"));
        let mut spec: Self = legacy_value
            .try_into()
            .map_err(|error| Error::from(format!("pack.toml: {error}")))?;
        append_map_entries(&mut spec.mods, mods, ContentSide::Both, ContentKind::Mod)?;
        append_map_entries(
            &mut spec.mods,
            client_mods,
            ContentSide::Client,
            ContentKind::Mod,
        )?;
        append_map_entries(
            &mut spec.shader,
            shaders,
            ContentSide::Client,
            ContentKind::Shader,
        )?;
        spec.validate()?;
        Ok(spec)
    }

    pub fn validate(&self) -> Result<()> {
        if self.format != 1 {
            return Err(format!("unsupported pack.toml format {}", self.format).into());
        }
        validate_pack_meta(&self.pack)?;
        if self.mods.is_empty() && self.shader.is_empty() {
            return Err("pack.toml has no mods or shaders".into());
        }
        let mut seen = std::collections::BTreeSet::new();
        for (kind, content) in self.content() {
            let id = content.source.id();
            check_content_id(id)?;
            if content.source.version().trim().is_empty() {
                return Err(format!("{id} version is required").into());
            }
            if !seen.insert(id) {
                return Err(format!("duplicate content ID {id}").into());
            }
            if let ContentSource::Direct { filename, url, .. } = &content.source {
                check_pack_path(&format!("{}/{filename}", kind.folder()))?;
                if !(url.starts_with("https://") || url.starts_with("file:")) {
                    return Err(format!("{id} direct URL is not https or file: {url}").into());
                }
            }
        }
        Ok(())
    }

    pub fn content(&self) -> impl Iterator<Item = (ContentKind, &ContentSpec)> {
        self.mods
            .iter()
            .map(|content| (ContentKind::Mod, content))
            .chain(
                self.shader
                    .iter()
                    .map(|content| (ContentKind::Shader, content)),
            )
    }

    pub fn content_count(&self) -> usize {
        self.mods.len() + self.shader.len()
    }
}

fn validate_source_keys(value: &toml::Value) -> Result<()> {
    let root = value
        .as_table()
        .ok_or_else(|| Error::from("pack.toml must contain a TOML table"))?;
    reject_unknown_keys(
        root,
        &[
            "format",
            "pack",
            "mod",
            "shader",
            "mods",
            "client_mods",
            "shaders",
            "publish",
        ],
        "pack.toml",
    )?;
    if let Some(pack) = root.get("pack").and_then(toml::Value::as_table) {
        reject_unknown_keys(
            pack,
            &[
                "name",
                "slug",
                "version",
                "group",
                "minecraft",
                "loader",
                "loader_version",
            ],
            "pack.toml [pack]",
        )?;
    }
    for section in ["mod", "shader"] {
        let Some(entries) = root.get(section).and_then(toml::Value::as_array) else {
            continue;
        };
        for (index, entry) in entries.iter().enumerate() {
            let Some(table) = entry.as_table() else {
                continue;
            };
            let allowed = if table.contains_key("modrinth") {
                &["modrinth", "version", "side"][..]
            } else {
                &["id", "version", "filename", "url", "side"][..]
            };
            reject_unknown_keys(
                table,
                allowed,
                &format!("pack.toml [[{section}]] entry {}", index + 1),
            )?;
        }
    }
    for section in ["mods", "client_mods", "shaders"] {
        let Some(entries) = root.get(section).and_then(toml::Value::as_table) else {
            continue;
        };
        for (id, version) in entries {
            check_content_id(id)?;
            if !version.is_str() {
                return Err(
                    format!("pack.toml [{section}] `{id}` must be a version string").into(),
                );
            }
        }
    }
    Ok(())
}

fn append_map_entries(
    destination: &mut Vec<ContentSpec>,
    value: Option<toml::Value>,
    side: ContentSide,
    kind: ContentKind,
) -> Result<()> {
    let Some(table) = value else {
        return Ok(());
    };
    let Some(table) = table.as_table() else {
        return Err(format!("pack.toml [{}] must be a table", kind.section()).into());
    };
    for (id, version) in table {
        let version = version.as_str().ok_or_else(|| {
            format!(
                "pack.toml [{}] `{id}` must be a version string",
                kind.section()
            )
        })?;
        destination.push(ContentSpec {
            side,
            source: ContentSource::Modrinth {
                modrinth: id.clone(),
                version: version.to_string(),
            },
        });
    }
    Ok(())
}

fn reject_unknown_keys(
    table: &toml::map::Map<String, toml::Value>,
    allowed: &[&str],
    path: &str,
) -> Result<()> {
    if let Some(key) = table.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(format!("{path} has unknown key `{key}`").into());
    }
    Ok(())
}

fn validate_pack_meta(pack: &PackMeta) -> Result<()> {
    if pack.name.trim().is_empty() {
        return Err("pack.name is required".into());
    }
    if pack.slug.trim().is_empty() {
        return Err("pack.slug is required".into());
    }
    check_coordinate_part("pack.slug", &pack.slug, false)?;
    if pack.version.trim().is_empty() {
        return Err("pack.version is required".into());
    }
    check_coordinate_part("pack.version", &pack.version, true)?;
    if pack.group.trim().is_empty() {
        return Err("pack.group is required".into());
    }
    check_coordinate_part("pack.group", &pack.group, true)?;
    if pack.minecraft.trim().is_empty() {
        return Err("pack.minecraft is required".into());
    }
    if pack.loader_version.trim().is_empty() {
        return Err("pack.loader_version is required".into());
    }
    if pack.loader != "fabric" {
        return Err(format!(
            "pack.loader `{}` is not supported yet; Forever World is Fabric-only",
            pack.loader
        )
        .into());
    }
    Ok(())
}

fn check_coordinate_part(name: &str, value: &str, allow_dots: bool) -> Result<()> {
    let valid = !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'-' | b'_')
                || (allow_dots && byte == b'.')
        })
        && (!allow_dots
            || value
                .split('.')
                .all(|component| !component.is_empty() && component != "." && component != ".."));
    if !valid {
        return Err(format!(
            "{name} `{value}` must use only ASCII letters, digits, `-`, `_`{}",
            if allow_dots { " or separated `.`" } else { "" }
        )
        .into());
    }
    Ok(())
}

fn validate_locked_files(files: &[FileSpec], lock_version: u32) -> Result<()> {
    if files.is_empty() {
        return Err("pack.lock.toml has no [[file]] entries".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut seen_ids = std::collections::BTreeSet::new();
    for file in files {
        validate_locked_file(file, lock_version)?;
        if lock_version >= 2 && !seen_ids.insert(file.id.as_str()) {
            return Err(format!("duplicate locked content ID {}", file.id).into());
        }
        if !seen.insert(file.path.clone()) {
            return Err(format!("duplicate pack path {}", file.path).into());
        }
    }
    Ok(())
}

fn validate_locked_file(file: &FileSpec, lock_version: u32) -> Result<()> {
    check_pack_path(&file.path)?;
    if lock_version >= 2 {
        check_content_id(&file.id)?;
        if file.requested_version.trim().is_empty() {
            return Err(format!("{} has no requested_version", file.path).into());
        }
    }
    if file.downloads.is_empty() {
        return Err(format!("{} has no downloads", file.path).into());
    }
    for url in &file.downloads {
        if !(url.starts_with("https://") || url.starts_with("file:")) {
            return Err(format!("{} download is not https or file: {url}", file.path).into());
        }
    }
    if file.sha1.len() != 40 || file.sha512.len() != 128 {
        return Err(format!("{} is missing a full sha1/sha512 pin", file.path).into());
    }
    Ok(())
}

fn check_content_id(id: &str) -> Result<()> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(format!(
            "content ID `{id}` must use only ASCII letters, digits, `-`, `_` or `.`"
        )
        .into());
    }
    Ok(())
}

impl PackMeta {
    pub fn mrpack_name(&self) -> String {
        format!("{}-{}.mrpack", self.slug, self.version)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lockfile {
    pub version: u32,
    pub pack: PackMeta,
    pub file: Vec<FileSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub curseforge: Vec<CurseForgeFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurseForgeFile {
    pub path: String,
    pub sha1: String,
    pub project_id: u32,
    pub file_id: u32,
}

impl Lockfile {
    pub fn new(pack: PackMeta, file: Vec<FileSpec>) -> Self {
        Self {
            version: 2,
            pack,
            file,
            curseforge: Vec::new(),
        }
    }

    pub fn retain_curseforge_from(&mut self, previous: &Self) {
        let pins: std::collections::BTreeSet<_> = self
            .file
            .iter()
            .map(|file| (file.path.as_str(), file.sha1.as_str()))
            .collect();
        self.curseforge = previous
            .curseforge
            .iter()
            .filter(|file| pins.contains(&(file.path.as_str(), file.sha1.as_str())))
            .cloned()
            .collect();
    }

    pub fn parse(text: &str) -> Result<Self> {
        let lock: Self = toml::from_str(text)
            .map_err(|error| Error::from(format!("pack.lock.toml: {error}")))?;
        if !matches!(lock.version, 1 | 2) {
            return Err(format!("unsupported lock version {}", lock.version).into());
        }
        validate_pack_meta(&lock.pack)?;
        validate_locked_files(&lock.file, lock.version)?;
        let pins: std::collections::BTreeSet<_> = lock
            .file
            .iter()
            .map(|file| (file.path.as_str(), file.sha1.as_str()))
            .collect();
        let mut seen = std::collections::BTreeSet::new();
        for file in &lock.curseforge {
            check_pack_path(&file.path)?;
            if file.project_id == 0 || file.file_id == 0 {
                return Err(format!("{} has an invalid CurseForge ID", file.path).into());
            }
            if !pins.contains(&(file.path.as_str(), file.sha1.as_str())) {
                return Err(format!("{} has a stale CurseForge mapping", file.path).into());
            }
            if !seen.insert(file.path.as_str()) {
                return Err(format!("duplicate CurseForge mapping for {}", file.path).into());
            }
        }
        Ok(lock)
    }

    pub fn to_toml(&self) -> Result<String> {
        Ok(toml::to_string_pretty(self)?)
    }
}

pub fn check_pack_path(path: &str) -> Result<()> {
    if path.is_empty() {
        return Err("pack path is empty".into());
    }
    if path.contains('\0') || path.contains('\\') {
        return Err(format!("pack path `{path}` must be a slash-separated relative path").into());
    }
    if path.starts_with('/') {
        return Err(format!("pack path `{path}` must not be absolute").into());
    }
    if path
        .split('/')
        .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(format!("pack path `{path}` must not contain empty, `.` or `..` parts").into());
    }
    let parsed = Path::new(path);
    if parsed.is_absolute() {
        return Err(format!("pack path `{path}` must not be absolute").into());
    }
    for component in parsed.components() {
        match component {
            Component::Normal(part) => {
                if part.is_empty() {
                    return Err(format!("pack path `{path}` has an empty component").into());
                }
            }
            Component::CurDir | Component::ParentDir => {
                return Err(format!("pack path `{path}` must not contain `.` or `..`").into());
            }
            _ => {
                return Err(format!("pack path `{path}` is not a clean relative path").into());
            }
        }
    }
    let first = path.split('/').next().unwrap_or("");
    if first == "world" {
        return Err(format!("pack path `{path}` must not replace a server world").into());
    }
    Ok(())
}

pub fn server_file(file: &FileSpec) -> bool {
    file.env.server != SideRequirement::Unsupported
}

pub fn client_file(file: &FileSpec) -> bool {
    file.env.client != SideRequirement::Unsupported
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PackRoot, load_spec};
    use std::path::PathBuf;

    #[test]
    fn rejects_world_and_traversal() {
        assert!(check_pack_path("world/level.dat").is_err());
        assert!(check_pack_path("../mods/x.jar").is_err());
        assert!(check_pack_path("/mods/x.jar").is_err());
        assert!(check_pack_path("mods\\x.jar").is_err());
        assert!(check_pack_path("mods//x.jar").is_err());
        assert!(check_pack_path("mods/./x.jar").is_err());
        assert!(check_pack_path("mods/x.jar/").is_err());
        assert!(check_pack_path("mods/sodium.jar").is_ok());
    }

    #[test]
    fn pack_coordinates_cannot_escape_distribution_paths() {
        assert!(check_coordinate_part("pack.slug", "forever-world", false).is_ok());
        assert!(check_coordinate_part("pack.version", "1.2.0", true).is_ok());
        assert!(check_coordinate_part("pack.group", "com.iamkaf.modpacks", true).is_ok());
        assert!(check_coordinate_part("pack.slug", "../elsewhere", false).is_err());
        assert!(check_coordinate_part("pack.version", "../1.2.0", true).is_err());
        assert!(check_coordinate_part("pack.group", "com..modpacks", true).is_err());
    }

    #[test]
    fn source_config_rejects_unknown_content_keys() {
        let text = r#"
format = 1

[pack]
name = "Example"
slug = "example"
version = "1.0.0"
minecraft = "26.2"
loader = "fabric"
loader_version = "0.19.3"

[[mod]]
modrinth = "sodium"
version = "mc26.2-0.9.1-fabric"
sdie = "client"
"#;
        let error = PackSpec::parse(text)
            .expect_err("unknown source key")
            .to_string();
        assert!(error.contains("unknown key `sdie`"));
    }

    #[test]
    fn lockfiles_receive_full_spec_validation() {
        let lock = Lockfile {
            version: 1,
            pack: PackMeta {
                name: "FOREVER WORLD".into(),
                slug: "forever-world".into(),
                version: "1.1.1".into(),
                group: "com.iamkaf.modpacks".into(),
                minecraft: "26.2".into(),
                loader: "fabric".into(),
                loader_version: "0.19.3".into(),
            },
            file: vec![FileSpec {
                id: "world".into(),
                provider: SourceProvider::Direct,
                requested_version: "1.0.0".into(),
                path: "world/level.dat".into(),
                file_size: 1,
                sha1: "a".repeat(40),
                sha512: "b".repeat(128),
                env: EnvSpec {
                    client: SideRequirement::Required,
                    server: SideRequirement::Required,
                },
                downloads: vec!["https://example.invalid/level.dat".into()],
            }],
            curseforge: Vec::new(),
        };
        let text = lock.to_toml().expect("lock TOML");
        assert!(Lockfile::parse(&text).is_err());
    }

    #[test]
    fn parses_current_pack() {
        let root = PackRoot {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        };
        let spec = load_spec(&root).expect("pack.toml");
        assert_eq!(spec.pack.version, "1.2.0");
        assert_eq!(spec.pack.minecraft, "26.2");
        assert_eq!(spec.pack.loader_version, "0.19.3");
        assert_eq!(spec.content_count(), 48);
        let sodium = spec
            .content()
            .map(|(_, content)| content)
            .find(|content| content.source.id() == "sodium")
            .expect("sodium");
        assert_eq!(sodium.side, ContentSide::Client);
        let amber = spec
            .content()
            .map(|(_, content)| content)
            .find(|content| content.source.id() == "amber")
            .expect("amber");
        assert_eq!(amber.side, ContentSide::Both);
        assert_eq!(spec.shader.len(), 1);
        assert_eq!(spec.shader[0].side, ContentSide::Client);
    }
}
