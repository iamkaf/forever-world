use crate::fetch;
use crate::publish::PublishMode;
use crate::spec::{CurseForgeFile, Lockfile, SideRequirement, check_pack_path, client_file};
use crate::{PackRoot, Result, USER_AGENT, hash};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

const UPLOAD_BASE: &str = "https://minecraft.curseforge.com/api/projects";

#[derive(Debug, Deserialize)]
struct Platforms {
    curseforge: Config,
}

#[derive(Debug, Deserialize)]
struct Config {
    packwiz_commit: String,
    author: String,
    #[serde(default)]
    add: Vec<ExplicitFile>,
    #[serde(default)]
    exclude: Vec<ExcludedFile>,
}

#[derive(Debug, Deserialize)]
struct ExplicitFile {
    path: String,
    project_id: u32,
    file_id: u32,
}

#[derive(Debug, Deserialize)]
struct ExcludedFile {
    path: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
struct PackwizMeta {
    filename: String,
    download: PackwizDownload,
    update: PackwizUpdate,
}

#[derive(Debug, Deserialize)]
struct PackwizDownload {
    #[serde(rename = "hash-format")]
    hash_format: String,
    hash: String,
}

#[derive(Debug, Deserialize)]
struct PackwizUpdate {
    curseforge: PackwizCurseForge,
}

#[derive(Debug, Deserialize)]
struct PackwizCurseForge {
    #[serde(rename = "project-id")]
    project_id: u32,
    #[serde(rename = "file-id")]
    file_id: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ResolveReport {
    pub resolved: usize,
    pub excluded: Vec<String>,
    pub unresolved: Vec<String>,
}

pub fn resolve(root: &PackRoot) -> Result<ResolveReport> {
    let config = load_config(root)?;
    let mut lock = crate::load_lock(root)?;
    let excluded = validate_config(&config, &lock)?;
    let packwiz = std::env::var_os("PACKWIZ_BIN")
        .ok_or_else(|| crate::Error::from("set PACKWIZ_BIN to the pinned Packwiz binary"))?;
    verify_packwiz(&packwiz, &config.packwiz_commit)?;
    let temp = tempfile::tempdir()?;
    initialise_packwiz(temp.path(), &lock)?;

    for file in lock.file.iter().filter(|file| {
        client_file(file)
            && !excluded.contains(&file.path)
            && file.path.starts_with("mods/")
            && file.path.ends_with(".jar")
    }) {
        let source = fetch::ensure_cached(root, file)?;
        let destination = temp.path().join(&file.path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, destination)?;
    }

    run_packwiz(
        &packwiz,
        temp.path(),
        &["--yes", "curseforge", "detect"],
        &config.packwiz_commit,
    )?;
    for file in &config.add {
        check_pack_path(&file.path)?;
        let locked = lock
            .file
            .iter()
            .find(|candidate| candidate.path == file.path)
            .ok_or_else(|| format!("{} is not in pack.lock.toml", file.path))?;
        if !client_file(locked) {
            return Err(format!("{} is not a client file", file.path).into());
        }
        let folder = Path::new(&file.path)
            .parent()
            .and_then(Path::to_str)
            .ok_or_else(|| format!("{} has no metadata folder", file.path))?;
        let project_id = file.project_id.to_string();
        let file_id = file.file_id.to_string();
        run_packwiz(
            &packwiz,
            temp.path(),
            &[
                "--meta-folder",
                folder,
                "--yes",
                "curseforge",
                "add",
                "--addon-id",
                &project_id,
                "--file-id",
                &file_id,
            ],
            &config.packwiz_commit,
        )?;
    }

    lock.curseforge = mappings_from_packwiz(temp.path(), &lock)?;
    lock.curseforge
        .sort_by(|left, right| left.path.cmp(&right.path));
    let mapped: BTreeSet<_> = lock
        .curseforge
        .iter()
        .map(|file| file.path.as_str())
        .collect();
    let unresolved = lock
        .file
        .iter()
        .filter(|file| {
            client_file(file)
                && !mapped.contains(file.path.as_str())
                && !excluded.contains(&file.path)
        })
        .map(|file| file.path.clone())
        .collect();
    fs::write(root.lock_toml(), lock.to_toml()?)?;
    Ok(ResolveReport {
        resolved: lock.curseforge.len(),
        excluded: excluded.into_iter().collect(),
        unresolved,
    })
}

fn validate_config(config: &Config, lock: &Lockfile) -> Result<BTreeSet<String>> {
    let client_files: BTreeMap<_, _> = lock
        .file
        .iter()
        .filter(|file| client_file(file))
        .map(|file| (file.path.as_str(), file))
        .collect();
    let mut additions = BTreeSet::new();
    for file in &config.add {
        check_pack_path(&file.path)?;
        if !client_files.contains_key(file.path.as_str()) {
            return Err(format!(
                "platforms.toml [[curseforge.add]] path is not a locked client file: {}",
                file.path
            )
            .into());
        }
        if file.project_id == 0 || file.file_id == 0 {
            return Err(format!(
                "platforms.toml [[curseforge.add]] has an invalid ID: {}",
                file.path
            )
            .into());
        }
        if !additions.insert(file.path.as_str()) {
            return Err(format!(
                "duplicate platforms.toml [[curseforge.add]] path: {}",
                file.path
            )
            .into());
        }
    }

    let mut exclusions = BTreeSet::new();
    for file in &config.exclude {
        check_pack_path(&file.path)?;
        let Some(locked) = client_files.get(file.path.as_str()) else {
            return Err(format!(
                "platforms.toml [[curseforge.exclude]] path is not a locked client file: {}",
                file.path
            )
            .into());
        };
        if locked.env.server != SideRequirement::Unsupported {
            return Err(format!(
                "platforms.toml may exclude only client-only files: {}",
                file.path
            )
            .into());
        }
        if file.reason.trim().is_empty() {
            return Err(format!(
                "platforms.toml [[curseforge.exclude]] reason is required: {}",
                file.path
            )
            .into());
        }
        if additions.contains(file.path.as_str()) {
            return Err(format!(
                "platforms.toml cannot add and exclude the same file: {}",
                file.path
            )
            .into());
        }
        if !exclusions.insert(file.path.clone()) {
            return Err(format!(
                "duplicate platforms.toml [[curseforge.exclude]] path: {}",
                file.path
            )
            .into());
        }
    }
    Ok(exclusions)
}

fn initialise_packwiz(dir: &Path, lock: &Lockfile) -> Result<()> {
    let pack = format!(
        "name = {}\nauthor = \"iamkaf\"\nversion = {}\npack-format = \"packwiz:1.1.0\"\n\n[index]\nfile = \"index.toml\"\nhash-format = \"sha256\"\nhash = \"\"\n\n[versions]\nfabric = {}\nminecraft = {}\n",
        toml_string(&lock.pack.name),
        toml_string(&lock.pack.version),
        toml_string(&lock.pack.loader_version),
        toml_string(&lock.pack.minecraft),
    );
    fs::write(dir.join("pack.toml"), pack)?;
    fs::write(dir.join("index.toml"), "hash-format = \"sha256\"\n")?;
    Ok(())
}

fn run_packwiz(
    binary: &std::ffi::OsStr,
    dir: &Path,
    args: &[&str],
    expected_commit: &str,
) -> Result<()> {
    let status = Command::new(binary)
        .current_dir(dir)
        .args(args)
        .status()
        .map_err(|error| {
            crate::Error::from(format!(
                "could not run Packwiz: {error}; build commit {expected_commit} and set PACKWIZ_BIN"
            ))
        })?;
    if !status.success() {
        return Err(format!("Packwiz exited with {status}").into());
    }
    Ok(())
}

fn verify_packwiz(binary: &std::ffi::OsStr, expected_commit: &str) -> Result<()> {
    let output = Command::new("go")
        .args([
            std::ffi::OsStr::new("version"),
            std::ffi::OsStr::new("-m"),
            binary,
        ])
        .output()
        .map_err(|error| crate::Error::from(format!("could not inspect Packwiz build: {error}")))?;
    if !output.status.success() {
        return Err("could not inspect Packwiz build metadata with `go version -m`".into());
    }
    let metadata = String::from_utf8_lossy(&output.stdout);
    if !metadata.contains("github.com/packwiz/packwiz") || !metadata.contains(expected_commit) {
        return Err(
            format!("PACKWIZ_BIN was not built from pinned commit {expected_commit}").into(),
        );
    }
    Ok(())
}

fn mappings_from_packwiz(dir: &Path, lock: &Lockfile) -> Result<Vec<CurseForgeFile>> {
    let mut metadata = Vec::new();
    collect_metadata(dir, &mut metadata)?;
    let mut mappings = Vec::new();
    let mut seen = BTreeSet::new();
    for path in metadata {
        let text = fs::read_to_string(&path)?;
        let meta: PackwizMeta = toml::from_str(&text).map_err(crate::Error::from_display)?;
        if meta.download.hash_format != "sha1" {
            return Err(format!(
                "{} used unsupported Packwiz hash format {}",
                path.display(),
                meta.download.hash_format
            )
            .into());
        }
        let matches: Vec<_> = lock
            .file
            .iter()
            .filter(|file| {
                Path::new(&file.path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    == Some(meta.filename.as_str())
            })
            .collect();
        let [file] = matches.as_slice() else {
            return Err(format!(
                "{} did not identify exactly one locked file named {}",
                path.display(),
                meta.filename
            )
            .into());
        };
        if file.sha1 != meta.download.hash {
            return Err(format!("Packwiz hash did not match the pin for {}", file.path).into());
        }
        if !seen.insert(file.path.as_str()) {
            return Err(format!("Packwiz mapped {} more than once", file.path).into());
        }
        mappings.push(CurseForgeFile {
            path: file.path.clone(),
            sha1: file.sha1.clone(),
            project_id: meta.update.curseforge.project_id,
            file_id: meta.update.curseforge.file_id,
        });
    }
    Ok(mappings)
}

fn collect_metadata(dir: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.collect();
    entries.sort_by_key(|entry| {
        entry
            .as_ref()
            .map(|entry| entry.file_name())
            .unwrap_or_default()
    });
    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            collect_metadata(&path, output)?;
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("toml")
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".pw.toml"))
        {
            output.push(path);
        }
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub minecraft: Minecraft,
    pub manifest_type: String,
    pub manifest_version: u32,
    pub name: String,
    pub version: String,
    pub author: String,
    pub files: Vec<ManifestFile>,
    pub overrides: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Minecraft {
    pub version: String,
    #[serde(rename = "modLoaders")]
    pub mod_loaders: Vec<ModLoader>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModLoader {
    pub id: String,
    pub primary: bool,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestFile {
    #[serde(rename = "projectID")]
    pub project_id: u32,
    #[serde(rename = "fileID")]
    pub file_id: u32,
    pub required: bool,
}

fn manifest_from_lock(
    lock: &Lockfile,
    author: &str,
    excluded: &BTreeSet<String>,
) -> Result<Manifest> {
    let mappings: BTreeMap<_, _> = lock
        .curseforge
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();
    let missing: Vec<_> = lock
        .file
        .iter()
        .filter(|file| {
            client_file(file)
                && !excluded.contains(&file.path)
                && !mappings.contains_key(file.path.as_str())
        })
        .map(|file| file.path.as_str())
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "CurseForge has no locked file for: {}; run `pack curseforge resolve` and do not publish until every client file resolves",
            missing.join(", ")
        )
        .into());
    }
    let mut files: Vec<_> = lock
        .file
        .iter()
        .filter(|file| client_file(file) && !excluded.contains(&file.path))
        .map(|file| {
            let mapped = mappings[&file.path.as_str()];
            ManifestFile {
                project_id: mapped.project_id,
                file_id: mapped.file_id,
                required: true,
            }
        })
        .collect();
    files.sort_by_key(|file| (file.project_id, file.file_id));
    Ok(Manifest {
        minecraft: Minecraft {
            version: lock.pack.minecraft.clone(),
            mod_loaders: vec![ModLoader {
                id: format!("{}-{}", lock.pack.loader, lock.pack.loader_version),
                primary: true,
            }],
        },
        manifest_type: "minecraftModpack".into(),
        manifest_version: 1,
        name: lock.pack.name.clone(),
        version: lock.pack.version.clone(),
        author: author.into(),
        files,
        overrides: "overrides".into(),
    })
}

pub fn export(root: &PackRoot) -> Result<PathBuf> {
    let lock = crate::load_lock(root)?;
    let config = load_config(root)?;
    let excluded = validate_config(&config, &lock)?;
    let manifest = manifest_from_lock(&lock, &config.author, &excluded)?;
    let mut manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    manifest_bytes.push(b'\n');
    let name = format!("{}-{}-curseforge.zip", lock.pack.slug, lock.pack.version);
    fs::create_dir_all(root.dist_dir())?;
    let destination = root.dist_dir().join(&name);
    write_archive(root, &destination, &manifest_bytes)?;
    let digest = hash::sha512_hex(&fs::read(&destination)?);
    fs::write(
        destination.with_extension("zip.sha512"),
        format!("{digest}  {name}\n"),
    )?;
    Ok(destination)
}

fn write_archive(root: &PackRoot, destination: &Path, manifest: &[u8]) -> Result<()> {
    let mut entries = BTreeMap::new();
    collect_overrides(root.overrides_dir(), "overrides", &mut entries)?;
    collect_overrides(root.client_overrides_dir(), "overrides", &mut entries)?;
    let file = File::create(destination)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("manifest.json", options)?;
    zip.write_all(manifest)?;
    for (path, bytes) in entries {
        zip.start_file(path, options)?;
        zip.write_all(&bytes)?;
    }
    zip.finish()?;
    Ok(())
}

fn collect_overrides(
    dir: PathBuf,
    prefix: &str,
    output: &mut BTreeMap<String, Vec<u8>>,
) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    collect_override_dir(&dir, prefix, output)
}

fn collect_override_dir(
    dir: &Path,
    prefix: &str,
    output: &mut BTreeMap<String, Vec<u8>>,
) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.collect();
    entries.sort_by_key(|entry| {
        entry
            .as_ref()
            .map(|entry| entry.file_name())
            .unwrap_or_default()
    });
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == ".DS_Store" || name.starts_with("._") || name.ends_with(".bak") {
            continue;
        }
        let archive_path = format!("{prefix}/{name}");
        check_pack_path(&archive_path)?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "refusing symbolic link in pack overrides: {}",
                path.display()
            )
            .into());
        }
        if metadata.is_dir() {
            collect_override_dir(&path, &archive_path, output)?;
        } else if metadata.is_file() {
            let mut bytes = Vec::new();
            File::open(&path)?.read_to_end(&mut bytes)?;
            if output.insert(archive_path.clone(), bytes).is_some() {
                return Err(format!("duplicate CurseForge override {archive_path}").into());
            }
        }
    }
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UploadMetadata {
    changelog: String,
    changelog_type: String,
    display_name: String,
    game_version_names: Vec<String>,
    release_type: String,
}

#[derive(Debug, Deserialize)]
struct UploadResponse {
    id: u64,
}

pub fn publish(root: &PackRoot, mode: PublishMode) -> Result<String> {
    let lock = crate::load_lock(root)?;
    if let PublishMode::Confirmed { version } = &mode
        && version != &lock.pack.version
    {
        return Err(format!(
            "publish confirmation `{version}` does not match `{}`",
            lock.pack.version
        )
        .into());
    }
    let archive = export(root)?;
    let name = archive
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| crate::Error::from("CurseForge archive name is not valid UTF-8"))?
        .to_string();
    let changelog = fs::read_to_string(root.path.join("CHANGELOG.md"))?;
    let metadata = UploadMetadata {
        changelog,
        changelog_type: "markdown".into(),
        display_name: format!("{} {}", lock.pack.name, lock.pack.version),
        game_version_names: vec!["Fabric".into(), lock.pack.minecraft.clone()],
        release_type: "release".into(),
    };
    let metadata_json = serde_json::to_string(&metadata)?;
    if mode == PublishMode::DryRun {
        let project = std::env::var("CURSEFORGE_PROJECT_ID").unwrap_or_else(|_| "<unset>".into());
        return Ok(format!(
            "DRY {UPLOAD_BASE}/{project}/upload-file {name}\n{metadata_json}"
        ));
    }
    let project_id = std::env::var("CURSEFORGE_PROJECT_ID")
        .map_err(|_| crate::Error::from("set CURSEFORGE_PROJECT_ID"))?;
    project_id
        .parse::<u64>()
        .map_err(|_| crate::Error::from("CURSEFORGE_PROJECT_ID must be a positive integer"))?;
    let token = std::env::var("CURSEFORGE_TOKEN")
        .map_err(|_| crate::Error::from("set CURSEFORGE_TOKEN"))?;
    let url = format!("{UPLOAD_BASE}/{project_id}/upload-file");
    let archive_bytes = fs::read(&archive)?;
    let form = reqwest::blocking::multipart::Form::new()
        .text("metadata", metadata_json)
        .part(
            "file",
            reqwest::blocking::multipart::Part::bytes(archive_bytes).file_name(name),
        );
    let client = reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(300))
        .build()?;
    let response = client
        .post(&url)
        .header("X-Api-Token", token)
        .multipart(form)
        .send()?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        return Err(format!("CurseForge upload failed: {status}: {}", body.trim()).into());
    }
    let uploaded: UploadResponse = serde_json::from_str(&response.text()?)?;
    Ok(format!("uploaded CurseForge file {}", uploaded.id))
}

fn load_config(root: &PackRoot) -> Result<Config> {
    let path = root.path.join("platforms.toml");
    let text = fs::read_to_string(&path)
        .map_err(|error| crate::Error::from(format!("{}: {error}", path.display())))?;
    let platforms: Platforms = toml::from_str(&text)
        .map_err(|error| crate::Error::from(format!("platforms.toml: {error}")))?;
    Ok(platforms.curseforge)
}

fn toml_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{EnvSpec, FileSpec, PackMeta, SideRequirement};

    fn lock(mapped: bool) -> Lockfile {
        let file = FileSpec {
            path: "mods/example.jar".into(),
            file_size: 1,
            sha1: "a".repeat(40),
            sha512: "b".repeat(128),
            env: EnvSpec {
                client: SideRequirement::Required,
                server: SideRequirement::Required,
            },
            downloads: vec!["https://example.invalid/example.jar".into()],
        };
        Lockfile {
            version: 1,
            pack: PackMeta {
                name: "FOREVER WORLD".into(),
                slug: "forever-world".into(),
                version: "1.2.0".into(),
                group: "com.iamkaf.modpacks".into(),
                minecraft: "26.2".into(),
                loader: "fabric".into(),
                loader_version: "0.19.3".into(),
            },
            file: vec![file.clone()],
            curseforge: mapped
                .then_some(CurseForgeFile {
                    path: file.path,
                    sha1: file.sha1,
                    project_id: 123,
                    file_id: 456,
                })
                .into_iter()
                .collect(),
        }
    }

    fn no_exclusions() -> BTreeSet<String> {
        BTreeSet::new()
    }

    #[test]
    fn manifest_uses_locked_ids_and_loader() {
        let manifest =
            manifest_from_lock(&lock(true), "iamkaf", &no_exclusions()).expect("manifest");
        assert_eq!(manifest.files[0].project_id, 123);
        assert_eq!(manifest.files[0].file_id, 456);
        assert_eq!(manifest.minecraft.mod_loaders[0].id, "fabric-0.19.3");
        let json = serde_json::to_value(manifest).expect("manifest JSON");
        assert_eq!(json["files"][0]["projectID"], 123);
        assert_eq!(json["files"][0]["fileID"], 456);
        assert!(json["files"][0].get("projectId").is_none());
    }

    #[test]
    fn manifest_rejects_an_unresolved_client_file() {
        let error = manifest_from_lock(&lock(false), "iamkaf", &no_exclusions())
            .expect_err("unresolved mapping")
            .to_string();
        assert!(error.contains("mods/example.jar"));
    }

    #[test]
    fn manifest_omits_an_explicitly_excluded_file() {
        let excluded = BTreeSet::from(["mods/example.jar".to_string()]);
        let manifest =
            manifest_from_lock(&lock(false), "iamkaf", &excluded).expect("manifest with exclusion");
        assert!(manifest.files.is_empty());
    }

    #[test]
    fn config_rejects_a_stale_exclusion() {
        let config = Config {
            packwiz_commit: "commit".into(),
            author: "iamkaf".into(),
            add: Vec::new(),
            exclude: vec![ExcludedFile {
                path: "mods/missing.jar".into(),
                reason: "Unavailable".into(),
            }],
        };
        let error = validate_config(&config, &lock(false))
            .expect_err("stale exclusion")
            .to_string();
        assert!(error.contains("mods/missing.jar"));
    }

    #[test]
    fn config_rejects_excluding_a_server_file() {
        let config = Config {
            packwiz_commit: "commit".into(),
            author: "iamkaf".into(),
            add: Vec::new(),
            exclude: vec![ExcludedFile {
                path: "mods/example.jar".into(),
                reason: "Unavailable".into(),
            }],
        };
        let error = validate_config(&config, &lock(false))
            .expect_err("server file exclusion")
            .to_string();
        assert!(error.contains("client-only"));
    }

    #[test]
    fn current_config_excludes_only_presence_footsteps() {
        let root = PackRoot {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        };
        let config = load_config(&root).expect("platforms.toml");
        let lock = crate::load_lock(&root).expect("pack.lock.toml");
        let excluded = validate_config(&config, &lock).expect("valid exclusions");
        assert_eq!(
            excluded,
            BTreeSet::from(["mods/PresenceFootsteps-1.13.3+26.2.jar".to_string()])
        );
    }

    #[test]
    fn archive_merges_only_common_and_client_overrides() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let root = PackRoot {
            path: temp.path().into(),
        };
        fs::create_dir_all(root.overrides_dir().join("config")).expect("common overrides");
        fs::create_dir_all(root.client_overrides_dir()).expect("client overrides");
        fs::create_dir_all(root.server_overrides_dir()).expect("server overrides");
        fs::write(root.overrides_dir().join("config/common.txt"), b"common").expect("common file");
        fs::write(root.client_overrides_dir().join("client.txt"), b"client").expect("client file");
        fs::write(root.server_overrides_dir().join("server.txt"), b"server").expect("server file");
        let destination = temp.path().join("pack.zip");
        write_archive(&root, &destination, b"{}\n").expect("archive");

        let file = File::open(destination).expect("archive file");
        let mut zip = zip::ZipArchive::new(file).expect("zip");
        let names: Vec<_> = (0..zip.len())
            .map(|index| zip.by_index(index).expect("entry").name().to_string())
            .collect();
        assert_eq!(
            names,
            [
                "manifest.json",
                "overrides/client.txt",
                "overrides/config/common.txt"
            ]
        );
    }
}
