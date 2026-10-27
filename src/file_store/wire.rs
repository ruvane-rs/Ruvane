use super::*;
pub(super) fn encode(records: &Records, application: &str) -> Vec<u8> {
    let mut bytes = b"RFS1".to_vec();
    bytes.push(application.len() as u8);
    bytes.extend(application.as_bytes());
    bytes.extend((records.len() as u32).to_le_bytes());
    for (id, record) in records {
        bytes.extend(id.0);
        bytes.push(record.owner.0.len() as u8);
        bytes.extend(record.owner.0.as_bytes());
        bytes.extend((record.file.info.name.as_str().len() as u16).to_le_bytes());
        bytes.extend(record.file.info.name.as_str().as_bytes());
        bytes.extend((record.file.bytes.len() as u32).to_le_bytes());
        bytes.extend(&record.file.bytes);
    }
    bytes.extend(crc32(&bytes).to_le_bytes());
    bytes
}
pub(super) fn decode(
    bytes: &[u8],
    application: &str,
    limits: FileStoreLimits,
) -> Result<Records, FileStoreError> {
    let bad = FileStoreError::Corrupt;
    if bytes.len() < 13 || bytes.len() > MAX_CATALOG || &bytes[..4] != b"RFS1" {
        return Err(bad);
    }
    let end = bytes.len() - 4;
    if crc32(&bytes[..end]) != u32::from_le_bytes(bytes[end..].try_into().map_err(|_| bad)?) {
        return Err(bad);
    }
    let mut input = Input {
        bytes: &bytes[4..end],
        at: 0,
    };
    let length = input.take(1)?[0] as usize;
    if input.take(length)? != application.as_bytes() {
        return Err(bad);
    }
    let count = input.number()? as usize;
    if count > MAX_FILES {
        return Err(bad);
    }
    let mut records = Records::new();
    for _ in 0..count {
        let id = FileId(input.take(32)?.try_into().map_err(|_| bad)?);
        if id.0 == [0; 32] || records.contains_key(&id) {
            return Err(bad);
        }
        let length = input.take(1)?[0] as usize;
        let owner = FileOwner::new(std::str::from_utf8(input.take(length)?).map_err(|_| bad)?)
            .map_err(|_| bad)?;
        let length = u16::from_le_bytes(input.take(2)?.try_into().map_err(|_| bad)?) as usize;
        let name = DownloadName::new(std::str::from_utf8(input.take(length)?).map_err(|_| bad)?)
            .map_err(|_| bad)?;
        let size = input.number()? as usize;
        if size > crate::MAX_UPLOAD_BYTES {
            return Err(bad);
        }
        let bytes = input.take(size)?.to_vec();
        records.insert(
            id,
            Record {
                owner,
                file: StoredFile {
                    info: FileInfo { id, name, size },
                    bytes,
                },
            },
        );
    }
    if input.at != input.bytes.len() {
        return Err(bad);
    }
    limits.check(&records)?;
    Ok(records)
}
struct Input<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Input<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], FileStoreError> {
        let end = self.at.checked_add(length).ok_or(FileStoreError::Corrupt)?;
        let bytes = self
            .bytes
            .get(self.at..end)
            .ok_or(FileStoreError::Corrupt)?;
        self.at = end;
        Ok(bytes)
    }
    fn number(&mut self) -> Result<u32, FileStoreError> {
        Ok(u32::from_le_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| FileStoreError::Corrupt)?,
        ))
    }
}
// CRC detects accidental corruption. It is not authentication.
pub(super) fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}
