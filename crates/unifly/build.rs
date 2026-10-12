use std::fs;
use std::path::PathBuf;

use clap::CommandFactory;

// Pull in args.rs directly -- it only depends on clap + clap_complete (both
// listed as build-dependencies), so this compiles cleanly without dragging
// in the rest of the crate.
#[path = "src/cli/args.rs"]
mod args;

fn main() {
    // Only generate man pages when the CLI feature is active.
    if std::env::var("CARGO_FEATURE_CLI").is_err() {
        return;
    }

    // Re-run if the CLI definitions change.
    println!("cargo::rerun-if-changed=src/cli/args.rs");
    println!("cargo::rerun-if-changed=src/cli/args");

    let out_dir: PathBuf = std::env::var_os("OUT_DIR")
        .expect("OUT_DIR not set by Cargo")
        .into();
    let man_dir = out_dir.join("man");
    fs::create_dir_all(&man_dir).expect("failed to create man output directory");

    let cmd = args::Cli::command();
    generate_manpages(&cmd, &man_dir);
}

/// Recursively generate man pages for a command and all its subcommands.
fn generate_manpages(cmd: &clap::Command, dir: &PathBuf) {
    let name = cmd.get_name().to_owned();
    let path = dir.join(format!("{name}.1"));

    let mut buf = Vec::new();
    clap_mangen::Man::new(cmd.clone())
        .render(&mut buf)
        .unwrap_or_else(|e| panic!("failed to render man page for `{name}`: {e}"));
    fs::write(&path, buf).unwrap_or_else(|e| panic!("failed to write {}: {e}", path.display()));

    for sub in cmd.get_subcommands() {
        if sub.is_hide_set() {
            continue;
        }

        let sub = sub.clone().name(format!("{name}-{}", sub.get_name()));
        generate_manpages(&sub, dir);
    }
}
