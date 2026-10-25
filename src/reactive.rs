//! Single-threaded reactivity with scoped lifetimes.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::{Rc, Weak};

/// Resource limits for one reactive runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReactiveLimits {
    /// Live owners; defaults to 256, supported range 1..=4096.
    pub max_scopes: usize,
    /// Live signals including memo caches; defaults to 1024, range 1..=65536.
    pub max_signals: usize,
    /// Live computations including memos; defaults to 1024, range 1..=65536.
    pub max_effects: usize,
    /// Input entries per computation; defaults to 32, range 1..=128.
    pub max_dependencies: usize,
    /// Live dependency edges; defaults to 4096, range 1..=1048576.
    pub max_subscriptions: usize,
    /// Callback executions per flush; defaults to 1024, range 1..=1048576.
    pub max_runs: usize,
}

impl Default for ReactiveLimits {
    fn default() -> Self {
        Self {
            max_scopes: 256,
            max_signals: 1024,
            max_effects: 1024,
            max_dependencies: 32,
            max_subscriptions: 4096,
            max_runs: 1024,
        }
    }
}

/// Reactive runtime errors. Messages contain no application data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ReactiveError {
    /// Configuration is outside the documented supported ranges.
    InvalidLimits,
    /// An owner, resource, dependency, or identifier budget is exhausted.
    Limit,
    /// The owner, source, or runtime has been disposed.
    Disposed,
    /// A dependency belongs to another runtime.
    ForeignRuntime,
    /// A newly registered memo has not run yet inside a batch or callback.
    Pending,
    /// A value is already borrowed during custom Clone or Drop code.
    Borrowed,
    /// A memo computation attempted a write, registration, or batch.
    WriteDuringMemo,
    /// An explicit flush was requested from inside a running callback or batch.
    ReentrantFlush,
    /// Propagation exceeded its execution budget, possibly because of a cycle.
    RunLimit,
    /// A callback unwound; recovery requires panic=unwind.
    CallbackPanicked,
    /// A previous callback failure permanently stopped this runtime.
    Faulted,
}

impl fmt::Display for ReactiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidLimits => "invalid reactive limits",
            Self::Limit => "reactive resource limit exceeded",
            Self::Disposed => "reactive resource disposed",
            Self::ForeignRuntime => "dependency belongs to another reactive runtime",
            Self::Pending => "reactive memo has not run yet",
            Self::Borrowed => "reactive value already borrowed",
            Self::WriteDuringMemo => "mutation during reactive memo computation",
            Self::ReentrantFlush => "reactive flush already running or deferred",
            Self::RunLimit => "reactive propagation limit exceeded",
            Self::CallbackPanicked => "reactive callback panicked",
            Self::Faulted => "reactive runtime stopped after a failure",
        })
    }
}

impl std::error::Error for ReactiveError {}

/// Resource counts and scheduler state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReactiveStats {
    /// Live owner scopes.
    pub scopes: usize,
    /// Live values, including memo caches.
    pub signals: usize,
    /// Live callbacks, including memo recomputations.
    pub effects: usize,
    /// Live, deduplicated dependency edges.
    pub subscriptions: usize,
    /// Distinct computations waiting to run.
    pub queued: usize,
    /// Active, nested batch bodies.
    pub batch_depth: usize,
    /// Whether a propagation flush is currently running.
    pub flushing: bool,
    /// Whether a callback failure has permanently stopped propagation.
    pub faulted: bool,
}

type Id = u64;
type Callback = Rc<RefCell<Box<dyn FnMut() -> Result<(), ReactiveError>>>>;

trait ErasedValue {
    fn clear(&self);
}

struct Value<T>(RefCell<Option<T>>);

impl<T> ErasedValue for Value<T> {
    fn clear(&self) {
        // If Clone disposes this scope mid-read, the reader releases the value.
        let previous = self.0.try_borrow_mut().ok().and_then(|mut v| v.take());
        drop(previous);
    }
}

struct Owner {
    parent: Option<Id>,
    children: BTreeSet<Id>,
    signals: BTreeSet<Id>,
    effects: BTreeSet<Id>,
}

struct Node {
    owner: Id,
    value: Rc<dyn ErasedValue>,
    subscribers: BTreeSet<Id>,
    producer: Option<Id>,
}

struct Computation {
    owner: Id,
    dependencies: BTreeSet<Id>,
    callback: Callback,
    output: Option<Id>,
}

struct State {
    limits: ReactiveLimits,
    next: Id,
    owners: BTreeMap<Id, Owner>,
    nodes: BTreeMap<Id, Node>,
    effects: BTreeMap<Id, Computation>,
    memos: BTreeSet<Id>,
    pending: BTreeSet<Id>,
    subscriptions: usize,
    batch_depth: usize,
    flushing: bool,
    in_memo: bool,
    failure: Option<ReactiveError>,
}

impl State {
    fn active(&self) -> Result<(), ReactiveError> {
        if self.failure.is_some() {
            Err(ReactiveError::Faulted)
        } else {
            Ok(())
        }
    }

    fn writable(&self) -> Result<(), ReactiveError> {
        self.active()?;
        if self.in_memo {
            Err(ReactiveError::WriteDuringMemo)
        } else {
            Ok(())
        }
    }

    fn id(&mut self) -> Result<Id, ReactiveError> {
        let next = self.next.checked_add(1).ok_or(ReactiveError::Limit)?;
        self.next = next;
        Ok(next)
    }

    fn enqueue(&mut self, id: Id) {
        if let Some(effect) = self.effects.get(&id) {
            if effect.output.is_some() {
                self.memos.insert(id);
            } else {
                self.pending.insert(id);
            }
        }
    }
}

struct Inner(RefCell<State>);

enum Remove {
    Scope(Id),
    Signal(Id),
    Effect(Id),
}

impl Inner {
    fn fail(&self, error: ReactiveError) {
        let mut state = self.0.borrow_mut();
        state.failure.get_or_insert(error);
        state.memos.clear();
        state.pending.clear();
    }

    fn remove(&self, first: Remove) -> bool {
        let mut values = Vec::new();
        let mut callbacks = Vec::new();
        let mut removed = false;
        {
            let mut state = self.0.borrow_mut();
            let mut work = vec![first];
            while let Some(item) = work.pop() {
                match item {
                    Remove::Scope(id) => {
                        let Some(owner) = state.owners.remove(&id) else {
                            continue;
                        };
                        removed = true;
                        if let Some(parent) = owner.parent.and_then(|p| state.owners.get_mut(&p)) {
                            parent.children.remove(&id);
                        }
                        work.extend(owner.children.into_iter().map(Remove::Scope));
                        work.extend(owner.signals.into_iter().map(Remove::Signal));
                        work.extend(owner.effects.into_iter().map(Remove::Effect));
                    }
                    Remove::Signal(id) => {
                        let Some(node) = state.nodes.remove(&id) else {
                            continue;
                        };
                        removed = true;
                        if let Some(owner) = state.owners.get_mut(&node.owner) {
                            owner.signals.remove(&id);
                        }
                        work.extend(node.subscribers.into_iter().map(Remove::Effect));
                        work.extend(node.producer.map(Remove::Effect));
                        values.push(node.value);
                    }
                    Remove::Effect(id) => {
                        let Some(effect) = state.effects.remove(&id) else {
                            continue;
                        };
                        removed = true;
                        state.memos.remove(&id);
                        state.pending.remove(&id);
                        state.subscriptions -= effect.dependencies.len();
                        for source in effect.dependencies {
                            if let Some(node) = state.nodes.get_mut(&source) {
                                node.subscribers.remove(&id);
                            }
                        }
                        if let Some(owner) = state.owners.get_mut(&effect.owner) {
                            owner.effects.remove(&id);
                        }
                        work.extend(effect.output.map(Remove::Signal));
                        callbacks.push(effect.callback);
                    }
                }
            }
        }
        // Drop outside the state borrow: destructors may call back into the runtime.
        for value in values {
            value.clear();
        }
        drop(callbacks);
        removed
    }

    fn notify(&self, node: Id) {
        let mut state = self.0.borrow_mut();
        let subscribers = state
            .nodes
            .get(&node)
            .map(|n| n.subscribers.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        for id in subscribers {
            state.enqueue(id);
        }
    }

    fn settle(&self) -> Result<(), ReactiveError> {
        {
            let state = self.0.borrow();
            state.active()?;
            if state.flushing || state.batch_depth != 0 {
                return Ok(());
            }
        }
        self.flush()
    }

    fn flush(&self) -> Result<(), ReactiveError> {
        {
            let mut state = self.0.borrow_mut();
            state.active()?;
            if state.flushing || state.batch_depth != 0 {
                return Err(ReactiveError::ReentrantFlush);
            }
            state.flushing = true;
        }
        let mut runs = 0;
        let outcome = loop {
            let next = {
                let mut state = self.0.borrow_mut();
                let next = state
                    .memos
                    .pop_first()
                    .or_else(|| state.pending.pop_first());
                match next {
                    None => break Ok(()),
                    Some(_) if runs == state.limits.max_runs => {
                        break Err(ReactiveError::RunLimit);
                    }
                    Some(id) => {
                        let effect = &state.effects[&id];
                        let callback = Rc::clone(&effect.callback);
                        state.in_memo = effect.output.is_some();
                        callback
                    }
                }
            };
            runs += 1;
            let result = match catch_unwind(AssertUnwindSafe(|| {
                let mut callback = next.try_borrow_mut().map_err(|_| ReactiveError::Borrowed)?;
                callback()
            })) {
                Ok(result) => result,
                Err(_) => Err(ReactiveError::CallbackPanicked),
            };
            let prior_failure = {
                let mut state = self.0.borrow_mut();
                state.in_memo = false;
                state.failure
            };
            if let Some(error) = prior_failure {
                break Err(error);
            }
            if let Err(error) = result {
                break Err(error);
            }
        };
        {
            let mut state = self.0.borrow_mut();
            state.flushing = false;
            state.in_memo = false;
        }
        if let Err(error) = outcome {
            self.fail(error);
        }
        outcome
    }
}

/// A single-threaded reactive graph with explicit dependencies.
///
/// Scopes own values and callbacks; signal and effect handles are weak.
/// Dependencies are declared at registration. Reads do not subscribe.
/// Callback CPU and memory use are not limited.
///
/// ```
/// use ruvane::{ReactiveError, ReactiveRuntime};
/// let runtime = ReactiveRuntime::new();
/// let scope = runtime.scope()?;
/// let count = scope.signal(1u32)?;
/// let input = count.clone();
/// let doubled = scope.memo(&[count.source()], move || Ok(input.get()? * 2))?;
/// count.set(3)?;
/// assert_eq!(doubled.get()?, 6);
/// drop(scope);
/// assert_eq!(count.get(), Err(ReactiveError::Disposed));
/// # Ok::<(), ReactiveError>(())
/// ```
///
/// Create the runtime in the thread that uses it:
///
/// ```compile_fail
/// use ruvane::ReactiveRuntime;
/// let runtime = ReactiveRuntime::new();
/// std::thread::spawn(move || drop(runtime));
/// ```
pub struct ReactiveRuntime {
    inner: Rc<Inner>,
}

impl Default for ReactiveRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl ReactiveRuntime {
    /// Creates a runtime with default limits.
    pub fn new() -> Self {
        Self::configured(ReactiveLimits::default())
    }

    fn configured(limits: ReactiveLimits) -> Self {
        Self {
            inner: Rc::new(Inner(RefCell::new(State {
                limits,
                next: 0,
                owners: BTreeMap::new(),
                nodes: BTreeMap::new(),
                effects: BTreeMap::new(),
                memos: BTreeSet::new(),
                pending: BTreeSet::new(),
                subscriptions: 0,
                batch_depth: 0,
                flushing: false,
                in_memo: false,
                failure: None,
            }))),
        }
    }

    /// Creates a runtime with validated limits.
    pub fn with_limits(limits: ReactiveLimits) -> Result<Self, ReactiveError> {
        if !(1..=4096).contains(&limits.max_scopes)
            || !(1..=65536).contains(&limits.max_signals)
            || !(1..=65536).contains(&limits.max_effects)
            || !(1..=128).contains(&limits.max_dependencies)
            || !(1..=1048576).contains(&limits.max_subscriptions)
            || !(1..=1048576).contains(&limits.max_runs)
        {
            return Err(ReactiveError::InvalidLimits);
        }
        Ok(Self::configured(limits))
    }

    /// Creates a root owner. Dropping it disposes its descendants and resources.
    pub fn scope(&self) -> Result<Scope, ReactiveError> {
        Scope::create(&self.inner, None)
    }

    /// Defers propagation until the outermost successful batch finishes.
    ///
    /// Signal reads see committed writes. Memo reads see the last settled value,
    /// or Pending for a new memo. An error or unwind permanently faults the
    /// runtime; writes and external callback side effects are not rolled back.
    pub fn batch<T>(
        &self,
        body: impl FnOnce() -> Result<T, ReactiveError>,
    ) -> Result<T, ReactiveError> {
        {
            let mut state = self.inner.0.borrow_mut();
            state.writable()?;
            state.batch_depth = state
                .batch_depth
                .checked_add(1)
                .ok_or(ReactiveError::Limit)?;
        }
        let result =
            catch_unwind(AssertUnwindSafe(body)).unwrap_or(Err(ReactiveError::CallbackPanicked));
        self.inner.0.borrow_mut().batch_depth -= 1;
        match result {
            Ok(value) => {
                self.inner.settle()?;
                Ok(value)
            }
            Err(error) => {
                self.inner.fail(error);
                Err(error)
            }
        }
    }

    /// Runs queued work. Cannot be called inside a batch or callback.
    pub fn flush(&self) -> Result<(), ReactiveError> {
        self.inner.flush()
    }

    /// Returns resource counts and scheduler state.
    pub fn stats(&self) -> ReactiveStats {
        let state = self.inner.0.borrow();
        ReactiveStats {
            scopes: state.owners.len(),
            signals: state.nodes.len(),
            effects: state.effects.len(),
            subscriptions: state.subscriptions,
            queued: state.memos.len() + state.pending.len(),
            batch_depth: state.batch_depth,
            flushing: state.flushing,
            faulted: state.failure.is_some(),
        }
    }

    /// Returns the first failure. A failed runtime only supports inspection and disposal.
    pub fn failure(&self) -> Option<ReactiveError> {
        self.inner.0.borrow().failure
    }
}

/// Owns values, computations and child scopes. Cannot be cloned.
#[must_use = "dropping the scope immediately disposes its reactive resources"]
pub struct Scope {
    inner: Rc<Inner>,
    id: Id,
}

impl Scope {
    fn create(inner: &Rc<Inner>, parent: Option<Id>) -> Result<Self, ReactiveError> {
        let mut state = inner.0.borrow_mut();
        state.writable()?;
        if parent.is_some_and(|id| !state.owners.contains_key(&id)) {
            return Err(ReactiveError::Disposed);
        }
        if state.owners.len() == state.limits.max_scopes {
            return Err(ReactiveError::Limit);
        }
        let id = state.id()?;
        state.owners.insert(
            id,
            Owner {
                parent,
                children: BTreeSet::new(),
                signals: BTreeSet::new(),
                effects: BTreeSet::new(),
            },
        );
        if let Some(parent) = parent {
            state.owners.get_mut(&parent).unwrap().children.insert(id);
        }
        Ok(Self {
            inner: Rc::clone(inner),
            id,
        })
    }

    /// Creates a child scope that shares this scope's lifetime.
    pub fn child(&self) -> Result<Self, ReactiveError> {
        Self::create(&self.inner, Some(self.id))
    }

    /// Disposes the complete subtree, even after a failure; returns false if gone.
    ///
    /// Removing a source cancels dependent computations, including downstream
    /// memo caches owned by other scopes. Currently running code is not interrupted.
    pub fn dispose(&self) -> bool {
        self.inner.remove(Remove::Scope(self.id))
    }

    fn value<T: 'static>(&self, initial: Option<T>) -> Result<Signal<T>, ReactiveError> {
        let mut state = self.inner.0.borrow_mut();
        state.writable()?;
        if !state.owners.contains_key(&self.id) {
            return Err(ReactiveError::Disposed);
        }
        if state.nodes.len() == state.limits.max_signals {
            return Err(ReactiveError::Limit);
        }
        let id = state.id()?;
        let value = Rc::new(Value(RefCell::new(initial)));
        state.nodes.insert(
            id,
            Node {
                owner: self.id,
                value: value.clone(),
                subscribers: BTreeSet::new(),
                producer: None,
            },
        );
        state.owners.get_mut(&self.id).unwrap().signals.insert(id);
        Ok(Signal {
            source: Source {
                inner: Rc::downgrade(&self.inner),
                id,
            },
            value: Rc::downgrade(&value),
        })
    }

    /// Creates an owner-scoped value. Every successful set notifies subscribers.
    pub fn signal<T: 'static>(&self, initial: T) -> Result<Signal<T>, ReactiveError> {
        self.value(Some(initial))
    }

    fn register(
        &self,
        dependencies: &[Source],
        callback: impl FnMut() -> Result<(), ReactiveError> + 'static,
        output: Option<Id>,
    ) -> Result<Effect, ReactiveError> {
        let id;
        {
            let mut state = self.inner.0.borrow_mut();
            state.writable()?;
            if !state.owners.contains_key(&self.id) {
                return Err(ReactiveError::Disposed);
            }
            if dependencies.len() > state.limits.max_dependencies {
                return Err(ReactiveError::Limit);
            }
            let mut inputs = BTreeSet::new();
            for source in dependencies {
                if !source.inner.ptr_eq(&Rc::downgrade(&self.inner)) {
                    return Err(ReactiveError::ForeignRuntime);
                }
                if !state.nodes.contains_key(&source.id) {
                    return Err(ReactiveError::Disposed);
                }
                inputs.insert(source.id);
            }
            if state.effects.len() == state.limits.max_effects
                || state.subscriptions + inputs.len() > state.limits.max_subscriptions
            {
                return Err(ReactiveError::Limit);
            }
            id = state.id()?;
            for source in &inputs {
                state.nodes.get_mut(source).unwrap().subscribers.insert(id);
            }
            state.subscriptions += inputs.len();
            state.effects.insert(
                id,
                Computation {
                    owner: self.id,
                    dependencies: inputs,
                    callback: Rc::new(RefCell::new(Box::new(callback))),
                    output,
                },
            );
            state.owners.get_mut(&self.id).unwrap().effects.insert(id);
            if let Some(output) = output {
                state.nodes.get_mut(&output).unwrap().producer = Some(id);
            }
            state.enqueue(id);
        }
        if let Err(error) = self.inner.settle() {
            self.inner.remove(Remove::Effect(id));
            return Err(error);
        }
        Ok(Effect {
            inner: Rc::downgrade(&self.inner),
            id,
        })
    }

    /// Registers an effect with explicit, fixed dependencies and an initial run.
    ///
    /// Duplicate sources are deduplicated after checking the input-entry budget.
    /// During a batch or callback the initial run is deferred. Dropping the
    /// returned handle does not unsubscribe; its owner controls the lifetime.
    pub fn effect(
        &self,
        dependencies: &[Source],
        callback: impl FnMut() -> Result<(), ReactiveError> + 'static,
    ) -> Result<Effect, ReactiveError> {
        self.register(dependencies, callback, None)
    }

    /// Creates a read-only derived value from explicit, fixed dependencies.
    ///
    /// Memos settle before effects and only notify on PartialEq changes. A memo
    /// callback must not mutate the graph. New memos inside a batch or callback
    /// return Pending on reads until propagation reaches their initial run.
    pub fn memo<T: Clone + PartialEq + 'static>(
        &self,
        dependencies: &[Source],
        mut compute: impl FnMut() -> Result<T, ReactiveError> + 'static,
    ) -> Result<Memo<T>, ReactiveError> {
        let output = self.value(None)?;
        let target = output.clone();
        let result = self.register(
            dependencies,
            move || {
                let next = compute()?;
                let Some(inner) = target.source.inner.upgrade() else {
                    return Ok(());
                };
                if !inner.0.borrow().nodes.contains_key(&target.source.id) {
                    return Ok(());
                }
                let value = target.value.upgrade().ok_or(ReactiveError::Disposed)?;
                let same = value
                    .0
                    .try_borrow()
                    .map_err(|_| ReactiveError::Borrowed)?
                    .as_ref()
                    .is_some_and(|old| *old == next);
                if !same {
                    let old = value
                        .0
                        .try_borrow_mut()
                        .map_err(|_| ReactiveError::Borrowed)?
                        .replace(next);
                    inner.notify(target.source.id);
                    drop(old);
                }
                Ok(())
            },
            Some(output.source.id),
        );
        if let Err(error) = result {
            self.inner.remove(Remove::Signal(output.source.id));
            return Err(error);
        }
        Ok(Memo { signal: output })
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        self.dispose();
    }
}

/// A dependency ID that does not keep its value alive.
#[derive(Clone)]
pub struct Source {
    inner: Weak<Inner>,
    id: Id,
}

impl Source {
    fn checked(&self, write: bool) -> Result<Rc<Inner>, ReactiveError> {
        let inner = self.inner.upgrade().ok_or(ReactiveError::Disposed)?;
        {
            let state = inner.0.borrow();
            if write {
                state.writable()?;
            } else {
                state.active()?;
            }
            if !state.nodes.contains_key(&self.id) {
                return Err(ReactiveError::Disposed);
            }
        }
        Ok(inner)
    }
}

/// A weak, cloneable handle to an owner-scoped reactive value.
pub struct Signal<T> {
    source: Source,
    value: Weak<Value<T>>,
}

impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
            value: self.value.clone(),
        }
    }
}

impl<T> Signal<T> {
    /// Returns the dependency ID. Reading a value does not subscribe.
    pub fn source(&self) -> Source {
        self.source.clone()
    }

    /// Writes and schedules subscribers, even when the new value compares equal.
    ///
    /// Reentrant writes from effects are queued. A propagation error does not
    /// roll back the committed value or any external callback side effects.
    pub fn set(&self, next: T) -> Result<(), ReactiveError> {
        let inner = self.source.checked(true)?;
        let value = self.value.upgrade().ok_or(ReactiveError::Disposed)?;
        let previous = value
            .0
            .try_borrow_mut()
            .map_err(|_| ReactiveError::Borrowed)?
            .replace(next);
        inner.notify(self.source.id);
        drop(previous);
        inner.settle()
    }

    /// Disposes this source and its dependent computations; idempotent.
    pub fn dispose(&self) -> bool {
        self.source
            .inner
            .upgrade()
            .is_some_and(|inner| inner.remove(Remove::Signal(self.source.id)))
    }
}

impl<T: Clone> Signal<T> {
    /// Clones the current value without subscribing.
    pub fn get(&self) -> Result<T, ReactiveError> {
        self.source.checked(false)?;
        let value = self.value.upgrade().ok_or(ReactiveError::Disposed)?;
        let slot = value.0.try_borrow().map_err(|_| ReactiveError::Borrowed)?;
        slot.as_ref().cloned().ok_or(ReactiveError::Pending)
    }
}

/// A read-only memo cache; its computation and value belong to its scope.
pub struct Memo<T> {
    signal: Signal<T>,
}

impl<T> Clone for Memo<T> {
    fn clone(&self) -> Self {
        Self {
            signal: self.signal.clone(),
        }
    }
}

impl<T> Memo<T> {
    /// Returns the memo's output as an explicit dependency token.
    pub fn source(&self) -> Source {
        self.signal.source()
    }

    /// Disposes the memo and downstream computations; idempotent.
    pub fn dispose(&self) -> bool {
        self.signal.dispose()
    }
}

impl<T: Clone> Memo<T> {
    /// Reads the last settled value, or Pending before its first computation.
    pub fn get(&self) -> Result<T, ReactiveError> {
        self.signal.get()
    }
}

/// A weak effect handle. The scope owns the computation.
#[derive(Clone)]
pub struct Effect {
    inner: Weak<Inner>,
    id: Id,
}

impl Effect {
    /// Cancels pending and future executions, including during another callback.
    /// Currently executing callback code is not interrupted.
    pub fn dispose(&self) -> bool {
        self.inner
            .upgrade()
            .is_some_and(|inner| inner.remove(Remove::Effect(self.id)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exhausted_identifiers_never_wrap_or_alias_existing_resources() {
        let runtime = ReactiveRuntime::new();
        let scope = runtime.scope().unwrap();
        let value = scope.signal(7).unwrap();
        let before = runtime.stats();
        runtime.inner.0.borrow_mut().next = u64::MAX;
        assert!(matches!(runtime.scope(), Err(ReactiveError::Limit)));
        assert!(matches!(scope.signal(9), Err(ReactiveError::Limit)));
        assert!(matches!(
            scope.effect(&[], || Ok(())),
            Err(ReactiveError::Limit)
        ));
        assert_eq!(runtime.stats(), before);
        assert_eq!(value.get(), Ok(7));
        scope.dispose();
        assert_eq!(runtime.stats().scopes, 0);
    }
}
