use super::*;
use serde_json::{Value, json};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const NETWORK_ID: &str = "00000000-0000-0000-0000-000000000010";

fn network(id: &str) -> Value {
    json!({"id": id, "name": "Guest", "enabled": true,
        "management": "UNMANAGED", "vlanId": 20, "metadata": {}})
}

async fn inventory(server: &MockServer, records: Vec<Value>) {
    let detail = records
        .first()
        .cloned()
        .unwrap_or_else(|| network(NETWORK_ID));
    Mock::given(method("GET"))
        .and(path(format!(
            "/proxy/network/integration/v1/sites/{MOCK_SITE_ID}/networks"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "offset": 0, "limit": 200, "count": records.len(),
            "totalCount": records.len(), "data": records
        })))
        .with_priority(1)
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/proxy/network/integration/v1/sites/{MOCK_SITE_ID}/networks/{NETWORK_ID}"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(detail))
        .with_priority(1)
        .mount(server)
        .await;
}

#[tokio::test]
async fn network_updates_deletes_and_references_resolve_names_and_ids() {
    for identifier in ["gUeSt", NETWORK_ID] {
        for operation in ["update", "delete", "refs"] {
            let server = mock_api_key_controller().await;
            inventory(&server, vec![network(NETWORK_ID)]).await;
            let route =
                format!("/proxy/network/integration/v1/sites/{MOCK_SITE_ID}/networks/{NETWORK_ID}");
            let (verb, route, response) = match operation {
                "update" => (
                    "PUT",
                    route,
                    ResponseTemplate::new(200).set_body_json(network(NETWORK_ID)),
                ),
                "delete" => ("DELETE", route, ResponseTemplate::new(204)),
                _ => (
                    "GET",
                    format!("{route}/references"),
                    ResponseTemplate::new(200).set_body_json(json!({})),
                ),
            };
            let mut expected = Mock::given(method(verb)).and(path(route));
            if operation == "update" {
                expected = expected.and(body_partial_json(json!({"name": "Visitors"})));
            }
            expected
                .respond_with(response)
                .expect(1)
                .with_priority(1)
                .mount(&server)
                .await;
            let mut command = mock_cmd(&server);
            command.args(["--yes", "networks", operation, identifier]);
            if operation == "update" {
                command.args(["--name", "Visitors"]);
            }
            command.assert().success();
            server.verify().await;
        }
    }
}

#[tokio::test]
async fn missing_or_ambiguous_networks_fail_before_confirmation_or_mutation() {
    for ambiguous in [false, true] {
        let server = mock_api_key_controller().await;
        let records = if ambiguous {
            vec![
                network(NETWORK_ID),
                network("00000000-0000-0000-0000-000000000011"),
            ]
        } else {
            vec![]
        };
        inventory(&server, records).await;
        for operation in ["update", "delete", "refs"] {
            let mut command = mock_cmd(&server);
            command.args(["networks", operation, "Guest"]);
            if operation == "update" {
                command.args(["--name", "Visitors"]);
            }
            let output = command.output().expect("CLI");
            assert!(!output.status.success());
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stderr.contains("networks list"), "{stderr}");
            assert!(
                !stderr.contains("--yes"),
                "confirmation ran before resolution: {stderr}"
            );
        }
        let requests = server.received_requests().await.expect("requests");
        assert!(requests.iter().all(|request| request.method == "GET"));
    }
}

#[tokio::test]
async fn gateway_network_creation_without_dhcp_sends_null_configuration() {
    let server = mock_api_key_controller().await;
    Mock::given(method("POST"))
        .and(path(format!(
            "/proxy/network/integration/v1/sites/{MOCK_SITE_ID}/networks"
        )))
        .and(body_partial_json(json!({"ipv4Configuration": {
            "hostIpAddress": "10.0.50.1", "prefixLength": 24, "dhcpConfiguration": null
        }})))
        .respond_with(ResponseTemplate::new(200).set_body_json(network(NETWORK_ID)))
        .expect(1)
        .mount(&server)
        .await;
    mock_cmd(&server)
        .args([
            "networks",
            "create",
            "--name",
            "Guest",
            "--management",
            "gateway",
            "--vlan",
            "20",
            "--ipv4-host",
            "10.0.50.1/24",
        ])
        .assert()
        .success();
    server.verify().await;
}

#[tokio::test]
async fn dhcp_options_without_an_enabled_server_fail_without_a_write() {
    let server = mock_api_key_controller().await;
    for (flag, value) in [
        ("--dhcp-start", "10.0.50.100"),
        ("--dhcp-stop", "10.0.50.200"),
        ("--dhcp-lease", "3600"),
        ("--dns", "1.1.1.1"),
    ] {
        mock_cmd(&server)
            .args([
                "networks",
                "create",
                "--name",
                "Guest",
                "--management",
                "gateway",
                "--ipv4-host",
                "10.0.50.1/24",
                flag,
                value,
            ])
            .assert()
            .failure()
            .stderr(predicate::str::contains("DHCP server settings require"));
    }
    let gateway = json!({
        "id": NETWORK_ID, "name": "Guest", "enabled": true,
        "management": "GATEWAY", "vlanId": 20, "metadata": {},
        "ipv4Configuration": {"hostIpAddress": "10.0.50.1", "prefixLength": 24}
    });
    inventory(&server, vec![gateway]).await;
    let input = tempfile::NamedTempFile::new().expect("input");
    std::fs::write(input.path(), r#"{"dhcp":{"dns_servers":["1.1.1.1"]}}"#).expect("write input");
    mock_cmd(&server)
        .args(["networks", "update", "Guest", "--from-file"])
        .arg(input.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "editing DHCP server settings requires",
        ));
    let requests = server.received_requests().await.expect("requests");
    assert!(requests.iter().all(|request| request.method == "GET"));
}

#[tokio::test]
async fn switch_network_creation_sends_its_owner_and_valid_dhcp_configuration() {
    let server = mock_api_key_controller().await;
    let owner = "00000000-0000-0000-0000-000000000099";
    Mock::given(method("POST"))
        .and(path(format!(
            "/proxy/network/integration/v1/sites/{MOCK_SITE_ID}/networks"
        )))
        .and(body_partial_json(json!({
            "management": "SWITCH", "deviceId": owner,
            "cellularBackupEnabled": false, "isolationEnabled": false,
            "ipv4Configuration": {"hostIpAddress": "10.0.50.1", "prefixLength": 24,
                "dhcpConfiguration": {"mode": "SERVER", "ipAddressRange": {
                    "start": "10.0.50.100", "stop": "10.0.50.200"
                }}}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(network(NETWORK_ID)))
        .expect(1)
        .mount(&server)
        .await;
    mock_cmd(&server)
        .args([
            "networks",
            "create",
            "--name",
            "Guest",
            "--management",
            "switch",
            "--device-id",
            owner,
            "--vlan",
            "20",
            "--ipv4-host",
            "10.0.50.1/24",
            "--dhcp",
            "--dhcp-start",
            "10.0.50.100",
            "--dhcp-stop",
            "10.0.50.200",
        ])
        .assert()
        .success();
    let requests = server.received_requests().await.expect("requests");
    let request = requests
        .iter()
        .find(|request| request.method == "POST")
        .expect("create");
    let body: Value = request.body_json().expect("JSON");
    assert!(
        body["ipv4Configuration"]["dhcpConfiguration"]
            .get("pingConflictDetectionEnabled")
            .is_none()
    );
    server.verify().await;
}
