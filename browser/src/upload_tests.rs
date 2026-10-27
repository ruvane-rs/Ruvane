use super::*;
use ruvane::{
    BrowserFile, BrowserTag::*, FileSelection, FormData, InternalUrl, UploadOutcome, UploadRequest,
};
struct Probe {
    file: Option<BrowserFile>,
    old: Option<BrowserFile>,
    epoch: u32,
    mode: u32,
    upload: Option<UploadRequest>,
    form: bool,
    json: bool,
    message: String,
}
impl Component for Probe {
    fn view(&self) -> Result<View, ComponentError> {
        let mut view = View::new();
        if self.mode != 3 || self.epoch == 0 {
            let label = view.keyed_element(0, 1, Label)?;
            view.keyed_leaf(label, 1, Span, "File")?;
            let file = view.file(label, 2, 1, false)?;
            view.field_epoch(file, self.epoch)?;
        }
        view.button(0, "Reset", 2, false)?;
        view.button(0, "Send", 3, false)?;
        view.button(0, "Old", 4, false)?;
        view.leaf(0, Output, &self.message)?;
        Ok(view)
    }
    fn dispatch_value(&mut self, event: u32, value: EventValue) -> Result<(), ComponentError> {
        if let (1, EventValue::File(selection)) = (event, &value) {
            self.file = match selection {
                FileSelection::Selected(file) => {
                    self.old = self.file.clone().or_else(|| Some(file.clone()));
                    Some(file.clone())
                }
                _ => None,
            };
            return Ok(());
        }
        if value != EventValue::None {
            return Err(ComponentError::InvalidEvent);
        }
        self.dispatch(event)
    }
    fn dispatch(&mut self, event: u32) -> Result<(), ComponentError> {
        match event {
            2 => {
                self.epoch += 1;
                self.file = None;
                Ok(())
            }
            3 | 4 => {
                let file = if event == 4 { &self.old } else { &self.file };
                self.upload = Some(
                    UploadRequest::new(
                        42,
                        InternalUrl::new("/upload").unwrap(),
                        "attachment",
                        file.clone().ok_or(ComponentError::InvalidEvent)?,
                        FormData::default(),
                    )
                    .unwrap(),
                );
                self.form = self.mode == 1;
                self.json = self.mode == 2;
                Ok(())
            }
            _ => Err(ComponentError::InvalidEvent),
        }
    }
    fn take_upload(&mut self) -> Option<UploadRequest> {
        self.upload.take()
    }
    fn take_action(&mut self) -> Option<ruvane::ActionRequest> {
        std::mem::take(&mut self.form).then(|| {
            ruvane::ActionRequest::new(9, InternalUrl::new("/form").unwrap(), FormData::default())
                .unwrap()
        })
    }
    fn take_json(&mut self) -> Option<ruvane::JsonRequest> {
        std::mem::take(&mut self.json).then(|| {
            ruvane::JsonRequest::new(
                9,
                ruvane::JsonMethod::Get,
                InternalUrl::new("/json").unwrap(),
                None,
            )
            .unwrap()
        })
    }
    fn upload_result(&mut self, action: u32, result: UploadOutcome) -> Result<(), ComponentError> {
        assert_eq!(action, 42);
        self.message = match result {
            Ok(reply) => format!("HTTP {}", reply.status()),
            Err(error) => format!("{:?}", error.kind()),
        };
        Ok(())
    }
}
fn runtime() -> BrowserRuntime {
    BrowserRuntime::new(|props| {
        let ruvane::InitialState::Number(mode) = props.state() else {
            return Err(ComponentError::InvalidProps);
        };
        Ok(Box::new(Probe {
            file: None,
            old: None,
            epoch: 0,
            mode,
            upload: None,
            form: false,
            json: false,
            message: String::new(),
        }))
    })
}
fn mount(runtime: &mut BrowserRuntime, mode: u32) -> u32 {
    let view = runtime
        .mount(
            1,
            ComponentProps::new(99, ruvane::InitialState::Number(mode)).unwrap(),
        )
        .unwrap();
    runtime.clear_commands();
    view
}
fn select(runtime: &mut BrowserRuntime, view: u32, token: u32) -> Result<(), BrowserError> {
    let node = runtime.instances[&view].snapshot.node_for_event(1).unwrap();
    let mut bytes = b"RUF1\x01".to_vec();
    bytes.extend(token.to_le_bytes());
    bytes.extend(3u32.to_le_bytes());
    bytes.extend(5u16.to_le_bytes());
    bytes.extend(b"x.bin");
    bytes.extend(0u16.to_le_bytes());
    runtime.begin_text(view, node, 1, bytes.len() as u32)?;
    for chunk in bytes.chunks(4) {
        let mut word = [0; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        runtime.text_chunk(u32::from_le_bytes(word), chunk.len() as u32)?;
    }
    runtime.dispatch_wire(view, node, 1, 3, 0)
}
#[test]
fn file_references_expire_on_reset_replacement_and_disposal() {
    for mode in [0, 3] {
        let mut runtime = runtime();
        let view = mount(&mut runtime, mode);
        select(&mut runtime, view, 1).unwrap();
        runtime.clear_commands();
        if mode == 0 {
            select(&mut runtime, view, 2).unwrap();
        } else {
            runtime.dispatch(view, 2).unwrap();
            assert!(runtime.instances[&view].file.is_none());
            assert!(
                runtime.instances[&view]
                    .snapshot
                    .node_for_event(1)
                    .is_none()
            );
        }
        runtime.clear_commands();
        assert_eq!(runtime.dispatch(view, 4), Err(BrowserError::InvalidView));
        assert_eq!(runtime.live_instances(), 0);
    }
    let mut runtime = runtime();
    let view = mount(&mut runtime, 0);
    select(&mut runtime, view, 1).unwrap();
    runtime.clear_commands();
    assert!(runtime.instances[&view].file.is_some());
    assert_eq!(
        select(&mut runtime, view, 1),
        Err(BrowserError::InvalidPayload)
    );
    runtime.clear_commands();
    select(&mut runtime, view, 2).unwrap();
    runtime.clear_commands();
    runtime.dispatch(view, 2).unwrap();
    assert!(runtime.instances[&view].file.is_none());
    runtime.clear_commands();
    assert_eq!(runtime.dispatch(view, 4), Err(BrowserError::InvalidView));
    assert_eq!(runtime.live_instances(), 0);
    let other = mount(&mut runtime, 0);
    assert!(other > view);
    assert_eq!(
        select(&mut runtime, other, 2),
        Err(BrowserError::InvalidPayload)
    );
    runtime.clear_commands();
    select(&mut runtime, other, 3).unwrap();
    runtime.clear_commands();
    runtime.dispose(other).unwrap();
    runtime.clear_commands();
    assert_eq!(
        select_stale(&mut runtime, other),
        Err(BrowserError::InvalidHandle)
    );
}
fn select_stale(runtime: &mut BrowserRuntime, view: u32) -> Result<(), BrowserError> {
    runtime.begin_text(view, 1, 1, 5)
}
#[test]
fn uploads_share_the_pending_slot_and_decode_only_their_own_results() {
    for mode in [1, 2] {
        let mut runtime = runtime();
        let view = mount(&mut runtime, mode);
        select(&mut runtime, view, 1).unwrap();
        runtime.clear_commands();
        assert_eq!(runtime.dispatch(view, 3), Err(BrowserError::InvalidView));
        assert_eq!(runtime.live_instances(), 0);
    }
    let mut runtime = runtime();
    let view = mount(&mut runtime, 0);
    select(&mut runtime, view, 1).unwrap();
    runtime.clear_commands();
    runtime.dispatch(view, 3).unwrap();
    let token = runtime.instances[&view].action.unwrap().0;
    runtime.clear_commands();
    assert_eq!(
        runtime.complete_action(view, token, Err(ActionFailure::Network)),
        Err(BrowserError::InvalidAction)
    );
    let bytes = b"RVJ1\0\0\x90\x01\0\0\0\0\0\0{}";
    runtime
        .begin_reply(view, token, bytes.len() as u32)
        .unwrap();
    for chunk in bytes.chunks(4) {
        let mut word = [0; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        runtime
            .reply_chunk(u32::from_le_bytes(word), chunk.len() as u32)
            .unwrap();
    }
    assert_eq!(runtime.complete_wire(view, token, 0), Ok(true));
    runtime.clear_commands();
    assert!(runtime.instances[&view].file.is_some());
    assert!(
        runtime.instances[&view]
            .view
            .render()
            .unwrap()
            .as_str()
            .contains("HTTP 400")
    );
    runtime.dispatch(view, 3).unwrap();
    let retired = runtime.instances[&view].action.unwrap().0;
    runtime.clear_commands();
    runtime.dispose(view).unwrap();
    runtime.clear_commands();
    let fresh = mount(&mut runtime, 0);
    assert_ne!(fresh, view);
    assert_eq!(
        runtime.complete_wire(view, retired, 4),
        Err(BrowserError::InvalidAction)
    );
}
