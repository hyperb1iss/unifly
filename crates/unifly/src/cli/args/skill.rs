use std::path::PathBuf;

use clap::{Args, ValueEnum};

#[derive(Debug, Args)]
pub struct SkillArgs {
    /// Guide to print (always plain text, independent of --output)
    #[arg(value_enum, default_value_t = SkillTopic::Core)]
    pub topic: SkillTopic,

    /// Example filename to print verbatim; omit to list available examples
    #[arg(value_name = "NAME")]
    pub name: Option<String>,

    /// Install the agent discovery skill for the current user instead of
    /// printing a guide (default: ~/.agents/skills/unifly/SKILL.md)
    #[arg(long, conflicts_with_all = ["topic", "name"], help_heading = "Install")]
    pub install: bool,

    /// Skills directory to install into; the skill lands in DIR/unifly/SKILL.md
    #[arg(
        long,
        value_name = "DIR",
        requires = "install",
        help_heading = "Install"
    )]
    pub dir: Option<PathBuf>,

    /// Install into Claude Code's skills directory ($CLAUDE_CONFIG_DIR/skills,
    /// or ~/.claude/skills) instead of ~/.agents/skills
    #[arg(
        long,
        requires = "install",
        conflicts_with = "dir",
        help_heading = "Install"
    )]
    pub claude: bool,

    /// Replace an existing SKILL.md even when it is not a unifly skill this
    /// version recognizes, such as a locally edited copy
    #[arg(long, requires = "install", help_heading = "Install")]
    pub force: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SkillTopic {
    /// Essential workflow, authentication, and topic routing
    Core,
    /// Generated command inventory and operational gotchas
    Commands,
    /// Authentication, configuration, identifiers, and output contracts
    Concepts,
    /// Task-oriented automation recipes
    Workflows,
    /// List payload templates, or print one by filename
    Examples,
}
