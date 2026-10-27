//! Private file management. File bytes and owners stay outside the component.
use crate::{
    model::{self, Role},
    pages::url,
};
use ruvane::{BrowserTag::*, *};
use std::collections::BTreeMap;
enum Pending {
    Read,
    Upload,
    Delete,
    PrepareUpload,
    CheckUpload,
}
struct Manager {
    workspace: u32,
    ticket: u32,
    workspace_name: String,
    editable: bool,
    listing: FileListing,
    keys: BTreeMap<FileId, u32>,
    next_key: u32,
    file: Option<BrowserFile>,
    epoch: u32,
    operation: Option<FileUploadId>,
    upload_uncertain: bool,
    retired: bool,
    pending: Option<Pending>,
    authorized: bool,
    uncertain: bool,
    invalid: bool,
    message: String,
    outgoing: Option<JsonRequest>,
    upload: Option<UploadRequest>,
}
impl Manager {
    fn api(&self, suffix: &str) -> InternalUrl {
        url(&format!(
            "/api/w/{}/tickets/{}/{suffix}",
            self.workspace, self.ticket
        ))
    }
    fn web(&self, suffix: &str) -> InternalUrl {
        url(&format!(
            "/w/{}/tickets/{}/{suffix}",
            self.workspace, self.ticket
        ))
    }

    fn adopt(&mut self, listing: FileListing) -> Result<(), ComponentError> {
        if listing.files().len() > 3 || listing.next().is_some() {
            return Err(ComponentError::InvalidProps);
        }
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
        self.operation = None;
        self.upload_uncertain = false;
        self.retired = false;
        self.invalid = false;
        Ok(())
    }
    fn read(&mut self) -> Result<(), ComponentError> {
        self.outgoing = Some(
            JsonRequest::new(1, JsonMethod::Get, self.api("files"), None)
                .map_err(|_| ComponentError::InvalidEvent)?,
        );
        self.pending = Some(Pending::Read);
        Ok(())
    }
    fn send_upload(&mut self) -> Result<(), ComponentError> {
        let file = self.file.clone().ok_or(ComponentError::InvalidEvent)?;
        let operation = self.operation.ok_or(ComponentError::InvalidEvent)?;
        self.upload = Some(
            UploadRequest::new(
                2,
                self.web("files"),
                "attachment",
                file,
                FormData::default()
                    .field("operation", &operation.to_string())
                    .map_err(|_| ComponentError::InvalidEvent)?,
            )
            .map_err(|_| ComponentError::InvalidEvent)?,
        );
        self.pending = Some(Pending::Upload);
        self.invalid = false;
        self.message = "Storing this upload attempt…".into();
        Ok(())
    }
    fn confirmed(&mut self, info: FileInfo) -> Result<(), ComponentError> {
        if self
            .file
            .as_ref()
            .is_none_or(|file| info.len() != file.size() as usize || info.name() != file.name())
        {
            self.uncertain = true;
            self.upload_uncertain = true;
            self.message =
                "Invalid upload result. Check or resume the same attempt explicitly.".into();
            return Ok(());
        }
        self.reset()?;
        self.uncertain = false;
        self.message = "File stored. Selection cleared.".into();
        self.read()
    }
    fn failed(&mut self, result: JsonOutcome, upload: bool) -> Result<(), ComponentError> {
        match result {
            Ok(reply) => {
                self.uncertain |= reply.uncertain();
                if matches!(reply.status(), 401 | 403 | 404) {
                    self.authorized = false;
                    self.editable = false;
                    self.uncertain = false;
                    self.adopt(FileListing::new(vec![], None).unwrap())?;
                    self.reset()?;
                    self.message =
                        "Access changed or session expired. Open workspaces to continue.".into();
                    return Ok(());
                }
                self.invalid = reply.status() == 400;
                self.message = match reply.status() {
                    400 => "Validation failed. Selection retained.".into(),
                    409 => "Request refused: quota, changed input or retired attempt. Check the upload status.".into(),
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
        if self.uncertain && upload && self.operation.is_some() {
            self.upload_uncertain = true;
        }
        if self.upload_uncertain {
            self.message = "Upload result uncertain. Check or resume the same attempt explicitly; nothing was retried.".into();
        } else if self.uncertain {
            self.message =
                "Result uncertain. Inspect files before another mutation; nothing was retried."
                    .into();
        }
        Ok(())
    }
}
impl Component for Manager {
    fn page_data(&mut self, data: ActionData) -> Result<(), ComponentError> {
        if data.number("workspace") != Some(self.workspace as u64)
            || data.number("ticket") != Some(self.ticket as u64)
        {
            return Err(ComponentError::InvalidProps);
        }
        let name = data
            .text("workspace_name")
            .ok_or(ComponentError::InvalidProps)?;
        model::WorkspaceInput::new(name).map_err(|_| ComponentError::InvalidProps)?;
        self.workspace_name = name.into();
        self.editable = data
            .text("role")
            .and_then(Role::parse)
            .ok_or(ComponentError::InvalidProps)?
            .can_edit();
        let count = data
            .number("count")
            .filter(|n| *n <= 3)
            .ok_or(ComponentError::InvalidProps)? as usize;
        if data.len() != 6 + 3 * count {
            return Err(ComponentError::InvalidProps);
        }
        let mut fields = ActionData::default();
        for key in ["count".to_owned(), "next".to_owned()].into_iter().chain(
            (0..count).flat_map(|i| [format!("id{i}"), format!("name{i}"), format!("bytes{i}")]),
        ) {
            fields
                .insert(
                    &key,
                    data.get(&key).ok_or(ComponentError::InvalidProps)?.clone(),
                )
                .map_err(|_| ComponentError::InvalidProps)?;
        }
        let listing = FileListing::from_seed(&fields).map_err(|_| ComponentError::InvalidProps)?;
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
        view.keyed_leaf(root, 1, Heading, "Ticket attachments")?;
        view.keyed_leaf(
            root,
            2,
            Span,
            &format!(
                "{} · ticket #{} · up to 3 files, 32 KiB each · {}",
                self.workspace_name,
                self.ticket,
                if self.editable {
                    "Editor access"
                } else {
                    "Read-only access"
                }
            ),
        )?;
        if self.editable {
            let form = view.form(root, 3, 2)?;
            let label = view.keyed_element(form, 1, Label)?;
            view.keyed_leaf(label, 1, Span, "File (maximum 32 KiB)")?;
            let file = view.file(label, 2, 3, self.invalid)?;
            view.field_epoch(file, self.epoch)?;
            view.field_disabled(file, blocked || self.operation.is_some())?;
            view.submit(
                form,
                2,
                if busy {
                    "Working…"
                } else if self.operation.is_some() {
                    "Resume upload"
                } else {
                    "Store file"
                },
                blocked || self.retired || (self.uncertain && !self.upload_uncertain),
            )?;
            view.keyed_button(
                root,
                4,
                if self.operation.is_some() {
                    "Discard attempt"
                } else {
                    "Clear selection"
                },
                4,
                blocked,
            )?;
            view.keyed_button(
                root,
                10,
                "Check upload",
                7,
                blocked || self.operation.is_none(),
            )?;
        }
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
                &self.api(&format!("files/{}", file.id())),
                "Download file",
            )?;
            if self.editable {
                view.keyed_button(
                    row,
                    3,
                    "Delete file",
                    10 + index as u32,
                    blocked || self.uncertain,
                )?;
            }
        }
        view.link(
            root,
            8,
            &url(&model::ticket_path(self.workspace, Some(self.ticket))),
            "Back to ticket",
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
                self.read()
            }
            2 if self.authorized
                && self.editable
                && !self.retired
                && (!self.uncertain || self.upload_uncertain) =>
            {
                if self.file.is_none() {
                    self.invalid = true;
                    self.message = "Select a file before submitting.".into();
                    return Ok(());
                }
                if self.operation.is_some() {
                    return self.send_upload();
                }
                self.outgoing = Some(
                    JsonRequest::new(4, JsonMethod::Get, self.api("upload-operation"), None)
                        .map_err(|_| ComponentError::InvalidEvent)?,
                );
                self.pending = Some(Pending::PrepareUpload);
                self.message = "Preparing an upload attempt…".into();
                Ok(())
            }
            7 if self.operation.is_some() && self.authorized && self.editable => {
                self.outgoing = Some(
                    JsonRequest::new(
                        5,
                        JsonMethod::Get,
                        self.api(&format!("operations/{}", self.operation.unwrap())),
                        None,
                    )
                    .map_err(|_| ComponentError::InvalidEvent)?,
                );
                self.pending = Some(Pending::CheckUpload);
                self.message = "Checking this upload attempt…".into();
                Ok(())
            }
            4 => {
                if self.upload_uncertain {
                    self.uncertain = false;
                }
                self.reset()?;
                self.message =
                    "Selection and attempt cleared. Inspect files before starting another upload."
                        .into();
                Ok(())
            }
            10..=12 if self.authorized && self.editable && !self.uncertain => {
                let info = self
                    .listing
                    .files()
                    .get((event - 10) as usize)
                    .ok_or(ComponentError::InvalidEvent)?;
                self.outgoing = Some(
                    JsonRequest::new(
                        3,
                        JsonMethod::Delete,
                        self.api(&format!("files/{}", info.id())),
                        None,
                    )
                    .map_err(|_| ComponentError::InvalidEvent)?,
                );
                self.pending = Some(Pending::Delete);
                self.message = "Deleting file…".into();
                Ok(())
            }
            2 if self.uncertain || self.retired => Ok(()),
            10..=12 if self.uncertain => Ok(()),
            _ => Err(ComponentError::InvalidEvent),
        }
    }
    fn dispatch_value(&mut self, event: u32, value: EventValue) -> Result<(), ComponentError> {
        match (event, value) {
            (3, EventValue::File(selection))
                if self.pending.is_none()
                    && self.authorized
                    && self.editable
                    && self.operation.is_none() =>
            {
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
            Ok(reply) if reply.is_success() => match FileInfo::from_json(reply.value()) {
                Ok(info) => self.confirmed(info),
                Err(_) => {
                    self.uncertain = true;
                    self.upload_uncertain = true;
                    self.message =
                        "Invalid upload result. Check or resume the same attempt explicitly."
                            .into();
                    Ok(())
                }
            },
            other => self.failed(other, true),
        }
    }
    fn json_result(&mut self, action: u32, result: JsonOutcome) -> Result<(), ComponentError> {
        let pending = self.pending.take().ok_or(ComponentError::InvalidEvent)?;
        match (pending, action, result) {
            (Pending::Read, 1, Ok(reply)) if reply.is_success() => {
                let Some((listing, role)) = parse_listing(reply.value()) else {
                    self.message = "Invalid file list. Previous state retained.".into();
                    return Ok(());
                };
                self.editable = role.can_edit();
                if !self.editable {
                    self.reset()?;
                }
                self.adopt(listing)?;
                self.authorized = true;
                if self.upload_uncertain {
                    self.message = if self.retired {
                        "Files inspected. This upload attempt is unavailable; discard it before starting another."
                    } else {
                        "Files inspected. Upload still unconfirmed; check or resume this attempt."
                    }.into();
                    return Ok(());
                }
                self.uncertain = false;
                if !self.message.starts_with("File stored.")
                    && !self.message.starts_with("File deleted.")
                {
                    self.message = "Private files inspected. No mutation was retried.".into();
                }
                Ok(())
            }
            (Pending::PrepareUpload, 4, Ok(reply)) if reply.is_success() => {
                let Ok(status) = FileUploadStatus::from_json(reply.value()) else {
                    self.message = "Invalid upload identity. Nothing was submitted.".into();
                    return Ok(());
                };
                if status.state() != &FileUploadState::Unconfirmed {
                    self.message = "Upload identity unavailable. Nothing was submitted.".into();
                    return Ok(());
                }
                self.operation = Some(status.operation());
                self.send_upload()
            }
            (Pending::CheckUpload, 5, Ok(reply)) if reply.is_success() => {
                let Ok(status) = FileUploadStatus::from_json(reply.value()) else {
                    self.message = "Invalid upload status. Attempt retained.".into();
                    return Ok(());
                };
                if Some(status.operation()) != self.operation {
                    self.message = "Wrong upload identity. Attempt retained.".into();
                    return Ok(());
                }
                match status.state() {
                    FileUploadState::Stored(info) => self.confirmed(info.clone()),
                    FileUploadState::Unconfirmed => {
                        self.message = "Upload unconfirmed. Resume explicitly with the same file and identity.".into();
                        Ok(())
                    }
                    state => {
                        self.retired = true;
                        self.uncertain = true;
                        self.upload_uncertain = true;
                        self.message = if state == &FileUploadState::Removed {"Upload confirmed, then deleted. Discard this attempt; it cannot recreate the file."} else {"Upload attempt expired or unavailable. Inspect files, then discard it before starting another upload."}.into();
                        Ok(())
                    }
                }
            }
            (Pending::Delete, 3, Ok(reply)) if reply.is_success() => {
                if reply.value().as_bool() != Some(true) {
                    self.uncertain = true;
                    self.message =
                        "Invalid deletion result. Inspect files before another mutation.".into();
                    return Ok(());
                }
                self.message = "File deleted.".into();
                self.read()
            }
            (Pending::Read, 1, other)
            | (Pending::Delete, 3, other)
            | (Pending::PrepareUpload, 4, other)
            | (Pending::CheckUpload, 5, other) => self.failed(other, false),
            _ => Err(ComponentError::InvalidEvent),
        }
    }
}
pub fn create(workspace: u32, ticket: u32) -> Box<dyn Component> {
    Box::new(Manager {
        workspace,
        ticket,
        workspace_name: String::new(),
        editable: false,
        listing: FileListing::new(vec![], None).unwrap(),
        keys: BTreeMap::new(),
        next_key: 0,
        file: None,
        epoch: 0,
        operation: None,
        upload_uncertain: false,
        retired: false,
        pending: None,
        authorized: false,
        uncertain: false,
        invalid: false,
        message: "Open a workspace to inspect ticket attachments.".into(),
        outgoing: None,
        upload: None,
    })
}

pub fn listing_json(listing: &FileListing, role: Role) -> JsonValue {
    let mut fields = listing.to_json().as_object().unwrap().to_vec();
    fields.push(("role".into(), JsonValue::string(role.wire()).unwrap()));
    JsonValue::object(fields).unwrap()
}
fn parse_listing(value: &JsonValue) -> Option<(FileListing, Role)> {
    if value.as_object()?.len() != 3 {
        return None;
    }
    let role = Role::parse(value.get("role")?.as_str()?)?;
    let bare = JsonValue::object(vec![
        ("files".into(), value.get("files")?.clone()),
        ("next".into(), value.get("next")?.clone()),
    ])
    .ok()?;
    let listing = FileListing::from_json(&bare).ok()?;
    if listing.files().len() > 3 || listing.next().is_some() {
        return None;
    }
    Some((listing, role))
}

#[cfg(test)]
mod tests;
