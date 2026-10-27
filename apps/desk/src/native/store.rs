use crate::model::{
    Actor, MAX_TICKETS, MAX_WORKSPACES, Role, Space, State, Ticket, TicketInput, WorkspaceInput,
};
use ruvane::{EntityTag, EntityTagSource, FileOwner, IfMatch};
use std::{
    path::Path,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
mod disk;
mod wire;
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
    Forbidden,
    FileConflict,
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
#[derive(Clone)]
pub struct SpaceSnapshot {
    pub space: Space,
    pub tag: EntityTag,
}
#[derive(Clone)]
struct Catalog {
    spaces: Vec<SpaceSnapshot>,
    tickets: Vec<Snapshot>,
}
#[derive(Clone, Copy)]
pub enum Access {
    Read,
    Edit,
    Manage,
}
pub struct Store {
    disk: disk::Disk,
    versions: Vec<EntityTagSource>,
    state: Mutex<Catalog>,
    failed: AtomicBool,
}
fn defaults() -> Vec<Space> {
    vec![
        Space {
            id: 1,
            name: "Ruvane".into(),
            revision: 1,
            members: [
                Some(Role::Owner),
                Some(Role::Editor),
                Some(Role::Viewer),
                None,
            ],
        },
        Space {
            id: 2,
            name: "Sandbox".into(),
            revision: 1,
            members: [Some(Role::Owner), None, None, Some(Role::Viewer)],
        },
    ]
}
impl Store {
    pub fn open(path: &Path, recover: bool) -> Result<Self, StoreError> {
        let versions = (0..MAX_WORKSPACES)
            .map(|_| EntityTagSource::new().map_err(|_| StoreError::Unavailable))
            .collect::<Result<Vec<_>, _>>()?;
        let (disk, saved) = disk::Disk::open(path, "desk", recover)?;
        let (spaces, tickets) = saved
            .as_deref()
            .map(wire::decode)
            .transpose()?
            .unwrap_or((defaults(), vec![]));
        let spaces = spaces
            .into_iter()
            .map(|space| {
                Ok(SpaceSnapshot {
                    tag: versions[space.id as usize - 1]
                        .next_tag()
                        .map_err(|_| StoreError::Unavailable)?,
                    space,
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        let tickets = tickets
            .into_iter()
            .map(|ticket| {
                Ok(Snapshot {
                    tag: versions[ticket.workspace as usize - 1]
                        .next_tag()
                        .map_err(|_| StoreError::Unavailable)?,
                    ticket,
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        let state = Catalog { spaces, tickets };
        if saved.is_none() {
            disk.publish(&wire::encode(&state)?)?;
        }
        Ok(Self {
            disk,
            versions,
            state: Mutex::new(state),
            failed: AtomicBool::new(false),
        })
    }
    fn locked(&self) -> Result<std::sync::MutexGuard<'_, Catalog>, StoreError> {
        let state = self.state.lock().map_err(|_| StoreError::Unavailable)?;
        if self.failed.load(Ordering::Relaxed) {
            return Err(StoreError::Uncertain);
        }
        Ok(state)
    }
    fn authorize(
        state: &Catalog,
        actor: Actor,
        workspace: u32,
        access: Access,
    ) -> Result<&SpaceSnapshot, StoreError> {
        let space = state
            .spaces
            .iter()
            .find(|s| s.space.id == workspace)
            .ok_or(StoreError::Missing)?;
        let role = space.space.role(actor).ok_or(StoreError::Missing)?;
        if matches!(access, Access::Edit) && !role.can_edit()
            || matches!(access, Access::Manage) && role != Role::Owner
        {
            return Err(StoreError::Forbidden);
        }
        Ok(space)
    }
    pub fn spaces(&self, actor: Actor) -> Result<Vec<Space>, StoreError> {
        Ok(self
            .locked()?
            .spaces
            .iter()
            .filter(|s| s.space.role(actor).is_some())
            .map(|s| s.space.clone())
            .collect())
    }
    pub fn space(
        &self,
        actor: Actor,
        workspace: u32,
        access: Access,
    ) -> Result<SpaceSnapshot, StoreError> {
        let state = self.locked()?;
        Ok(Self::authorize(&state, actor, workspace, access)?.clone())
    }
    pub fn count(&self) -> Result<usize, StoreError> {
        Ok(self.locked()?.tickets.len())
    }
    pub fn create_space(&self, actor: Actor, input: WorkspaceInput) -> Result<Space, StoreError> {
        let mut state = self.locked()?;
        if !state
            .spaces
            .iter()
            .any(|s| s.space.role(actor) == Some(Role::Owner))
        {
            return Err(StoreError::Forbidden);
        }
        if state.spaces.len() == MAX_WORKSPACES as usize {
            return Err(StoreError::Full);
        }
        if state
            .spaces
            .iter()
            .any(|s| s.space.role(actor).is_some() && s.space.name == input.name)
        {
            return Err(StoreError::InvalidInput);
        }
        WorkspaceInput::new(&input.name).map_err(|_| StoreError::InvalidInput)?;
        let mut members = [None; 4];
        members[actor.index()] = Some(Role::Owner);
        let space = Space {
            id: state.spaces.len() as u32 + 1,
            name: input.name,
            revision: 1,
            members,
        };
        let mut next = state.clone();
        next.spaces.push(SpaceSnapshot {
            tag: self.tag(space.id)?,
            space: space.clone(),
        });
        self.publish(&next)?;
        *state = next;
        Ok(space)
    }
    pub fn members(&self, actor: Actor, workspace: u32) -> Result<SpaceSnapshot, StoreError> {
        self.space(actor, workspace, Access::Manage)
    }
    pub fn set_members(
        &self,
        actor: Actor,
        workspace: u32,
        members: [Option<Role>; 4],
        condition: Option<&IfMatch>,
    ) -> Result<SpaceSnapshot, StoreError> {
        let mut state = self.locked()?;
        let space = Self::authorize(&state, actor, workspace, Access::Manage)?;
        check(condition, &space.tag)?;
        if !members.contains(&Some(Role::Owner)) {
            return Err(StoreError::InvalidInput);
        }
        if space.space.members == members {
            return Ok(space.clone());
        }
        let mut next = state.clone();
        let row = &mut next.spaces[workspace as usize - 1];
        row.space.members = members;
        row.space.revision = row.space.revision.checked_add(1).ok_or(StoreError::Full)?;
        row.tag = self.tag(workspace)?;
        let result = row.clone();
        self.publish(&next)?;
        *state = next;
        Ok(result)
    }
    pub fn list(&self, actor: Actor, workspace: u32) -> Result<Vec<Ticket>, StoreError> {
        let state = self.locked()?;
        Self::authorize(&state, actor, workspace, Access::Read)?;
        Ok(state
            .tickets
            .iter()
            .filter(|s| s.ticket.workspace == workspace)
            .map(|s| s.ticket.clone())
            .collect())
    }
    fn find(state: &Catalog, workspace: u32, id: u32) -> Result<&Snapshot, StoreError> {
        state
            .tickets
            .iter()
            .find(|s| s.ticket.workspace == workspace && s.ticket.id == id)
            .ok_or(StoreError::Missing)
    }
    pub fn get(&self, actor: Actor, workspace: u32, id: u32) -> Result<Snapshot, StoreError> {
        let state = self.locked()?;
        Self::authorize(&state, actor, workspace, Access::Read)?;
        Ok(Self::find(&state, workspace, id)?.clone())
    }
    pub fn create(
        &self,
        actor: Actor,
        workspace: u32,
        input: TicketInput,
    ) -> Result<Snapshot, StoreError> {
        let mut state = self.locked()?;
        Self::authorize(&state, actor, workspace, Access::Edit)?;
        if state.tickets.len() == MAX_TICKETS {
            return Err(StoreError::Full);
        }
        let row = Snapshot {
            ticket: Ticket {
                id: state
                    .tickets
                    .iter()
                    .filter(|s| s.ticket.workspace == workspace)
                    .count() as u32
                    + 1,
                workspace,
                revision: 1,
                input,
            },
            tag: self.tag(workspace)?,
        };
        let mut next = state.clone();
        next.tickets.push(row.clone());
        next.tickets
            .sort_by_key(|s| (s.ticket.workspace, s.ticket.id));
        self.publish(&next)?;
        *state = next;
        Ok(row)
    }
    pub fn update(
        &self,
        actor: Actor,
        workspace: u32,
        id: u32,
        input: TicketInput,
        condition: Option<&IfMatch>,
    ) -> Result<Snapshot, StoreError> {
        let mut state = self.locked()?;
        Self::authorize(&state, actor, workspace, Access::Edit)?;
        let current = Self::find(&state, workspace, id)?;
        check(condition, &current.tag)?;
        if current.ticket.input == input {
            return Ok(current.clone());
        }
        let mut next = state.clone();
        let index = next
            .tickets
            .iter()
            .position(|s| s.ticket.workspace == workspace && s.ticket.id == id)
            .ok_or(StoreError::Missing)?;
        let row = &mut next.tickets[index];
        row.ticket.input = input;
        row.ticket.revision = row.ticket.revision.checked_add(1).ok_or(StoreError::Full)?;
        row.tag = self.tag(workspace)?;
        let result = row.clone();
        self.publish(&next)?;
        *state = next;
        Ok(result)
    }
    pub fn with_ticket<T>(
        &self,
        actor: Actor,
        workspace: u32,
        id: u32,
        access: Access,
        operation: impl FnOnce(&FileOwner, Role) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let state = self.locked()?;
        let role = Self::authorize(&state, actor, workspace, access)?
            .space
            .role(actor)
            .unwrap();
        Self::find(&state, workspace, id)?;
        let owner = FileOwner::new(&format!("desk-w{workspace}-t{id}"))
            .map_err(|_| StoreError::InvalidInput)?;
        let result = operation(&owner, role);
        drop(state);
        result
    }
    fn tag(&self, workspace: u32) -> Result<EntityTag, StoreError> {
        self.versions
            .get(workspace as usize - 1)
            .ok_or(StoreError::InvalidInput)?
            .next_tag()
            .map_err(|_| StoreError::Unavailable)
    }
    fn publish(&self, state: &Catalog) -> Result<(), StoreError> {
        self.disk.publish(&wire::encode(state)?).inspect_err(|_| {
            self.failed.store(true, Ordering::Relaxed);
        })
    }
}
fn check(condition: Option<&IfMatch>, tag: &EntityTag) -> Result<(), StoreError> {
    let condition = condition
        .filter(|c| !c.is_any())
        .ok_or(StoreError::Precondition)?;
    if !condition.matches(Some(tag)) {
        return Err(StoreError::Conflict);
    }
    Ok(())
}
#[cfg(test)]
mod tests;
