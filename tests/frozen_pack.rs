use forever_world::spec::{Lockfile, PackSpec, SideRequirement, server_file};
use forever_world::{PackRoot, load_lock, load_spec};
use std::collections::BTreeSet;
use std::path::PathBuf;

fn root() -> PackRoot {
    PackRoot {
        path: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
    }
}

fn jar_stem(path: &str) -> &str {
    path.rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".jar")
}

fn jar_family(path: &str) -> String {
    let stem = jar_stem(path);
    match stem.split_once(['-', '_', '+']) {
        Some((family, _)) => family.to_ascii_lowercase(),
        None => stem.to_ascii_lowercase(),
    }
}

/// Jars allowed to load on the dedicated server. Anything that writes blocks
/// into the save has to be a Kaf mod. Libraries and client-sync extras can sit
/// here only if they do not add foreign blocks.
const SERVER_FAMILIES: &[&str] = &[
    "amber",
    "appleskin",
    "bonded",
    "c2me",
    "creativecore",
    "fabric",
    "ferritecore",
    "forgeconfigapiport",
    "happyghastimprovements",
    "jei",
    "kafvalentine",
    "konfig",
    "liteminer",
    "lithium",
    "mochila",
    "mru",
    "placeholder",
    "puzzleslib",
    "sit",
    "snapshears",
    "sound",
    "torchtoss",
    "yet",
];

const CONTENT_FAMILIES: &[&str] = &[
    "amber",
    "bonded",
    "happyghastimprovements",
    "kafvalentine",
    "konfig",
    "liteminer",
    "mochila",
    "snapshears",
    "torchtoss",
];

#[test]
fn published_1_1_1_stays_frozen() {
    let spec = load_spec(&root()).expect("pack.toml");
    let lock = load_lock(&root()).expect("pack.lock.toml");
    assert_eq!(spec.pack.name, "FOREVER WORLD");
    assert_eq!(spec.pack.version, "1.1.1");
    assert_eq!(spec.pack.minecraft, "26.2");
    assert_eq!(spec.pack.loader, "fabric");
    assert_eq!(spec.pack.loader_version, "0.19.3");
    assert_eq!(spec.file.len(), 63);
    assert_eq!(
        Lockfile::from_spec(PackSpec {
            pack: spec.pack.clone(),
            file: spec.file.clone(),
        }),
        lock
    );
}

#[test]
fn only_allowed_jars_load_on_the_server() {
    let spec = load_spec(&root()).expect("pack.toml");
    let allowed: BTreeSet<&str> = SERVER_FAMILIES.iter().copied().collect();
    let mut server_families = BTreeSet::new();
    for file in spec.file.iter().filter(|file| server_file(file)) {
        assert!(
            file.path.starts_with("mods/"),
            "{} is server-side but not a mod jar",
            file.path
        );
        let family = jar_family(&file.path);
        assert!(
            allowed.contains(family.as_str()),
            "{} loaded on the server; that puts someone else's world content in the save",
            file.path
        );
        server_families.insert(family);
    }
    for family in CONTENT_FAMILIES {
        assert!(
            server_families.iter().any(|found| found == family),
            "{family} must load on the server; the save already contains it"
        );
    }
}

#[test]
fn shaders_and_client_perf_stay_off_the_server() {
    let spec = load_spec(&root()).expect("pack.toml");
    for file in &spec.file {
        if file.path.starts_with("shaderpacks/")
            || file.path.contains("sodium")
            || file.path.contains("iris-")
            || file.path.contains("kafhud")
            || file.path.contains("gentlehurtcam")
        {
            assert_eq!(
                file.env.server,
                SideRequirement::Unsupported,
                "{} must not load on the dedicated server",
                file.path
            );
        }
    }
}
