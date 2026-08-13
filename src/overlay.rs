use crate::spec::{FileSpec, Lockfile, client_file, server_file};
use crate::{PackRoot, Result, fetch};
use std::fs;
use std::path::Path;

/// Test-only extra. Never export this into the `.mrpack`.
pub const TEAKIT_FABRIC: &str = "maven:com.iamkaf.teakit:teakit-fabric:0.14.0+26.2";

pub fn overlay(root: &PackRoot) -> Result<std::path::PathBuf> {
    let lock = crate::load_lock(root)?;
    for file in &lock.file {
        fetch::ensure_cached(root, file)?;
    }
    fs::create_dir_all(root.generated_dir())?;
    let dest = root.generated_dir().join("modstage.toml");
    fs::write(&dest, overlay_toml(&lock))?;
    Ok(dest)
}

pub(crate) fn overlay_toml(lock: &Lockfile) -> String {
    let mut out = String::from(
        "# Generated from pack.lock.toml. Do not edit; do not commit.\n\
         [project]\n\
         name = \"forever-world\"\n\
         \n\
         [repositories]\n\
         mavenLocal = \"mavenLocal\"\n\
         kaf = \"https://maven.kaf.sh\"\n",
    );

    write_instance(
        &mut out,
        "forever-world-server",
        lock,
        &["server"],
        lock.file.iter().filter(|file| server_file(file)),
        &[],
        true,
    );

    write_instance(
        &mut out,
        "forever-world-client",
        lock,
        &["client"],
        lock.file
            .iter()
            .filter(|file| client_file(file) && Path::new(&file.path).starts_with("mods")),
        &[],
        false,
    );
    write_shader_fixtures(
        &mut out,
        lock.file
            .iter()
            .filter(|file| client_file(file) && Path::new(&file.path).starts_with("shaderpacks")),
    );

    write_instance(
        &mut out,
        "forever-world-pair",
        lock,
        &["client", "server"],
        lock.file.iter().filter(|file| server_file(file)),
        &[TEAKIT_FABRIC],
        true,
    );
    write_mod_fixtures(
        &mut out,
        lock.file.iter().filter(|file| {
            client_file(file) && !server_file(file) && Path::new(&file.path).starts_with("mods")
        }),
    );
    write_shader_fixtures(
        &mut out,
        lock.file
            .iter()
            .filter(|file| client_file(file) && Path::new(&file.path).starts_with("shaderpacks")),
    );

    out
}

fn write_instance<'a>(
    out: &mut String,
    name: &str,
    lock: &Lockfile,
    sides: &[&str],
    mods: impl Iterator<Item = &'a FileSpec>,
    extra_mods: &[&str],
    server_properties: bool,
) {
    out.push_str("\n[[instance]]\n");
    out.push_str(&format!("name = {}\n", toml_string(name)));
    out.push_str(&format!(
        "minecraft = {}\n",
        toml_string(&lock.pack.minecraft)
    ));
    out.push_str(&format!("loader = {}\n", toml_string(&lock.pack.loader)));
    out.push_str(&format!(
        "loader_version = {}\n",
        toml_string(&lock.pack.loader_version)
    ));
    let side_list = sides
        .iter()
        .map(|side| toml_string(side))
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!("sides = [{side_list}]\n"));
    if server_properties {
        out.push_str(
            "server_properties = { online-mode = \"false\", enforce-secure-profile = \"false\" }\n",
        );
    }
    out.push_str("mods = [\n");
    for file in mods {
        out.push_str(&format!("  {},\n", toml_string(&cache_relative(file))));
    }
    for extra in extra_mods {
        out.push_str(&format!("  {},\n", toml_string(extra)));
    }
    out.push_str("]\n");
}

fn write_mod_fixtures<'a>(out: &mut String, files: impl Iterator<Item = &'a FileSpec>) {
    for file in files {
        write_fixture(out, file, &file.path);
    }
}

fn write_shader_fixtures<'a>(out: &mut String, files: impl Iterator<Item = &'a FileSpec>) {
    for file in files {
        write_fixture(out, file, &file.path);
    }
}

fn write_fixture(out: &mut String, file: &FileSpec, to: &str) {
    out.push_str("\n[[instance.fixture]]\n");
    out.push_str(&format!("from = {}\n", toml_string(&cache_relative(file))));
    out.push_str(&format!("to = {}\n", toml_string(to)));
    out.push_str("side = \"client\"\n");
    out.push_str("replace = true\n");
}

fn cache_relative(file: &FileSpec) -> String {
    let name = Path::new(&file.path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file.bin");
    format!("../.cache/objects/{}/{}", file.sha512, name)
}

fn toml_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{EnvSpec, PackMeta, SideRequirement};

    fn sample_lock() -> Lockfile {
        Lockfile {
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
                file(
                    "mods/amber-fabric-11.1.2+26.2.jar",
                    SideRequirement::Required,
                    SideRequirement::Required,
                    "aa".repeat(64),
                ),
                file(
                    "mods/sodium-fabric-0.9.1+mc26.2.jar",
                    SideRequirement::Required,
                    SideRequirement::Unsupported,
                    "bb".repeat(64),
                ),
                file(
                    "shaderpacks/BSL_v10.1.3.zip",
                    SideRequirement::Required,
                    SideRequirement::Unsupported,
                    "cc".repeat(64),
                ),
            ],
        }
    }

    fn file(
        path: &str,
        client: SideRequirement,
        server: SideRequirement,
        sha512: String,
    ) -> FileSpec {
        FileSpec {
            path: path.into(),
            file_size: 1,
            sha1: "a".repeat(40),
            sha512,
            env: EnvSpec { client, server },
            downloads: vec!["https://cdn.modrinth.com/x".into()],
        }
    }

    #[test]
    fn pair_layers_teakit_without_putting_client_only_jars_on_the_server() {
        let toml = overlay_toml(&sample_lock());
        assert!(toml.contains("name = \"forever-world-pair\""));
        assert!(toml.contains(TEAKIT_FABRIC));
        assert!(toml.contains("sides = [\"client\", \"server\"]"));
        assert!(toml.contains("loader_version = \"0.19.3\""));
        let pair = toml
            .split("name = \"forever-world-pair\"")
            .nth(1)
            .expect("pair instance");
        assert!(pair.contains("amber-fabric-11.1.2+26.2.jar"));
        assert!(pair.contains("to = \"mods/sodium-fabric-0.9.1+mc26.2.jar\""));
        let pair_mods = pair.split("[[instance.fixture]]").next().expect("mods");
        assert!(
            !pair_mods.contains("sodium-fabric"),
            "sodium must be a client fixture, not a shared mod:\n{pair_mods}"
        );
        let server = toml
            .split("name = \"forever-world-server\"")
            .nth(1)
            .expect("server")
            .split("[[instance]]")
            .next()
            .expect("server body");
        assert!(!server.contains(TEAKIT_FABRIC));
        assert!(!server.contains("sodium-fabric"));
        assert!(!server.contains("shaderpacks"));
    }
}
