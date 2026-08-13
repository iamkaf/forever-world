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
        "name" => {
            require_no_args(&args, "name")?;
            println!("{}", forever_world::load_lock(&root)?.pack.mrpack_name());
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
            let lock = forever_world::load_lock(&root)?;
            let version = lock.pack.version.clone();
            let mode = parse_publish_mode(&args, &version)?;
            let uploaded = publish::publish(&root, mode)?;
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

fn require_no_args(args: &[String], command: &str) -> forever_world::Result<()> {
    if args.is_empty() {
        Ok(())
    } else {
        Err(format!("`pack {command}` does not accept arguments").into())
    }
}

fn parse_publish_mode(
    args: &[String],
    expected_version: &str,
) -> forever_world::Result<publish::PublishMode> {
    match args {
        [flag] if flag == "--dry-run" => Ok(publish::PublishMode::DryRun),
        [flag, version] if flag == "--confirm" && version == expected_version => {
            Ok(publish::PublishMode::Confirmed {
                version: version.clone(),
            })
        }
        [flag, version] if flag == "--confirm" => Err(format!(
            "publish confirmation `{version}` does not match `{expected_version}`"
        )
        .into()),
        [] => {
            Err(format!("publishing requires `--dry-run` or `--confirm {expected_version}`").into())
        }
        _ => Err(format!(
            "invalid publish arguments; use `--dry-run` or `--confirm {expected_version}`"
        )
        .into()),
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
  pack name                 Print the exported archive name from the lock
  pack verify [--against]   Compare the full archive to a published artifact
  pack overlay              Write generated/modstage.toml for client and server boots
  pack publish --dry-run    Show release upload keys
  pack publish --confirm <version>
                            Publish to the release repository
"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn publishing_requires_an_exact_mode_and_version() {
        assert_eq!(
            parse_publish_mode(&args(&["--dry-run"]), "1.1.2").expect("dry run"),
            publish::PublishMode::DryRun
        );
        assert_eq!(
            parse_publish_mode(&args(&["--confirm", "1.1.2"]), "1.1.2").expect("confirmed publish"),
            publish::PublishMode::Confirmed {
                version: "1.1.2".into()
            }
        );
        assert!(parse_publish_mode(&[], "1.1.2").is_err());
        assert!(parse_publish_mode(&args(&["--dry-rnu"]), "1.1.2").is_err());
        assert!(parse_publish_mode(&args(&["--confirm", "1.1.1"]), "1.1.2").is_err());
    }
}
