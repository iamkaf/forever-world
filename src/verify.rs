use crate::export::{MrpackFile, MrpackIndex, index_from_lock};
use crate::spec::Lockfile;
use crate::{PackRoot, Result, USER_AGENT};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::time::Duration;
use zip::ZipArchive;

#[derive(Debug, Deserialize)]
struct ParsedIndex {
    #[serde(rename = "formatVersion")]
    format_version: u32,
    game: String,
    #[serde(rename = "versionId")]
    version_id: String,
    name: String,
    files: Vec<ParsedFile>,
    dependencies: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct ParsedFile {
    path: String,
    hashes: BTreeMap<String, String>,
    env: ParsedEnv,
    downloads: Vec<String>,
    #[serde(rename = "fileSize")]
    file_size: u64,
}

#[derive(Debug, Deserialize)]
struct ParsedEnv {
    client: String,
    server: String,
}

pub fn verify(root: &PackRoot, against: &str) -> Result<()> {
    let lock = crate::load_lock(root)?;
    let ours = index_from_lock(&lock)?;
    let local_path = root.dist_dir().join(lock.pack.mrpack_name());
    let local_bytes = fs::read(&local_path).map_err(|_| {
        crate::Error::from(format!(
            "missing {}; run `pack export` first",
            local_path.display()
        ))
    })?;
    let published_bytes = load_mrpack(against)?;
    let published = index_from_mrpack_bytes(&published_bytes)?;
    compare(&ours, &published)?;
    compare_archive_entries(&local_bytes, &published_bytes)?;
    eprintln!(
        "verified {} files and every archive entry against {against}",
        ours.files.len()
    );
    Ok(())
}

fn load_mrpack(against: &str) -> Result<Vec<u8>> {
    if against.starts_with("https://") {
        let client = reqwest::blocking::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok(client
            .get(against)
            .send()?
            .error_for_status()?
            .bytes()?
            .to_vec())
    } else {
        Ok(fs::read(against)?)
    }
}

fn index_from_mrpack_bytes(bytes: &[u8]) -> Result<ParsedIndex> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor)?;
    let mut file = archive
        .by_name("modrinth.index.json")
        .map_err(|_| crate::Error::from("published pack is missing modrinth.index.json"))?;
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    Ok(serde_json::from_str(&text)?)
}

fn archive_entries(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor)?;
    let mut entries = BTreeMap::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index)?;
        if file.is_dir() {
            continue;
        }
        let name = file.name().to_string();
        if name != "modrinth.index.json" {
            crate::spec::check_pack_path(&name)?;
        }
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)?;
        if entries.insert(name.clone(), contents).is_some() {
            return Err(format!("archive contains duplicate entry {name}").into());
        }
    }
    Ok(entries)
}

fn compare_archive_entries(ours: &[u8], published: &[u8]) -> Result<()> {
    let ours = archive_entries(ours)?;
    let published = archive_entries(published)?;
    for name in ours.keys() {
        if !published.contains_key(name) {
            return Err(format!("published archive is missing {name}").into());
        }
    }
    for name in published.keys() {
        if !ours.contains_key(name) {
            return Err(format!("published archive has extra entry {name}").into());
        }
    }
    for (name, contents) in &ours {
        if published.get(name) != Some(contents) {
            return Err(format!("archive entry {name} differs").into());
        }
    }
    Ok(())
}

fn compare(ours: &MrpackIndex, published: &ParsedIndex) -> Result<()> {
    if published.format_version != ours.format_version {
        return Err(format!(
            "formatVersion {} != {}",
            ours.format_version, published.format_version
        )
        .into());
    }
    if published.game != ours.game {
        return Err(format!("game {} != {}", ours.game, published.game).into());
    }
    if published.name != ours.name {
        return Err(format!("name {} != {}", ours.name, published.name).into());
    }
    if published.version_id != ours.version_id {
        return Err(format!("versionId {} != {}", ours.version_id, published.version_id).into());
    }
    if published.dependencies != ours.dependencies {
        return Err(format!(
            "dependencies {:?} != {:?}",
            ours.dependencies, published.dependencies
        )
        .into());
    }
    if ours.files.len() != published.files.len() {
        return Err(format!(
            "file count {} != {}",
            ours.files.len(),
            published.files.len()
        )
        .into());
    }
    let mut published_by_path = BTreeMap::new();
    for file in &published.files {
        published_by_path.insert(file.path.clone(), file);
    }
    for file in &ours.files {
        let Some(expected) = published_by_path.get(&file.path) else {
            return Err(format!("exported extra file {}", file.path).into());
        };
        compare_file(file, expected)?;
    }
    Ok(())
}

fn compare_file(ours: &MrpackFile, published: &ParsedFile) -> Result<()> {
    if ours.file_size != published.file_size {
        return Err(format!(
            "{} size {} != {}",
            ours.path, ours.file_size, published.file_size
        )
        .into());
    }
    if ours.downloads != published.downloads {
        return Err(format!("{} download URLs differ", ours.path).into());
    }
    if ours.hashes.get("sha1") != published.hashes.get("sha1") {
        return Err(format!("{} sha1 differs", ours.path).into());
    }
    if ours.hashes.get("sha512") != published.hashes.get("sha512") {
        return Err(format!("{} sha512 differs", ours.path).into());
    }
    let client = ours.env.client.as_str();
    let server = ours.env.server.as_str();
    if client != published.env.client {
        return Err(format!(
            "{} client env {} != {}",
            ours.path, client, published.env.client
        )
        .into());
    }
    if server != published.env.server {
        return Err(format!(
            "{} server env {} != {}",
            ours.path, server, published.env.server
        )
        .into());
    }
    Ok(())
}

pub fn default_against(lock: &Lockfile) -> String {
    format!(
        "https://maven.kaf.sh/{}/{}/{}/{}-{}.mrpack",
        lock.pack.group.replace('.', "/"),
        lock.pack.slug,
        lock.pack.version,
        lock.pack.slug,
        lock.pack.version
    )
}

pub fn default_against_from_root(root: &PackRoot) -> Result<String> {
    Ok(default_against(&crate::load_lock(root)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{EnvSpec, FileSpec, PackMeta, SideRequirement};
    use std::io::Write;
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(cursor);
        for (name, contents) in entries {
            zip.start_file(*name, SimpleFileOptions::default())
                .expect("archive entry");
            zip.write_all(contents).expect("archive contents");
        }
        zip.finish().expect("archive").into_inner()
    }

    #[test]
    fn reports_env_mismatch() {
        let ours = index_from_lock(&Lockfile {
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
                path: "mods/a.jar".into(),
                file_size: 1,
                sha1: "a".repeat(40),
                sha512: "b".repeat(128),
                env: EnvSpec {
                    client: SideRequirement::Required,
                    server: SideRequirement::Unsupported,
                },
                downloads: vec!["https://example.invalid/a.jar".into()],
            }],
            curseforge: Vec::new(),
        })
        .expect("index");
        let published = ParsedIndex {
            format_version: 1,
            game: "minecraft".into(),
            version_id: "1.1.1".into(),
            name: "FOREVER WORLD".into(),
            dependencies: ours.dependencies.clone(),
            files: vec![ParsedFile {
                path: "mods/a.jar".into(),
                hashes: ours.files[0].hashes.clone(),
                env: ParsedEnv {
                    client: "required".into(),
                    server: "required".into(),
                },
                downloads: ours.files[0].downloads.clone(),
                file_size: 1,
            }],
        };
        let error = compare(&ours, &published).expect_err("env mismatch");
        assert!(error.to_string().contains("server env"));
    }

    #[test]
    fn compares_every_uncompressed_archive_entry() {
        let ours = archive(&[
            ("modrinth.index.json", b"{}"),
            ("overrides/config/example.toml", b"enabled = true\n"),
        ]);
        let same_contents = archive(&[
            ("modrinth.index.json", b"{}"),
            ("overrides/config/example.toml", b"enabled = true\n"),
        ]);
        compare_archive_entries(&ours, &same_contents).expect("matching entries");

        let changed = archive(&[
            ("modrinth.index.json", b"{}"),
            ("overrides/config/example.toml", b"enabled = false\n"),
        ]);
        let error = compare_archive_entries(&ours, &changed).expect_err("changed override");
        assert!(error.to_string().contains("example.toml differs"));

        let extra = archive(&[
            ("modrinth.index.json", b"{}"),
            ("overrides/config/example.toml", b"enabled = true\n"),
            ("overrides/options.txt", b"extra"),
        ]);
        let error = compare_archive_entries(&ours, &extra).expect_err("extra entry");
        assert!(error.to_string().contains("extra entry"));
    }
}
