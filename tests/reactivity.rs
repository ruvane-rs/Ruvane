use ruvane::{ReactiveError as Error, ReactiveLimits, ReactiveRuntime};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[test]
fn signals_notify_only_declared_dependencies_in_registration_order() {
    let runtime = ReactiveRuntime::new();
    let scope = runtime.scope().unwrap();
    let a = scope.signal(1).unwrap();
    let other = scope.signal(9).unwrap();
    let seen = Rc::new(RefCell::new(Vec::new()));
    for index in 0..3 {
        let input = a.clone();
        let untracked = other.clone();
        let output = seen.clone();
        scope
            .effect(&[a.source()], move || {
                let _ = untracked.get()?;
                output.borrow_mut().push((index, input.get()?));
                Ok(())
            })
            .unwrap();
    }
    seen.borrow_mut().clear();
    other.set(10).unwrap();
    assert!(seen.borrow().is_empty());
    a.set(2).unwrap();
    assert_eq!(*seen.borrow(), [(0, 2), (1, 2), (2, 2)]);
    a.set(2).unwrap();
    assert_eq!(seen.borrow().len(), 6);
    assert_eq!(runtime.stats().queued, 0);
}

#[test]
fn diamond_memos_settle_before_consumers_and_suppress_equal_outputs() {
    let runtime = ReactiveRuntime::new();
    let scope = runtime.scope().unwrap();
    let n = scope.signal(1).unwrap();
    let input = n.clone();
    let left = scope
        .memo(&[n.source()], move || Ok(input.get()? * 2))
        .unwrap();
    let input = n.clone();
    let right = scope
        .memo(&[n.source()], move || Ok(input.get()? + 10))
        .unwrap();
    let (a, b) = (left.clone(), right.clone());
    let sum = scope
        .memo(&[left.source(), right.source()], move || {
            Ok(a.get()? + b.get()?)
        })
        .unwrap();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let output = seen.clone();
    let (input, derived) = (n.clone(), sum.clone());
    scope
        .effect(&[n.source(), sum.source()], move || {
            output.borrow_mut().push((input.get()?, derived.get()?));
            Ok(())
        })
        .unwrap();
    n.set(2).unwrap();
    n.set(2).unwrap();
    assert_eq!(*seen.borrow(), [(1, 13), (2, 16), (2, 16)]);

    let calls = Rc::new(Cell::new(0));
    let output = calls.clone();
    scope
        .effect(&[sum.source()], move || {
            output.set(output.get() + 1);
            Ok(())
        })
        .unwrap();
    n.set(2).unwrap();
    assert_eq!(calls.get(), 1);
    n.set(3).unwrap();
    assert_eq!(calls.get(), 2);
    assert_eq!(sum.get(), Ok(19));
}

#[test]
fn nested_batches_deduplicate_runs_and_memo_reads_are_last_settled() {
    let runtime = ReactiveRuntime::new();
    let scope = runtime.scope().unwrap();
    let n = scope.signal(1).unwrap();
    let input = n.clone();
    let doubled = scope
        .memo(&[n.source()], move || Ok(input.get()? * 2))
        .unwrap();
    let calls = Rc::new(Cell::new(0));
    let output = calls.clone();
    scope
        .effect(&[n.source(), doubled.source()], move || {
            output.set(output.get() + 1);
            Ok(())
        })
        .unwrap();
    runtime
        .batch(|| {
            n.set(2)?;
            runtime.batch(|| {
                n.set(3)?;
                Ok(())
            })?;
            assert_eq!(n.get(), Ok(3));
            assert_eq!(doubled.get(), Ok(2));
            assert_eq!(calls.get(), 1);
            assert_eq!(runtime.flush(), Err(Error::ReentrantFlush));
            Ok(())
        })
        .unwrap();
    assert_eq!(doubled.get(), Ok(6));
    assert_eq!(calls.get(), 2);
    assert_eq!(runtime.stats().batch_depth, 0);
}

#[test]
fn new_computations_in_a_batch_wait_for_the_outer_flush() {
    let runtime = ReactiveRuntime::new();
    let scope = runtime.scope().unwrap();
    let n = scope.signal(2).unwrap();
    let calls = Rc::new(Cell::new(0));
    let memo = runtime
        .batch(|| {
            let input = n.clone();
            let memo = scope.memo(&[n.source()], move || Ok(input.get()? * 3))?;
            let output = calls.clone();
            scope.effect(&[memo.source()], move || {
                output.set(output.get() + 1);
                Ok(())
            })?;
            assert_eq!(memo.get(), Err(Error::Pending));
            assert_eq!(calls.get(), 0);
            Ok(memo)
        })
        .unwrap();
    assert_eq!(memo.get(), Ok(6));
    assert_eq!(calls.get(), 1);
}

#[test]
fn reentrant_effect_writes_are_queued_without_borrow_panics() {
    let runtime = ReactiveRuntime::new();
    let scope = runtime.scope().unwrap();
    let n = scope.signal(0).unwrap();
    let input = n.clone();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let output = seen.clone();
    scope
        .effect(&[n.source()], move || {
            let current = input.get()?;
            output.borrow_mut().push(current);
            if current < 4 {
                input.set(current + 1)?;
            }
            Ok(())
        })
        .unwrap();
    assert_eq!(n.get(), Ok(4));
    assert_eq!(*seen.borrow(), [0, 1, 2, 3, 4]);
    assert!(!runtime.stats().flushing);
}

#[test]
fn cycles_stop_at_the_budget_and_permanently_fault_the_runtime() {
    let runtime = ReactiveRuntime::with_limits(ReactiveLimits {
        max_runs: 7,
        ..ReactiveLimits::default()
    })
    .unwrap();
    let scope = runtime.scope().unwrap();
    let n = scope.signal(0).unwrap();
    let input = n.clone();
    let calls = Rc::new(Cell::new(0));
    let output = calls.clone();
    let result = scope.effect(&[n.source()], move || {
        output.set(output.get() + 1);
        input.set(input.get()? + 1)
    });
    assert!(matches!(result, Err(Error::RunLimit)));
    assert_eq!(calls.get(), 7);
    assert_eq!(runtime.failure(), Some(Error::RunLimit));
    assert_eq!(runtime.flush(), Err(Error::Faulted));
    assert_eq!(n.set(99), Err(Error::Faulted));
    assert_eq!(n.get(), Err(Error::Faulted));
    assert_eq!(runtime.stats().queued, 0);
    assert_eq!(runtime.stats().effects, 0);
    assert!(!runtime.stats().flushing);
    assert!(scope.dispose());
    assert_eq!(runtime.stats().signals, 0);
}

#[test]
fn memo_callbacks_cannot_write_register_or_start_batches() {
    let runtime = Rc::new(ReactiveRuntime::new());
    let scope = Rc::new(runtime.scope().unwrap());
    let n = scope.signal(1).unwrap();
    let (input, owner, engine) = (n.clone(), scope.clone(), runtime.clone());
    let memo = scope
        .memo(&[n.source()], move || {
            assert_eq!(input.set(3), Err(Error::WriteDuringMemo));
            assert!(matches!(owner.signal(4), Err(Error::WriteDuringMemo)));
            assert!(matches!(engine.scope(), Err(Error::WriteDuringMemo)));
            assert_eq!(engine.batch(|| Ok(())), Err(Error::WriteDuringMemo));
            input.get()
        })
        .unwrap();
    assert_eq!(memo.get(), Ok(1));
    assert_eq!(n.get(), Ok(1));
    // Disposal breaks the callback's strong reference to its owner.
    scope.dispose();
}

#[test]
fn disposal_cancels_queued_work_and_cross_scope_derived_dependents() {
    let runtime = ReactiveRuntime::new();
    let root = runtime.scope().unwrap();
    let child = root.child().unwrap();
    let source = child.signal(1).unwrap();
    let input = source.clone();
    let memo = root
        .memo(&[source.source()], move || Ok(input.get()? * 2))
        .unwrap();
    let runs = Rc::new(Cell::new(0));
    let output = runs.clone();
    root.effect(&[memo.source()], move || {
        output.set(output.get() + 1);
        Ok(())
    })
    .unwrap();
    runtime
        .batch(|| {
            source.set(2)?;
            child.dispose();
            Ok(())
        })
        .unwrap();
    assert_eq!(runs.get(), 1);
    assert_eq!(source.get(), Err(Error::Disposed));
    assert_eq!(memo.get(), Err(Error::Disposed));
    assert_eq!(runtime.stats().effects, 0);
    assert_eq!(runtime.stats().signals, 0);
    assert_eq!(runtime.stats().subscriptions, 0);
    assert!(!child.dispose());
    assert!(matches!(child.signal(3), Err(Error::Disposed)));
}

#[test]
fn explicit_effect_unsubscribe_removes_edges_and_queued_callbacks() {
    let runtime = ReactiveRuntime::new();
    let scope = runtime.scope().unwrap();
    let n = scope.signal(0).unwrap();
    let calls = Rc::new(Cell::new(0));
    let output = calls.clone();
    let effect = scope
        .effect(&[n.source(), n.source()], move || {
            output.set(output.get() + 1);
            Ok(())
        })
        .unwrap();
    assert_eq!(runtime.stats().subscriptions, 1);
    runtime
        .batch(|| {
            n.set(2)?;
            assert!(effect.dispose());
            assert!(!effect.dispose());
            Ok(())
        })
        .unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(runtime.stats().subscriptions, 0);
    assert_eq!(runtime.stats().queued, 0);
}

#[test]
fn child_scope_drop_clears_values_and_closures_even_with_stale_handles() {
    let runtime = ReactiveRuntime::new();
    let root = runtime.scope().unwrap();
    let baseline = runtime.stats();
    let payload = Rc::new(());
    let captured = Rc::new(());
    let stale;
    for _ in 0..1000 {
        let child = root.child().unwrap();
        let value = child.signal(payload.clone()).unwrap();
        let capture = captured.clone();
        child
            .effect(&[value.source()], move || {
                assert!(Rc::strong_count(&capture) >= 2);
                Ok(())
            })
            .unwrap();
        assert_eq!(Rc::strong_count(&payload), 2);
        drop(child);
        assert_eq!(value.get(), Err(Error::Disposed));
        assert_eq!(runtime.stats(), baseline);
        assert_eq!(Rc::strong_count(&payload), 1);
        assert_eq!(Rc::strong_count(&captured), 1);
    }
    {
        let child = root.child().unwrap();
        stale = child.signal(1).unwrap();
        root.dispose();
        assert!(matches!(child.child(), Err(Error::Disposed)));
    }
    assert_eq!(stale.set(2), Err(Error::Disposed));
    assert_eq!(runtime.stats().scopes, 0);
}

#[test]
fn failed_registrations_are_atomic_and_foreign_sources_are_rejected() {
    let runtime = ReactiveRuntime::with_limits(ReactiveLimits {
        max_effects: 1,
        max_subscriptions: 1,
        max_dependencies: 2,
        ..ReactiveLimits::default()
    })
    .unwrap();
    let scope = runtime.scope().unwrap();
    let n = scope.signal(0).unwrap();
    let foreign = ReactiveRuntime::new();
    let foreign_scope = foreign.scope().unwrap();
    let other = foreign_scope.signal(0).unwrap();
    let before = runtime.stats();
    assert!(matches!(
        scope.effect(&[other.source()], || panic!(
            "invalid callback must not run"
        )),
        Err(Error::ForeignRuntime)
    ));
    assert_eq!(runtime.stats(), before);
    assert!(matches!(
        scope.memo(&[other.source()], || Ok(7)),
        Err(Error::ForeignRuntime)
    ));
    assert_eq!(runtime.stats(), before);
    let effect = scope.effect(&[n.source()], || Ok(())).unwrap();
    let before = runtime.stats();
    assert!(matches!(scope.effect(&[], || Ok(())), Err(Error::Limit)));
    assert!(matches!(scope.memo(&[], || Ok(7)), Err(Error::Limit)));
    assert_eq!(runtime.stats(), before);
    effect.dispose();
    n.dispose();
    let before = runtime.stats();
    assert!(matches!(
        scope.effect(&[n.source()], || Ok(())),
        Err(Error::Disposed)
    ));
    assert_eq!(runtime.stats(), before);
    assert_eq!(runtime.failure(), None);
}

#[test]
fn scope_signal_dependency_and_configuration_limits_are_enforced() {
    assert!(matches!(
        ReactiveRuntime::with_limits(ReactiveLimits {
            max_runs: 0,
            ..ReactiveLimits::default()
        }),
        Err(Error::InvalidLimits)
    ));
    assert!(matches!(
        ReactiveRuntime::with_limits(ReactiveLimits {
            max_signals: usize::MAX,
            ..ReactiveLimits::default()
        }),
        Err(Error::InvalidLimits)
    ));
    let runtime = ReactiveRuntime::with_limits(ReactiveLimits {
        max_scopes: 1,
        max_signals: 2,
        max_dependencies: 1,
        max_subscriptions: 1,
        ..ReactiveLimits::default()
    })
    .unwrap();
    let scope = runtime.scope().unwrap();
    assert!(matches!(scope.child(), Err(Error::Limit)));
    let a = scope.signal(0).unwrap();
    let b = scope.signal(1).unwrap();
    assert!(matches!(scope.signal(2), Err(Error::Limit)));
    assert!(matches!(
        scope.effect(&[a.source(), b.source()], || Ok(())),
        Err(Error::Limit)
    ));
    let effect = scope.effect(&[a.source()], || Ok(())).unwrap();
    assert!(matches!(
        scope.effect(&[b.source()], || Ok(())),
        Err(Error::Limit)
    ));
    effect.dispose();
    scope.effect(&[b.source()], || Ok(())).unwrap();
}

#[test]
fn callback_panics_and_batch_errors_clear_scheduler_state_without_rollback() {
    let runtime = ReactiveRuntime::new();
    let scope = runtime.scope().unwrap();
    let n = scope.signal(0).unwrap();
    let input = n.clone();
    scope
        .effect(&[n.source()], move || {
            if input.get()? == 1 {
                panic!("application callback");
            }
            Ok(())
        })
        .unwrap();
    assert_eq!(n.set(1), Err(Error::CallbackPanicked));
    assert_eq!(runtime.failure(), Some(Error::CallbackPanicked));
    assert!(!runtime.stats().flushing);
    scope.dispose();
    assert_eq!(runtime.stats().effects, 0);

    let runtime = ReactiveRuntime::new();
    let scope = runtime.scope().unwrap();
    let n = scope.signal(0).unwrap();
    let externally_visible = Cell::new(0);
    let result: Result<(), Error> = runtime.batch(|| {
        n.set(2)?;
        externally_visible.set(2);
        runtime.batch(|| Err(Error::Disposed))
    });
    assert_eq!(result, Err(Error::Disposed));
    assert_eq!(externally_visible.get(), 2);
    assert_eq!(runtime.failure(), Some(Error::Disposed));
    assert_eq!(runtime.stats().batch_depth, 0);
    assert_eq!(runtime.stats().queued, 0);

    let runtime = ReactiveRuntime::new();
    let result: Result<(), Error> = runtime.batch(|| panic!("batch callback"));
    assert_eq!(result, Err(Error::CallbackPanicked));
    assert_eq!(runtime.stats().batch_depth, 0);
}

#[test]
fn runtimes_are_independent_and_handles_do_not_keep_values_alive() {
    let a = ReactiveRuntime::new();
    let b = ReactiveRuntime::new();
    let (owner_a, owner_b) = (a.scope().unwrap(), b.scope().unwrap());
    let (signal_a, signal_b) = (owner_a.signal(1).unwrap(), owner_b.signal(9).unwrap());
    signal_a.set(2).unwrap();
    assert_eq!(signal_b.get(), Ok(9));
    drop(owner_a);
    drop(a);
    assert_eq!(signal_a.get(), Err(Error::Disposed));
    assert_eq!(signal_b.get(), Ok(9));
}

#[test]
fn disposing_a_pending_effect_from_another_callback_prevents_its_execution() {
    let runtime = ReactiveRuntime::new();
    let scope = runtime.scope().unwrap();
    let n = scope.signal(0).unwrap();
    let target = Rc::new(RefCell::new(None::<ruvane::Effect>));
    let handle = target.clone();
    let input = n.clone();
    scope
        .effect(&[n.source()], move || {
            if input.get()? > 0 {
                handle.borrow().as_ref().unwrap().dispose();
            }
            Ok(())
        })
        .unwrap();
    let calls = Rc::new(Cell::new(0));
    let output = calls.clone();
    *target.borrow_mut() = Some(
        scope
            .effect(&[n.source()], move || {
                output.set(output.get() + 1);
                Ok(())
            })
            .unwrap(),
    );
    n.set(1).unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(runtime.stats().effects, 1);
}

#[test]
fn batch_inside_effect_queues_work_and_explicit_flush_rejects_reentry() {
    let runtime = Rc::new(ReactiveRuntime::new());
    let scope = runtime.scope().unwrap();
    let a = scope.signal(0).unwrap();
    let b = scope.signal(0).unwrap();
    let (engine, input, output) = (runtime.clone(), a.clone(), b.clone());
    scope
        .effect(&[a.source()], move || {
            assert_eq!(engine.flush(), Err(Error::ReentrantFlush));
            engine.batch(|| {
                output.set(input.get()? + 1)?;
                output.set(input.get()? + 2)
            })
        })
        .unwrap();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let (input, log) = (b.clone(), seen.clone());
    scope
        .effect(&[b.source()], move || {
            log.borrow_mut().push(input.get()?);
            Ok(())
        })
        .unwrap();
    a.set(3).unwrap();
    assert_eq!(*seen.borrow(), [2, 5]);
    assert_eq!(runtime.stats().batch_depth, 0);
}

#[test]
fn values_and_callback_captures_are_destroyed_outside_metadata_borrows() {
    struct OnDrop(ruvane::Signal<u32>);
    impl Drop for OnDrop {
        fn drop(&mut self) {
            self.0.set(self.0.get().unwrap() + 1).unwrap();
        }
    }
    let runtime = ReactiveRuntime::new();
    let root = runtime.scope().unwrap();
    let count = root.signal(0).unwrap();
    let child = root.child().unwrap();
    child.signal(OnDrop(count.clone())).unwrap();
    let guard = OnDrop(count.clone());
    child
        .effect(&[], move || {
            std::hint::black_box(&guard);
            Ok(())
        })
        .unwrap();
    child.dispose();
    assert_eq!(count.get(), Ok(2));
    assert_eq!(runtime.failure(), None);
}

#[test]
fn custom_clone_reentrant_write_returns_an_error_instead_of_borrow_panic() {
    struct Value {
        handle: Rc<RefCell<Option<ruvane::Signal<Value>>>>,
        error: Rc<Cell<Option<Error>>>,
    }
    impl Clone for Value {
        fn clone(&self) -> Self {
            self.error.set(
                self.handle
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .set(Self {
                        handle: self.handle.clone(),
                        error: self.error.clone(),
                    })
                    .err(),
            );
            Self {
                handle: self.handle.clone(),
                error: self.error.clone(),
            }
        }
    }
    let runtime = ReactiveRuntime::new();
    let scope = runtime.scope().unwrap();
    let handle = Rc::new(RefCell::new(None));
    let error = Rc::new(Cell::new(None));
    let value = scope
        .signal(Value {
            handle: handle.clone(),
            error: error.clone(),
        })
        .unwrap();
    *handle.borrow_mut() = Some(value.clone());
    assert!(value.get().is_ok());
    assert_eq!(error.get(), Some(Error::Borrowed));
    assert_eq!(runtime.failure(), None);
}

#[test]
fn memo_panics_release_the_failed_registration_and_captured_values() {
    let runtime = ReactiveRuntime::new();
    let scope = runtime.scope().unwrap();
    let capture = Rc::new(());
    let guard = capture.clone();
    let result = scope.memo::<u32>(&[], move || {
        std::hint::black_box(&guard);
        panic!("private application computation");
    });
    assert!(matches!(result, Err(Error::CallbackPanicked)));
    assert_eq!(Rc::strong_count(&capture), 1);
    assert_eq!(runtime.stats().signals, 0);
    assert_eq!(runtime.stats().effects, 0);
    assert_eq!(runtime.stats().subscriptions, 0);
    assert_eq!(runtime.stats().queued, 0);
    assert!(!runtime.stats().flushing);
}

#[test]
fn stale_handles_cannot_alias_new_values_when_owner_budgets_are_reused() {
    let runtime = ReactiveRuntime::with_limits(ReactiveLimits {
        max_scopes: 1,
        max_signals: 1,
        ..ReactiveLimits::default()
    })
    .unwrap();
    let mut stale = Vec::new();
    for n in 0..1000 {
        let scope = runtime.scope().unwrap();
        stale.push(scope.signal(n).unwrap());
    }
    let scope = runtime.scope().unwrap();
    let current = scope.signal(42).unwrap();
    for handle in stale {
        assert_eq!(handle.get(), Err(Error::Disposed));
        assert_eq!(handle.set(100), Err(Error::Disposed));
        assert!(!handle.dispose());
    }
    assert_eq!(current.get(), Ok(42));
    assert_eq!(runtime.stats().signals, 1);
}

#[test]
fn disposal_during_a_custom_clone_finishes_only_the_in_progress_snapshot() {
    struct Value(std::rc::Weak<ruvane::Scope>);
    impl Clone for Value {
        fn clone(&self) -> Self {
            self.0.upgrade().unwrap().dispose();
            Self(self.0.clone())
        }
    }
    let runtime = ReactiveRuntime::new();
    let scope = Rc::new(runtime.scope().unwrap());
    let value = scope.signal(Value(Rc::downgrade(&scope))).unwrap();
    assert!(value.get().is_ok());
    assert!(matches!(value.get(), Err(Error::Disposed)));
    assert_eq!(runtime.stats().signals, 0);
    assert_eq!(runtime.stats().scopes, 0);
    assert_eq!(runtime.failure(), None);
}

#[test]
fn separate_workers_create_isolated_graphs_instead_of_sharing_request_state() {
    let workers = (0..4)
        .map(|n| {
            std::thread::spawn(move || {
                let runtime = ReactiveRuntime::new();
                let scope = runtime.scope().unwrap();
                let value = scope.signal(n).unwrap();
                let input = value.clone();
                let memo = scope
                    .memo(&[value.source()], move || Ok(input.get()? + 100))
                    .unwrap();
                value.set(n * 2).unwrap();
                let result = memo.get().unwrap();
                scope.dispose();
                assert_eq!(runtime.stats().signals, 0);
                result
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        workers
            .into_iter()
            .map(|t| t.join().unwrap())
            .collect::<Vec<_>>(),
        [100, 102, 104, 106]
    );
}
