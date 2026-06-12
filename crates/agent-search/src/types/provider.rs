#[async_trait::async_trait]
pub trait SearchProvider: Send + Sync {
    fn id(&self) -> crate::types::ProviderId;
    fn capabilities(&self) -> crate::types::ProviderCapabilities;
    async fn search(
        &self,
        req: crate::types::ProviderSearchRequest,
    ) -> crate::types::Result<crate::types::ProviderSearchResponse>;
}
