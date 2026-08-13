use forever_world::spec::{Lockfile, PackSpec, SideRequirement, server_file};
use forever_world::{PackRoot, load_lock, load_spec};
use std::collections::BTreeSet;
use std::path::PathBuf;

fn root() -> PackRoot {
    PackRoot {
        path: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
    }
}

/// Jars allowed to load on the dedicated server. Anything that writes blocks
/// into the save has to be a Kaf mod. Libraries and client-sync extras can sit
/// here only if they do not add foreign blocks.
const SERVER_JARS: &[&str] = &[
    "mods/CreativeCore_FABRIC_v2.14.16_mc26.2.jar",
    "mods/ForgeConfigAPIPort-v26.2.1-mc26.2.x-Fabric.jar",
    "mods/PuzzlesLib-v26.2.0-mc26.2.x-Fabric.jar",
    "mods/amber-fabric-11.1.2+26.2.jar",
    "mods/appleskin-fabric-mc26.2-3.0.10.jar",
    "mods/bonded-fabric-4.1.0+26.2.jar",
    "mods/c2me-fabric-mc26.2-0.4.2-alpha.0.13.jar",
    "mods/fabric-api-0.154.2+26.2.jar",
    "mods/fabric-language-kotlin-1.13.12+kotlin.2.4.0.jar",
    "mods/ferritecore-9.0.0-fabric.jar",
    "mods/happyghastimprovements-fabric-2.1.0+26.2.jar",
    "mods/jei-26.2-fabric-30.9.0.57.jar",
    "mods/kafvalentine-fabric-5.1.0+26.2.jar",
    "mods/konfig-fabric-0.5.0+26.2.jar",
    "mods/liteminer-fabric-4.1.1+26.2.jar",
    "mods/lithium-fabric-0.25.2+mc26.2.jar",
    "mods/mochila-fabric-6.1.0+26.2.jar",
    "mods/mru-1.0.30+26.2-fabric.jar",
    "mods/placeholder-api-3.1.0-beta.1+26.2.jar",
    "mods/sit-fabric-26.1.1-1.5.1.jar",
    "mods/snapshears-fabric-5.1.0+26.2.jar",
    "mods/sound-physics-remastered-fabric-1.5.1+26.2.jar",
    "mods/torchtoss-fabric-5.1.0+26.2.jar",
    "mods/yet_another_config_lib_v3-3.9.5+26.2-fabric.jar",
];

const PERSISTENT_CONTENT_JARS: &[&str] = &[
    "mods/amber-fabric-11.1.2+26.2.jar",
    "mods/bonded-fabric-4.1.0+26.2.jar",
    "mods/happyghastimprovements-fabric-2.1.0+26.2.jar",
    "mods/kafvalentine-fabric-5.1.0+26.2.jar",
    "mods/konfig-fabric-0.5.0+26.2.jar",
    "mods/liteminer-fabric-4.1.1+26.2.jar",
    "mods/mochila-fabric-6.1.0+26.2.jar",
    "mods/snapshears-fabric-5.1.0+26.2.jar",
    "mods/torchtoss-fabric-5.1.0+26.2.jar",
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
    let expected: BTreeSet<&str> = SERVER_JARS.iter().copied().collect();
    let actual: BTreeSet<&str> = spec
        .file
        .iter()
        .filter(|file| server_file(file))
        .map(|file| file.path.as_str())
        .collect();
    assert_eq!(
        actual, expected,
        "the dedicated-server jar set changed; review every new jar against the save promise"
    );
    for path in PERSISTENT_CONTENT_JARS {
        assert!(
            actual.contains(path),
            "{path} must load on the server; the save already contains its content"
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
