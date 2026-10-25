# Reactivity

Ruvane alpha.11 introduces a single-threaded reactive engine using only the Rust
standard library. It is independent of HTML rendering and transport. No global
runtime, thread-local dependency tracker, browser binding, script injection or
hydration is introduced in this version.

## Counter and derived state

```rust
use ruvane::{ReactiveError, ReactiveRuntime};
use std::cell::RefCell;
use std::rc::Rc;

let runtime = ReactiveRuntime::new();
let owner = runtime.scope()?;
let count = owner.signal(0u32)?;
let input = count.clone();
let doubled = owner.memo(&[count.source()], move || Ok(input.get()? * 2))?;

let observed = Rc::new(RefCell::new(Vec::new()));
let output = observed.clone();
let (input, derived) = (count.clone(), doubled.clone());
owner.effect(&[count.source(), doubled.source()], move || {
    output.borrow_mut().push((input.get()?, derived.get()?));
    Ok(())
})?;

count.set(1)?;
runtime.batch(|| {
    count.set(2)?;
    count.set(3)
})?;
assert_eq!(*observed.borrow(), [(0, 0), (1, 2), (3, 6)]);

drop(owner);
assert_eq!(count.get(), Err(ReactiveError::Disposed));
assert_eq!(runtime.stats().effects, 0);
# Ok::<(), ReactiveError>(())
```

The runnable version is `cargo run --example reactivity --offline --locked`.
The example uses a bounded number of values and snapshots. A real application
must bound any collections it maintains itself.

## Values and explicit dependencies

`Scope::signal(value)` creates a writable value. `Signal::get` clones it;
`Signal::set` commits a replacement and notifies declared subscribers. Every
successful set notifies, including a replacement equal to the old value.
`Signal` cloning only clones a weak handle and does not require `T: Clone`.
Reading requires `T: Clone`; values are owned and `'static`.

`Signal::source` and `Memo::source` return non-generic dependency tokens. Supply
all reactive inputs to `Scope::effect` or `Scope::memo`. Reading a value in a
callback does not register it automatically. Dependency lists are fixed after
registration. Duplicate entries are deduplicated, but the full input slice must
fit the per-computation entry budget. Mixed-runtime and disposed inputs fail
before running the callback or installing edges.

An effect takes `FnMut() -> Result<(), ReactiveError>`. It runs initially, then
on declared changes. Its scope retains it: discarding the returned `Effect`
handle does not unsubscribe it. Use `Effect::dispose` or dispose its scope.

A memo takes `FnMut() -> Result<T, ReactiveError>`, where `T: Clone + PartialEq`.
It exposes reads and a source, without a setter. Its cached output notifies only
when PartialEq reports a change. The computation must be pure: signal writes,
registration and batching during memo evaluation return `WriteDuringMemo`.
Scope/source disposal remains possible for cleanup; a computation disposed
during its own callback does not publish a new output.

## Scheduling and batches

An idle write settles synchronously before returning. A pending computation
appears only once in its queue. Memos have priority over user effects; each
queue uses monotonically assigned registration order. Memo dependency tokens
refer to already registered sources, so explicit derived graphs settle before
their observers. Undeclared reads do not receive this dependency guarantee.

Writes from effects enqueue work in the same flush instead of recursively
running callbacks. A callback may start a nested batch. An explicit
`ReactiveRuntime::flush` during a callback or batch returns `ReentrantFlush`.

`ReactiveRuntime::batch` defers propagation until the outermost successful batch
finishes. During a batch, signal reads see committed values. Memo reads return
the last settled cache; a new memo returns `Pending` until its first execution.
Initial effects are also deferred when registered inside a batch or callback.

Batching coalesces scheduling, not application writes into a transaction.
Values and external side effects are not rolled back if subsequent work fails.
An effect that writes its own input may run repeatedly; the execution budget
stops cycles even when a setter repeats an equal value.

## Ownership and cleanup

`ReactiveRuntime::scope` creates a root. `Scope::child` creates a descendant.
Scopes are not cloneable, and a scope keeps its runtime metadata alive. Dropping
or explicitly disposing a scope removes its subtree, owned values, callbacks,
dependency edges and queued computations. Signal, memo, source and effect
handles use weak references and do not keep application values alive.

`Signal::dispose` removes an individual source. `Memo::dispose` also cancels its
producer. Removing a source cancels its dependent computations; if a dependent
is a memo, its output and downstream dependents are removed too, even in other
scopes. Unrelated resources and owners remain live. Disposal is idempotent.
A currently executing callback continues, but cannot schedule a disposed source.

Identifiers never wrap or reuse old slots. A stale handle cannot address a new
value after an owner budget is freed. Disposal traverses metadata iteratively.
Captured application values and callbacks are dropped outside metadata borrows,
so their destructors may reenter valid resources without borrowing scheduler
metadata already mutably. A value being cloned can finish its in-progress
snapshot after disposal; subsequent reads fail. Reentrant writes while its
value is borrowed return `Borrowed`.

Application code can still build its own strong Rc cycles, for example by
capturing `Rc<Scope>` in a callback owned by that scope. Prefer weak application
captures or dispose the owner explicitly. The engine cannot collect arbitrary
application reference cycles. Its own captured signal handles are weak.

Create a runtime for a view or request, without a process-global mutable graph.
The runtime and its handles are intentionally not Send or Sync. Server handlers
can create independent runtimes in their own threads; sharing them across
workers is not supported. For example, this must not compile:

```compile_fail
use ruvane::ReactiveRuntime;
let runtime = ReactiveRuntime::new();
std::thread::spawn(move || drop(runtime));
```

## Budgets and errors

`ReactiveRuntime::with_limits` validates these supported ranges before creating
the runtime. `ReactiveRuntime::new` uses their defaults.

| Setting | Default | Range |
| --- | --- | --- |
| `max_scopes` | 256 | 1–4096 |
| `max_signals` including memo caches | 1024 | 1–65536 |
| `max_effects` including memo callbacks | 1024 | 1–65536 |
| `max_dependencies` input entries | 32 | 1–128 |
| `max_subscriptions` deduplicated edges | 4096 | 1–1048576 |
| `max_runs` callback executions per flush | 1024 | 1–1048576 |

The distinct pending queues cannot exceed live computations. Invalid resource
registrations do not retain new resources, edges or callbacks. Capacity is
reusable after disposal; the checked identifier counter itself is never reused.
Stats report live counts, queues, batch depth, flushing and fault status, without
exposing values. Error messages likewise do not reflect application input.

A callback error, unwinding panic or run-budget exhaustion faults the runtime
permanently and clears both pending queues. `failure` returns the first reason.
Later reads, writes, batches, registrations and flushes return `Faulted`.
Stats and disposal remain usable. Create a new runtime after fixing the cause;
there is no reset that silently reuses potentially inconsistent cached state.

The run budget counts callbacks, not CPU instructions. Callback code and custom
Clone, PartialEq and Drop implementations remain trusted Rust code. Their
allocations, recursion, I/O, execution time and side effects are not sandboxed.
Live-node counts do not bound the byte size of an arbitrary stored T. Rust
integer arithmetic and application collection growth need their own bounds.

Unwind recovery does not suppress the normal panic hook. Applications must keep
secrets out of their own panic/log messages. `panic=abort` terminates the process
and cannot be recovered by this engine.

## Verification and next boundary

Regression tests cover ordering, diamonds, equality filtering, nested batches,
deferred initialization, reentrant writes and destruction, cycle budgets,
foreign/disposed sources, registration rollback, panic cleanup, monotonic IDs
and 1,000 owner mount/unmount cycles with stale handles and captured values.

The reactive engine itself does not attach DOM events or hydrate server HTML.
Alpha.12 adds the separate [browser adapter](browser.md), an explicit Wasm ABI
and opt-in interactive responses. Alpha.13 adds shared `View` trees and hydration;
application components own their reactive scopes and supply their view and events.
Alpha.14 adds keyed structural updates and a list whose item scopes drop on
logical removal. Filtering removes DOM bindings but keeps hidden item model state.
Default server responses and static exports retain the script-blocking CSP.
