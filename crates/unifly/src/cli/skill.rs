//! Offline, version-matched agent guidance shipped inside the CLI package.

use std::io::{self, Write};

use clap::CommandFactory;

use super::args::{Cli, SkillArgs, SkillTopic};
use super::error::CliError;

mod install;

const CORE: &str = include_str!("../../skill/core.md");
const COMMANDS: &str = include_str!("../../skill/commands.md");
const CONCEPTS: &str = include_str!("../../skill/concepts.md");
const WORKFLOWS: &str = include_str!("../../skill/workflows.md");

macro_rules! examples {
    ($($name:literal),+ $(,)?) => {
        &[$(($name, include_str!(concat!("../../skill/examples/", $name)))),+]
    };
}

const EXAMPLES: &[(&str, &str)] = examples![
    "config.toml",
    "firewall-block-iot.json",
    "nat-masquerade.json",
    "nat-port-forward.json",
    "network-iot-vlan.json",
    "switch-ports.jsonc",
    "vpn-client-openvpn.json",
    "vpn-remote-access-wireguard.json",
    "vpn-site-to-site-ipsec.json",
    "vpn-wireguard-peer.json",
    "wifi-iot.json",
];

/// Print guidance, or install the discovery skill, without reading
/// configuration, credentials, or the network. A closed downstream pipe is
/// normal for documentation consumers such as `head`.
pub fn handle(args: &SkillArgs, quiet: bool) -> Result<(), CliError> {
    let output = &mut io::stdout().lock();
    let result = if args.install {
        install::run(args, quiet, output)
    } else {
        write_guide(args, output)
    };
    match result {
        Err(CliError::Io(error)) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        result => result,
    }
}

fn write_guide(args: &SkillArgs, output: &mut impl Write) -> Result<(), CliError> {
    if let Some(name) = &args.name {
        if args.topic != SkillTopic::Examples {
            return Err(CliError::Validation {
                field: "NAME".into(),
                reason: "a filename is only accepted with `unifly skill examples`".into(),
            });
        }
        let (_, content) = EXAMPLES
            .iter()
            .find(|(candidate, _)| candidate == name)
            .ok_or_else(|| CliError::Validation {
                field: "NAME".into(),
                reason: format!(
                    "unknown example '{name}'; run `unifly skill examples` to list names"
                ),
            })?;
        output.write_all(content.as_bytes())?;
        return Ok(());
    }

    writeln!(
        output,
        "# unifly {} agent guide\n",
        env!("CARGO_PKG_VERSION")
    )?;
    match args.topic {
        SkillTopic::Core => output.write_all(CORE.as_bytes())?,
        SkillTopic::Concepts => output.write_all(CONCEPTS.as_bytes())?,
        SkillTopic::Workflows => output.write_all(WORKFLOWS.as_bytes())?,
        SkillTopic::Commands => {
            output.write_all(COMMANDS.as_bytes())?;
            writeln!(output, "\n## Command inventory\n")?;
            writeln!(
                output,
                "Generated from this binary's command definitions. Run any path with `--help` for its flags, defaults, and accepted values.\n"
            )?;
            write_commands(&Cli::command(), "unifly", output)?;
        }
        SkillTopic::Examples => {
            writeln!(
                output,
                "Print a template with `unifly skill examples NAME`. To save it, redirect to a new file without overwriting existing configuration. Replace placeholders and inspect it before applying.\n"
            )?;
            for (name, _) in EXAMPLES {
                writeln!(output, "- `{name}`")?;
            }
        }
    }
    Ok(())
}

fn write_commands(command: &clap::Command, path: &str, output: &mut impl Write) -> io::Result<()> {
    for child in command
        .get_subcommands()
        .filter(|child| !child.is_hide_set())
    {
        let child_path = format!("{path} {}", child.get_name());
        write!(output, "- `{child_path}`")?;
        if let Some(about) = child.get_about() {
            write!(output, ": {about}")?;
        }
        writeln!(output)?;
        write_commands(child, &child_path, output)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_tracks_nested_commands_and_omits_hidden_entries() {
        let command = clap::Command::new("test")
            .subcommand(
                clap::Command::new("new-resource").subcommand(clap::Command::new("inspect")),
            )
            .subcommand(clap::Command::new("internal").hide(true));
        let mut output = Vec::new();
        write_commands(&command, "test", &mut output).expect("write inventory");
        assert_eq!(
            output,
            b"- `test new-resource`\n- `test new-resource inspect`\n"
        );
    }

    #[test]
    fn every_packaged_example_is_registered() {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("skill/examples");
        let mut actual: Vec<_> = std::fs::read_dir(directory)
            .expect("example directory")
            .map(|entry| {
                entry
                    .expect("example entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        actual.sort();
        let mut registered: Vec<_> = EXAMPLES
            .iter()
            .map(|(name, _)| (*name).to_owned())
            .collect();
        registered.sort();
        assert_eq!(actual, registered);
    }
}
