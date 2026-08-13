use crate::hash;
use crate::{PackRoot, Result, USER_AGENT};
use serde::Serialize;
use std::fs;
use std::time::Duration;

const PUBLISH_BASE: &str = "https://z.kaf.sh";

#[derive(Serialize)]
struct PublishMeta {
    group: String,
    artifact: String,
    version: String,
    #[serde(rename = "packFile")]
    pack_file: String,
    pom: String,
}

pub fn publish(root: &PackRoot, dry_run: bool) -> Result<Vec<String>> {
    let lock = crate::load_lock(root)?;
    let pack_file = format!("{}-{}.mrpack", lock.pack.slug, lock.pack.version);
    let mrpack = root.dist_dir().join(&pack_file);
    if !mrpack.is_file() {
        return Err("missing exported .mrpack; run `pack export` first".into());
    }
    let pom_name = format!("{}-{}.pom", lock.pack.slug, lock.pack.version);
    let pom = minimal_pom(
        &lock.pack.group,
        &lock.pack.slug,
        &lock.pack.version,
        &lock.pack.name,
    );
    fs::write(root.dist_dir().join(&pom_name), &pom)?;
    write_sha512(root, &pom_name, &hash::sha512_hex(pom.as_bytes()))?;
    let mrpack_sha = root.dist_dir().join(format!("{pack_file}.sha512"));
    if !mrpack_sha.is_file() {
        let bytes = fs::read(&mrpack)?;
        write_sha512(root, &pack_file, &hash::sha512_hex(&bytes))?;
    }
    let metadata = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<metadata>\n\
  <groupId>{}</groupId>\n\
  <artifactId>{}</artifactId>\n\
  <versioning>\n\
    <latest>{}</latest>\n\
    <release>{}</release>\n\
    <versions>\n\
      <version>{}</version>\n\
    </versions>\n\
  </versioning>\n\
</metadata>\n",
        xml(&lock.pack.group),
        xml(&lock.pack.slug),
        xml(&lock.pack.version),
        xml(&lock.pack.version),
        xml(&lock.pack.version)
    );
    fs::write(root.dist_dir().join("maven-metadata.xml"), metadata)?;
    let publish_json = PublishMeta {
        group: lock.pack.group.clone(),
        artifact: lock.pack.slug.clone(),
        version: lock.pack.version.clone(),
        pack_file: pack_file.clone(),
        pom: pom_name.clone(),
    };
    let mut json = serde_json::to_vec_pretty(&publish_json)?;
    json.push(b'\n');
    fs::write(root.dist_dir().join("publish.json"), json)?;

    let group_path = lock.pack.group.replace('.', "/");
    let version_prefix = format!("{}/{}/{}", group_path, lock.pack.slug, lock.pack.version);
    let artifact_prefix = format!("{}/{}", group_path, lock.pack.slug);
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

    if dry_run {
        return Ok(uploads
            .iter()
            .map(|(_, key)| format!("DRY {PUBLISH_BASE}/releases/{key}"))
            .collect());
    }

    let username = std::env::var("MAVEN_PUBLISH_USERNAME")
        .map_err(|_| crate::Error::from("set MAVEN_PUBLISH_USERNAME"))?;
    let password = std::env::var("MAVEN_PUBLISH_PASSWORD")
        .map_err(|_| crate::Error::from("set MAVEN_PUBLISH_PASSWORD"))?;
    let client = reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(300))
        .build()?;
    let mut uploaded = Vec::new();
    for (name, key) in &uploads {
        let path = root.dist_dir().join(name);
        let url = format!("{PUBLISH_BASE}/releases/{key}");
        put_file(&client, &url, &path, &username, &password)?;
        uploaded.push(key.clone());
    }
    Ok(uploaded)
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
    if status.as_u16() == 409 {
        return Err(format!("{url} already exists (immutable). Bump the pack version.").into());
    }
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        return Err(format!("{url}: {status}: {}", body.trim()).into());
    }
    Ok(())
}
