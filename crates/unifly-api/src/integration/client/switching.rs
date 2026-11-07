use uuid::Uuid;

use super::{Error, IntegrationClient, types};

impl IntegrationClient {
    pub async fn list_lags(
        &self,
        site_id: &Uuid,
        offset: i64,
        limit: i32,
        filter: Option<&str>,
    ) -> Result<types::Page<types::LagResponse>, Error> {
        let mut params = vec![("offset", offset.to_string()), ("limit", limit.to_string())];
        if let Some(filter) = filter {
            params.push(("filter", filter.to_owned()));
        }
        self.get_with_params(&format!("v1/sites/{site_id}/switching/lags"), &params)
            .await
    }

    pub async fn get_lag(&self, site_id: &Uuid, id: &Uuid) -> Result<types::LagResponse, Error> {
        self.get(&format!("v1/sites/{site_id}/switching/lags/{id}"))
            .await
    }
    pub async fn list_mc_lag_domains(
        &self,
        site_id: &Uuid,
        offset: i64,
        limit: i32,
        filter: Option<&str>,
    ) -> Result<types::Page<types::McLagDomainResponse>, Error> {
        let mut params = vec![("offset", offset.to_string()), ("limit", limit.to_string())];
        if let Some(filter) = filter {
            params.push(("filter", filter.to_owned()));
        }
        self.get_with_params(
            &format!("v1/sites/{site_id}/switching/mc-lag-domains"),
            &params,
        )
        .await
    }

    pub async fn get_mc_lag_domain(
        &self,
        site_id: &Uuid,
        id: &Uuid,
    ) -> Result<types::McLagDomainResponse, Error> {
        self.get(&format!("v1/sites/{site_id}/switching/mc-lag-domains/{id}"))
            .await
    }
    pub async fn list_switch_stacks(
        &self,
        site_id: &Uuid,
        offset: i64,
        limit: i32,
        filter: Option<&str>,
    ) -> Result<types::Page<types::SwitchStackResponse>, Error> {
        let mut params = vec![("offset", offset.to_string()), ("limit", limit.to_string())];
        if let Some(filter) = filter {
            params.push(("filter", filter.to_owned()));
        }
        self.get_with_params(
            &format!("v1/sites/{site_id}/switching/switch-stacks"),
            &params,
        )
        .await
    }

    pub async fn get_switch_stack(
        &self,
        site_id: &Uuid,
        id: &Uuid,
    ) -> Result<types::SwitchStackResponse, Error> {
        self.get(&format!("v1/sites/{site_id}/switching/switch-stacks/{id}"))
            .await
    }
}
