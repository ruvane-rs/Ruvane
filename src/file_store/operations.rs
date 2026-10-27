use super::*;
impl FileStore {
    /// Creates an OS-random upload identity valid for one hour. It grants no authorization.
    /// Keeps a bounded owner-bound reservation in memory. Unsubmitted reservations expire on restart.
    /// Issuance changes no files or durable operation records.
    pub fn upload_id(&self, owner: &FileOwner) -> Result<FileUploadId, FileStoreError> {
        let mut state = self.state()?;
        let issued = self.now(&state)?;
        state
            .pending
            .retain(|operation, _| operation.expires_at() > issued);
        let live = state
            .operations
            .keys()
            .filter(|(_, operation)| operation.expires_at() > issued);
        if live.clone().count() + state.pending.len() >= MAX_FILE_UPLOAD_OPERATIONS
            || live.filter(|(key, _)| key == owner).count()
                + state.pending.values().filter(|key| *key == owner).count()
                >= MAX_OWNER_FILE_UPLOAD_OPERATIONS
        {
            return Err(FileStoreError::Limit);
        }

        for _ in 0..8 {
            let random = state
                .entropy
                .read()
                .map_err(|_| FileStoreError::Unavailable)?;
            let id = FileUploadId { issued, random };
            if random != [0; 32]
                && !state
                    .operations
                    .keys()
                    .any(|(_, operation)| *operation == id)
                && !state.pending.contains_key(&id)
            {
                state.pending.insert(id, owner.clone());
                return Ok(id);
            }
        }
        Err(FileStoreError::Unavailable)
    }
    /// Publishes a file and an owner-scoped operation result in the same snapshot.
    /// Within the identity's lifetime, exact repeats return the original metadata.
    /// Different names/bytes, deleted results and expired identities cannot create another file.
    pub fn save_once(
        &self,
        owner: &FileOwner,
        operation: FileUploadId,
        upload: &UploadedFile,
    ) -> Result<FileInfo, FileStoreError> {
        self.write_file(owner, upload, Some(operation))
    }
    /// Reads only this owner's operation. Unconfirmed does not prove that no request is running.
    pub fn upload_status(
        &self,
        owner: &FileOwner,
        operation: FileUploadId,
    ) -> Result<FileUploadStatus, FileStoreError> {
        let state = self.state()?;
        let now = self.now(&state)?;
        let result = if operation.issued > now || operation.expires_at() <= now {
            FileUploadState::Expired
        } else if let Some(id) = state.operations.get(&(owner.clone(), operation)) {
            match state
                .records
                .get(id)
                .filter(|record| &record.owner == owner)
            {
                Some(record) => FileUploadState::Stored(record.file.info.clone()),
                None => FileUploadState::Removed,
            }
        } else if state.pending.get(&operation) == Some(owner) {
            FileUploadState::Unconfirmed
        } else {
            FileUploadState::Expired
        };
        Ok(FileUploadStatus::new(operation, result))
    }
    pub(super) fn now(&self, state: &State) -> Result<u64, FileStoreError> {
        #[cfg(test)]
        let fixed = self.0.time.load(std::sync::atomic::Ordering::Relaxed);
        #[cfg(not(test))]
        let fixed = 0;
        let now = if fixed != 0 {
            fixed
        } else {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| FileStoreError::Unavailable)?
                .as_secs()
        };
        if now == 0
            || now < state.clock
            || now
                .checked_add(crate::FILE_UPLOAD_RETENTION_SECONDS)
                .is_none()
        {
            return Err(FileStoreError::Unavailable);
        }
        Ok(now)
    }
    pub(super) fn write_file(
        &self,
        owner: &FileOwner,
        upload: &UploadedFile,
        operation: Option<FileUploadId>,
    ) -> Result<FileInfo, FileStoreError> {
        let name =
            DownloadName::new(upload.file_name()).map_err(|_| FileStoreError::InvalidInput)?;
        if upload.len() > crate::MAX_UPLOAD_BYTES {
            return Err(FileStoreError::Limit);
        }
        let mut state = self.state()?;
        let now = self.now(&state)?;
        let mut operations = state.operations.clone();
        operations.retain(|(_, operation), _| operation.expires_at() > now);
        if let Some(operation) = operation {
            if operation.issued > now || operation.expires_at() <= now {
                return Err(FileStoreError::OperationExpired);
            }
            if let Some(id) = operations.get(&(owner.clone(), operation)) {
                return match state.records.get(id) {
                    Some(record)
                        if record.owner == *owner
                            && record.file.info.name == name
                            && record.file.bytes == upload.bytes() =>
                    {
                        Ok(record.file.info.clone())
                    }
                    _ => Err(FileStoreError::OperationConflict),
                };
            }
            if state.pending.get(&operation) != Some(owner) {
                return Err(FileStoreError::OperationExpired);
            }
            if operations.len() >= MAX_FILE_UPLOAD_OPERATIONS
                || operations.keys().filter(|(key, _)| key == owner).count()
                    >= MAX_OWNER_FILE_UPLOAD_OPERATIONS
            {
                return Err(FileStoreError::Limit);
            }
        }
        let mut id = None;
        for _ in 0..8 {
            let bytes = state
                .entropy
                .read()
                .map_err(|_| FileStoreError::Unavailable)?;
            let candidate = FileId(bytes);
            if bytes != [0; 32]
                && !state.records.contains_key(&candidate)
                && !operations.values().any(|id| *id == candidate)
            {
                id = Some(candidate);
                break;
            }
        }
        let id = id.ok_or(FileStoreError::Unavailable)?;
        let info = FileInfo {
            id,
            name,
            size: upload.len(),
        };
        let mut records = state.records.clone();
        records.insert(
            id,
            Record {
                owner: owner.clone(),
                file: StoredFile {
                    info: info.clone(),
                    bytes: upload.bytes().to_vec(),
                },
            },
        );
        self.0.limits.check(&records)?;
        if let Some(operation) = operation {
            operations.insert((owner.clone(), operation), id);
        }
        self.publish(&mut state, records, operations, now)?;
        Ok(info)
    }
}
