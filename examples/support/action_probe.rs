//! Action transport fixture.
use ruvane::{
    ActionOutcome, ActionRequest, BrowserTag, Component, ComponentError, FormData, InternalUrl,
    View,
};
struct Probe {
    mode: u32,
    pending: bool,
    outgoing: Option<ActionRequest>,
    message: String,
}
impl Probe {
    fn send(&mut self) {
        if self.pending {
            return;
        }
        self.pending = true;
        self.message = "pending".into();
        self.outgoing = Some(
            ActionRequest::new(
                99,
                InternalUrl::new(&format!("/actions/fixtures/{}", self.mode)).unwrap(),
                FormData::default(),
            )
            .unwrap(),
        );
    }
}
impl Component for Probe {
    fn view(&self) -> Result<View, ComponentError> {
        let mut view = View::new();
        view.button(0, "Send", 1, self.pending)?;
        view.leaf(0, BrowserTag::Output, &self.message)?;
        Ok(view)
    }
    fn dispatch(&mut self, event: u32) -> Result<(), ComponentError> {
        if event != 1 {
            return Err(ComponentError::InvalidEvent);
        }
        self.send();
        Ok(())
    }
    fn take_action(&mut self) -> Option<ActionRequest> {
        self.outgoing.take()
    }
    fn action_result(&mut self, _: u32, outcome: ActionOutcome) -> Result<(), ComponentError> {
        self.pending = false;
        self.message = match outcome {
            Ok(reply) => {
                if reply.is_validation() {
                    "validation".into()
                } else {
                    "success".into()
                }
            }
            Err(error) => format!("{error:?}").to_lowercase(),
        };
        Ok(())
    }
}
pub(super) fn create(mode: u32) -> Box<dyn Component> {
    let mut probe = Probe {
        mode,
        pending: false,
        outgoing: None,
        message: String::new(),
    };
    probe.send();
    Box::new(probe)
}
