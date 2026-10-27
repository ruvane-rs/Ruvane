use crate::model::{MAX_TICKETS, State, Ticket, TicketInput};
use ruvane::{EntityTag, EntityTagSource, IfMatch};
use std::{
    path::Path,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
mod disk;
const MAX_BYTES: usize = 160 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreError {
    InvalidInput,
    Busy,
    Corrupt,
    Unavailable,
    Uncertain,
    UnsupportedPlatform,
    Full,
    Missing,
    Conflict,
    Precondition,
}
impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Desk storage: {self:?}")
    }
}
impl std::error::Error for StoreError {}
#[derive(Clone)]
pub struct Snapshot {
    pub ticket: Ticket,
    pub tag: EntityTag,
}
pub struct Store {
    disk: disk::Disk,
    versions: EntityTagSource,
    rows: Mutex<Vec<Snapshot>>,
    failed: AtomicBool,
}
impl Store {
    pub fn open(path: &Path, recover: bool) -> Result<Self, StoreError> {
        let versions = EntityTagSource::new().map_err(|_| StoreError::Unavailable)?;
        let (disk, saved) = disk::Disk::open(path, "desk", recover)?;
        let tickets = saved
            .as_deref()
            .map(decode)
            .transpose()?
            .unwrap_or_default();
        let rows = tickets
            .into_iter()
            .map(|ticket| {
                Ok(Snapshot {
                    ticket,
                    tag: versions.next_tag().map_err(|_| StoreError::Unavailable)?,
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        if saved.is_none() {
            disk.publish(&encode(&[])?)?;
        }
        Ok(Self {
            disk,
            versions,
            rows: Mutex::new(rows),
            failed: AtomicBool::new(false),
        })
    }
    fn locked(&self) -> Result<std::sync::MutexGuard<'_, Vec<Snapshot>>, StoreError> {
        let rows = self.rows.lock().map_err(|_| StoreError::Unavailable)?;
        if self.failed.load(Ordering::Relaxed) {
            return Err(StoreError::Uncertain);
        }
        Ok(rows)
    }
    pub fn list(&self) -> Result<Vec<Ticket>, StoreError> {
        Ok(self.locked()?.iter().map(|s| s.ticket.clone()).collect())
    }
    pub fn get(&self, id: u32) -> Result<Snapshot, StoreError> {
        self.locked()?
            .iter()
            .find(|s| s.ticket.id == id)
            .cloned()
            .ok_or(StoreError::Missing)
    }
    pub fn create(&self, input: TicketInput) -> Result<Snapshot, StoreError> {
        let mut rows = self.locked()?;
        if rows.len() == MAX_TICKETS {
            return Err(StoreError::Full);
        }
        let row = Snapshot {
            ticket: Ticket {
                id: rows.len() as u32 + 1,
                revision: 1,
                input,
            },
            tag: self
                .versions
                .next_tag()
                .map_err(|_| StoreError::Unavailable)?,
        };
        let mut next = rows.clone();
        next.push(row.clone());
        self.publish(&next)?;
        *rows = next;
        Ok(row)
    }
    pub fn update(
        &self,
        id: u32,
        input: TicketInput,
        condition: Option<&IfMatch>,
    ) -> Result<Snapshot, StoreError> {
        let mut rows = self.locked()?;
        let index = rows
            .iter()
            .position(|s| s.ticket.id == id)
            .ok_or(StoreError::Missing)?;
        let condition = condition
            .filter(|c| !c.is_any())
            .ok_or(StoreError::Precondition)?;
        if !condition.matches(Some(&rows[index].tag)) {
            return Err(StoreError::Conflict);
        }
        if rows[index].ticket.input == input {
            return Ok(rows[index].clone());
        }
        let mut next = rows.clone();
        next[index] = Snapshot {
            ticket: Ticket {
                id,
                revision: rows[index]
                    .ticket
                    .revision
                    .checked_add(1)
                    .ok_or(StoreError::Full)?,
                input,
            },
            tag: self
                .versions
                .next_tag()
                .map_err(|_| StoreError::Unavailable)?,
        };
        self.publish(&next)?;
        let row = next[index].clone();
        *rows = next;
        Ok(row)
    }
    fn publish(&self, rows: &[Snapshot]) -> Result<(), StoreError> {
        let bytes = encode(&rows.iter().map(|s| s.ticket.clone()).collect::<Vec<_>>())?;
        self.disk.publish(&bytes).inspect_err(|_| {
            self.failed.store(true, Ordering::Relaxed);
        })
    }
}
fn crc(bytes: &[u8]) -> u32 {
    let mut c = !0u32;
    for b in bytes {
        c ^= *b as u32;
        for _ in 0..8 {
            c = (c >> 1) ^ (0xedb88320u32 & (0u32.wrapping_sub(c & 1)));
        }
    }
    !c
}
fn encode(rows: &[Ticket]) -> Result<Vec<u8>, StoreError> {
    if rows.len() > MAX_TICKETS {
        return Err(StoreError::Corrupt);
    }
    let mut bytes = b"RDS1".to_vec();
    bytes.extend((rows.len() as u32).to_le_bytes());
    for (i, row) in rows.iter().enumerate() {
        if row.id != i as u32 + 1
            || row.revision == 0
            || TicketInput::new(
                &row.input.title,
                &row.input.description,
                row.input.state.wire(),
            )
            .is_err()
        {
            return Err(StoreError::Corrupt);
        }
        bytes.extend(row.id.to_le_bytes());
        bytes.extend(row.revision.to_le_bytes());
        bytes.push(match row.input.state {
            State::Open => 0,
            State::InProgress => 1,
            State::Closed => 2,
        });
        bytes.extend((row.input.title.len() as u16).to_le_bytes());
        bytes.extend((row.input.description.len() as u16).to_le_bytes());
        bytes.extend(row.input.title.as_bytes());
        bytes.extend(row.input.description.as_bytes());
    }
    bytes.extend(crc(&bytes).to_le_bytes());
    Ok(bytes)
}
fn decode(bytes: &[u8]) -> Result<Vec<Ticket>, StoreError> {
    let bad = || StoreError::Corrupt;
    if bytes.len() < 12
        || bytes.len() > MAX_BYTES
        || &bytes[..4] != b"RDS1"
        || crc(&bytes[..bytes.len() - 4])
            != u32::from_le_bytes(bytes[bytes.len() - 4..].try_into().map_err(|_| bad())?)
    {
        return Err(bad());
    }
    let mut rest = &bytes[4..bytes.len() - 4];
    fn take<'a>(rest: &mut &'a [u8], n: usize) -> Result<&'a [u8], StoreError> {
        if rest.len() < n {
            return Err(StoreError::Corrupt);
        }
        let (a, b) = rest.split_at(n);
        *rest = b;
        Ok(a)
    }
    let count = u32::from_le_bytes(take(&mut rest, 4)?.try_into().map_err(|_| bad())?) as usize;
    if count > MAX_TICKETS {
        return Err(bad());
    }
    let mut rows = Vec::with_capacity(count);
    for i in 0..count {
        let id = u32::from_le_bytes(take(&mut rest, 4)?.try_into().map_err(|_| bad())?);
        let revision = u64::from_le_bytes(take(&mut rest, 8)?.try_into().map_err(|_| bad())?);
        let state = match take(&mut rest, 1)?[0] {
            0 => State::Open,
            1 => State::InProgress,
            2 => State::Closed,
            _ => return Err(bad()),
        };
        let title_len =
            u16::from_le_bytes(take(&mut rest, 2)?.try_into().map_err(|_| bad())?) as usize;
        let description_len =
            u16::from_le_bytes(take(&mut rest, 2)?.try_into().map_err(|_| bad())?) as usize;
        let title = std::str::from_utf8(take(&mut rest, title_len)?).map_err(|_| bad())?;
        let description =
            std::str::from_utf8(take(&mut rest, description_len)?).map_err(|_| bad())?;
        let input = TicketInput::new(title, description, state.wire()).map_err(|_| bad())?;
        if id != i as u32 + 1 || revision == 0 {
            return Err(bad());
        }
        rows.push(Ticket {
            id,
            revision,
            input,
        });
    }
    if !rest.is_empty() {
        return Err(bad());
    }
    Ok(rows)
}
#[cfg(test)]
mod tests;
