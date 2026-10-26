use super::*;

impl App {
    /// Sets the read-only Number(404) component for unknown URLs and unhandled client 404s.
    /// Native pages keep their separate App::not_found renderer.
    pub fn client_not_found(&mut self, component: Option<u32>) -> Result<(), AppError> {
        if self.client_factory.is_none() || component == Some(0) {
            return Err(AppError::InvalidPage);
        }
        self.client_not_found = component;
        Ok(())
    }
    /// Sets a client page's read-only Number(404) presentation.
    pub fn client_page_not_found(
        &mut self,
        name: &str,
        component: Option<u32>,
    ) -> Result<(), AppError> {
        if component == Some(0) {
            return Err(AppError::InvalidPage);
        }
        let page = self
            .pages
            .iter_mut()
            .find(|p| p.route.name() == name)
            .ok_or(AppError::UnknownRoute)?;
        page.client.as_mut().ok_or(AppError::InvalidPage)?.not_found = component;
        Ok(())
    }
    /// Sets a client layout's child 404 presentation, excluding its own loader.
    pub fn client_layout_not_found(
        &mut self,
        name: &str,
        component: Option<u32>,
    ) -> Result<(), AppError> {
        if component == Some(0) {
            return Err(AppError::InvalidPage);
        }
        let index = self.client_layout_index(Some(name))?.unwrap();
        self.client_layouts[index].not_found = component;
        Ok(())
    }
    pub(super) fn failure_component(&self, status: StatusCode) -> u32 {
        if status == StatusCode::NotFound {
            self.client_not_found.unwrap_or(self.client_error)
        } else {
            self.client_error
        }
    }
    pub(super) fn has_not_found(&self, index: usize) -> bool {
        let client = self.pages[index].client.unwrap();
        self.client_not_found.is_some()
            || client.not_found.is_some()
            || self
                .layout_chain(client.layout)
                .iter()
                .any(|index| self.client_layouts[*index].not_found.is_some())
    }
    pub(super) fn scoped_not_found_packet_required(&self, url: &InternalUrl) -> bool {
        let path = url.as_str().split('?').next().unwrap();
        self.pages
            .iter()
            .position(|p| Some(p.route.name()) == self.owner(path) && p.client.is_some())
            .is_some_and(|index| self.has_not_found(index) && self.client_props(url).is_ok())
    }
    pub(super) fn failure_presentation(
        &self,
        index: usize,
        failed: usize,
        status: StatusCode,
    ) -> Option<(usize, u32)> {
        let client = self.pages[index].client?;
        let chain = self.layout_chain(client.layout);
        if failed > chain.len() {
            return None;
        }
        if status == StatusCode::NotFound {
            if failed == chain.len() {
                if let Some(component) = client.not_found {
                    return Some((failed, component));
                }
            }
            if let Some(found) =
                chain
                    .iter()
                    .take(failed)
                    .enumerate()
                    .rev()
                    .find_map(|(depth, index)| {
                        self.client_layouts[*index]
                            .not_found
                            .map(|component| (depth + 1, component))
                    })
            {
                return Some(found);
            }
            if let Some(component) = self.client_not_found {
                return Some((0, component));
            }
        }
        self.error_boundary(index, failed)
            .map(|(depth, boundary)| (depth, boundary.error))
    }
}
