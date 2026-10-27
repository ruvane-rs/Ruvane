use super::*;
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
pub(super) fn legacy_encode(rows: &[Ticket]) -> Result<Vec<u8>, StoreError> {
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
fn legacy_decode(bytes: &[u8]) -> Result<Vec<Ticket>, StoreError> {
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
            workspace: 1,
            revision,
            input,
        });
    }
    if !rest.is_empty() {
        return Err(bad());
    }
    Ok(rows)
}

pub(super) fn encode(state: &Catalog) -> Result<Vec<u8>, StoreError> {
    let spaces = state
        .spaces
        .iter()
        .map(|s| s.space.clone())
        .collect::<Vec<_>>();
    let tickets = state
        .tickets
        .iter()
        .map(|s| s.ticket.clone())
        .collect::<Vec<_>>();
    validate(&spaces, &tickets)?;
    let mut bytes = b"RDS2".to_vec();
    bytes.push(spaces.len() as u8);
    for space in spaces {
        bytes.extend(space.id.to_le_bytes());
        bytes.extend(space.revision.to_le_bytes());
        bytes.push(space.name.len() as u8);
        bytes.extend(space.name.as_bytes());
        for role in space.members {
            bytes.push(match role {
                None => 0,
                Some(Role::Viewer) => 1,
                Some(Role::Editor) => 2,
                Some(Role::Owner) => 3,
            });
        }
    }
    bytes.extend((tickets.len() as u32).to_le_bytes());
    for ticket in tickets {
        bytes.extend(ticket.workspace.to_le_bytes());
        let single = Ticket {
            id: 1,
            ..ticket.clone()
        };
        let row = legacy_encode(&[single])?;
        bytes.extend(ticket.id.to_le_bytes());
        bytes.extend(&row[12..row.len() - 4]);
    }
    bytes.extend(crc(&bytes).to_le_bytes());
    if bytes.len() > MAX_BYTES {
        return Err(StoreError::Corrupt);
    }
    Ok(bytes)
}
fn validate(spaces: &[Space], tickets: &[Ticket]) -> Result<(), StoreError> {
    if spaces.is_empty() || spaces.len() > MAX_WORKSPACES as usize || tickets.len() > MAX_TICKETS {
        return Err(StoreError::Corrupt);
    }
    for (i, space) in spaces.iter().enumerate() {
        if space.id != i as u32 + 1
            || space.revision == 0
            || WorkspaceInput::new(&space.name).is_err()
            || !space.members.contains(&Some(Role::Owner))
        {
            return Err(StoreError::Corrupt);
        }
    }
    let mut counts = [0u32; 4];
    let mut previous = (0, 0);
    for ticket in tickets {
        if ticket.workspace == 0
            || ticket.workspace > spaces.len() as u32
            || ticket.revision == 0
            || TicketInput::new(
                &ticket.input.title,
                &ticket.input.description,
                ticket.input.state.wire(),
            )
            .is_err()
        {
            return Err(StoreError::Corrupt);
        }
        let count = &mut counts[ticket.workspace as usize - 1];
        *count += 1;
        if ticket.id != *count || (ticket.workspace, ticket.id) <= previous {
            return Err(StoreError::Corrupt);
        }
        previous = (ticket.workspace, ticket.id);
    }
    Ok(())
}
pub(super) fn decode(bytes: &[u8]) -> Result<(Vec<Space>, Vec<Ticket>), StoreError> {
    if bytes.starts_with(b"RDS1") {
        return Ok((defaults(), legacy_decode(bytes)?));
    }
    if bytes.len() < 13
        || bytes.len() > MAX_BYTES
        || !bytes.starts_with(b"RDS2")
        || crc(&bytes[..bytes.len() - 4])
            != u32::from_le_bytes(
                bytes[bytes.len() - 4..]
                    .try_into()
                    .map_err(|_| StoreError::Corrupt)?,
            )
    {
        return Err(StoreError::Corrupt);
    }
    struct Reader<'a>(&'a [u8]);
    impl Reader<'_> {
        fn take(&mut self, n: usize) -> Result<&[u8], StoreError> {
            if n > self.0.len() {
                return Err(StoreError::Corrupt);
            }
            let (a, b) = self.0.split_at(n);
            self.0 = b;
            Ok(a)
        }
        fn byte(&mut self) -> Result<u8, StoreError> {
            Ok(self.take(1)?[0])
        }
        fn word(&mut self) -> Result<u32, StoreError> {
            Ok(u32::from_le_bytes(
                self.take(4)?.try_into().map_err(|_| StoreError::Corrupt)?,
            ))
        }
        fn wide(&mut self) -> Result<u64, StoreError> {
            Ok(u64::from_le_bytes(
                self.take(8)?.try_into().map_err(|_| StoreError::Corrupt)?,
            ))
        }
        fn short(&mut self) -> Result<usize, StoreError> {
            Ok(
                u16::from_le_bytes(self.take(2)?.try_into().map_err(|_| StoreError::Corrupt)?)
                    as usize,
            )
        }
    }
    let mut r = Reader(&bytes[4..bytes.len() - 4]);
    let count = r.byte()? as usize;
    if count == 0 || count > MAX_WORKSPACES as usize {
        return Err(StoreError::Corrupt);
    }
    let mut spaces = vec![];
    for _ in 0..count {
        let id = r.word()?;
        let revision = r.wide()?;
        let n = r.byte()? as usize;
        let name = std::str::from_utf8(r.take(n)?)
            .map_err(|_| StoreError::Corrupt)?
            .to_owned();
        let mut members = [None; 4];
        for role in &mut members {
            *role = match r.byte()? {
                0 => None,
                1 => Some(Role::Viewer),
                2 => Some(Role::Editor),
                3 => Some(Role::Owner),
                _ => return Err(StoreError::Corrupt),
            }
        }
        spaces.push(Space {
            id,
            name,
            revision,
            members,
        });
    }
    let count = r.word()? as usize;
    if count > MAX_TICKETS {
        return Err(StoreError::Corrupt);
    }
    let mut tickets = vec![];
    for _ in 0..count {
        let workspace = r.word()?;
        let id = r.word()?;
        let revision = r.wide()?;
        let state = match r.byte()? {
            0 => "open",
            1 => "in_progress",
            2 => "closed",
            _ => return Err(StoreError::Corrupt),
        };
        let a = r.short()?;
        let b = r.short()?;
        let title = std::str::from_utf8(r.take(a)?)
            .map_err(|_| StoreError::Corrupt)?
            .to_owned();
        let description = std::str::from_utf8(r.take(b)?).map_err(|_| StoreError::Corrupt)?;
        let input =
            TicketInput::new(&title, description, state).map_err(|_| StoreError::Corrupt)?;
        tickets.push(Ticket {
            id,
            workspace,
            revision,
            input,
        });
    }
    if !r.0.is_empty() {
        return Err(StoreError::Corrupt);
    }
    validate(&spaces, &tickets)?;
    Ok((spaces, tickets))
}
