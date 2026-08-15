use crate::spec::{ContentKind, ContentSource, ContentSpec, FileSpec, PackMeta, SourceProvider};
use crate::{PackRoot, Result, USER_AGENT, fetch, hash};
use reqwest::blocking::Client;
use serde::Deserialize;
use std::time::Duration;

const MODRINTH_API: &str = "https://api.modrinth.com/v2";

pub struct Resolver {
    client: Client,
}

impl Resolver {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .user_agent(USER_AGENT)
                .timeout(Duration::from_secs(60))
                .build()?,
        })
    }

    pub fn resolve(
        &self,
        root: &PackRoot,
        pack: &PackMeta,
        kind: ContentKind,
        content: &ContentSpec,
    ) -> Result<FileSpec> {
        match &content.source {
            ContentSource::Modrinth { modrinth, version } => {
                self.resolve_modrinth(pack, kind, content, modrinth, version)
            }
            ContentSource::Direct {
                id,
                version,
                filename,
                url,
            } => {
                let bytes = fetch::download(url)
                    .map_err(|error| format!("could not download direct content {id}: {error}"))?;
                let file = FileSpec {
                    id: id.to_string(),
                    provider: SourceProvider::Direct,
                    requested_version: version.to_string(),
                    path: format!("{}/{}", kind.folder(), filename),
                    file_size: bytes.len() as u64,
                    sha1: hash::sha1_hex(&bytes),
                    sha512: hash::sha512_hex(&bytes),
                    env: content.side.env(),
                    downloads: vec![url.to_string()],
                };
                fetch::cache_bytes(root, &file, &bytes)?;
                Ok(file)
            }
        }
    }

    fn resolve_modrinth(
        &self,
        pack: &PackMeta,
        kind: ContentKind,
        content: &ContentSpec,
        project: &str,
        requested_version: &str,
    ) -> Result<FileSpec> {
        let url = format!("{MODRINTH_API}/project/{project}/version");
        let request = self.client.get(&url);
        let request = match kind {
            ContentKind::Mod => request.query(&[
                ("loaders", serde_json::to_string(&[pack.loader.as_str()])?),
                (
                    "game_versions",
                    serde_json::to_string(&[pack.minecraft.as_str()])?,
                ),
            ]),
            ContentKind::Shader => request,
        };
        let response = request
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(|error| format!("could not resolve Modrinth project {project}: {error}"))?;
        let versions: Vec<ModrinthVersion> =
            serde_json::from_str(&response.text().map_err(|error| {
                format!("could not read Modrinth response for {project}: {error}")
            })?)
            .map_err(|error| format!("invalid Modrinth response for {project}: {error}"))?;
        let version = exact_version(project, requested_version, &versions)?;
        let file = primary_file(project, requested_version, &version.files)?;
        Ok(FileSpec {
            id: project.to_string(),
            provider: SourceProvider::Modrinth,
            requested_version: requested_version.to_string(),
            path: format!("{}/{}", kind.folder(), file.filename),
            file_size: file.size,
            sha1: file.hashes.sha1.clone(),
            sha512: file.hashes.sha512.clone(),
            env: content.side.env(),
            downloads: vec![file.url.clone()],
        })
    }
}

#[derive(Debug, Deserialize)]
struct ModrinthVersion {
    id: String,
    version_number: String,
    files: Vec<ModrinthFile>,
}

#[derive(Debug, Deserialize)]
struct ModrinthFile {
    hashes: ModrinthHashes,
    url: String,
    filename: String,
    primary: bool,
    size: u64,
}

#[derive(Debug, Deserialize)]
struct ModrinthHashes {
    sha1: String,
    sha512: String,
}

fn exact_version<'a>(
    project: &str,
    requested: &str,
    versions: &'a [ModrinthVersion],
) -> Result<&'a ModrinthVersion> {
    let matches: Vec<_> = versions
        .iter()
        .filter(|version| version.version_number == requested)
        .collect();
    match matches.as_slice() {
        [version] => Ok(version),
        [] => Err(
            format!("Modrinth project {project} has no compatible version `{requested}`").into(),
        ),
        versions => {
            let first = versions[0];
            let first_file = primary_file(project, requested, &first.files)?;
            let same_file = versions.iter().skip(1).all(|version| {
                primary_file(project, requested, &version.files)
                    .is_ok_and(|file| file.hashes.sha512 == first_file.hashes.sha512)
            });
            if same_file {
                return Ok(first);
            }
            let ids = versions
                .iter()
                .map(|version| version.id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            Err(format!(
                "Modrinth project {project} has multiple compatible versions named `{requested}`: {ids}"
            )
            .into())
        }
    }
}

fn primary_file<'a>(
    project: &str,
    version: &str,
    files: &'a [ModrinthFile],
) -> Result<&'a ModrinthFile> {
    let primary: Vec<_> = files.iter().filter(|file| file.primary).collect();
    match primary.as_slice() {
        [file] => Ok(file),
        [] if files.len() == 1 => Ok(&files[0]),
        [] => Err(
            format!("Modrinth project {project} version `{version}` has no primary file").into(),
        ),
        _ => Err(format!(
            "Modrinth project {project} version `{version}` has multiple primary files"
        )
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, primary: bool) -> ModrinthFile {
        ModrinthFile {
            hashes: ModrinthHashes {
                sha1: "a".repeat(40),
                sha512: "b".repeat(128),
            },
            url: format!("https://example.invalid/{name}"),
            filename: name.into(),
            primary,
            size: 1,
        }
    }

    #[test]
    fn exact_versions_and_primary_files_must_be_unambiguous() {
        let versions = vec![ModrinthVersion {
            id: "version-id".into(),
            version_number: "1.2.3".into(),
            files: vec![file("main.jar", true), file("sources.jar", false)],
        }];
        let version = exact_version("example", "1.2.3", &versions).expect("exact version");
        assert_eq!(
            primary_file("example", "1.2.3", &version.files)
                .expect("primary file")
                .filename,
            "main.jar"
        );
        assert!(exact_version("example", "latest", &versions).is_err());

        let duplicate = ModrinthVersion {
            id: "duplicate-version-id".into(),
            version_number: "1.2.3".into(),
            files: vec![file("main.jar", true)],
        };
        let mut versions = vec![versions.into_iter().next().expect("version"), duplicate];
        assert_eq!(
            exact_version("example", "1.2.3", &versions)
                .expect("duplicate metadata for the same file")
                .id,
            "version-id"
        );

        let mut conflicting = file("other.jar", true);
        conflicting.hashes.sha512 = "c".repeat(128);
        let conflict = ModrinthVersion {
            id: "conflicting-version-id".into(),
            version_number: "1.2.3".into(),
            files: vec![conflicting],
        };
        assert!(exact_version("example", "1.2.3", &[versions.remove(0), conflict]).is_err());
    }
}
