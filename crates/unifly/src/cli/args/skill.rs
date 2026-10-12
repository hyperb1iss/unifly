use clap::{Args, ValueEnum};

#[derive(Debug, Args)]
pub struct SkillArgs {
    /// Guide to print (always plain text, independent of --output)
    #[arg(value_enum, default_value_t = SkillTopic::Core)]
    pub topic: SkillTopic,

    /// Example filename to print verbatim; omit to list available examples
    #[arg(value_name = "NAME")]
    pub name: Option<String>,
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
