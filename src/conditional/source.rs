use super::EntityTag;
use crate::{SessionError, sessions::entropy::Entropy};
use std::{
    fmt,
    sync::{Arc, Mutex},
};

/// A native version-source failure. No clock, PID or fallback random generator is used.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntityTagSourceError {
    /// The OS random source or shared state is unavailable.
    Unavailable,
    /// This platform has no supported OS random provider.
    UnsupportedPlatform,
    /// The monotonic counter has reached its maximum value.
    Exhausted,
}
impl fmt::Display for EntityTagSourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unavailable => "entity tag source unavailable",
            Self::UnsupportedPlatform => "entity tag source unsupported on this platform",
            Self::Exhausted => "entity tag source exhausted",
        })
    }
}
impl std::error::Error for EntityTagSourceError {}
struct State {
    namespace: String,
    next: u64,
}
/// A native source of strong version tags with a fresh 256-bit OS-random namespace.
/// Clones share a checked counter. Create a new source for each independent store lifecycle.
/// Tags are public and do not prove authorization or represent a content hash.
#[derive(Clone)]
pub struct EntityTagSource(Arc<Mutex<State>>);
impl EntityTagSource {
    /// Opens the existing macOS/Linux OS provider. Entropy failure aborts initialization.
    pub fn new() -> Result<Self, EntityTagSourceError> {
        let mut entropy = Entropy::open().map_err(|e| match e {
            SessionError::UnsupportedPlatform => EntityTagSourceError::UnsupportedPlatform,
            _ => EntityTagSourceError::Unavailable,
        })?;
        Self::from_entropy(&mut entropy)
    }
    fn from_entropy(entropy: &mut Entropy) -> Result<Self, EntityTagSourceError> {
        use std::fmt::Write;
        for _ in 0..4 {
            let bytes = entropy
                .read()
                .map_err(|_| EntityTagSourceError::Unavailable)?;
            if bytes == [0; 32] {
                continue;
            }
            let mut namespace = String::with_capacity(64);
            for byte in bytes {
                write!(namespace, "{byte:02x}").unwrap();
            }
            return Ok(Self(Arc::new(Mutex::new(State { namespace, next: 0 }))));
        }
        Err(EntityTagSourceError::Unavailable)
    }
    /// Allocates one unique tag within this source. Counter exhaustion and poisoning fail closed.
    /// Call under the resource mutation lock after checking its precondition, before committing.
    pub fn next_tag(&self) -> Result<EntityTag, EntityTagSourceError> {
        let mut state = self
            .0
            .lock()
            .map_err(|_| EntityTagSourceError::Unavailable)?;
        let next = state
            .next
            .checked_add(1)
            .ok_or(EntityTagSourceError::Exhausted)?;
        let tag = EntityTag::strong(&format!("{}-{next:016x}", state.namespace))
            .map_err(|_| EntityTagSourceError::Unavailable)?;
        state.next = next;
        Ok(tag)
    }
}
impl fmt::Debug for EntityTagSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EntityTagSource([redacted])")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_failure_never_falls_back_and_counter_exhaustion_never_reuses_tags() {
        let mut entropy = Entropy::Scripted([Err(SessionError::EntropyUnavailable)].into());
        assert!(matches!(
            EntityTagSource::from_entropy(&mut entropy),
            Err(EntityTagSourceError::Unavailable)
        ));
        let mut entropy = Entropy::Scripted([Ok([0; 32]); 4].into());
        assert!(matches!(
            EntityTagSource::from_entropy(&mut entropy),
            Err(EntityTagSourceError::Unavailable)
        ));
        let mut entropy = Entropy::Scripted([Ok([0; 32]), Ok([1; 32])].into());
        let source = EntityTagSource::from_entropy(&mut entropy).unwrap();
        let first = source.next_tag().unwrap();
        assert_ne!(first, source.clone().next_tag().unwrap());
        source.0.lock().unwrap().next = u64::MAX - 1;
        let last = source.next_tag().unwrap();
        assert!(last.as_str().ends_with("ffffffffffffffff\""));
        assert_eq!(source.next_tag(), Err(EntityTagSourceError::Exhausted));
        assert_eq!(source.next_tag(), Err(EntityTagSourceError::Exhausted));
    }
    #[test]
    fn clones_share_one_counter_across_threads_and_sources_have_separate_namespaces() {
        let source =
            EntityTagSource::from_entropy(&mut Entropy::Scripted([Ok([1; 32])].into())).unwrap();
        let threads = (0..8)
            .map(|_| {
                let source = source.clone();
                std::thread::spawn(move || {
                    (0..64)
                        .map(|_| source.next_tag().unwrap().as_str().to_owned())
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>();
        let tags = threads
            .into_iter()
            .flat_map(|t| t.join().unwrap())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(tags.len(), 512);
        let other =
            EntityTagSource::from_entropy(&mut Entropy::Scripted([Ok([2; 32])].into())).unwrap();
        assert!(!tags.contains(other.next_tag().unwrap().as_str()));
        let broken = source.clone();
        let _ = std::thread::spawn(move || {
            let _guard = broken.0.lock().unwrap();
            panic!("poison");
        })
        .join();
        assert_eq!(source.next_tag(), Err(EntityTagSourceError::Unavailable));
    }
}
