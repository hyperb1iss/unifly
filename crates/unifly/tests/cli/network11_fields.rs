use super::*;

use serde_json::{Value, json};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const POLICY_ID: &str = "00000000-0000-0000-0000-000000000001";

fn policy() -> Value {
    json!({"id": POLICY_ID, "name": "Policy", "enabled": true,
        "action": {"type": "BLOCK"}, "source": {"zoneId": MOCK_SITE_ID},
        "destination": {"zoneId": MOCK_SITE_ID}, "loggingEnabled": false,
        "reportFlowEnabled": true, "ipProtocolScope": {"ipVersion": "IPV4_AND_IPV6"}})
}

fn wifi() -> Value {
    json!({"id": MOCK_WIFI_ID, "name": "Wifi", "enabled": true, "type": "STANDARD",
        "securityConfiguration": {"type": "OPEN"}, "metadata": {},
        "channel2gLockedTo6": true, "dtimPeriod2gLockedTo3": true})
}

async fn mount_write(
    server: &MockServer,
    resource: &str,
    verb: &str,
    expected: Value,
    response: Value,
) {
    let route = format!("/proxy/network/integration/v1/sites/{MOCK_SITE_ID}/{resource}");
    if verb != "POST" {
        Mock::given(method("GET"))
            .and(path(&route))
            .respond_with(ResponseTemplate::new(200).set_body_json(response.clone()))
            .with_priority(1)
            .mount(server)
            .await;
    }
    Mock::given(method(verb))
        .and(path(route))
        .and(body_partial_json(expected))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .expect(1)
        .mount(server)
        .await;
}

#[tokio::test]
async fn firewall_flow_flag_preserves_true_and_false_on_create_update_and_patch() {
    for (value, literal) in [(false, "false"), (true, "true")] {
        for operation in ["create", "update", "patch"] {
            let server = mock_api_key_controller().await;
            let (resource, verb) = match operation {
                "create" => ("firewall/policies".to_owned(), "POST"),
                "update" => (format!("firewall/policies/{POLICY_ID}"), "PUT"),
                _ => (format!("firewall/policies/{POLICY_ID}"), "PATCH"),
            };
            mount_write(
                &server,
                &resource,
                verb,
                json!({"reportFlowEnabled": value}),
                policy(),
            )
            .await;
            let mut command = mock_cmd(&server);
            command.args(["firewall", "policies", operation]);
            if operation == "create" {
                command.args([
                    "--name",
                    "Policy",
                    "--action",
                    "block",
                    "--source-zone",
                    MOCK_SITE_ID,
                    "--dest-zone",
                    MOCK_SITE_ID,
                ]);
            } else {
                command.arg(POLICY_ID);
            }
            command.args(["--report-flow", literal]).assert().success();
            server.verify().await;
        }
    }
}

#[tokio::test]
async fn firewall_from_file_preserves_both_flow_field_spellings() {
    for value in [false, true] {
        for field in ["report_flow_enabled", "reportFlowEnabled"] {
            for operation in ["create", "update"] {
                let server = mock_api_key_controller().await;
                let (resource, verb) = if operation == "create" {
                    ("firewall/policies".to_owned(), "POST")
                } else {
                    (format!("firewall/policies/{POLICY_ID}"), "PUT")
                };
                mount_write(
                    &server,
                    &resource,
                    verb,
                    json!({"reportFlowEnabled": value}),
                    policy(),
                )
                .await;
                let mut input = if operation == "create" {
                    json!({"name": "Policy", "action": "Block", "source_zone_id": MOCK_SITE_ID, "destination_zone_id": MOCK_SITE_ID})
                } else {
                    json!({})
                };
                input[field] = json!(value);
                let directory = TempDir::new().unwrap();
                let file = directory.path().join("policy.json");
                std::fs::write(&file, input.to_string()).unwrap();
                let mut command = mock_cmd(&server);
                command.args(["firewall", "policies", operation]);
                if operation == "update" {
                    command.arg(POLICY_ID);
                }
                command.arg("--from-file").arg(&file).assert().success();
                server.verify().await;
            }
        }
    }
}

#[tokio::test]
async fn wifi_lock_flags_preserve_true_and_false_on_create_and_update() {
    for (channel, dtim, channel_literal, dtim_literal) in [
        (false, true, "false", "true"),
        (true, false, "true", "false"),
    ] {
        for operation in ["create", "update"] {
            let server = mock_api_key_controller().await;
            let (resource, verb) = if operation == "create" {
                ("wifi/broadcasts".to_owned(), "POST")
            } else {
                (format!("wifi/broadcasts/{MOCK_WIFI_ID}"), "PUT")
            };
            mount_write(
                &server,
                &resource,
                verb,
                json!({"channel2gLockedTo6": channel, "dtimPeriod2gLockedTo3": dtim}),
                wifi(),
            )
            .await;
            let mut command = mock_cmd(&server);
            command.args(["wifi", operation]);
            if operation == "create" {
                command.args([
                    "--name",
                    "Wifi",
                    "--network",
                    "native",
                    "--security",
                    "open",
                ]);
            } else {
                command.arg(MOCK_WIFI_ID);
            }
            command
                .args([
                    "--channel-2g-locked-to-6",
                    channel_literal,
                    "--dtim-period-2g-locked-to-3",
                    dtim_literal,
                ])
                .assert()
                .success();
            server.verify().await;
        }
    }
}

#[tokio::test]
async fn wifi_from_file_preserves_both_lock_field_spellings() {
    for (channel_value, dtim_value) in [(false, true), (true, false)] {
        for (channel, dtim) in [
            ("channel2g_locked_to6", "dtim_period2g_locked_to3"),
            ("channel2gLockedTo6", "dtimPeriod2gLockedTo3"),
        ] {
            for operation in ["create", "update"] {
                let server = mock_api_key_controller().await;
                let (resource, verb) = if operation == "create" {
                    ("wifi/broadcasts".to_owned(), "POST")
                } else {
                    (format!("wifi/broadcasts/{MOCK_WIFI_ID}"), "PUT")
                };
                mount_write(
                &server,
                &resource,
                verb,
                json!({"channel2gLockedTo6": channel_value, "dtimPeriod2gLockedTo3": dtim_value}),
                wifi(),
            )
            .await;
                let mut input = if operation == "create" {
                    json!({"name": "Wifi", "security_mode": "Open"})
                } else {
                    json!({})
                };
                input[channel] = json!(channel_value);
                input[dtim] = json!(dtim_value);
                let directory = TempDir::new().unwrap();
                let file = directory.path().join("wifi.json");
                std::fs::write(&file, input.to_string()).unwrap();
                let mut command = mock_cmd(&server);
                command.args(["wifi", operation]);
                if operation == "update" {
                    command.arg(MOCK_WIFI_ID);
                }
                command.arg("--from-file").arg(&file).assert().success();
                server.verify().await;
            }
        }
    }
}

#[test]
fn explicit_settings_cannot_be_silently_ignored_beside_from_file() {
    for args in [
        vec![
            "wifi",
            "create",
            "--from-file",
            "unused.json",
            "--channel-2g-locked-to-6",
            "false",
        ],
        vec![
            "wifi",
            "update",
            MOCK_WIFI_ID,
            "--from-file",
            "unused.json",
            "--dtim-period-2g-locked-to-3",
            "false",
        ],
        vec![
            "firewall",
            "policies",
            "create",
            "--from-file",
            "unused.json",
            "--report-flow",
            "false",
        ],
        vec![
            "firewall",
            "policies",
            "update",
            POLICY_ID,
            "--from-file",
            "unused.json",
            "--report-flow",
            "false",
        ],
    ] {
        unifly_cmd()
            .args(args)
            .assert()
            .failure()
            .code(2)
            .stderr(predicate::str::contains("cannot be used with"));
    }
}

#[tokio::test]
async fn allow_policy_only_creates_a_return_rule_when_explicitly_requested() {
    for explicit in [false, true] {
        let server = mock_api_key_controller().await;
        mount_write(
            &server,
            "firewall/policies",
            "POST",
            json!({"action": {"type": "ALLOW", "allowReturnTraffic": explicit}}),
            policy(),
        )
        .await;
        let mut command = mock_cmd(&server);
        command.args([
            "firewall",
            "policies",
            "create",
            "--name",
            "Allow outbound",
            "--action",
            "allow",
            "--source-zone",
            MOCK_SITE_ID,
            "--dest-zone",
            MOCK_SITE_ID,
        ]);
        if explicit {
            command.args(["--allow-return-traffic", "true"]);
        }
        command.assert().success();
        server.verify().await;
    }
}
