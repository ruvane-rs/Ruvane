//! Shared routes and the file-manager component registry.
use ruvane::*;
struct Notice(u32);
impl Component for Notice {
    fn page_data(&mut self, data: ActionData) -> Result<(), ComponentError> {
        if data.is_empty() {
            Ok(())
        } else {
            Err(ComponentError::InvalidProps)
        }
    }
    fn view(&self) -> Result<View, ComponentError> {
        let mut view = View::new();
        view.leaf(
            0,
            BrowserTag::Heading,
            if self.0 == 0 {
                "File manager home"
            } else {
                "Page unavailable"
            },
        )?;
        Ok(view)
    }
    fn dispatch(&mut self, _: u32) -> Result<(), ComponentError> {
        Err(ComponentError::InvalidEvent)
    }
}
pub fn create(props: ComponentProps) -> Result<Box<dyn Component>, ComponentError> {
    match (props.component(), props.state()) {
        (30, InitialState::Number(0)) => Ok(super::file_manager_component::create()),
        (31, InitialState::Number(value))
            if [0, 400, 403, 404, 409, 429, 500, 503].contains(&value) =>
        {
            Ok(Box::new(Notice(value)))
        }
        _ => Err(ComponentError::InvalidProps),
    }
}
pub fn application() -> App {
    let mut app = App::new();
    app.components(create, 31).unwrap();
    app.client_page(
        PageRoute::new("file-home", "/file-home").unwrap(),
        31,
        |_| Ok(InitialState::Number(0)),
        |_| {
            Ok(ClientPageData::new(
                "File manager home",
                ActionData::default(),
            ))
        },
    )
    .unwrap();
    #[cfg(target_family = "wasm")]
    app.client_page(
        PageRoute::new("file-manager", "/file-manager").unwrap(),
        30,
        |_| Ok(InitialState::Number(0)),
        |_| Err(PageError::InternalServerError),
    )
    .unwrap();
    app
}
