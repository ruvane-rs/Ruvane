use super::*;
use ruvane::{BrowserTag, FormData, InternalUrl, JsonMethod, JsonOutcome, JsonRequest};
struct Probe {
    json: Option<JsonRequest>,
    form: bool,
    message: String,
}
impl Component for Probe {
    fn view(&self) -> Result<View, ComponentError> {
        let mut view = View::new();
        view.button(0, "Read", 1, false)?;
        view.button(0, "Form", 2, false)?;
        view.button(0, "Both", 3, false)?;
        view.leaf(0, BrowserTag::Output, &self.message)?;
        Ok(view)
    }
    fn dispatch(&mut self, event: u32) -> Result<(), ComponentError> {
        if event != 2 {
            self.json = Some(
                JsonRequest::new(9, JsonMethod::Get, InternalUrl::new("/api").unwrap(), None)
                    .unwrap(),
            );
        }
        self.form = event != 1;
        Ok(())
    }
    fn take_json(&mut self) -> Option<JsonRequest> {
        self.json.take()
    }
    fn take_action(&mut self) -> Option<ruvane::ActionRequest> {
        std::mem::take(&mut self.form).then(|| {
            ruvane::ActionRequest::new(8, InternalUrl::new("/form").unwrap(), FormData::default())
                .unwrap()
        })
    }
    fn json_result(&mut self, action: u32, result: JsonOutcome) -> Result<(), ComponentError> {
        assert_eq!(action, 9);
        self.message = match result {
            Ok(reply) => reply.value().encode(),
            Err(failure) => format!("{:?}", failure.kind()),
        };
        Ok(())
    }
}
fn runtime() -> BrowserRuntime {
    BrowserRuntime::new(|_| {
        Ok(Box::new(Probe {
            json: None,
            form: false,
            message: String::new(),
        }))
    })
}
fn props() -> ComponentProps {
    ComponentProps::new(99, ruvane::InitialState::Number(0)).unwrap()
}
fn deliver(
    runtime: &mut BrowserRuntime,
    view: u32,
    token: u32,
    bytes: &[u8],
) -> Result<bool, BrowserError> {
    runtime.begin_reply(view, token, bytes.len() as u32)?;
    for chunk in bytes.chunks(4) {
        let mut word = [0; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        runtime.reply_chunk(u32::from_le_bytes(word), chunk.len() as u32)?;
    }
    runtime.complete_wire(view, token, 0)
}
#[test]
fn json_staging_is_exact_and_late_replies_cannot_reach_reused_roots() {
    let mut runtime = runtime();
    let view = runtime.mount(1, props()).unwrap();
    runtime.clear_commands();
    runtime.dispatch(view, 1).unwrap();
    let token = runtime.instances[&view].action.unwrap().0;
    runtime.clear_commands();
    assert_eq!(
        runtime.complete_action(view, token, Err(ActionFailure::Network)),
        Err(BrowserError::InvalidAction)
    );
    let bytes = b"RVJ1\0\0\xc8\0\0\0\0\0\0\0{\"n\":18446744073709551615}";
    assert_eq!(deliver(&mut runtime, view, token, bytes), Ok(true));
    runtime.clear_commands();
    assert!(
        runtime.instances[&view]
            .view
            .render()
            .unwrap()
            .as_str()
            .contains("18446744073709551615")
    );
    assert_eq!(
        deliver(&mut runtime, view, token, bytes),
        Err(BrowserError::InvalidAction)
    );
    runtime.dispatch(view, 1).unwrap();
    let retired = runtime.instances[&view].action.unwrap().0;
    runtime.clear_commands();
    runtime.dispose(view).unwrap();
    runtime.clear_commands();
    let other = runtime.mount(1, props()).unwrap();
    runtime.clear_commands();
    runtime.dispatch(other, 1).unwrap();
    let current = runtime.instances[&other].action.unwrap().0;
    runtime.clear_commands();
    assert!(current > retired);
    assert_eq!(
        deliver(&mut runtime, view, retired, bytes),
        Err(BrowserError::InvalidAction)
    );
    assert_eq!(
        deliver(&mut runtime, other, retired, bytes),
        Err(BrowserError::InvalidAction)
    );
    assert_eq!(deliver(&mut runtime, other, current, b"bad"), Ok(false));
    assert!(
        runtime.instances[&other]
            .view
            .render()
            .unwrap()
            .as_str()
            .contains("Protocol")
    );
}
#[test]
fn forms_and_json_cannot_share_or_replace_an_active_slot() {
    for events in [vec![3], vec![1, 2], vec![2, 1]] {
        let mut runtime = runtime();
        let view = runtime.mount(1, props()).unwrap();
        runtime.clear_commands();
        for (i, event) in events.iter().enumerate() {
            let result = runtime.dispatch(view, *event);
            if i + 1 == events.len() {
                assert_eq!(result, Err(BrowserError::InvalidView));
            } else {
                assert!(result.is_ok());
            }
            runtime.clear_commands();
        }
        assert_eq!(runtime.live_instances(), 0);
    }
}
