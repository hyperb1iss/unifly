use uuid::Uuid;

use super::super::{Controller, integration_site_context};
use crate::{core_error::CoreError, integration_types};

impl Controller {
    pub async fn list_lags(&self) -> Result<Vec<integration_types::LagResponse>, CoreError> {
        let (client, site_id) = integration_site_context(self, "list_lags").await?;
        Ok(client
            .paginate_all(200, |offset, limit| {
                client.list_lags(&site_id, offset, limit, None)
            })
            .await?)
    }

    pub async fn get_lag(&self, id: &Uuid) -> Result<integration_types::LagResponse, CoreError> {
        let (client, site_id) = integration_site_context(self, "get_lag").await?;
        Ok(client.get_lag(&site_id, id).await?)
    }
    pub async fn list_mc_lag_domains(
        &self,
    ) -> Result<Vec<integration_types::McLagDomainResponse>, CoreError> {
        let (client, site_id) = integration_site_context(self, "list_mc_lag_domains").await?;
        Ok(client
            .paginate_all(200, |offset, limit| {
                client.list_mc_lag_domains(&site_id, offset, limit, None)
            })
            .await?)
    }

    pub async fn get_mc_lag_domain(
        &self,
        id: &Uuid,
    ) -> Result<integration_types::McLagDomainResponse, CoreError> {
        let (client, site_id) = integration_site_context(self, "get_mc_lag_domain").await?;
        Ok(client.get_mc_lag_domain(&site_id, id).await?)
    }
    pub async fn list_switch_stacks(
        &self,
    ) -> Result<Vec<integration_types::SwitchStackResponse>, CoreError> {
        let (client, site_id) = integration_site_context(self, "list_switch_stacks").await?;
        Ok(client
            .paginate_all(200, |offset, limit| {
                client.list_switch_stacks(&site_id, offset, limit, None)
            })
            .await?)
    }

    pub async fn get_switch_stack(
        &self,
        id: &Uuid,
    ) -> Result<integration_types::SwitchStackResponse, CoreError> {
        let (client, site_id) = integration_site_context(self, "get_switch_stack").await?;
        Ok(client.get_switch_stack(&site_id, id).await?)
    }
}
