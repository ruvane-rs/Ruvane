//! Private file management. File bytes and owners stay outside the component.
use ruvane::{BrowserTag::*, *};
use std::collections::BTreeMap;
const MAX_HISTORY: usize = 86;
enum Pending {
    Read {
        cursor: Option<FileId>,
        history: Vec<Option<FileId>>,
    },
    Upload,
    Delete,
}
struct Manager {
    listing: FileListing,
    cursor: Option<FileId>,
    history: Vec<Option<FileId>>,
    keys: BTreeMap<FileId, u32>,
    next_key: u32,
    file: Option<BrowserFile>,
    epoch: u32,
    pending: Option<Pending>,
    authorized: bool,
    uncertain: bool,
    invalid: bool,
    message: String,
    outgoing: Option<JsonRequest>,
    upload: Option<UploadRequest>,
}
impl Manager {
    fn adopt(&mut self, listing: FileListing) -> Result<(), ComponentError> {
        let mut keys = BTreeMap::new();
        for file in listing.files() {
            let key = if let Some(key) = self.keys.get(&file.id()) {
                *key
            } else {
                self.next_key = self
                    .next_key
                    .checked_add(1)
                    .ok_or(ComponentError::InvalidProps)?;
                self.next_key
            };
            keys.insert(file.id(), key);
        }
        self.keys = keys;
        self.listing = listing;
        Ok(())
    }
    fn reset(&mut self) -> Result<(), ComponentError> {
        self.epoch = self
            .epoch
            .checked_add(1)
            .ok_or(ComponentError::InvalidEvent)?;
        self.file = None;
        self.invalid = false;
        Ok(())
    }
    fn read(
        &mut self,
        cursor: Option<FileId>,
        history: Vec<Option<FileId>>,
    ) -> Result<(), ComponentError> {
        let mut url = InternalUrl::new("/private-files/list").unwrap();
        if let Some(id) = cursor {
            url = url
                .query("after", &id.to_string())
                .map_err(|_| ComponentError::InvalidEvent)?;
        }
        self.outgoing = Some(
            JsonRequest::new(1, JsonMethod::Get, url, None)
                .map_err(|_| ComponentError::InvalidEvent)?,
        );
        self.pending = Some(Pending::Read { cursor, history });
        Ok(())
    }
    fn failed(&mut self, result: JsonOutcome) -> Result<(), ComponentError> {
        match result {
            Ok(reply) => {
                self.uncertain |= reply.uncertain();
                if matches!(reply.status(), 401 | 403) {
                    self.authorized = false;
                    self.adopt(FileListing::new(vec![], None).unwrap())?;
                    self.cursor = None;
                    self.history.clear();
                    self.reset()?;
                    self.message = "Session unavailable. Open the native demo to sign in.".into();
                    return Ok(());
                }
                self.invalid = reply.status() == 400;
                self.message = match reply.status() {
                    400 => "Validation failed. Selection retained.".into(),
                    409 => "Storage quota reached. Selection retained.".into(),
                    413 => "The file exceeds the server limit.".into(),
                    429 => format!(
                        "Too many requests. Retry explicitly after {} seconds.",
                        reply.retry_after().unwrap_or(1)
                    ),
                    status => format!("Request rejected ({status}). State retained."),
                };
            }
            Err(failure) => {
                self.uncertain |= failure.uncertain();
                self.message = format!("Transport {:?}. State retained.", failure.kind());
            }
        }
        if self.uncertain {
            self.message =
                "Result uncertain. Inspect files before another mutation; nothing was retried."
                    .into();
        }
        Ok(())
    }
}
impl Component for Manager {
    fn page_data(&mut self, data: ActionData) -> Result<(), ComponentError> {
        let listing = FileListing::from_seed(&data).map_err(|_| ComponentError::InvalidProps)?;
        self.adopt(listing)?;
        self.authorized = true;
        self.message = "Private files loaded from the server.".into();
        Ok(())
    }
    fn view(&self) -> Result<View, ComponentError> {
        let mut view = View::new();
        let root = view.keyed_element(0, 1, Section)?;
        let busy = self.pending.is_some();
        let blocked = busy || !self.authorized;
        view.keyed_leaf(root, 1, Heading, "Private file manager")?;
        view.keyed_leaf(
            root,
            2,
            Span,
            "Rust + Wasm; persistent private storage. Maximum 32 KiB per file.",
        )?;
        let form = view.form(root, 3, 2)?;
        let label = view.keyed_element(form, 1, Label)?;
        view.keyed_leaf(label, 1, Span, "File (maximum 32 KiB)")?;
        let file = view.file(label, 2, 3, self.invalid)?;
        view.field_epoch(file, self.epoch)?;
        view.field_disabled(file, blocked)?;
        view.submit(
            form,
            2,
            if busy { "Working…" } else { "Store file" },
            blocked || self.uncertain,
        )?;
        view.keyed_button(root, 4, "Clear selection", 4, blocked)?;
        view.keyed_button(root, 5, "Inspect files", 1, busy)?;
        let selected = self
            .file
            .as_ref()
            .map_or("No file selected.".into(), |file| {
                format!("Selected: {} ({} bytes).", file.name(), file.size())
            });
        view.keyed_leaf(root, 6, Output, &format!("{selected} {}", self.message))?;
        let list = view.keyed_element(root, 7, List)?;
        for (index, file) in self.listing.files().iter().enumerate() {
            let row = view.keyed_element(list, self.keys[&file.id()], ListItem)?;
            view.keyed_leaf(
                row,
                1,
                Span,
                &format!("{} — {} bytes", file.name(), file.len()),
            )?;
            view.download(
                row,
                2,
                &InternalUrl::new(&format!("/private-files/files/{}", file.id())).unwrap(),
                "Download file",
            )?;
            view.keyed_button(
                row,
                3,
                "Delete file",
                10 + index as u32,
                blocked || self.uncertain,
            )?;
        }
        view.keyed_button(
            root,
            8,
            "Previous files",
            5,
            blocked || self.history.is_empty(),
        )?;
        view.keyed_button(
            root,
            9,
            "Next files",
            6,
            blocked || self.listing.next().is_none(),
        )?;
        Ok(view)
    }
    fn dispatch(&mut self, event: u32) -> Result<(), ComponentError> {
        if self.pending.is_some() {
            return Ok(());
        }
        match event {
            1 => {
                self.message = "Inspecting private files…".into();
                self.read(self.cursor, self.history.clone())
            }
            2 if self.authorized && !self.uncertain => {
                let Some(file) = self.file.clone() else {
                    self.invalid = true;
                    self.message = "Select a file before submitting.".into();
                    return Ok(());
                };
                self.upload = Some(
                    UploadRequest::new(
                        2,
                        InternalUrl::new("/private-files/files").unwrap(),
                        "attachment",
                        file,
                        FormData::default(),
                    )
                    .map_err(|_| ComponentError::InvalidEvent)?,
                );
                self.pending = Some(Pending::Upload);
                self.invalid = false;
                self.message = "Storing file…".into();
                Ok(())
            }
            4 => {
                self.reset()?;
                self.message = "Selection cleared.".into();
                Ok(())
            }
            5 if !self.history.is_empty() => {
                let mut history = self.history.clone();
                let cursor = history.pop().unwrap();
                self.read(cursor, history)
            }
            6 if self.listing.next().is_some() => {
                if self.history.len() == MAX_HISTORY {
                    return Err(ComponentError::InvalidEvent);
                }
                let mut history = self.history.clone();
                history.push(self.cursor);
                self.read(self.listing.next(), history)
            }
            10..=12 if self.authorized && !self.uncertain => {
                let info = self
                    .listing
                    .files()
                    .get((event - 10) as usize)
                    .ok_or(ComponentError::InvalidEvent)?;
                self.outgoing = Some(
                    JsonRequest::new(
                        3,
                        JsonMethod::Delete,
                        InternalUrl::new(&format!("/private-files/files/{}", info.id())).unwrap(),
                        None,
                    )
                    .map_err(|_| ComponentError::InvalidEvent)?,
                );
                self.pending = Some(Pending::Delete);
                self.message = "Deleting file…".into();
                Ok(())
            }
            2 | 10..=12 if self.uncertain => Ok(()),
            _ => Err(ComponentError::InvalidEvent),
        }
    }
    fn dispatch_value(&mut self, event: u32, value: EventValue) -> Result<(), ComponentError> {
        match (event, value) {
            (3, EventValue::File(selection)) if self.pending.is_none() && self.authorized => {
                self.invalid = false;
                self.file = match selection {
                    FileSelection::Selected(file) => {
                        self.message = "File selected. Submit explicitly when ready.".into();
                        Some(file)
                    }
                    FileSelection::Empty => {
                        self.message = "Selection cleared.".into();
                        None
                    }
                    FileSelection::Rejected(error) => {
                        self.invalid = true;
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
    fn take_json(&mut self) -> Option<JsonRequest> {
        self.outgoing.take()
    }
    fn take_upload(&mut self) -> Option<UploadRequest> {
        self.upload.take()
    }
    fn upload_result(&mut self, action: u32, result: UploadOutcome) -> Result<(), ComponentError> {
        if action != 2 || !matches!(self.pending.take(), Some(Pending::Upload)) {
            return Err(ComponentError::InvalidEvent);
        }
        match result {
            Ok(reply) if reply.is_success() => {
                if FileInfo::from_json(reply.value())
                    .ok()
                    .zip(self.file.as_ref())
                    .is_none_or(|(info, file)| info.len() != file.size() as usize)
                {
                    self.uncertain = true;
                    self.message =
                        "Invalid upload result. Inspect files before another mutation.".into();
                    return Ok(());
                }
                self.reset()?;
                self.message = "File stored. Selection cleared.".into();
                self.read(None, vec![])
            }
            other => self.failed(other),
        }
    }
    fn json_result(&mut self, action: u32, result: JsonOutcome) -> Result<(), ComponentError> {
        let pending = self.pending.take().ok_or(ComponentError::InvalidEvent)?;
        match (pending, action, result) {
            (Pending::Read { cursor, history }, 1, Ok(reply)) if reply.is_success() => {
                let Ok(listing) = FileListing::from_json(reply.value()) else {
                    self.message = "Invalid file list. Previous state retained.".into();
                    return Ok(());
                };
                if cursor.is_some_and(|after| listing.files().iter().any(|file| file.id() <= after))
                {
                    self.message = "Invalid continuation. Previous state retained.".into();
                    return Ok(());
                }
                self.adopt(listing)?;
                self.cursor = cursor;
                self.history = history;
                self.authorized = true;
                self.uncertain = false;
                if !self.message.starts_with("File stored.")
                    && !self.message.starts_with("File deleted.")
                {
                    self.message = "Private files inspected. No mutation was retried.".into();
                }
                Ok(())
            }
            (Pending::Delete, 3, Ok(reply)) if reply.is_success() => {
                if reply.value().as_bool() != Some(true) {
                    self.uncertain = true;
                    self.message =
                        "Invalid deletion result. Inspect files before another mutation.".into();
                    return Ok(());
                }
                self.message = "File deleted.".into();
                self.read(self.cursor, self.history.clone())
            }
            (Pending::Read { .. }, 1, other) | (Pending::Delete, 3, other) => self.failed(other),
            _ => Err(ComponentError::InvalidEvent),
        }
    }
}
pub(super) fn create() -> Box<dyn Component> {
    Box::new(Manager {
        listing: FileListing::new(vec![], None).unwrap(),
        cursor: None,
        history: vec![],
        keys: BTreeMap::new(),
        next_key: 0,
        file: None,
        epoch: 0,
        pending: None,
        authorized: false,
        uncertain: false,
        invalid: false,
        message: "Inspect files or open the native demo to sign in.".into(),
        outgoing: None,
        upload: None,
    })
}
