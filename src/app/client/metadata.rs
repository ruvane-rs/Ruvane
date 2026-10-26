use super::*;
impl App {
    /// Sets the trusted canonical origin shared by native and Wasm declarations.
    /// No request Host or forwarded header supplies this value.
    pub fn client_metadata_origin(&mut self, origin: Option<SiteOrigin>) {
        self.client_metadata_origin = origin;
    }
    pub(super) fn packet_metadata(
        &self,
        page: NavigationPage,
        metadata: Option<Metadata>,
    ) -> Result<NavigationPage, PageError> {
        let page = match metadata {
            Some(metadata) => page
                .with_metadata(metadata)
                .map_err(|_| PageError::InternalServerError)?,
            None => page,
        };
        if page
            .metadata()
            .is_some_and(|metadata| !metadata.accepts_origin(self.client_metadata_origin.as_ref()))
        {
            return Err(PageError::InternalServerError);
        }
        Ok(page)
    }
}
