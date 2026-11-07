use super::*;

use assert_cmd::assert::OutputAssertExt;
use serde_json::{Value, json};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const NETWORK_ID: &str = "00000000-0000-0000-0000-000000000010";
const POLICY_ID: &str = "00000000-0000-0000-0000-000000000020";

fn policy() -> Value {
    json!({"id": POLICY_ID, "name": "Policy", "enabled": true,
        "action": {"type": "BLOCK"}, "source": {"zoneId": MOCK_SITE_ID},
        "destination": {"zoneId": MOCK_SITE_ID}, "loggingEnabled": false,
        "ipProtocolScope": {"ipVersion": "IPV4_AND_IPV6"}})
}

async fn inventory(server: &MockServer, ids: &[&str]) {
    let records: Vec<_> = ids
        .iter()
        .map(|id| {
            json!({
                "id": id, "name": "Guest", "enabled": true,
                "management": "UNMANAGED", "vlanId": 20, "metadata": {}
            })
        })
        .collect();
    Mock::given(method("GET"))
        .and(path(format!("/proxy/network/integration/v1/sites/{MOCK_SITE_ID}/networks")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "offset": 0, "limit": 200, "count": records.len(), "totalCount": records.len(), "data": records
        }))).with_priority(1).mount(server).await;
}

fn command(server: &MockServer, operation: &str) -> assert_cmd::Command {
    let mut command = mock_cmd(server);
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
    command
}

#[tokio::test]
async fn firewall_create_and_update_resolve_network_names_and_ids_on_both_sides() {
    for operation in ["create", "update"] {
        for identifier in ["gUeSt", NETWORK_ID] {
            let server = mock_api_key_controller().await;
            inventory(&server, &[NETWORK_ID]).await;
            let base =
                format!("/proxy/network/integration/v1/sites/{MOCK_SITE_ID}/firewall/policies");
            let (verb, route) = if operation == "create" {
                ("POST", base)
            } else {
                ("PUT", format!("{base}/{POLICY_ID}"))
            };
            Mock::given(method("GET")).and(path(format!("/proxy/network/integration/v1/sites/{MOCK_SITE_ID}/firewall/policies/{POLICY_ID}")))
                .respond_with(ResponseTemplate::new(200).set_body_json(policy())).with_priority(1).mount(&server).await;
            let filter = json!({"type": "NETWORK", "networkFilter": {"networkIds": [NETWORK_ID], "matchOpposite": false}});
            Mock::given(method(verb)).and(path(route))
                .and(body_partial_json(json!({"source": {"trafficFilter": filter}, "destination": {"trafficFilter": filter}})))
                .respond_with(ResponseTemplate::new(200).set_body_json(policy())).expect(1).mount(&server).await;
            command(&server, operation)
                .args(["--src-network", identifier, "--dst-network", identifier])
                .assert()
                .success();
            server.verify().await;
        }
    }
}

#[tokio::test]
async fn missing_and_ambiguous_firewall_network_names_never_write() {
    for ambiguous in [false, true] {
        let server = mock_api_key_controller().await;
        let ids = if ambiguous {
            vec![NETWORK_ID, "00000000-0000-0000-0000-000000000011"]
        } else {
            vec![]
        };
        inventory(&server, &ids).await;
        for operation in ["create", "update"] {
            for side in ["--src-network", "--dst-network"] {
                let output = command(&server, operation)
                    .args([side, "Guest"])
                    .output()
                    .expect("CLI");
                assert!(!output.status.success());
                assert!(String::from_utf8_lossy(&output.stderr).contains("networks list"));
            }
        }
        assert!(
            server
                .received_requests()
                .await
                .expect("requests")
                .iter()
                .all(|request| request.method == "GET")
        );
    }
}

#[tokio::test]
async fn firewall_from_file_resolves_canonical_and_shorthand_network_names() {
    for operation in ["create", "update"] {
        for canonical in [false, true] {
            let server = mock_api_key_controller().await;
            inventory(&server, &[NETWORK_ID]).await;
            let base =
                format!("/proxy/network/integration/v1/sites/{MOCK_SITE_ID}/firewall/policies");
            let (verb, route) = if operation == "create" {
                ("POST", base)
            } else {
                ("PUT", format!("{base}/{POLICY_ID}"))
            };
            Mock::given(method("GET")).and(path(format!("/proxy/network/integration/v1/sites/{MOCK_SITE_ID}/firewall/policies/{POLICY_ID}")))
                .respond_with(ResponseTemplate::new(200).set_body_json(policy())).with_priority(1).mount(&server).await;
            let filter = json!({"type": "NETWORK", "networkFilter": {"networkIds": [NETWORK_ID], "matchOpposite": false}});
            Mock::given(method(verb)).and(path(route))
                .and(body_partial_json(json!({"source": {"trafficFilter": filter}, "destination": {"trafficFilter": filter}})))
                .respond_with(ResponseTemplate::new(200).set_body_json(policy())).expect(1).mount(&server).await;
            let input = if canonical {
                json!({
                    "source_filter": {"type": "network", "network_ids": ["Guest"]},
                    "destination_filter": {"type": "network", "network_ids": ["gUeSt"]}
                })
            } else {
                json!({"src_network": ["Guest"], "dst_network": ["gUeSt"]})
            };
            run_file(&server, operation, input).assert().success();
            server.verify().await;
        }
    }
}

fn run_file(server: &MockServer, operation: &str, mut input: Value) -> std::process::Output {
    if operation == "create" {
        input["name"] = json!("Policy");
        input["action"] = json!("Block");
        input["source_zone_id"] = json!(MOCK_SITE_ID);
        input["destination_zone_id"] = json!(MOCK_SITE_ID);
    }
    let directory = TempDir::new().unwrap();
    let file = directory.path().join("policy.json");
    std::fs::write(&file, input.to_string()).unwrap();
    let mut command = mock_cmd(server);
    command.args(["firewall", "policies", operation]);
    if operation == "update" {
        command.arg(POLICY_ID);
    }
    command.arg("--from-file").arg(&file).output().expect("CLI")
}

#[tokio::test]
async fn firewall_from_file_unknown_or_ambiguous_networks_do_not_write() {
    for ambiguous in [false, true] {
        let server = mock_api_key_controller().await;
        let ids = if ambiguous {
            vec![NETWORK_ID, "00000000-0000-0000-0000-000000000011"]
        } else {
            vec![]
        };
        inventory(&server, &ids).await;
        for operation in ["create", "update"] {
            for side in ["source_filter", "destination_filter"] {
                let mut input = json!({});
                input[side] = json!({"type": "network", "network_ids": ["Guest"]});
                let output = run_file(&server, operation, input);
                assert!(!output.status.success());
                assert!(String::from_utf8_lossy(&output.stderr).contains("networks list"));
            }
        }
        assert!(
            server
                .received_requests()
                .await
                .expect("requests")
                .iter()
                .all(|request| request.method == "GET")
        );
    }
}
