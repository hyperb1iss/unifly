//! Install the discovery skill into a user's agent skills directory.
//!
//! Package managers install unifly system-wide and cannot write into a
//! user's home, so the binary installs its own discovery skill on request.
//! Like the guides, this never reads configuration, credentials, or the
//! network.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::cli::args::SkillArgs;
use crate::cli::error::CliError;

/// The installable discovery skill. A copy of `skills/unifly/SKILL.md` lives
/// inside the crate so packaged and standalone binaries carry it.
const DISCOVERY_SKILL: &str = include_str!("../../../skill/discovery-skill.md");

const SKILL_NAME: &str = "unifly";
const SKILL_FILE: &str = "SKILL.md";

/// FNV-1a digests (CRLF normalized to LF) of every version of
/// `skills/unifly/SKILL.md` committed to main, ending with the current
/// discovery skill. A file matching one is an unmodified unifly
/// skill, from `npx skills add` or an earlier `--install`, and is replaced
/// without `--force`. Append a digest whenever the skill changes; removing
/// one would turn that version into a refusal.
const PUBLISHED_DIGESTS: &[u64] = &[
    0x11b8_d23e_2f0e_0644,
    0xe68d_a70a_60f5_1fca,
    0x4799_4753_11b6_c462,
    0x0326_6992_115e_22d5,
    0x6d96_171c_ad7e_fd8d,
    0xe59a_25ad_9e59_a03c,
    0x14ef_d724_892c_2a82,
    0x42c9_494c_8834_86f8,
    0xc32e_b009_d02c_25c5,
    0xaf05_a1cd_9344_3fc0,
    0x50fa_81b9_9c81_3950,
    0xa3c8_363e_0661_84e8,
    0x70a5_52f5_259c_e8a2,
    0xc451_e991_056b_63fc,
    0x47d7_8113_693a_7d87,
    0x8c93_a358_dcfe_fca4,
    0xe08c_05f4_c072_c777,
    0xac6c_daef_9bb8_53ca,
    0x698d_e975_8574_fb62,
    0x60ab_c037_a32d_64f6,
    0x33a5_c0b3_d8dc_01f6,
    0xf620_e5df_606d_f856,
    0xc3b4_95a5_f777_9219,
    0xe86e_0627_1aae_6fc5,
    0xa688_ab24_62b2_13dc,
    0x34fb_8491_c4ef_27de,
    0xce8a_bcfb_8988_4532,
    0x1419_d422_c608_ea41,
    0x0400_43de_760e_345d,
    0x189b_d2bb_afd0_9fd5,
    0x892a_7550_5a13_d9df,
    0x72f6_58b6_5358_e66c,
    0x04aa_5884_4327_02cd,
    0x137b_feaa_5345_3fdd,
    0x0971_891f_64d2_f9d0,
    0xaa48_6f4a_f00f_ee45,
    0x5757_974e_2531_1d41,
    0xc716_ec96_a730_d2e1,
    0x5909_36f7_ffbb_00dc,
    0x1a7c_e4ed_6f72_87b1,
    0x42dc_a94e_2373_859c,
    0x4f8b_2a6f_2e51_ba7c,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Installed,
    Unchanged,
    Updated,
    Replaced,
}

/// Install the discovery skill, report what happened on stderr, and print
/// the installed path on stdout for scripts.
pub(super) fn run(args: &SkillArgs, quiet: bool, output: &mut impl Write) -> Result<(), CliError> {
    let default_target = args.dir.is_none() && !args.claude;
    let skills_root = match (&args.dir, args.claude) {
        (Some(dir), _) => dir.clone(),
        (None, true) => claude_skills_dir()?,
        (None, false) => home_dir()?.join(".agents").join("skills"),
    };
    let skill_dir = skills_root.join(SKILL_NAME);
    let path = skill_dir.join(SKILL_FILE);
    let outcome = install(&path, args.force)?;

    if !quiet {
        let shown = path.display();
        match outcome {
            Outcome::Installed => eprintln!("Installed the unifly agent skill at {shown}"),
            Outcome::Unchanged => eprintln!("The unifly agent skill at {shown} is up to date"),
            Outcome::Updated => eprintln!("Updated the unifly agent skill at {shown}"),
            Outcome::Replaced => eprintln!("Replaced {shown} with the unifly agent skill"),
        }
        if outcome == Outcome::Updated {
            let others = other_entries(&skill_dir);
            if !others.is_empty() {
                eprintln!(
                    "note: {} also contains {}, which this skill does not use",
                    skill_dir.display(),
                    others.join(", ")
                );
            }
        }
        if default_target && outcome != Outcome::Unchanged && claude_lacks_skill() {
            eprintln!(
                "To install it for Claude Code as well, run `unifly skill --install --claude`"
            );
        }
    }

    writeln!(output, "{}", path.display())?;
    Ok(())
}

/// Write the discovery skill unless an unrecognized file is in the way.
fn install(path: &Path, force: bool) -> Result<Outcome, CliError> {
    let outcome = match fs::read(path) {
        Ok(existing) if existing == DISCOVERY_SKILL.as_bytes() => return Ok(Outcome::Unchanged),
        Ok(existing) if PUBLISHED_DIGESTS.contains(&digest(&existing)) => Outcome::Updated,
        Ok(_) if force => Outcome::Replaced,
        Ok(_) => {
            return Err(CliError::SkillFileExists {
                path: path.display().to_string(),
            });
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Outcome::Installed,
        Err(error) => return Err(install_failed(path, error)),
    };
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|error| install_failed(parent, error))?;
    // Write beside the target and rename over it, so an interrupted write
    // never leaves a truncated SKILL.md that the next run would refuse.
    let staged = parent.join(format!(".{SKILL_FILE}.{}.tmp", std::process::id()));
    fs::write(&staged, DISCOVERY_SKILL)
        .and_then(|()| fs::rename(&staged, path))
        .map_err(|error| {
            fs::remove_file(&staged).ok();
            install_failed(path, error)
        })?;
    Ok(outcome)
}

/// 64-bit FNV-1a with CRLF read as LF, so a checkout or copy that converted
/// line endings still counts as the published file.
fn digest(content: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut bytes = content.iter().peekable();
    while let Some(&byte) = bytes.next() {
        if byte == b'\r' && bytes.peek() == Some(&&b'\n') {
            continue;
        }
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn other_entries(skill_dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(skill_dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name != SKILL_FILE)
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// True when Claude Code looks installed but has no unifly skill of its own.
/// A `~/.claude/skills` that links to `~/.agents/skills` already sees it.
fn claude_lacks_skill() -> bool {
    claude_skills_dir().is_ok_and(|dir| {
        dir.parent().is_some_and(Path::is_dir) && !dir.join(SKILL_NAME).join(SKILL_FILE).exists()
    })
}

/// Claude Code reads user skills from `$CLAUDE_CONFIG_DIR/skills`, which
/// defaults to `~/.claude/skills`.
fn claude_skills_dir() -> Result<PathBuf, CliError> {
    match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) if !dir.is_empty() => Ok(PathBuf::from(dir).join("skills")),
        _ => Ok(home_dir()?.join(".claude").join("skills")),
    }
}

fn home_dir() -> Result<PathBuf, CliError> {
    directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .ok_or_else(|| CliError::SkillInstallFailed {
            path: "the home directory".into(),
            source: io::Error::new(
                io::ErrorKind::NotFound,
                "no home directory found; pass --dir to choose a skills directory",
            ),
        })
}

fn install_failed(path: &Path, source: io::Error) -> CliError {
    CliError::SkillInstallFailed {
        path: path.display().to_string(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_skill_is_the_last_published_digest() {
        let current = digest(DISCOVERY_SKILL.as_bytes());
        assert_eq!(
            PUBLISHED_DIGESTS.last(),
            Some(&current),
            "the discovery skill changed: append {current:#018x} to PUBLISHED_DIGESTS"
        );
    }

    #[test]
    fn digest_reads_crlf_as_lf_and_keeps_lone_carriage_returns() {
        assert_eq!(digest(b"a\r\nb\r\n"), digest(b"a\nb\n"));
        assert_ne!(digest(b"a\rb"), digest(b"ab"));
        assert_ne!(digest(b"unifly"), digest(b"unifly "));
    }

    /// The installable copy must match the skill that `npx skills add` and
    /// ClawHub publish. A crate unpacked from crates.io has no repository
    /// copy to compare against.
    #[test]
    fn packaged_skill_matches_the_repository_skill() {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/unifly/SKILL.md");
        match fs::read_to_string(&repository) {
            Ok(published) => assert_eq!(
                DISCOVERY_SKILL, published,
                "crates/unifly/skill/discovery-skill.md and skills/unifly/SKILL.md differ"
            ),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => panic!("read {}: {error}", repository.display()),
        }
    }

    #[test]
    fn install_creates_updates_and_guards_the_skill() {
        let directory = tempfile::TempDir::new().expect("temporary directory");
        let path = directory.path().join("skills/unifly/SKILL.md");

        assert_eq!(install(&path, false).expect("fresh"), Outcome::Installed);
        assert_eq!(install(&path, false).expect("rerun"), Outcome::Unchanged);

        let converted = DISCOVERY_SKILL.replace('\n', "\r\n");
        fs::write(&path, converted).expect("write converted copy");
        assert_eq!(install(&path, false).expect("known"), Outcome::Updated);
        assert_eq!(fs::read_to_string(&path).expect("read"), DISCOVERY_SKILL);

        fs::write(&path, "local notes\n").expect("write local edit");
        assert!(matches!(
            install(&path, false),
            Err(CliError::SkillFileExists { .. })
        ));
        assert_eq!(fs::read_to_string(&path).expect("read"), "local notes\n");
        assert_eq!(install(&path, true).expect("forced"), Outcome::Replaced);
        assert_eq!(fs::read_to_string(&path).expect("read"), DISCOVERY_SKILL);

        let entries: Vec<_> = fs::read_dir(path.parent().expect("skill dir"))
            .expect("read skill dir")
            .map(|entry| entry.expect("entry").file_name())
            .collect();
        assert_eq!(entries, [SKILL_FILE], "no staged file is left behind");
    }
}
