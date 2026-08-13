use forever_world::spec::Lockfile;
use forever_world::{PackRoot, export, fetch, overlay, publish, verify};
use std::env;
use std::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> forever_world::Result<()> {
    let mut args = args;
    if args.is_empty() {
        print_help();
        return Ok(());
    }
    let command = args.remove(0);
    let root = PackRoot::discover(&env::current_dir()?)?;
    match command.as_str() {
        "resolve" => {
            let lock = resolve(&root)?;
            eprintln!("locked {} files", lock.file.len());
            Ok(())
        }
        "export" => {
            let dest = export::export(&root)?;
            eprintln!("wrote {}", dest.display());
            Ok(())
        }
        "verify" => {
            let against = flag_value(&args, "--against")
                .map(ToOwned::to_owned)
                .map(Ok)
                .unwrap_or_else(|| verify::default_against_from_root(&root))?;
            verify::verify(&root, &against)
        }
        "overlay" => {
            let dest = overlay::overlay(&root)?;
            eprintln!("wrote {}", dest.display());
            Ok(())
        }
        "publish" => {
            let dry_run = args.iter().any(|arg| arg == "--dry-run");
            let uploaded = publish::publish(&root, dry_run)?;
            for item in uploaded {
                println!("{item}");
            }
            Ok(())
        }
        "-h" | "--help" | "help" => {
            print_help();
            Ok(())
        }
        other => Err(format!("unknown command `{other}`").into()),
    }
}

fn resolve(root: &PackRoot) -> forever_world::Result<Lockfile> {
    let spec = forever_world::load_spec(root)?;
    let total = spec.file.len();
    for (index, file) in spec.file.iter().enumerate() {
        eprintln!("[{}/{}] {}", index + 1, total, file.path);
        fetch::ensure_cached(root, file)?;
    }
    let lock = Lockfile::from_spec(spec);
    fs::write(root.lock_toml(), lock.to_toml()?)?;
    Ok(lock)
}

fn flag_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.windows(2).find_map(|window| {
        if window[0] == name {
            Some(window[1].as_str())
        } else {
            None
        }
    })
}

fn print_help() {
    eprintln!(
        "\
pack — Forever World pack tool

  pack resolve              Download and hash-verify pinned files, write pack.lock.toml
  pack export               Write dist/<slug>-<version>.mrpack from the lock
  pack verify [--against]   Compare the lock to the published 1.1.1 artifact
  pack overlay              Write generated/modstage.toml for client and server boots
  pack publish --dry-run    Show Kaf Maven upload keys (1.1.1 is already released)
"
    );
}
