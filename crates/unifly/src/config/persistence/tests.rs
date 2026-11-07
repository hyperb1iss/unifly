use super::super::{load_config_from, save_config_to};

const SOURCE: &str = r"# my controller settings

  default_profile = 'home' # active profile
future = { nested = [1, 2, 3] }

[defaults] # preferences
# wait for this controller
  timeout = 42 # patient controller
unknown = 'keep me'

[profiles.home] # primary
# keep this endpoint
  controller = 'https://home.local'
api_key = 'fixture-key' # explicit secret
future_profile_option = true

[profiles.work]
controller = 'https://work.local'

[future_table]
enabled = true # future feature
";

#[test]
fn unchanged_config_is_byte_for_byte_identical() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    std::fs::write(&path, SOURCE).expect("fixture");
    let config = load_config_from(&path).expect("load");
    save_config_to(&path, &config).expect("save");
    assert_eq!(std::fs::read_to_string(path).expect("read"), SOURCE);
}

#[test]
fn updates_preserve_comments_and_unknown_keys_at_every_level() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    std::fs::write(&path, SOURCE).expect("fixture");
    let mut config = load_config_from(&path).expect("load");
    config.default_profile = Some("work".into());
    config.defaults.timeout = 80;
    config.profiles.get_mut("home").expect("home").controller = "https://new.local".into();
    save_config_to(&path, &config).expect("save");
    let saved = std::fs::read_to_string(&path).expect("read");
    for expected in [
        "# my controller settings\n\n  default_profile",
        "# active profile",
        "# preferences",
        "# primary",
        "# explicit secret",
        "# wait for this controller\n  timeout = 80 # patient controller",
        "# keep this endpoint\n  controller",
        "future = { nested = [1, 2, 3] }",
        "unknown = 'keep me'",
        "future_profile_option = true",
        "enabled = true # future feature",
    ] {
        assert!(saved.contains(expected), "missing {expected}");
    }
    let config = load_config_from(&path).expect("reload");
    assert_eq!(config.profiles["home"].controller, "https://new.local");
    assert_eq!(config.profiles["work"].controller, "https://work.local");
}

#[test]
fn intentional_removals_clear_secrets_and_profiles() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    std::fs::write(&path, SOURCE).expect("fixture");
    let mut config = load_config_from(&path).expect("load");
    config.profiles.get_mut("home").expect("home").api_key = None;
    config.profiles.remove("work");
    save_config_to(&path, &config).expect("save");
    let saved = std::fs::read_to_string(&path).expect("read");
    assert!(!saved.contains("fixture-key"));
    assert!(!saved.contains("profiles.work"));
    assert!(saved.contains("future_profile_option"));
}

#[test]
fn inline_profile_tables_keep_their_unknown_fields() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "profiles.home = {controller = 'https://home.local', future = 7}\n",
    )
    .expect("fixture");
    let mut config = load_config_from(&path).expect("load");
    config.profiles.get_mut("home").expect("home").timeout = Some(77);
    save_config_to(&path, &config).expect("save");
    let saved = std::fs::read_to_string(&path).expect("read");
    assert!(saved.contains("future = 7"));
    assert_eq!(
        load_config_from(&path).expect("reload").profiles["home"].timeout,
        Some(77)
    );
}

#[test]
fn invalid_existing_file_is_never_overwritten() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    for invalid in [
        "password = 'unterminated",
        "[defaults]\ntimeout = 'invalid'",
    ] {
        std::fs::write(&path, invalid).expect("fixture");
        assert!(save_config_to(&path, &super::super::Config::default()).is_err());
        assert_eq!(std::fs::read_to_string(&path).expect("read"), invalid);
    }
}
