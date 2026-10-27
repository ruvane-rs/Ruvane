use super::*;

pub(super) fn encode(clock: u64, records: &[Record]) -> Vec<u8> {
    let mut bytes = b"RDC1".to_vec();
    bytes.extend(clock.to_le_bytes());
    bytes.extend((records.len() as u32).to_le_bytes());
    for record in records {
        bytes.extend(record.published.to_le_bytes());
        bytes.extend(record.seconds.unwrap_or(0).to_le_bytes());
        bytes.extend(record.stale.to_le_bytes());
        bytes.extend((record.tags.len() as u32).to_le_bytes());
        for tag in &record.tags {
            put(&mut bytes, tag.as_bytes());
        }
        put(&mut bytes, &record.scope);
        put(&mut bytes, &record.plan.encode());
    }
    bytes.extend(checksum(&bytes).to_le_bytes());
    bytes
}
pub(super) fn decode(bytes: &[u8]) -> Result<(u64, Vec<Record>), ServerCacheError> {
    if !(24..=MAX_SERVER_CACHE_DISK_BYTES).contains(&bytes.len()) || &bytes[..4] != b"RDC1" {
        return Err(unavailable());
    }
    let end = bytes.len() - 8;
    if checksum(&bytes[..end]) != u64::from_le_bytes(bytes[end..].try_into().unwrap()) {
        return Err(unavailable());
    }
    let mut input = Input {
        bytes: &bytes[4..end],
        offset: 0,
    };
    let clock = input.long()?;
    let count = input.number()? as usize;
    if count > MAX_SERVER_CACHE_ENTRIES {
        return Err(unavailable());
    }
    let mut records: Vec<Record> = Vec::new();
    for _ in 0..count {
        let published = input.long()?;
        let seconds = match input.number()? {
            0 => None,
            seconds @ 1..=300 => Some(seconds),
            _ => return Err(unavailable()),
        };
        let stale = input.number()?;
        let count = input.number()? as usize;
        if published > clock
            || stale > MAX_SERVER_CACHE_STALE_SECONDS
            || (seconds.is_none() && stale != 0)
            || count > MAX_SERVER_CACHE_TAGS
        {
            return Err(unavailable());
        }
        let mut tags = Vec::new();
        for _ in 0..count {
            let tag = std::str::from_utf8(input.field(crate::MAX_PAGE_NAME_BYTES)?)
                .map_err(|_| unavailable())?
                .to_owned();
            if !crate::page_route::valid_name(&tag) || tags.contains(&tag) {
                return Err(unavailable());
            }
            tags.push(tag);
        }
        let scope = input.field(MAX_SCOPE_BYTES)?.to_vec();
        if scope.is_empty() {
            return Err(unavailable());
        }
        let payload = input.field(2 * 1024 * 1024)?;
        let payload_len = payload.len();
        let plan = ClientPrerender::decode(payload).map_err(|_| unavailable())?;
        if plan.rows.len() != 1
            || records
                .iter()
                .any(|record| record.scope == scope && record.plan.rows[0].url == plan.rows[0].url)
        {
            return Err(unavailable());
        }
        records.push(Record {
            scope,
            published,
            seconds,
            stale,
            tags,
            plan,
            payload_len,
        });
    }
    if input.offset != input.bytes.len() {
        return Err(unavailable());
    }
    Ok((clock, records))
}
fn put(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend((bytes.len() as u32).to_le_bytes());
    out.extend(bytes);
}
fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}
struct Input<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> Input<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], ServerCacheError> {
        let end = self.offset.checked_add(len).ok_or_else(unavailable)?;
        let value = self.bytes.get(self.offset..end).ok_or_else(unavailable)?;
        self.offset = end;
        Ok(value)
    }
    fn number(&mut self) -> Result<u32, ServerCacheError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn long(&mut self) -> Result<u64, ServerCacheError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn field(&mut self, max: usize) -> Result<&'a [u8], ServerCacheError> {
        let len = self.number()? as usize;
        if len > max {
            return Err(unavailable());
        }
        self.take(len)
    }
}
