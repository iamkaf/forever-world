use crate::hash;
use crate::spec::Lockfile;
use crate::{PackRoot, Result, USER_AGENT};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::time::Duration;

const PUBLISH_BASE: &str = "https://z.kaf.sh";
const PUBLISH_REPOSITORY: &str = "releases";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishMode {
    DryRun,
    Confirmed { version: String },
}

#[derive(Serialize)]
struct PublishMeta {
    group: String,
    artifact: String,
    version: String,
    #[serde(rename = "packFile")]
    pack_file: String,
    pom: String,
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

pub fn publish(root: &PackRoot, mode: PublishMode) -> Result<Vec<String>> {
    let lock = crate::load_lock(root)?;
    let version = lock.pack.version.clone();
    if let PublishMode::Confirmed { version: confirmed } = &mode
        && confirmed != &version
    {
        return Err(
            format!("publish confirmation `{confirmed}` does not match `{version}`").into(),
        );
    }

    let exported_name = lock.pack.mrpack_name();
    let mrpack = root.dist_dir().join(&exported_name);
    if !mrpack.is_file() {
        return Err("missing exported .mrpack; run `pack export` first".into());
    }
    let pack_file = exported_name;
    let mrpack_bytes = fs::read(&mrpack)?;
    write_sha512(root, &pack_file, &hash::sha512_hex(&mrpack_bytes))?;

    let pom_name = format!("{}-{version}.pom", lock.pack.slug);
    let pom = minimal_pom(&lock.pack.group, &lock.pack.slug, &version, &lock.pack.name);
    fs::write(root.dist_dir().join(&pom_name), &pom)?;
    write_sha512(root, &pom_name, &hash::sha512_hex(pom.as_bytes()))?;

    let group_path = lock.pack.group.replace('.', "/");
    let artifact_prefix = format!("{group_path}/{}", lock.pack.slug);
    let metadata_url =
        format!("{PUBLISH_BASE}/{PUBLISH_REPOSITORY}/{artifact_prefix}/maven-metadata.xml");
    let client = reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(300))
        .build()?;
    let versions = merged_versions(&client, &metadata_url, &version)?;
    let metadata = metadata_xml(&lock, &version, &versions);
    fs::write(root.dist_dir().join("maven-metadata.xml"), metadata)?;

    let publish_json = PublishMeta {
        group: lock.pack.group.clone(),
        artifact: lock.pack.slug.clone(),
        version: version.clone(),
        pack_file: pack_file.clone(),
        pom: pom_name.clone(),
    };
    let mut json = serde_json::to_vec_pretty(&publish_json)?;
    json.push(b'\n');
    fs::write(root.dist_dir().join("publish.json"), json)?;

    let version_prefix = format!("{artifact_prefix}/{version}");
    let uploads = [
        (pack_file.clone(), format!("{version_prefix}/{pack_file}")),
        (
            format!("{pack_file}.sha512"),
            format!("{version_prefix}/{pack_file}.sha512"),
        ),
        (pom_name.clone(), format!("{version_prefix}/{pom_name}")),
        (
            format!("{pom_name}.sha512"),
            format!("{version_prefix}/{pom_name}.sha512"),
        ),
        (
            "maven-metadata.xml".to_string(),
            format!("{artifact_prefix}/maven-metadata.xml"),
        ),
    ];

    if mode == PublishMode::DryRun {
        return Ok(uploads
            .iter()
            .map(|(_, key)| format!("DRY {PUBLISH_BASE}/{PUBLISH_REPOSITORY}/{key}"))
            .collect());
    }

    let username = std::env::var("MAVEN_PUBLISH_USERNAME")
        .map_err(|_| crate::Error::from("set MAVEN_PUBLISH_USERNAME"))?;
    let password = std::env::var("MAVEN_PUBLISH_PASSWORD")
        .map_err(|_| crate::Error::from("set MAVEN_PUBLISH_PASSWORD"))?;
    let mut uploaded = Vec::new();
    for (name, key) in &uploads {
        let path = root.dist_dir().join(name);
        let url = format!("{PUBLISH_BASE}/{PUBLISH_REPOSITORY}/{key}");
        put_file(&client, &url, &path, &username, &password)?;
        uploaded.push(key.clone());
    }
    Ok(uploaded)
}

fn merged_versions(
    client: &reqwest::blocking::Client,
    metadata_url: &str,
    version: &str,
) -> Result<Vec<String>> {
    let response = client.get(metadata_url).send()?;
    let mut versions = BTreeSet::new();
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        versions.insert(version.to_string());
        return Ok(versions.into_iter().collect());
    }
    let response = response.error_for_status()?;
    let text = response.text()?;
    let existing: ExistingMetadata =
        quick_xml::de::from_str(&text).map_err(crate::Error::from_display)?;
    versions.extend(existing.versioning.versions.version);
    versions.insert(version.to_string());
    Ok(versions.into_iter().collect())
}

fn metadata_xml(lock: &Lockfile, version: &str, versions: &[String]) -> String {
    let mut version_rows = String::new();
    for existing in versions {
        version_rows.push_str(&format!("      <version>{}</version>\n", xml(existing)));
    }
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
        xml(&lock.pack.group),
        xml(&lock.pack.slug),
        xml(version),
        xml(version),
        version_rows
    )
}

fn write_sha512(root: &PackRoot, name: &str, digest: &str) -> Result<()> {
    fs::write(
        root.dist_dir().join(format!("{name}.sha512")),
        format!("{digest}  {name}\n"),
    )?;
    Ok(())
}

fn minimal_pom(group: &str, artifact: &str, version: &str, name: &str) -> String {
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
  <description>Minecraft modpack (.mrpack) published for Pastel</description>
</project>
"#,
        xml(group),
        xml(artifact),
        xml(version),
        xml(name)
    )
}

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn put_file(
    client: &reqwest::blocking::Client,
    url: &str,
    path: &std::path::Path,
    username: &str,
    password: &str,
) -> Result<()> {
    if !url.starts_with("https://") {
        return Err("publish URL must use HTTPS".into());
    }
    let bytes = fs::read(path)?;
    let response = client
        .put(url)
        .basic_auth(username, Some(password))
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .body(bytes)
        .send()?;
    let status = response.status();
    if status == reqwest::StatusCode::CONFLICT {
        return Err(format!("{url} already exists (immutable). Bump the pack version.").into());
    }
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        return Err(format!("{url}: {status}: {}", body.trim()).into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::PackMeta;

    fn lock() -> Lockfile {
        Lockfile {
            version: 1,
            pack: PackMeta {
                name: "FOREVER WORLD".into(),
                slug: "forever-world".into(),
                version: "1.1.2".into(),
                group: "com.iamkaf.modpacks".into(),
                minecraft: "26.2".into(),
                loader: "fabric".into(),
                loader_version: "0.19.3".into(),
            },
            file: vec![],
        }
    }

    #[test]
    fn metadata_keeps_older_versions() {
        let metadata = metadata_xml(&lock(), "1.1.2", &["1.1.1".into(), "1.1.2".into()]);
        assert!(metadata.contains("<version>1.1.1</version>"));
        assert!(metadata.contains("<version>1.1.2</version>"));
        assert!(metadata.contains("<latest>1.1.2</latest>"));
    }
}
