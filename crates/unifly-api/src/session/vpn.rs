use serde_json::{Value, json};
use tracing::debug;

use crate::error::Error;
use crate::model::IpsecSa;
use crate::session::client::SessionClient;

impl SessionClient {
    pub async fn list_ipsec_sa(&self) -> Result<Vec<IpsecSa>, Error> {
        let url = self.site_url("stat/ipsec-sa");
        debug!("listing ipsec security associations");
        match self.get(url).await {
            Ok(sas) => Ok(sas),
            Err(error) if error.is_not_found() => Ok(Vec::new()),
            Err(error) => Err(error),
        }
    }

    /// Suggest available OpenVPN ports via the v2 API.
    ///
    /// `GET /v2/api/site/{site}/network/port-suggest?service=openvpn`
    pub async fn get_openvpn_port_suggestions(&self) -> Result<Value, Error> {
        let mut url = self.site_url_v2("network/port-suggest");
        url.query_pairs_mut().append_pair("service", "openvpn");
        debug!("fetching OpenVPN port suggestions (v2)");
        self.get_raw(url).await
    }

    /// List VPN client connections via the v2 API.
    ///
    /// `GET /v2/api/site/{site}/vpn/connections`
    pub async fn list_vpn_client_connections(&self) -> Result<Vec<Value>, Error> {
        let url = self.site_url_v2("vpn/connections");
        debug!("listing VPN client connections (v2)");
        let value = self.get_raw(url).await?;
        Ok(value
            .get("connections")
            .and_then(Value::as_array)
            .cloned()
            .or_else(|| value.as_array().cloned())
            .unwrap_or_default())
    }

    /// Restart a VPN client connection via the v2 API.
    ///
    /// `POST /v2/api/site/{site}/vpn/{connection_id}/restart`
    pub async fn restart_vpn_client_connection(&self, connection_id: &str) -> Result<Value, Error> {
        let path = format!("v2/api/site/{}/vpn/{connection_id}/restart", self.site());
        debug!(connection_id, "restarting VPN client connection (v2)");
        self.raw_post(&path, &json!({})).await
    }

    /// List magic site-to-site VPN configs via the v2 API.
    ///
    /// `GET /v2/api/site/{site}/magicsitetositevpn/configs`
    pub async fn list_magic_site_to_site_vpn_configs(&self) -> Result<Vec<Value>, Error> {
        let url = self.site_url_v2("magicsitetositevpn/configs");
        debug!("listing magic site-to-site VPN configs (v2)");
        let value = self.get_raw(url).await?;
        Ok(value.as_array().cloned().unwrap_or_default())
    }

    /// Download an OpenVPN client configuration via the v2 API.
    ///
    /// `GET /v2/api/site/{site}/vpn/openvpn/{server_id}/configuration`
    pub async fn download_openvpn_configuration(&self, server_id: &str) -> Result<Vec<u8>, Error> {
        let url = self.site_url_v2(&format!("vpn/openvpn/{server_id}/configuration"));
        debug!(server_id, "downloading OpenVPN configuration (v2)");
        let resp = self
            .http()
            .get(url)
            .send()
            .await
            .map_err(|e| self.transport_error(e))?;
        if !resp.status().is_success() {
            return Err(Error::SessionApi {
                message: format!(
                    "OpenVPN configuration download failed: HTTP {}",
                    resp.status()
                ),
            });
        }
        let bytes = resp.bytes().await.map_err(|e| self.transport_error(e))?;
        Ok(bytes.to_vec())
    }
}
