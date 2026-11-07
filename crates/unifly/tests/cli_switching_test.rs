use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn switching_commands_expose_list_filters_and_uuid_details() {
    for collection in ["lags", "mc-lag-domains", "switch-stacks"] {
        Command::new(assert_cmd::cargo::cargo_bin!("unifly"))
            .args(["devices", collection, "list", "--help"])
            .assert()
            .success()
            .stdout(contains("--all"))
            .stdout(contains("--filter"));
        Command::new(assert_cmd::cargo::cargo_bin!("unifly"))
            .args(["devices", collection, "get", "--help"])
            .assert()
            .success()
            .stdout(contains("<ID>"));
    }
}

#[test]
fn switching_commands_require_a_subcommand_and_detail_id() {
    for collection in ["lags", "mc-lag-domains", "switch-stacks"] {
        Command::new(assert_cmd::cargo::cargo_bin!("unifly"))
            .args(["devices", collection])
            .assert()
            .failure();
        Command::new(assert_cmd::cargo::cargo_bin!("unifly"))
            .args(["devices", collection, "get"])
            .assert()
            .failure();
    }
}
