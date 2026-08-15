use crate::spec::{FileSpec, Lockfile, SideRequirement, check_pack_path};
use crate::{PackRoot, Result, hash};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

#[derive(Debug, Serialize)]
pub struct MrpackIndex {
    #[serde(rename = "formatVersion")]
    pub format_version: u32,
    pub game: String,
    #[serde(rename = "versionId")]
    pub version_id: String,
    pub name: String,
    pub files: Vec<MrpackFile>,
    pub dependencies: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct MrpackFile {
    pub path: String,
    pub hashes: BTreeMap<String, String>,
    pub env: MrpackEnv,
    pub downloads: Vec<String>,
    #[serde(rename = "fileSize")]
    pub file_size: u64,
}

#[derive(Debug, Serialize, Clone)]
pub struct MrpackEnv {
    pub client: SideRequirement,
    pub server: SideRequirement,
}

pub fn export(root: &PackRoot) -> Result<std::path::PathBuf> {
    let lock = crate::load_lock(root)?;
    fs::create_dir_all(root.dist_dir())?;
    let index = index_from_lock(&lock)?;
    let index_bytes = serde_json::to_vec_pretty(&index)?;
    let mut index_bytes = index_bytes;
    if !index_bytes.ends_with(b"\n") {
        index_bytes.push(b'\n');
    }
    let name = format!("{}-{}.mrpack", lock.pack.slug, lock.pack.version);
    let dest = root.dist_dir().join(&name);
    write_mrpack(&dest, &index_bytes, root)?;
    let archive = fs::read(&dest)?;
    let sha512 = hash::sha512_hex(&archive);
    fs::write(
        dest.with_extension("mrpack.sha512"),
        format!("{sha512}  {name}\n"),
    )?;
    Ok(dest)
}

pub fn index_from_lock(lock: &Lockfile) -> Result<MrpackIndex> {
    let mut files = Vec::new();
    for file in &lock.file {
        files.push(mrpack_file(file)?);
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let mut dependencies = BTreeMap::new();
    dependencies.insert("minecraft".to_string(), lock.pack.minecraft.clone());
    dependencies.insert(
        loader_dependency_key(&lock.pack.loader)?.to_string(),
        lock.pack.loader_version.clone(),
    );
    Ok(MrpackIndex {
        format_version: 1,
        game: "minecraft".to_string(),
        version_id: lock.pack.version.clone(),
        name: lock.pack.name.clone(),
        files,
        dependencies,
    })
}

fn mrpack_file(file: &FileSpec) -> Result<MrpackFile> {
    check_pack_path(&file.path)?;
    let mut hashes = BTreeMap::new();
    hashes.insert("sha1".to_string(), file.sha1.clone());
    hashes.insert("sha512".to_string(), file.sha512.clone());
    Ok(MrpackFile {
        path: file.path.clone(),
        hashes,
        env: MrpackEnv {
            client: file.env.client,
            server: file.env.server,
        },
        downloads: file.downloads.clone(),
        file_size: file.file_size,
    })
}

fn loader_dependency_key(loader: &str) -> Result<&'static str> {
    match loader {
        "fabric" => Ok("fabric-loader"),
        "forge" => Ok("forge"),
        "neoforge" => Ok("neoforge"),
        other => Err(format!("unsupported loader `{other}`").into()),
    }
}

fn write_mrpack(dest: &Path, index_bytes: &[u8], root: &PackRoot) -> Result<()> {
    let file = File::create(dest)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("modrinth.index.json", options)?;
    zip.write_all(index_bytes)?;
    add_override_tree(&mut zip, options, root.overrides_dir(), "overrides")?;
    add_override_tree(
        &mut zip,
        options,
        root.client_overrides_dir(),
        "client-overrides",
    )?;
    add_override_tree(
        &mut zip,
        options,
        root.server_overrides_dir(),
        "server-overrides",
    )?;
    zip.finish()?;
    Ok(())
}

fn add_override_tree(
    zip: &mut ZipWriter<File>,
    options: SimpleFileOptions,
    dir: std::path::PathBuf,
    prefix: &str,
) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    add_dir(zip, options, &dir, prefix)?;
    Ok(())
}

fn add_dir(
    zip: &mut ZipWriter<File>,
    options: SimpleFileOptions,
    dir: &Path,
    prefix: &str,
) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.collect();
    entries.sort_by_key(|entry| {
        entry
            .as_ref()
            .map(|value| value.file_name())
            .unwrap_or_default()
    });
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == ".DS_Store" || name.starts_with("._") || name.ends_with(".bak") {
            continue;
        }
        let rel = format!("{prefix}/{name}");
        check_pack_path(&rel)?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            return Err(format!(
                "refusing symbolic link in pack overrides: {}",
                path.display()
            )
            .into());
        }
        if meta.is_dir() {
            add_dir(zip, options, &path, &rel)?;
        } else if meta.is_file() {
            zip.start_file(&rel, options)?;
            let mut input = File::open(&path)?;
            let mut bytes = Vec::new();
            input.read_to_end(&mut bytes)?;
            zip.write_all(&bytes)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{EnvSpec, FileSpec, PackMeta, SideRequirement};

    #[test]
    fn index_omits_nothing_and_sorts() {
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
            file: vec![
                FileSpec {
                    path: "mods/b.jar".into(),
                    file_size: 1,
                    sha1: "a".repeat(40),
                    sha512: "b".repeat(128),
                    env: EnvSpec {
                        client: SideRequirement::Required,
                        server: SideRequirement::Unsupported,
                    },
                    downloads: vec!["https://cdn.modrinth.com/b.jar".into()],
                },
                FileSpec {
                    path: "mods/a.jar".into(),
                    file_size: 1,
                    sha1: "a".repeat(40),
                    sha512: "b".repeat(128),
                    env: EnvSpec {
                        client: SideRequirement::Required,
                        server: SideRequirement::Required,
                    },
                    downloads: vec!["https://cdn.modrinth.com/a.jar".into()],
                },
            ],
            curseforge: Vec::new(),
        };
        let index = index_from_lock(&lock).expect("index");
        assert_eq!(index.files[0].path, "mods/a.jar");
        assert_eq!(index.files[1].env.server, SideRequirement::Unsupported);
        assert_eq!(
            index.dependencies.get("minecraft").map(String::as_str),
            Some("26.2")
        );
        assert_eq!(
            index.dependencies.get("fabric-loader").map(String::as_str),
            Some("0.19.3")
        );
    }
}
