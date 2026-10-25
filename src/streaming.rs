//! Pull-based HTML response bodies.
use crate::{Document, Html};
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

/// Largest decoded HTML chunk supported by this profile.
pub const MAX_STREAM_CHUNK_BYTES: usize = 64 * 1024;
/// Largest decoded response body supported by this profile.
pub const MAX_STREAM_BODY_BYTES: usize = 64 * 1024 * 1024;
/// Maximum chunks, including the document prefix, suffix and empty yields.
pub const MAX_STREAM_CHUNKS: usize = 1024;

/// Per-response limits, additionally constrained by the server's body/write budgets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamLimits {
    /// Decoded bytes per chunk, from 1 to 64 KiB; defaults to 16 KiB.
    pub chunk_bytes: usize,
    /// Total decoded document bytes, from 1 to 64 MiB; defaults to 1 MiB.
    pub body_bytes: usize,
    /// Chunks including prefix/suffix and empty yields, from 2 to 1024; defaults to 128.
    pub chunks: usize,
    /// Factory, production and socket-write deadline, from 1 ms to 60 s; defaults to 5 s.
    pub duration: Duration,
}
impl Default for StreamLimits {
    fn default() -> Self {
        Self {
            chunk_bytes: 16 * 1024,
            body_bytes: 1024 * 1024,
            chunks: 128,
            duration: Duration::from_secs(5),
        }
    }
}
impl StreamLimits {
    /// Checks the supported ranges without executing application code.
    pub fn validate(self) -> Result<(), StreamError> {
        if !(1..=MAX_STREAM_CHUNK_BYTES).contains(&self.chunk_bytes)
            || !(1..=MAX_STREAM_BODY_BYTES).contains(&self.body_bytes)
            || !(2..=MAX_STREAM_CHUNKS).contains(&self.chunks)
            || !(Duration::from_millis(1)..=Duration::from_secs(60)).contains(&self.duration)
        {
            return Err(StreamError::InvalidLimits);
        }
        Ok(())
    }
}

/// Fixed stream failures. Application diagnostics must stay in local logs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum StreamError {
    /// A configured limit is outside the supported profile.
    InvalidLimits,
    /// A chunk, total byte or producer-yield limit was exceeded.
    Limit,
    /// The absolute production/write deadline expired.
    Deadline,
    /// The server requested shutdown.
    Cancelled,
    /// A socket write failed or the peer disconnected.
    Transport,
    /// Application production failed or panicked with unwinding enabled.
    Producer,
}
impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidLimits => "invalid stream limits",
            Self::Limit => "stream limit exceeded",
            Self::Deadline => "stream deadline exceeded",
            Self::Cancelled => "stream cancelled",
            Self::Transport => "stream transport failed",
            Self::Producer => "stream producer failed",
        })
    }
}
impl std::error::Error for StreamError {}

/// Cooperative deadline/shutdown checks for trusted synchronous producers.
/// Socket disconnects are observed by writes; callbacks cannot be forcibly interrupted.
#[derive(Debug)]
pub struct StreamContext {
    deadline: Instant,
    stopped: Arc<AtomicBool>,
}
impl StreamContext {
    pub(crate) fn new(duration: Duration, stopped: Arc<AtomicBool>) -> Self {
        Self {
            deadline: Instant::now() + duration,
            stopped,
        }
    }
    pub(crate) fn deadline(&self) -> Instant {
        self.deadline
    }
    /// Remaining time, including future socket writes.
    pub fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }
    /// Checks shutdown and the absolute deadline.
    pub fn check(&self) -> Result<(), StreamError> {
        if self.stopped.load(Ordering::Acquire) {
            return Err(StreamError::Cancelled);
        }
        if self.remaining().is_zero() {
            return Err(StreamError::Deadline);
        }
        Ok(())
    }
    /// Waits cooperatively, checking shutdown/deadline at most every 10 ms.
    pub fn wait(&self, duration: Duration) -> Result<(), StreamError> {
        let end = Instant::now()
            .checked_add(duration)
            .ok_or(StreamError::Deadline)?;
        loop {
            self.check()?;
            let remaining = end.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(());
            }
            std::thread::sleep(
                remaining
                    .min(self.remaining())
                    .min(Duration::from_millis(10)),
            );
        }
    }
}

/// Supplies one typed body fragment per pull. None finishes the document.
/// Production is read-only trusted code and must cooperate with StreamContext.
pub trait HtmlProducer: Send {
    /// Produces the next fragment after the previous chunk has been written.
    fn next(&mut self, context: &StreamContext) -> Result<Option<Html>, StreamError>;
}
impl<F> HtmlProducer for F
where
    F: FnMut(&StreamContext) -> Result<Option<Html>, StreamError> + Send,
{
    fn next(&mut self, context: &StreamContext) -> Result<Option<Html>, StreamError> {
        self(context)
    }
}
type Factory = dyn Fn(&StreamContext) -> Result<Box<dyn HtmlProducer>, StreamError> + Send + Sync;

/// A cloneable stream plan. Each GET creates its own producer; HEAD creates none.
/// Prefix and suffix are serialized by Document. Deferred fragments remain typed Html.
#[derive(Clone)]
pub struct HtmlStream {
    prefix: Html,
    limits: StreamLimits,
    factory: Arc<Factory>,
}
impl fmt::Debug for HtmlStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HtmlStream")
            .field("prefix_bytes", &self.prefix.as_str().len())
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}
impl HtmlStream {
    /// Creates a lazy plan with default budgets. The factory runs before headers;
    /// its producer is first pulled after the initial document chunk is written.
    /// Do not perform deferred data loading in the factory itself.
    pub fn new<F, P>(document: Document, factory: F) -> Self
    where
        F: Fn(&StreamContext) -> Result<P, StreamError> + Send + Sync + 'static,
        P: HtmlProducer + 'static,
    {
        Self {
            prefix: document.stream_prefix(),
            limits: StreamLimits::default(),
            factory: Arc::new(move |context| {
                factory(context).map(|producer| Box::new(producer) as Box<dyn HtmlProducer>)
            }),
        }
    }
    /// Sets supported per-response budgets. Server limits can tighten them further.
    pub fn limits(mut self, limits: StreamLimits) -> Result<Self, StreamError> {
        limits.validate()?;
        self.limits = limits;
        Ok(self)
    }
    /// Declared budgets. Their duration/body limits are intersected with server limits.
    pub fn configured_limits(&self) -> StreamLimits {
        self.limits
    }
    /// Already rendered initial bytes; inspecting them never executes the producer.
    pub fn prefix(&self) -> &Html {
        &self.prefix
    }
    pub(crate) fn into_body(self) -> BodyStream {
        let factory = self.factory;
        BodyStream::new(
            self.prefix.into_string().into_bytes(),
            END,
            self.limits,
            move |context| {
                let mut producer = factory(context)?;
                Ok(Box::new(move |context: &StreamContext| {
                    producer
                        .next(context)
                        .map(|value| value.map(|html| html.into_string().into_bytes()))
                }) as Box<dyn ByteProducer>)
            },
        )
    }
}

pub(crate) trait ByteProducer: Send {
    fn next(&mut self, context: &StreamContext) -> Result<Option<Vec<u8>>, StreamError>;
}
impl<F> ByteProducer for F
where
    F: FnMut(&StreamContext) -> Result<Option<Vec<u8>>, StreamError> + Send,
{
    fn next(&mut self, context: &StreamContext) -> Result<Option<Vec<u8>>, StreamError> {
        self(context)
    }
}
type ByteFactory =
    dyn Fn(&StreamContext) -> Result<Box<dyn ByteProducer>, StreamError> + Send + Sync;
pub(crate) struct BodyStream {
    prefix: Vec<u8>,
    suffix: &'static [u8],
    limits: StreamLimits,
    factory: Arc<ByteFactory>,
}
impl fmt::Debug for BodyStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BodyStream")
            .field("prefix_bytes", &self.prefix.len())
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}
impl BodyStream {
    pub(crate) fn new(
        prefix: Vec<u8>,
        suffix: &'static [u8],
        limits: StreamLimits,
        factory: impl Fn(&StreamContext) -> Result<Box<dyn ByteProducer>, StreamError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            prefix,
            suffix,
            limits,
            factory: Arc::new(factory),
        }
    }
    pub(crate) fn prefix(&self) -> &[u8] {
        &self.prefix
    }
    pub(crate) fn configured_limits(&self) -> StreamLimits {
        self.limits
    }
    pub(crate) fn validate(&self, server_bytes: usize) -> Result<usize, StreamError> {
        self.limits.validate()?;
        let cap = self.limits.body_bytes.min(server_bytes);
        if self.prefix.len() > self.limits.chunk_bytes
            || self.suffix.len() > self.limits.chunk_bytes
            || self.prefix.len().saturating_add(self.suffix.len()) > cap
        {
            return Err(StreamError::Limit);
        }
        Ok(cap)
    }
    pub(crate) fn start(
        &self,
        context: &StreamContext,
    ) -> Result<Box<dyn ByteProducer>, StreamError> {
        context.check()?;
        let producer = catch_unwind(AssertUnwindSafe(|| (self.factory)(context)))
            .unwrap_or(Err(StreamError::Producer))?;
        Ok(producer)
    }
    pub(crate) fn send(
        &self,
        producer: &mut dyn ByteProducer,
        context: &StreamContext,
        cap: usize,
        mut emit: impl FnMut(&[u8]) -> Result<(), StreamError>,
    ) -> Result<(), StreamError> {
        context.check()?;
        emit(&self.prefix)?;
        let mut bytes = self.prefix.len();
        let mut chunks = 1usize;
        loop {
            context.check()?;
            let next = catch_unwind(AssertUnwindSafe(|| producer.next(context)))
                .unwrap_or(Err(StreamError::Producer))?;
            context.check()?;
            let Some(fragment) = next else {
                break;
            };
            chunks += 1;
            if chunks >= self.limits.chunks
                || fragment.len() > self.limits.chunk_bytes
                || bytes
                    .saturating_add(fragment.len())
                    .saturating_add(self.suffix.len())
                    > cap
            {
                return Err(StreamError::Limit);
            }
            bytes += fragment.len();
            if !fragment.is_empty() {
                emit(&fragment)?;
            }
        }
        context.check()?;
        emit(self.suffix)
    }
}
const END: &[u8] = b"</body></html>";

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn failed_prefix_write_never_pulls_the_producer() {
        let pulls = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&pulls);
        let plan = HtmlStream::new(Document::new("Test", "Initial"), move |_| {
            let count = Arc::clone(&count);
            Ok(move |_: &StreamContext| {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(Some(crate::text("Next")))
            })
        });
        let context = StreamContext::new(Duration::from_secs(1), Arc::new(AtomicBool::new(false)));
        let plan = plan.into_body();
        let cap = plan.validate(1024).unwrap();
        let mut producer = plan.start(&context).unwrap();
        assert_eq!(
            plan.send(producer.as_mut(), &context, cap, |_| Err(
                StreamError::Transport
            )),
            Err(StreamError::Transport)
        );
        assert_eq!(pulls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn exact_body_and_chunk_budgets_include_the_document_wrapper() {
        let document = Document::new("Test", "Initial");
        let bytes = document.render().as_str().len() + "Next".len();
        let plan = HtmlStream::new(Document::new("Test", "Initial"), |_| {
            let mut sent = false;
            Ok(move |_: &StreamContext| {
                if sent {
                    Ok(None)
                } else {
                    sent = true;
                    Ok(Some(crate::text("Next")))
                }
            })
        })
        .limits(StreamLimits {
            body_bytes: bytes,
            chunks: 3,
            ..StreamLimits::default()
        })
        .unwrap();
        let context = StreamContext::new(Duration::from_secs(1), Arc::new(AtomicBool::new(false)));
        let plan = plan.into_body();
        let cap = plan.validate(bytes).unwrap();
        let mut producer = plan.start(&context).unwrap();
        let mut output = Vec::new();
        plan.send(producer.as_mut(), &context, cap, |chunk| {
            output.extend_from_slice(chunk);
            Ok(())
        })
        .unwrap();
        assert_eq!(output.len(), bytes);
        assert!(output.ends_with(b"Next</body></html>"));
    }
}
