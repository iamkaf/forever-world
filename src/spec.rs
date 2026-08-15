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
    pub path: String,
    pub file_size: u64,
    pub sha1: String,
    pub sha512: String,
    pub env: EnvSpec,
    pub downloads: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackSpec {
    pub pack: PackMeta,
    pub file: Vec<FileSpec>,
}

impl PackSpec {
    pub fn parse(text: &str) -> Result<Self> {
        let spec: Self = toml::from_str(text)?;
        spec.validate()?;
        Ok(spec)
    }

    pub fn validate(&self) -> Result<()> {
        if self.pack.name.trim().is_empty() {
            return Err("pack.name is required".into());
        }
        if self.pack.slug.trim().is_empty() {
            return Err("pack.slug is required".into());
        }
        if self.pack.version.trim().is_empty() {
            return Err("pack.version is required".into());
        }
        if self.pack.group.trim().is_empty() {
            return Err("pack.group is required".into());
        }
        if self.pack.minecraft.trim().is_empty() {
            return Err("pack.minecraft is required".into());
        }
        if self.pack.loader_version.trim().is_empty() {
            return Err("pack.loader_version is required".into());
        }
        if self.pack.loader != "fabric" {
            return Err(format!(
                "pack.loader `{}` is not supported yet; Forever World is Fabric-only",
                self.pack.loader
            )
            .into());
        }
        if self.file.is_empty() {
            return Err("pack.toml has no [[file]] entries".into());
        }
        let mut seen = std::collections::BTreeSet::new();
        for file in &self.file {
            check_pack_path(&file.path)?;
            if file.downloads.is_empty() {
                return Err(format!("{} has no downloads", file.path).into());
            }
            for url in &file.downloads {
                if !(url.starts_with("https://") || url.starts_with("file:")) {
                    return Err(
                        format!("{} download is not https or file: {url}", file.path).into(),
                    );
                }
            }
            if file.sha1.len() != 40 || file.sha512.len() != 128 {
                return Err(format!("{} is missing a full sha1/sha512 pin", file.path).into());
            }
            if !seen.insert(file.path.clone()) {
                return Err(format!("duplicate pack path {}", file.path).into());
            }
        }
        Ok(())
    }

    pub fn mrpack_name(&self) -> String {
        self.pack.mrpack_name()
    }
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
    pub fn from_spec(spec: PackSpec) -> Self {
        Self {
            version: 1,
            pack: spec.pack,
            file: spec.file,
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
        if lock.version != 1 {
            return Err(format!("unsupported lock version {}", lock.version).into());
        }
        PackSpec {
            pack: lock.pack.clone(),
            file: lock.file.clone(),
        }
        .validate()?;
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
        assert_eq!(spec.file.len(), 48);
        let sodium = spec
            .file
            .iter()
            .find(|file| file.path.starts_with("mods/sodium-"))
            .expect("sodium");
        assert_eq!(sodium.env.server, SideRequirement::Unsupported);
        let amber = spec
            .file
            .iter()
            .find(|file| file.path.starts_with("mods/amber-"))
            .expect("amber");
        assert_eq!(amber.env.server, SideRequirement::Required);
        assert!(
            spec.file
                .iter()
                .any(|file| file.path.starts_with("shaderpacks/")
                    && file.env.server == SideRequirement::Unsupported)
        );
    }
}
