use forever_world::{PackRoot, authoring, publish};
use std::env;
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
        "add" => {
            let (query, version, options) = parse_add_args(&args)?;
            let project = authoring::add(&root, &query, version.as_deref(), options)?;
            let report = authoring::install(&root)?;
            eprintln!("added {project} and installed {} files", report.files);
            Ok(())
        }
        "remove" => {
            if args.len() != 1 {
                return Err("usage: pack remove <project>".into());
            }
            authoring::remove(&root, &args[0])?;
            let report = authoring::install(&root)?;
            eprintln!("removed {} and installed {} files", args[0], report.files);
            Ok(())
        }
        "install" => {
            require_no_args(&args, "install")?;
            let report = authoring::install(&root)?;
            eprintln!(
                "installed {} files and generated {}",
                report.files,
                report.generated.display()
            );
            Ok(())
        }
        "run" => {
            if args.len() != 1 {
                return Err("usage: pack run client|server|pair".into());
            }
            let target = match args[0].as_str() {
                "client" => authoring::RunTarget::Client,
                "server" => authoring::RunTarget::Server,
                "pair" => authoring::RunTarget::Pair,
                target => {
                    return Err(format!(
                        "unknown run target `{target}`; use client, server, or pair"
                    )
                    .into());
                }
            };
            authoring::run(&root, target)
        }
        "publish" => {
            let mode = parse_publish_mode(&args)?;
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

fn parse_publish_mode(args: &[String]) -> forever_world::Result<publish::PublishMode> {
    match args {
        [flag] if flag == "--dry-run" => Ok(publish::PublishMode::DryRun),
        [] => Ok(publish::PublishMode::Publish),
        _ => {
            Err("invalid publish arguments; use `pack publish` or `pack publish --dry-run`".into())
        }
    }
}

fn parse_add_args(
    args: &[String],
) -> forever_world::Result<(String, Option<String>, authoring::AddOptions)> {
    let mut query = None;
    let mut version = None;
    let mut options = authoring::AddOptions::default();
    let mut requested_kind = None;
    let mut requested_side = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--client" => select_option(
                &mut requested_side,
                forever_world::spec::ContentSide::Client,
                "--client and --server cannot be used together",
            )?,
            "--server" => select_option(
                &mut requested_side,
                forever_world::spec::ContentSide::Server,
                "--client and --server cannot be used together",
            )?,
            "--shader" => select_option(
                &mut requested_kind,
                forever_world::spec::ContentKind::Shader,
                "--shader and --mod cannot be used together",
            )?,
            "--mod" => select_option(
                &mut requested_kind,
                forever_world::spec::ContentKind::Mod,
                "--shader and --mod cannot be used together",
            )?,
            "--version" => {
                index += 1;
                version = Some(args.get(index).ok_or("--version requires a value")?.clone());
            }
            value if value.starts_with("--version=") => {
                version = Some(value[10..].to_string());
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown add option `{value}`").into());
            }
            value if query.is_none() => {
                if let Some((project, requested)) = value.split_once('@') {
                    query = Some(project.to_string());
                    if version.is_none() {
                        version = Some(requested.to_string());
                    }
                } else {
                    query = Some(value.to_string());
                }
            }
            value => return Err(format!("unexpected add argument `{value}`").into()),
        }
        index += 1;
    }
    let query = query
        .ok_or("usage: pack add <project> [--version <version>] [--client|--server|--shader]")?;
    options.kind = requested_kind.unwrap_or(options.kind);
    options.side = requested_side;
    if options.kind == forever_world::spec::ContentKind::Shader {
        options.side = Some(forever_world::spec::ContentSide::Client);
    }
    Ok((query, version, options))
}

fn select_option<T: Copy + PartialEq>(
    slot: &mut Option<T>,
    value: T,
    error: &str,
) -> forever_world::Result<()> {
    if slot.is_some_and(|selected| selected != value) {
        return Err(error.into());
    }
    *slot = Some(value);
    Ok(())
}

fn print_help() {
    eprintln!(
        "\
pack — Forever World pack tool

  pack add <project>       Add a Modrinth mod or shader and install it
  pack remove <project>    Remove a mod or shader and install the pack
  pack install              Resolve, download, verify, and prepare the pack
  pack run client           Run the installed client
  pack run server           Run the installed dedicated server
  pack run pair             Run the installed client/server TeaKit pair
  pack publish --dry-run    Show release upload details without uploading
  pack publish               Publish the prepared release to configured targets
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
    fn publishing_accepts_dry_run_or_publish() {
        assert_eq!(
            parse_publish_mode(&args(&["--dry-run"])).expect("dry run"),
            publish::PublishMode::DryRun
        );
        assert_eq!(
            parse_publish_mode(&[]).expect("publish"),
            publish::PublishMode::Publish
        );
        assert!(parse_publish_mode(&args(&["--dry-rnu"])).is_err());
    }

    #[test]
    fn add_rejects_conflicting_content_options() {
        assert!(parse_add_args(&args(&["sodium", "--client", "--server"])).is_err());
        assert!(parse_add_args(&args(&["sodium", "--mod", "--shader"])).is_err());
    }
}
