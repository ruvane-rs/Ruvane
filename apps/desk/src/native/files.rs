use super::*;
impl Backend {
    pub(super) fn register_files(
        &self,
        routes: &mut FileRouteRegistry<'_>,
    ) -> Result<(), FileRouteError> {
        let b = self.clone();
        endpoint(
            routes,
            "/api/w/{workspace}/tickets/{id}/files",
            FileEndpoint::new().get(move |r| b.file_list(r)),
        )?;
        let b = self.clone();
        endpoint(
            routes,
            "/api/w/{workspace}/tickets/{id}/upload-operation",
            FileEndpoint::new().get(move |r| b.file_operation(r, true)),
        )?;
        let b = self.clone();
        endpoint(
            routes,
            "/api/w/{workspace}/tickets/{id}/operations/{operation}",
            FileEndpoint::new().get(move |r| b.file_operation(r, false)),
        )?;
        let read = self.clone();
        let remove = self.clone();
        endpoint(
            routes,
            "/api/w/{workspace}/tickets/{id}/files/{file}",
            FileEndpoint::new()
                .get(move |r| read.file_read(r))
                .delete(move |r| remove.file_remove(r, false)),
        )?;
        let b = self.clone();
        endpoint(
            routes,
            "/w/{workspace}/tickets/{id}/files",
            FileEndpoint::new().post_multipart(move |r| b.file_upload(r)),
        )?;
        let b = self.clone();
        endpoint(
            routes,
            "/w/{workspace}/tickets/{id}/files/{file}/remove",
            FileEndpoint::new().post_form(move |r| b.file_remove(r, true)),
        )?;
        Ok(())
    }
    fn file_list(&self, r: &Request) -> Response {
        if !r.query_params().is_empty() {
            return error(StatusCode::BadRequest);
        }
        let (a, w, id) = match self.ticket_scope(r) {
            Ok(v) => v,
            Err(s) => return error(s),
        };
        match self
            .store
            .with_ticket(a, w, id, Access::Read, |owner, role| {
                self.files
                    .list(owner)
                    .map_err(file_error)
                    .and_then(|files| {
                        FileListing::new(files, None).map_err(|_| StoreError::Corrupt)
                    })
                    .map(|list| (list, role))
            }) {
            Ok((list, role)) => Response::json(&crate::attachments::listing_json(&list, role)),
            Err(e) => error(status(e)),
        }
    }
    fn file_operation(&self, r: &Request, create: bool) -> Response {
        if !r.query_params().is_empty() {
            return error(StatusCode::BadRequest);
        }
        let (a, w, id) = match self.ticket_scope(r) {
            Ok(v) => v,
            Err(s) => return error(s),
        };
        let result = self
            .store
            .with_ticket(a, w, id, Access::Edit, |owner, _role| {
                if create {
                    let operation = self.files.upload_id(owner).map_err(file_error)?;
                    Ok(FileUploadStatus::new(
                        operation,
                        FileUploadState::Unconfirmed,
                    ))
                } else {
                    let operation = r
                        .param("operation")
                        .and_then(|s| FileUploadId::parse(s).ok())
                        .ok_or(StoreError::InvalidInput)?;
                    self.files
                        .upload_status(owner, operation)
                        .map_err(file_error)
                }
            });
        match result {
            Ok(value) => Response::json(&value.to_json()),
            Err(e) => error(status(e)),
        }
    }
    fn file_upload(&self, r: &Request) -> Response {
        let (a, w, id) = match self.ticket_scope(r) {
            Ok(v) => v,
            Err(s) => return error(s),
        };
        let form = r.multipart().unwrap();
        if !r.query_params().is_empty() || !form.fields().has_only(&["operation"]) {
            return error(StatusCode::BadRequest);
        }
        let Some(file) = form
            .file()
            .filter(|f| f.field_name() == "attachment" && !f.file_name().is_empty())
        else {
            return error(StatusCode::BadRequest);
        };
        let Some(operation) = form
            .fields()
            .get("operation")
            .and_then(|s| FileUploadId::parse(s).ok())
        else {
            return error(StatusCode::BadRequest);
        };
        let result = self
            .store
            .with_ticket(a, w, id, Access::Edit, |owner, _role| {
                self.files
                    .save_once(owner, operation, file)
                    .map_err(file_error)
            });
        match result{Ok(info)if r.header("accept")==Some(JSON_MEDIA_TYPE)=>Response::json(&info.to_json()),Ok(_)=>Response::see_other(url(&format!("/w/{w}/tickets/{id}/files?native=yes&operation={operation}"))),Err(e)if r.header("accept")==Some(JSON_MEDIA_TYPE)=>error(status(e)),Err(e)=>document("File not stored",element(Tag::Section).child(element(Tag::H2).child("File not stored"))
            .child(element(Tag::P).child("Inspect this attempt before retrying. Reselect the same file only when resuming the same identity."))
            .child(link(&url(&format!("/w/{w}/tickets/{id}/files?native=yes&operation={operation}")),"Inspect this upload attempt")).render(),status(e))}
    }
    fn file_read(&self, r: &Request) -> Response {
        if !r.query_params().is_empty() {
            return error(StatusCode::BadRequest);
        }
        let (a, w, id) = match self.ticket_scope(r) {
            Ok(v) => v,
            Err(s) => return error(s),
        };
        let Some(file) = r.param("file").and_then(|s| FileId::parse(s).ok()) else {
            return error(StatusCode::NotFound);
        };
        match self
            .store
            .with_ticket(a, w, id, Access::Read, |owner, _role| {
                self.files
                    .read(owner, file)
                    .map_err(file_error)
                    .and_then(|f| f.ok_or(StoreError::Missing))
            }) {
            Ok(file) => file.into_response(),
            Err(e) => error(status(e)),
        }
    }
    fn file_remove(&self, r: &Request, native: bool) -> Response {
        if !r.query_params().is_empty() || native && !r.form_data().unwrap().has_only(&[]) {
            return error(StatusCode::BadRequest);
        }
        let (a, w, id) = match self.ticket_scope(r) {
            Ok(v) => v,
            Err(s) => return error(s),
        };
        let Some(file) = r.param("file").and_then(|s| FileId::parse(s).ok()) else {
            return error(StatusCode::NotFound);
        };
        match self
            .store
            .with_ticket(a, w, id, Access::Edit, |owner, _role| {
                self.files
                    .remove(owner, file)
                    .map_err(file_error)
                    .and_then(|removed| {
                        if removed {
                            Ok(())
                        } else {
                            Err(StoreError::Missing)
                        }
                    })
            }) {
            Ok(()) if native => {
                Response::see_other(url(&format!("/w/{w}/tickets/{id}/files?native=yes")))
            }
            Ok(()) => Response::json(&true.into()),
            Err(e) => error(status(e)),
        }
    }
    pub(super) fn files_page(
        &self,
        r: &Request,
        actor: Actor,
        w: u32,
        id: u32,
    ) -> Result<ClientPageData, PageError> {
        query(r, &["native", "prepare", "operation"])?;
        if r.query_params().get("prepare").is_some_and(|v| v != "yes")
            || r.query_params().get("prepare").is_some()
                && r.query_params().get("operation").is_some()
        {
            return Err(PageError::BadRequest);
        }
        let space = self
            .store
            .space(actor, w, Access::Read)
            .map_err(|e| page_error(status(e)))?
            .space;
        let role = space.role(actor).unwrap();
        let token = self.token(r).map_err(page_error)?;
        let (list, operation) = self
            .store
            .with_ticket(
                actor,
                w,
                id,
                if r.query_params().get("prepare").is_some()
                    || r.query_params().get("operation").is_some()
                {
                    Access::Edit
                } else {
                    Access::Read
                },
                |owner, _role| {
                    let files = self.files.list(owner).map_err(file_error)?;
                    let operation = if r.query_params().get("prepare") == Some("yes") {
                        Some(FileUploadStatus::new(
                            self.files.upload_id(owner).map_err(file_error)?,
                            FileUploadState::Unconfirmed,
                        ))
                    } else if let Some(raw) = r.query_params().get("operation") {
                        Some(
                            self.files
                                .upload_status(
                                    owner,
                                    FileUploadId::parse(raw)
                                        .map_err(|_| StoreError::InvalidInput)?,
                                )
                                .map_err(file_error)?,
                        )
                    } else {
                        None
                    };
                    Ok((
                        FileListing::new(files, None).map_err(|_| StoreError::Corrupt)?,
                        operation,
                    ))
                },
            )
            .map_err(|e| page_error(status(e)))?;
        let mut seed = model::scope_seed(
            list.to_seed().map_err(|_| PageError::InternalServerError)?,
            &space,
            role,
        );
        number(&mut seed, "ticket", id as u64);
        let mut body = element(Tag::Section)
            .child(element(Tag::H2).child("Native attachments"))
            .child(element(Tag::P).child(
                "Up to 3 files per ticket, 32 KiB per file. Downloads are private attachments.",
            ));
        if let Some(operation) = operation {
            let message = match operation.state() {
                FileUploadState::Unconfirmed => {
                    "Upload unconfirmed. Resume explicitly with the same file and identity."
                }
                FileUploadState::Stored(_) => "File stored. This attempt is confirmed.",
                FileUploadState::Removed => {
                    "File was stored, then removed. This attempt cannot recreate it."
                }
                FileUploadState::Expired => {
                    "Upload attempt expired or unavailable. Inspect files before preparing another."
                }
            };
            body = body.child(element(Tag::P).child(message));
            if operation.state() == &FileUploadState::Unconfirmed {
                body = body.child(
                    MultipartForm::new(url(&format!("/w/{w}/tickets/{id}/files")))
                        .unwrap()
                        .csrf(token.clone())
                        .unwrap()
                        .hidden("operation", operation.operation().to_string())
                        .unwrap()
                        .file("attachment", "File (up to 32 KiB)")
                        .unwrap()
                        .submit_label("Store or resume this file")
                        .unwrap()
                        .render(),
                );
            }
        }
        if role.can_edit() {
            body = body.child(link(
                &url(&format!("/w/{w}/tickets/{id}/files?native=yes&prepare=yes")),
                "Prepare another native upload",
            ));
        }
        for file in list.files() {
            body = body.child(
                element(Tag::P)
                    .child(format!("{} · {} bytes ", file.name(), file.len()))
                    .child(link(
                        &url(&format!("/api/w/{w}/tickets/{id}/files/{}", file.id())),
                        "Download file",
                    )),
            );
            if role.can_edit() {
                body = body.child(
                    PostForm::new(url(&format!(
                        "/w/{w}/tickets/{id}/files/{}/remove",
                        file.id()
                    )))
                    .unwrap()
                    .csrf(token.clone())
                    .unwrap()
                    .submit_label("Remove file")
                    .unwrap(),
                );
            }
        }
        body = body.child(link(
            &url(&format!("/w/{w}/tickets/{id}?native=yes")),
            "Back to ticket",
        ));
        Ok(ClientPageData::new("Attachments / Ruvane Desk", seed)
            .fallback(body)
            .no_store())
    }
}
fn file_error(e: FileStoreError) -> StoreError {
    match e {
        FileStoreError::Limit
        | FileStoreError::OperationConflict
        | FileStoreError::OperationExpired => StoreError::FileConflict,
        FileStoreError::InvalidInput => StoreError::InvalidInput,
        FileStoreError::Uncertain => StoreError::Uncertain,
        _ => StoreError::Unavailable,
    }
}
