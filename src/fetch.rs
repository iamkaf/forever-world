use crate::spec::{FileSpec, check_pack_path};
use crate::{PackRoot, Result, USER_AGENT, hash};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub fn cached_file(root: &PackRoot, file: &FileSpec) -> PathBuf {
    let name = Path::new(&file.path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file.bin");
    root.cache_dir()
        .join("objects")
        .join(&file.sha512)
        .join(name)
}

pub fn ensure_cached(root: &PackRoot, file: &FileSpec) -> Result<PathBuf> {
    check_pack_path(&file.path)?;
    let dest = cached_file(root, file);
    if dest.is_file() {
        let bytes = fs::read(&dest)?;
        verify_bytes(file, &bytes)?;
        return Ok(dest);
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = download_first(file)?;
    let tmp = dest.with_extension("tmp");
    {
        let mut out = fs::File::create(&tmp)?;
        out.write_all(&bytes)?;
    }
    fs::rename(tmp, &dest)?;
    Ok(dest)
}

fn download_first(file: &FileSpec) -> Result<Vec<u8>> {
    let mut last_error = None;
    for url in &file.downloads {
        match download(url) {
            Ok(bytes) => match verify_bytes(file, &bytes) {
                Ok(()) => return Ok(bytes),
                Err(error) => last_error = Some(error),
            },
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| format!("{} had no usable download", file.path).into()))
}

fn download(url: &str) -> Result<Vec<u8>> {
    if let Some(path) = url.strip_prefix("file:") {
        let path = path.trim_start_matches("//");
        return Ok(fs::read(path)?);
    }
    let client = reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(180))
        .build()?;
    let response = client.get(url).send()?.error_for_status()?;
    Ok(response.bytes()?.to_vec())
}

fn verify_bytes(file: &FileSpec, bytes: &[u8]) -> Result<()> {
    if bytes.len() as u64 != file.file_size {
        return Err(format!(
            "{} size {} did not match pin {}",
            file.path,
            bytes.len(),
            file.file_size
        )
        .into());
    }
    let sha1 = hash::sha1_hex(bytes);
    if sha1 != file.sha1 {
        return Err(format!("{} sha1 {sha1} did not match pin {}", file.path, file.sha1).into());
    }
    let sha512 = hash::sha512_hex(bytes);
    if sha512 != file.sha512 {
        return Err(format!("{} sha512 did not match pin {}", file.path, file.sha512).into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{EnvSpec, SideRequirement};

    #[test]
    fn tries_the_next_mirror_after_invalid_bytes() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let bad = dir.path().join("bad.jar");
        let good = dir.path().join("good.jar");
        fs::write(&bad, b"wrong").expect("bad mirror");
        fs::write(&good, b"right").expect("good mirror");
        let file = FileSpec {
            path: "mods/example.jar".into(),
            file_size: 5,
            sha1: hash::sha1_hex(b"right"),
            sha512: hash::sha512_hex(b"right"),
            env: EnvSpec {
                client: SideRequirement::Required,
                server: SideRequirement::Required,
            },
            downloads: vec![
                format!("file:{}", bad.display()),
                format!("file:{}", good.display()),
            ],
        };

        assert_eq!(download_first(&file).expect("valid mirror"), b"right");
    }
}
