//! Shared routes and page models for the navigation demo.
use super::remote_tasks::{Task, decode};
use ruvane::{
    ActionData, ActionOutcome, ActionRequest, BrowserTag::*, Component, ComponentError,
    ComponentProps, FormData, InitialState, InternalUrl, NavigationPage, NavigationRoutes,
    PageRoute, StatusCode, View,
};

/// Home page component.
pub const HOME: u32 = 8;
/// Task list page component.
pub const TASKS: u32 = 9;
/// Task detail page component.
pub const DETAIL: u32 = 10;
/// Error page component.
pub const ERROR: u32 = 11;

/// Routes compiled into both server and browser.
pub fn routes() -> NavigationRoutes {
    let mut routes = NavigationRoutes::new();
    for (name, path, id) in [
        ("home", "/app", HOME),
        ("tasks", "/app/tasks", TASKS),
        ("task", "/app/tasks/{id}", DETAIL),
    ] {
        routes
            .register(PageRoute::new(name, path).unwrap(), id)
            .unwrap();
    }
    routes
}
/// Resolves route props; unknown or malformed task IDs become error pages.
pub fn props(url: &InternalUrl) -> ComponentProps {
    let (component, value) = match routes().resolve(url).ok().flatten() {
        Some(route) if route.component() == DETAIL => match route.param("id").and_then(|id| {
            id.parse::<u32>()
                .ok()
                .filter(|n| *n > 0 && n.to_string() == id)
        }) {
            Some(id) => (DETAIL, id),
            None => (ERROR, 400),
        },
        Some(route) => (route.component(), 0),
        None => (ERROR, 404),
    };
    ComponentProps::new(component, InitialState::Number(value)).unwrap()
}
/// Validates URL ownership, props and seed schema before rendering.
pub fn validate(page: &NavigationPage) -> bool {
    let path = page.url().as_str().split('?').next().unwrap();
    if path != "/app" && !path.starts_with("/app/") {
        return false;
    }
    if page.status() != StatusCode::Ok {
        return page.props()
            == ComponentProps::new(ERROR, InitialState::Number(page.status().as_u16() as u32))
                .unwrap()
            && page.data().is_empty();
    }
    if props(page.url()) != page.props() || page.props().component() == ERROR {
        return false;
    }
    let Some(tasks) = decode(page.data()) else {
        return false;
    };
    match page.props().state() {
        InitialState::Number(id) if page.props().component() == DETAIL => {
            tasks.iter().any(|t| t.id == id)
        }
        _ => true,
    }
}

struct Page {
    kind: u32,
    id: u32,
    tasks: Vec<Task>,
    outgoing: Option<ActionRequest>,
    pending: Option<u32>,
    message: String,
}
impl Component for Page {
    fn page_data(&mut self, data: ActionData) -> Result<(), ComponentError> {
        if self.kind == ERROR {
            return if data.is_empty() {
                Ok(())
            } else {
                Err(ComponentError::InvalidProps)
            };
        }
        self.tasks = decode(&data).ok_or(ComponentError::InvalidProps)?;
        if self.kind == DETAIL && !self.tasks.iter().any(|t| t.id == self.id) {
            return Err(ComponentError::InvalidProps);
        }
        Ok(())
    }
    fn view(&self) -> Result<View, ComponentError> {
        let mut view = View::new();
        let section = view.keyed_element(0, 1, Section)?;
        match self.kind {
            HOME => {
                view.keyed_leaf(section, 1, Heading, "Home")?;
                let row = view.keyed_element(section, 3, Paragraph)?;
                view.keyed_leaf(
                    row,
                    1,
                    Span,
                    &format!("{} tasks saved on the server.", self.tasks.len()),
                )?;
                view.link(
                    section,
                    4,
                    &routes().url("tasks", &[]).unwrap(),
                    "Browse tasks",
                )?;
            }
            ERROR => {
                view.keyed_leaf(
                    section,
                    1,
                    Heading,
                    if self.id == 404 {
                        "Page not found"
                    } else {
                        "Invalid page request"
                    },
                )?;
                view.link(section, 2, &routes().url("home", &[]).unwrap(), "Go home")?;
            }
            DETAIL => {
                if let Some(task) = self.tasks.iter().find(|t| t.id == self.id) {
                    view.keyed_leaf(section, 1, Heading, &task.title)?;
                    let row = view.keyed_element(section, 2, Paragraph)?;
                    view.keyed_leaf(row, 1, Span, &task.notes)?;
                    view.keyed_leaf(
                        section,
                        3,
                        Output,
                        if task.done { "Completed" } else { "Open" },
                    )?;
                    view.keyed_button(
                        section,
                        4,
                        if task.done {
                            "Mark open"
                        } else {
                            "Mark completed"
                        },
                        1,
                        self.pending.is_some(),
                    )?;
                    view.keyed_button(section, 5, "Remove task", 2, self.pending.is_some())?;
                } else {
                    view.keyed_leaf(section, 1, Heading, "Task removed")?;
                }
                view.keyed_leaf(section, 6, Output, &self.message)?;
                view.link(
                    section,
                    7,
                    &routes().url("tasks", &[]).unwrap(),
                    "Back to tasks",
                )?;
            }
            _ => return Err(ComponentError::InvalidProps),
        }
        Ok(view)
    }
    fn dispatch(&mut self, event: u32) -> Result<(), ComponentError> {
        if self.pending.is_some() {
            return Ok(());
        }
        let task = self
            .tasks
            .iter()
            .find(|t| t.id == self.id)
            .ok_or(ComponentError::InvalidEvent)?;
        let mut fields = FormData::default()
            .field("id", &task.id.to_string())
            .map_err(|_| ComponentError::InvalidProps)?;
        let path = match event {
            1 => {
                if !task.done {
                    fields = fields.field("done", "on").unwrap();
                }
                "/actions/tasks/status"
            }
            2 => "/actions/tasks/remove",
            _ => return Err(ComponentError::InvalidEvent),
        };
        self.outgoing = Some(
            ActionRequest::new(event, InternalUrl::new(path).unwrap(), fields)
                .map_err(|_| ComponentError::InvalidProps)?,
        );
        self.pending = Some(event);
        self.message = "Sending…".into();
        Ok(())
    }
    fn take_action(&mut self) -> Option<ActionRequest> {
        self.outgoing.take()
    }
    fn action_result(&mut self, action: u32, outcome: ActionOutcome) -> Result<(), ComponentError> {
        if self.pending.take() != Some(action) {
            return Err(ComponentError::InvalidEvent);
        }
        self.message = match outcome {
            Ok(reply) if !reply.is_validation() => match decode(reply.data()) {
                Some(tasks) => {
                    self.tasks = tasks;
                    "Saved."
                }
                None => "Invalid server response.",
            },
            Ok(_) => "The server rejected this change. Reload before retrying.",
            Err(_) => "Request failed. Reload before retrying.",
        }
        .into();
        Ok(())
    }
}
/// Creates a page model. Seeds are supplied separately before the first view.
pub fn create(props: ComponentProps) -> Result<Box<dyn Component>, ComponentError> {
    let InitialState::Number(id) = props.state() else {
        return Err(ComponentError::InvalidProps);
    };
    if props.component() == TASKS && id == 0 {
        return Ok(super::remote_tasks::create_page());
    }
    if !matches!(
        (props.component(), id),
        (HOME, 0) | (DETAIL, 1..) | (ERROR, 400 | 404)
    ) {
        return Err(ComponentError::InvalidProps);
    }
    Ok(Box::new(Page {
        kind: props.component(),
        id,
        tasks: Vec::new(),
        outgoing: None,
        pending: None,
        message: String::new(),
    }))
}
