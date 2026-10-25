//! Run with `cargo run --example reactivity --offline --locked`.
use ruvane::{ReactiveError, ReactiveRuntime};
use std::cell::RefCell;
use std::rc::Rc;

fn main() -> Result<(), ReactiveError> {
    let runtime = ReactiveRuntime::new();
    let owner = runtime.scope()?;
    let count = owner.signal(0u32)?;
    let input = count.clone();
    let doubled = owner.memo(&[count.source()], move || Ok(input.get()? * 2))?;
    let snapshots = Rc::new(RefCell::new(Vec::new()));
    let log = snapshots.clone();
    let (input, derived) = (count.clone(), doubled.clone());
    owner.effect(&[count.source(), doubled.source()], move || {
        let snapshot = (input.get()?, derived.get()?);
        println!("count={}, doubled={}", snapshot.0, snapshot.1);
        log.borrow_mut().push(snapshot);
        Ok(())
    })?;
    count.set(1)?;
    runtime.batch(|| {
        count.set(2)?;
        count.set(3)
    })?;
    assert_eq!(*snapshots.borrow(), [(0, 0), (1, 2), (3, 6)]);
    drop(owner);
    assert_eq!(runtime.stats().signals, 0);
    assert_eq!(runtime.stats().effects, 0);
    assert_eq!(count.get(), Err(ReactiveError::Disposed));
    println!("Owner disposed; no live reactive resources.");
    Ok(())
}
