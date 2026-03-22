use fabstir_llm_node::transcoder::capacity::{release, try_acquire, TranscodeSlotGuard};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[test]
fn test_try_acquire_success() {
    let counter = AtomicUsize::new(0);
    assert!(try_acquire(&counter, 3));
    assert_eq!(counter.load(Ordering::Relaxed), 1);
}

#[test]
fn test_try_acquire_at_capacity() {
    let counter = AtomicUsize::new(3);
    assert!(!try_acquire(&counter, 3));
    assert_eq!(counter.load(Ordering::Relaxed), 3);
}

#[test]
fn test_release_decrements() {
    let counter = AtomicUsize::new(2);
    release(&counter);
    assert_eq!(counter.load(Ordering::Relaxed), 1);
}

#[test]
fn test_release_underflow_guard() {
    let counter = AtomicUsize::new(0);
    release(&counter);
    assert_eq!(counter.load(Ordering::Relaxed), 0);
}

#[test]
fn test_acquire_release_cycle() {
    let counter = AtomicUsize::new(0);
    assert!(try_acquire(&counter, 3));
    assert!(try_acquire(&counter, 3));
    assert!(try_acquire(&counter, 3));
    assert!(!try_acquire(&counter, 3)); // 4th fails
    release(&counter);
    assert!(try_acquire(&counter, 3)); // 5th succeeds
    assert_eq!(counter.load(Ordering::Relaxed), 3);
}

#[tokio::test]
async fn test_concurrent_acquisition() {
    let counter = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(tokio::sync::Barrier::new(10));
    let mut handles = Vec::new();
    for _ in 0..10 {
        let c = counter.clone();
        let b = barrier.clone();
        handles.push(tokio::spawn(async move {
            b.wait().await;
            try_acquire(&c, 3)
        }));
    }
    let successes = futures::future::join_all(handles)
        .await
        .into_iter()
        .filter(|r| *r.as_ref().unwrap())
        .count();
    assert_eq!(successes, 3);
    assert_eq!(counter.load(Ordering::Relaxed), 3);
}

#[test]
fn test_slot_guard_releases_on_drop() {
    let counter = Arc::new(AtomicUsize::new(0));
    assert!(try_acquire(&counter, 3));
    assert_eq!(counter.load(Ordering::Relaxed), 1);
    {
        let _guard = TranscodeSlotGuard::new(counter.clone());
    }
    assert_eq!(counter.load(Ordering::Relaxed), 0);
}

#[test]
fn test_slot_guard_releases_on_panic() {
    let counter = Arc::new(AtomicUsize::new(0));
    assert!(try_acquire(&counter, 3));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = TranscodeSlotGuard::new(counter.clone());
        panic!("simulated panic");
    }));
    assert!(result.is_err());
    assert_eq!(counter.load(Ordering::Relaxed), 0);
}
