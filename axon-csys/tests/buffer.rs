//! v1.19.0 — Buffer pool slab allocator test suite.
//!
//! Exercises the full surface of `axon_csys::buffer::*`. The C kernel
//! (cache-line slabs + bitmap free-list + hugepages opt-in) and the
//! Rust shim (Slab RAII + tenant HashMap accounting) are tested
//! together; they are the same product.

use axon_csys::{BufferPool, PoolClass};

use std::sync::Arc;
use std::thread;

// ────────────────────────────────────────────────────────────────────────
// PoolClass — sizing, capacity, slug
// ────────────────────────────────────────────────────────────────────────

#[test]
fn class_for_size_maps_zero_to_small() {
    assert_eq!(PoolClass::for_size(0), PoolClass::Small);
    assert_eq!(PoolClass::for_size(1), PoolClass::Small);
}

#[test]
fn class_for_size_boundary_at_4_kib() {
    assert_eq!(PoolClass::for_size(4 * 1024), PoolClass::Small);
    assert_eq!(PoolClass::for_size(4 * 1024 + 1), PoolClass::Medium);
}

#[test]
fn class_for_size_boundary_at_64_kib() {
    assert_eq!(PoolClass::for_size(64 * 1024), PoolClass::Medium);
    assert_eq!(PoolClass::for_size(64 * 1024 + 1), PoolClass::Large);
}

#[test]
fn class_for_size_boundary_at_1_mib() {
    assert_eq!(PoolClass::for_size(1024 * 1024), PoolClass::Large);
    assert_eq!(PoolClass::for_size(1024 * 1024 + 1), PoolClass::Huge);
}

#[test]
fn class_for_size_boundary_at_10_mib() {
    assert_eq!(PoolClass::for_size(10 * 1024 * 1024), PoolClass::Huge);
    assert_eq!(
        PoolClass::for_size(10 * 1024 * 1024 + 1),
        PoolClass::Oversize
    );
}

#[test]
fn class_capacities_match_documented_values() {
    assert_eq!(PoolClass::Small.capacity(), 4 * 1024);
    assert_eq!(PoolClass::Medium.capacity(), 64 * 1024);
    assert_eq!(PoolClass::Large.capacity(), 1024 * 1024);
    assert_eq!(PoolClass::Huge.capacity(), 10 * 1024 * 1024);
    assert_eq!(PoolClass::Oversize.capacity(), usize::MAX);
}

#[test]
fn class_slugs_are_stable() {
    assert_eq!(PoolClass::Small.slug(), "small");
    assert_eq!(PoolClass::Medium.slug(), "medium");
    assert_eq!(PoolClass::Large.slug(), "large");
    assert_eq!(PoolClass::Huge.slug(), "huge");
    assert_eq!(PoolClass::Oversize.slug(), "oversize");
}

#[test]
fn non_oversize_returns_four_classes_in_order() {
    let classes = PoolClass::non_oversize();
    assert_eq!(
        classes,
        [
            PoolClass::Small,
            PoolClass::Medium,
            PoolClass::Large,
            PoolClass::Huge,
        ]
    );
}

// ────────────────────────────────────────────────────────────────────────
// Acquire — basic capacity contract
// ────────────────────────────────────────────────────────────────────────

#[test]
fn acquire_small_returns_4kib_slab() {
    let pool = BufferPool::default();
    let slab = pool.acquire(100);
    assert_eq!(slab.class(), PoolClass::Small);
    assert_eq!(slab.capacity(), 4 * 1024);
}

#[test]
fn acquire_medium_returns_64kib_slab() {
    let pool = BufferPool::default();
    let slab = pool.acquire(8 * 1024);
    assert_eq!(slab.class(), PoolClass::Medium);
    assert_eq!(slab.capacity(), 64 * 1024);
}

#[test]
fn acquire_large_returns_1mib_slab() {
    let pool = BufferPool::default();
    let slab = pool.acquire(500 * 1024);
    assert_eq!(slab.class(), PoolClass::Large);
    assert_eq!(slab.capacity(), 1024 * 1024);
}

#[test]
fn acquire_huge_returns_10mib_slab() {
    let pool = BufferPool::default();
    let slab = pool.acquire(2 * 1024 * 1024);
    assert_eq!(slab.class(), PoolClass::Huge);
    assert_eq!(slab.capacity(), 10 * 1024 * 1024);
}

#[test]
fn acquire_oversize_returns_requested_capacity() {
    let pool = BufferPool::default();
    let request = 50 * 1024 * 1024;
    let slab = pool.acquire(request);
    assert_eq!(slab.class(), PoolClass::Oversize);
    assert_eq!(slab.capacity(), request);
    assert!(slab.is_direct_alloc());
}

#[test]
fn acquire_zero_bytes_still_returns_small_slab() {
    let pool = BufferPool::default();
    let slab = pool.acquire(0);
    assert_eq!(slab.class(), PoolClass::Small);
    assert_eq!(slab.capacity(), 4 * 1024);
}

// ────────────────────────────────────────────────────────────────────────
// Cache-line alignment — the metal upgrade over Vec<u8>
// ────────────────────────────────────────────────────────────────────────

#[test]
fn small_slab_is_cache_line_aligned() {
    let pool = BufferPool::default();
    let slab = pool.acquire(100);
    let addr = slab.as_ptr() as usize;
    assert_eq!(
        addr % 64,
        0,
        "Small slab pointer 0x{addr:x} is not 64-byte aligned",
    );
}

#[test]
fn medium_slab_is_cache_line_aligned() {
    let pool = BufferPool::default();
    let slab = pool.acquire(50 * 1024);
    let addr = slab.as_ptr() as usize;
    assert_eq!(addr % 64, 0);
}

#[test]
fn large_slab_is_cache_line_aligned() {
    let pool = BufferPool::default();
    let slab = pool.acquire(900 * 1024);
    let addr = slab.as_ptr() as usize;
    assert_eq!(addr % 64, 0);
}

#[test]
fn oversize_slab_is_cache_line_aligned() {
    let pool = BufferPool::default();
    let slab = pool.acquire(20 * 1024 * 1024);
    let addr = slab.as_ptr() as usize;
    assert_eq!(addr % 64, 0);
}

// ────────────────────────────────────────────────────────────────────────
// Bitmap free-list reuse + RAII
// ────────────────────────────────────────────────────────────────────────

#[test]
fn release_reuses_slab_address() {
    let pool = BufferPool::default();
    let first_addr = {
        let slab = pool.acquire(100);
        slab.as_ptr() as usize
    }; // dropped here
    let second_addr = {
        let slab = pool.acquire(100);
        slab.as_ptr() as usize
    };
    assert_eq!(
        first_addr, second_addr,
        "bitmap free-list should hand the same slot back after release",
    );
}

#[test]
fn snapshot_increments_hits_after_reuse() {
    let pool = BufferPool::default();
    {
        let _ = pool.acquire(100); // miss → allocates slot 0
    }
    {
        let _ = pool.acquire(100); // hit → reuses slot 0
    }
    let snap = pool.snapshot();
    assert_eq!(snap.pool_misses[&PoolClass::Small], 1);
    assert_eq!(snap.pool_hits[&PoolClass::Small], 1);
}

#[test]
fn snapshot_oversize_counter_increments() {
    let pool = BufferPool::default();
    let _ = pool.acquire(20 * 1024 * 1024); // oversize
    let _ = pool.acquire(30 * 1024 * 1024); // oversize again
    let snap = pool.snapshot();
    assert_eq!(snap.oversize_allocations_total, 2);
}

#[test]
fn live_bytes_tracks_outstanding_allocations() {
    let pool = BufferPool::default();
    let snap_before = pool.snapshot();
    assert_eq!(snap_before.live_bytes, 0);
    let slab = pool.acquire(100);
    let snap_during = pool.snapshot();
    assert_eq!(snap_during.live_bytes, PoolClass::Small.capacity() as u64);
    drop(slab);
    let snap_after = pool.snapshot();
    assert_eq!(snap_after.live_bytes, 0);
}

#[test]
fn slot_id_progresses_for_concurrent_holders() {
    // Hold 3 slabs simultaneously; addresses must be distinct (different slots).
    let pool = BufferPool::default();
    let s1 = pool.acquire(100);
    let s2 = pool.acquire(100);
    let s3 = pool.acquire(100);
    let a1 = s1.as_ptr() as usize;
    let a2 = s2.as_ptr() as usize;
    let a3 = s3.as_ptr() as usize;
    assert_ne!(a1, a2);
    assert_ne!(a1, a3);
    assert_ne!(a2, a3);
}

#[test]
fn bitmap_full_falls_back_to_direct_alloc() {
    let pool = BufferPool::default();
    let mut held = Vec::new();
    // Acquire 64 slots — fills the Small class bitmap.
    for _ in 0..64 {
        held.push(pool.acquire(100));
    }
    // 65th request should fall back to direct alloc.
    let overflow = pool.acquire(100);
    assert_eq!(overflow.class(), PoolClass::Small);
    assert_eq!(overflow.capacity(), 4 * 1024);
    assert!(
        overflow.is_direct_alloc(),
        "65th Small slab should be direct-allocated when bitmap is full",
    );
    drop(overflow);
    drop(held);
}

#[test]
fn release_after_bitmap_full_resumes_pool_path() {
    let pool = BufferPool::default();
    let mut held = Vec::new();
    for _ in 0..64 {
        held.push(pool.acquire(100));
    }
    drop(held); // releases all 64 slots back
                // Next acquire should be a HIT (slot 0 reused).
    let hit_addr = {
        let slab = pool.acquire(100);
        slab.as_ptr() as usize
    };
    assert_ne!(hit_addr, 0);
    let snap = pool.snapshot();
    // 64 misses (initial fills), then ≥1 hit on the next acquire.
    assert_eq!(snap.pool_misses[&PoolClass::Small], 64);
    assert!(snap.pool_hits[&PoolClass::Small] >= 1);
}

#[test]
fn slab_as_slice_round_trips_data() {
    let pool = BufferPool::default();
    let mut slab = pool.acquire(4 * 1024);
    let mem = slab.as_mut_slice();
    for (i, byte) in mem.iter_mut().enumerate() {
        *byte = (i % 256) as u8;
    }
    // Re-borrow as immutable and verify.
    let read = slab.as_slice();
    for (i, &byte) in read.iter().enumerate() {
        assert_eq!(byte, (i % 256) as u8);
    }
}

#[test]
fn slab_capacity_matches_class() {
    let pool = BufferPool::default();
    let slab = pool.acquire(8 * 1024);
    assert_eq!(slab.capacity(), slab.class().capacity());
}

// ────────────────────────────────────────────────────────────────────────
// Tenant accounting — Rust-side HashMap state
// ────────────────────────────────────────────────────────────────────────

#[test]
fn tenant_record_allocation_creates_account_with_default_limit() {
    let pool = BufferPool::new(1024, false);
    pool.record_tenant_allocation("alpha", 512);
    let snap = pool.snapshot();
    assert_eq!(snap.tenant_live_bytes["alpha"], 512);
    assert_eq!(snap.tenant_soft_limit_exceeded_total["alpha"], 0);
}

#[test]
fn tenant_soft_limit_exceeded_increments_per_overflow_call() {
    let pool = BufferPool::new(1024, false);
    pool.record_tenant_allocation("alpha", 2048); // 2048 > 1024 → +1
    pool.record_tenant_allocation("alpha", 512); // 2560 > 1024 → +1
    let snap = pool.snapshot();
    assert_eq!(snap.tenant_soft_limit_exceeded_total["alpha"], 2);
    assert_eq!(snap.tenant_live_bytes["alpha"], 2560);
}

#[test]
fn tenant_release_decrements_live_bytes() {
    let pool = BufferPool::new(1024, false);
    pool.record_tenant_allocation("alpha", 800);
    pool.record_tenant_release("alpha", 300);
    let snap = pool.snapshot();
    assert_eq!(snap.tenant_live_bytes["alpha"], 500);
}

#[test]
fn tenant_release_saturates_at_zero() {
    let pool = BufferPool::new(1024, false);
    pool.record_tenant_allocation("alpha", 100);
    // Release more than allocated — saturating_sub clamps at 0.
    pool.record_tenant_release("alpha", 500);
    let snap = pool.snapshot();
    assert_eq!(snap.tenant_live_bytes["alpha"], 0);
}

#[test]
fn per_tenant_override_applies() {
    let pool = BufferPool::new(1024, false);
    pool.set_tenant_soft_limit("premium", 10 * 1024);
    pool.record_tenant_allocation("premium", 5 * 1024);
    let snap = pool.snapshot();
    // 5 KiB under the 10 KiB override → no exceed.
    assert_eq!(snap.tenant_soft_limit_exceeded_total["premium"], 0);
}

#[test]
fn per_tenant_override_can_be_set_after_first_allocation() {
    let pool = BufferPool::new(1024, false);
    pool.record_tenant_allocation("alpha", 5 * 1024); // +1 exceed (over 1024)
    pool.set_tenant_soft_limit("alpha", 10 * 1024); // raise to 10 KiB
    pool.record_tenant_allocation("alpha", 1024); // 6 KiB now under 10 KiB
    let snap = pool.snapshot();
    // First call: 5 KiB > 1 KiB → +1. Second call: 6 KiB still > 10 KiB? No, under.
    // After override, the live_bytes (6 KiB) is below soft_limit (10 KiB) → no +1.
    assert_eq!(snap.tenant_soft_limit_exceeded_total["alpha"], 1);
    assert_eq!(snap.tenant_live_bytes["alpha"], 6 * 1024);
}

#[test]
fn tenant_accounting_is_independent_per_tenant() {
    let pool = BufferPool::new(1024, false);
    pool.record_tenant_allocation("alpha", 500);
    pool.record_tenant_allocation("beta", 800);
    let snap = pool.snapshot();
    assert_eq!(snap.tenant_live_bytes["alpha"], 500);
    assert_eq!(snap.tenant_live_bytes["beta"], 800);
}

// ────────────────────────────────────────────────────────────────────────
// Snapshot composition
// ────────────────────────────────────────────────────────────────────────

#[test]
fn snapshot_initial_state_is_zero() {
    let pool = BufferPool::default();
    let snap = pool.snapshot();
    for cls in PoolClass::non_oversize() {
        assert_eq!(snap.pool_hits[&cls], 0);
        assert_eq!(snap.pool_misses[&cls], 0);
    }
    assert_eq!(snap.oversize_allocations_total, 0);
    assert_eq!(snap.live_bytes, 0);
    assert_eq!(snap.huge_page_allocations_total, 0);
    assert_eq!(snap.huge_page_fallbacks_total, 0);
    assert!(snap.tenant_live_bytes.is_empty());
}

#[test]
fn snapshot_per_class_counters_are_independent() {
    let pool = BufferPool::default();
    {
        let _s1 = pool.acquire(100); // Small miss
    }
    {
        let _m1 = pool.acquire(8_000); // Medium miss
    }
    {
        let _s2 = pool.acquire(100); // Small hit (slot 0 reused)
    }
    let snap = pool.snapshot();
    assert_eq!(snap.pool_misses[&PoolClass::Small], 1);
    assert_eq!(snap.pool_hits[&PoolClass::Small], 1);
    assert_eq!(snap.pool_misses[&PoolClass::Medium], 1);
    assert_eq!(snap.pool_hits[&PoolClass::Medium], 0);
}

#[test]
fn snapshot_huge_page_counters_default_zero_when_disabled() {
    let pool = BufferPool::new(256 * 1024 * 1024, false); // hugepages OFF
    let _ = pool.acquire(2 * 1024 * 1024); // Huge class
    let snap = pool.snapshot();
    assert_eq!(snap.huge_page_allocations_total, 0);
    assert_eq!(snap.huge_page_fallbacks_total, 0);
}

#[test]
fn snapshot_huge_page_fallback_counter_when_enabled_but_unsupported() {
    // Most adopter machines don't have huge-pages OS-configured, so
    // requesting them with `enable_hugepages=true` should fall back.
    // We don't assert which counter is incremented (depends on host),
    // but the SUM should equal the number of Large/Huge slabs allocated.
    let pool = BufferPool::new(256 * 1024 * 1024, true);
    let _l = pool.acquire(900 * 1024); // Large
    let _h = pool.acquire(2 * 1024 * 1024); // Huge
    let snap = pool.snapshot();
    let observed = snap.huge_page_allocations_total + snap.huge_page_fallbacks_total;
    assert_eq!(
        observed, 2,
        "every Large/Huge allocation with hugepages enabled MUST count as either active or fallback",
    );
}

// ────────────────────────────────────────────────────────────────────────
// Concurrency — pool is Sync; tenants accounting is Mutex-protected
// ────────────────────────────────────────────────────────────────────────

#[test]
fn concurrent_acquire_release_does_not_crash() {
    let pool = Arc::new(BufferPool::default());
    let mut handles = Vec::new();
    for _ in 0..8 {
        let pool = Arc::clone(&pool);
        handles.push(thread::spawn(move || {
            for _ in 0..200 {
                let _slab = pool.acquire(100);
                // dropped at end of iteration
            }
        }));
    }
    for h in handles {
        h.join().expect("worker thread panicked");
    }
    // After all threads finish, live_bytes must be 0.
    let snap = pool.snapshot();
    assert_eq!(snap.live_bytes, 0, "all slabs should have been released");
}

#[test]
fn concurrent_acquire_keeps_addresses_distinct_within_class() {
    use std::sync::Mutex;
    let pool = Arc::new(BufferPool::default());
    let addrs: Arc<Mutex<Vec<usize>>> = Arc::new(Mutex::new(Vec::new()));
    let mut handles = Vec::new();
    let barrier = Arc::new(std::sync::Barrier::new(8));
    for _ in 0..8 {
        let pool = Arc::clone(&pool);
        let addrs = Arc::clone(&addrs);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            let slab = pool.acquire(100);
            let addr = slab.as_ptr() as usize;
            addrs.lock().unwrap().push(addr);
            // Hold the slab until the test ends so each concurrent
            // acquire forces the bitmap to find a different slot.
            std::thread::sleep(std::time::Duration::from_millis(20));
            drop(slab);
        }));
    }
    for h in handles {
        h.join().expect("worker thread panicked");
    }
    let mut addrs = addrs.lock().unwrap().clone();
    addrs.sort_unstable();
    addrs.dedup();
    // 8 concurrent holders → 8 distinct slot addresses.
    assert_eq!(
        addrs.len(),
        8,
        "concurrent acquires must hand out distinct slots"
    );
}

#[test]
fn concurrent_tenant_accounting_is_consistent() {
    let pool = Arc::new(BufferPool::new(1024, false));
    let mut handles = Vec::new();
    for tid in 0..4 {
        let pool = Arc::clone(&pool);
        let name = format!("tenant_{tid}");
        handles.push(thread::spawn(move || {
            for _ in 0..100 {
                pool.record_tenant_allocation(name.clone(), 10);
                pool.record_tenant_release(name.clone(), 10);
            }
        }));
    }
    for h in handles {
        h.join().expect("worker thread panicked");
    }
    let snap = pool.snapshot();
    for tid in 0..4 {
        let key = format!("tenant_{tid}");
        assert_eq!(
            snap.tenant_live_bytes[&key], 0,
            "tenant {tid} live_bytes must net to 0 after balanced alloc/release",
        );
    }
}

// ────────────────────────────────────────────────────────────────────────
// Lifecycle — drop semantics
// ────────────────────────────────────────────────────────────────────────

#[test]
fn pool_drop_releases_all_backing_memory() {
    // No direct assertion possible from within Rust — this is a
    // valgrind / ASan target. The smoke test here at least proves
    // pool drop with outstanding-and-then-released slabs runs to
    // completion without panicking.
    {
        let pool = BufferPool::default();
        for _ in 0..32 {
            let _slab = pool.acquire(100);
        }
        // pool drops here — should free 32 (or fewer, after bitmap
        // reuse) Small-class slot regions.
    }
}

#[test]
fn many_pools_in_sequence_do_not_leak() {
    for _ in 0..50 {
        let pool = BufferPool::default();
        let _s = pool.acquire(100);
        let _m = pool.acquire(8 * 1024);
        let _l = pool.acquire(500 * 1024);
        // Drops at end of iteration.
    }
}
