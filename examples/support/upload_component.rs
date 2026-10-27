//! A file uploader whose model, validation and requests live in Rust.
use ruvane::{BrowserTag::*, *};
struct Record {
    caption: String,
    filename: String,
    bytes: u64,
}
fn record(value: &JsonValue) -> Option<Record> {
    let mut object = JsonObject::new(value, &["caption", "filename", "bytes"]);
    let caption =
        object.required_string("caption", TextRules::new(1, 80)?.non_blank().no_controls());
    let filename = object.required_string("filename", TextRules::new(1, 255)?.no_controls());
    let bytes = object.required_u64("bytes", 0, MAX_UPLOAD_BYTES as u64);
    object.finish().ok()?;
    Some(Record {
        caption: caption?.into(),
        filename: filename?.into(),
        bytes: bytes?,
    })
}
struct Uploader {
    caption: String,
    file: Option<BrowserFile>,
    epoch: u32,
    pending: bool,
    uncertain: bool,
    message: String,
    errors: ValidationErrors,
    outgoing: Option<UploadRequest>,
    json: Option<JsonRequest>,
    records: Vec<Record>,
    page: usize,
}
impl Uploader {
    fn reset(&mut self) -> Result<(), ComponentError> {
        self.epoch = self
            .epoch
            .checked_add(1)
            .ok_or(ComponentError::InvalidEvent)?;
        self.caption.clear();
        self.file = None;
        self.errors = ValidationErrors::new();
        Ok(())
    }
    fn read(&mut self, action: u32) -> Result<(), ComponentError> {
        self.pending = true;
        self.json = Some(
            JsonRequest::new(
                action,
                JsonMethod::Get,
                InternalUrl::new("/uploads/state").unwrap(),
                None,
            )
            .map_err(|_| ComponentError::InvalidEvent)?,
        );
        Ok(())
    }
    fn reject(&mut self, outcome: JsonOutcome) {
        self.pending = false;
        match outcome {
            Ok(reply) => {
                self.uncertain |= reply.uncertain();
                self.message = match reply.status() {
                    400 => {
                        self.errors = reply
                            .validation(&["caption", "attachment"])
                            .unwrap_or_default();
                        "Validation failed. File and caption retained.".into()
                    }
                    413 => "Upload exceeds the server limit. Select a smaller file.".into(),
                    429 => format!(
                        "Too many requests. Retry explicitly after {} seconds.",
                        reply.retry_after().unwrap_or(1)
                    ),
                    code => format!("Request rejected ({code}). File and caption retained."),
                };
            }
            Err(failure) => {
                self.uncertain |= failure.uncertain();
                self.message =
                    format!("Transport {:?}. File and caption retained.", failure.kind());
            }
        }
        if self.uncertain {
            self.message="Upload result uncertain. Inspect stored files before another upload; nothing was retried.".into();
        }
    }
}
impl Component for Uploader {
    fn page_data(&mut self, data: ActionData) -> Result<(), ComponentError> {
        if data.is_empty() {
            Ok(())
        } else {
            Err(ComponentError::InvalidProps)
        }
    }
    fn view(&self) -> Result<View, ComponentError> {
        let mut v = View::new();
        let root = v.keyed_element(0, 1, Section)?;
        v.keyed_leaf(root, 1, Heading, "Rust/Wasm file uploads")?;
        v.keyed_leaf(
            root,
            2,
            Span,
            "Local demo sessions. Shared memory; one file up to 32 KiB.",
        )?;
        v.keyed_button(
            root,
            3,
            "Open demo session",
            4,
            self.pending || self.uncertain,
        )?;
        v.keyed_button(root, 4, "Inspect stored files", 5, self.pending)?;
        v.keyed_button(
            root,
            5,
            "Close demo session",
            6,
            self.pending || self.uncertain,
        )?;
        let form = v.form(root, 6, 1)?;
        let caption = v.keyed_element(form, 1, Label)?;
        v.keyed_leaf(caption, 1, Span, "Caption")?;
        let invalid = self
            .errors
            .iter()
            .any(|issue| issue.field() == Some("caption"));
        let input = v.input(caption, 2, &self.caption, 2, invalid)?;
        v.field_epoch(input, self.epoch)?;
        v.field_disabled(input, self.pending)?;
        let label = v.keyed_element(form, 2, Label)?;
        v.keyed_leaf(label, 1, Span, "File (maximum 32 KiB)")?;
        let file = v.file(
            label,
            2,
            3,
            self.errors
                .iter()
                .any(|issue| issue.field() == Some("attachment")),
        )?;
        v.field_epoch(file, self.epoch)?;
        v.field_disabled(file, self.pending)?;
        v.submit(
            form,
            3,
            if self.pending {
                "Sending…"
            } else {
                "Upload file"
            },
            self.pending || self.uncertain,
        )?;
        v.keyed_button(
            root,
            7,
            "Clear selection and caption",
            7,
            self.pending || self.uncertain,
        )?;
        let selected = self
            .file
            .as_ref()
            .map_or("No file selected.".into(), |file| {
                format!("Selected: {} ({} bytes)", file.name(), file.size())
            });
        v.keyed_leaf(root, 8, Output, &selected)?;
        let errors = self
            .errors
            .iter()
            .map(|issue| {
                format!(
                    "{}: {}",
                    issue.field().unwrap_or("Request"),
                    issue.code().message()
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        v.keyed_leaf(root, 9, Output, &errors)?;
        v.keyed_leaf(root, 10, Output, &self.message)?;
        let list = v.keyed_element(root, 11, List)?;
        for (index, item) in self.records.iter().enumerate().skip(self.page * 3).take(3) {
            let row = v.keyed_element(list, index as u32 + 1, ListItem)?;
            v.keyed_leaf(
                row,
                1,
                Span,
                &format!(
                    "{} — {} — {} bytes",
                    item.caption, item.filename, item.bytes
                ),
            )?;
        }
        v.keyed_button(
            root,
            12,
            "Previous files",
            8,
            self.pending || self.page == 0,
        )?;
        v.keyed_button(
            root,
            13,
            "Next files",
            9,
            self.pending || (self.page + 1) * 3 >= self.records.len(),
        )?;
        Ok(v)
    }
    fn dispatch(&mut self, event: u32) -> Result<(), ComponentError> {
        if self.pending {
            return Ok(());
        }
        if self.uncertain && matches!(event, 1 | 4 | 6 | 7) {
            return Ok(());
        }
        match event {
            1 => {
                let mut errors = ValidationErrors::new();
                if self.caption.trim().is_empty() {
                    errors.add(Some("caption"), ValidationCode::Blank);
                }
                if self.caption.len() > 80 {
                    errors.add(Some("caption"), ValidationCode::TooLong);
                }
                if self.file.is_none() {
                    errors.add(Some("attachment"), ValidationCode::Required);
                }
                if !errors.is_empty() {
                    self.errors = errors;
                    self.message = "Correct the marked fields. Selection retained.".into();
                    return Ok(());
                }
                let fields = FormData::default()
                    .field("caption", &self.caption)
                    .map_err(|_| ComponentError::InvalidEvent)?;
                self.outgoing = Some(
                    UploadRequest::new(
                        1,
                        InternalUrl::new("/upload").unwrap(),
                        "attachment",
                        self.file.clone().unwrap(),
                        fields,
                    )
                    .map_err(|_| ComponentError::InvalidEvent)?,
                );
                self.pending = true;
                self.errors = ValidationErrors::new();
                self.message = "Sending file…".into();
                Ok(())
            }
            4 | 6 => {
                self.pending = true;
                self.message = "Loading…".into();
                self.json = Some(
                    JsonRequest::new(
                        event,
                        JsonMethod::Post,
                        InternalUrl::new(if event == 4 {
                            "/upload-session"
                        } else {
                            "/upload-logout"
                        })
                        .unwrap(),
                        Some(JsonValue::object(vec![]).unwrap()),
                    )
                    .map_err(|_| ComponentError::InvalidEvent)?,
                );
                Ok(())
            }
            5 => {
                self.message = "Inspecting stored files…".into();
                self.read(5)
            }
            7 => {
                self.reset()?;
                self.message = "Selection cleared.".into();
                Ok(())
            }
            8 if self.page > 0 => {
                self.page -= 1;
                Ok(())
            }
            9 if (self.page + 1) * 3 < self.records.len() => {
                self.page += 1;
                Ok(())
            }
            _ => Err(ComponentError::InvalidEvent),
        }
    }
    fn dispatch_value(&mut self, event: u32, value: EventValue) -> Result<(), ComponentError> {
        match (event, value) {
            (2, EventValue::Text(caption)) => {
                self.caption = caption;
                self.errors = ValidationErrors::new();
                Ok(())
            }
            (3, EventValue::File(selection)) => {
                self.errors = ValidationErrors::new();
                self.file = match selection {
                    FileSelection::Selected(file) => {
                        self.message = "File selected. Upload explicitly when ready.".into();
                        Some(file)
                    }
                    FileSelection::Empty => {
                        self.message = "No file selected.".into();
                        None
                    }
                    FileSelection::Rejected(error) => {
                        self.errors
                            .add(Some("attachment"), ValidationCode::InvalidValue);
                        self.message = error.message().into();
                        None
                    }
                };
                Ok(())
            }
            (event, EventValue::None) => self.dispatch(event),
            _ => Err(ComponentError::InvalidEvent),
        }
    }
    fn take_upload(&mut self) -> Option<UploadRequest> {
        self.outgoing.take()
    }
    fn take_json(&mut self) -> Option<JsonRequest> {
        self.json.take()
    }
    fn upload_result(&mut self, _: u32, outcome: UploadOutcome) -> Result<(), ComponentError> {
        self.pending = false;
        match outcome {
            Ok(reply) if reply.is_success() => {
                let valid =
                    record(reply.value())
                        .zip(self.file.as_ref())
                        .is_some_and(|(record, file)| {
                            record.caption == self.caption && record.bytes == u64::from(file.size())
                        });
                if !valid {
                    self.uncertain = true;
                    self.message =
                        "Invalid upload response. Inspect stored files; nothing was retried."
                            .into();
                    return Ok(());
                }
                self.reset()?;
                self.message = "File uploaded.".into();
                self.read(10)
            }
            other => {
                self.reject(other);
                Ok(())
            }
        }
    }
    fn json_result(&mut self, action: u32, outcome: JsonOutcome) -> Result<(), ComponentError> {
        self.pending = false;
        match outcome {
            Ok(reply) if reply.is_success() => match action {
                4 => {
                    if reply.value().as_bool() != Some(true) {
                        return Err(ComponentError::InvalidProps);
                    }
                    self.reset()?;
                    self.read(5)
                }
                6 => {
                    if reply.value().as_bool() != Some(false) {
                        return Err(ComponentError::InvalidProps);
                    }
                    self.reset()?;
                    self.records.clear();
                    self.message = "Demo session closed.".into();
                    Ok(())
                }
                5 | 10 => {
                    let records = reply
                        .value()
                        .get("files")
                        .and_then(JsonValue::as_array)
                        .filter(|records| records.len() <= 8)
                        .and_then(|records| records.iter().map(record).collect::<Option<Vec<_>>>());
                    if let Some(records) = records {
                        self.records = records;
                        self.page = self.page.min(self.records.len().saturating_sub(1) / 3);
                        self.uncertain = false;
                        self.message = if action == 10 {
                            "File uploaded. Selection cleared."
                        } else {
                            "Stored files inspected. Review them before another upload."
                        }
                        .into();
                        Ok(())
                    } else {
                        self.message = "Invalid file list. Upload state unchanged.".into();
                        Ok(())
                    }
                }
                _ => Err(ComponentError::InvalidEvent),
            },
            other => {
                self.reject(other);
                Ok(())
            }
        }
    }
}
pub(super) fn create() -> Box<dyn Component> {
    Box::new(Uploader {
        caption: String::new(),
        file: None,
        epoch: 0,
        pending: false,
        uncertain: false,
        message: "Open a demo session to begin.".into(),
        errors: ValidationErrors::new(),
        outgoing: None,
        json: None,
        records: Vec::new(),
        page: 0,
    })
}
