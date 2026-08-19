//! v1.19.0 — Buffer pool slab allocator (Rust shim).
//!
//! Safe Rust wrapper around the C23 cache-line-aligned slab allocator
//! in `c-src/buffer/pool.c`. The boundary follows the founder pillar
//! split:
//!
//!   - C side: cache-line-aligned slab regions (`_Alignas(64)`),
//!     bitmap free-list with `__builtin_ctzll` / `_BitScanForward64`,
//!     huge-pages opt-in (Linux MAP_HUGETLB / Windows MEM_LARGE_PAGES
//!     with graceful fallback), atomic counters.
//!   - Rust side: per-tenant accounting (HashMap<Arc<str>,
//!     TenantAccount>), Slab RAII via Drop with lifetime `'pool`,
//!     snapshot composition.
//!
//! The categorical shape from `axon-rs/src/buffer/pool.rs` is preserved:
//! [`PoolClass`] is the same 5-element coproduct; [`BufferPool::acquire`]
//! returns a slab from the smallest class that fits; tenant soft-limit
//! is a non-blocking observability counter (preserves no-coercion).

use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::{Arc, Mutex};

// ──────────────────────────────────────────────────────────────────────
// Raw FFI surface
// ──────────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct RawSlab {
    ptr: *mut u8,
    capacity: usize,
    pool_class: u32,
    slot_id: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct RawSnapshot {
    pool_hits: [u64; 4],
    pool_misses: [u64; 4],
    oversize_allocations_total: u64,
    live_bytes: u64,
    huge_page_allocations_total: u64,
    huge_page_fallbacks_total: u64,
}

extern "C" {
    fn axon_csys_buffer_pool_create(enable_hugepages: bool) -> *mut c_void;
    fn axon_csys_buffer_pool_destroy(pool: *mut c_void);
    fn axon_csys_buffer_pool_class_for_size(requested_bytes: usize) -> u32;
    fn axon_csys_buffer_pool_class_capacity(pool_class: u32) -> usize;
    fn axon_csys_buffer_pool_acquire(pool: *mut c_void, requested_bytes: usize) -> RawSlab;
    fn axon_csys_buffer_pool_release(pool: *mut c_void, slab: *const RawSlab);
    fn axon_csys_buffer_pool_snapshot(pool: *const c_void, out: *mut RawSnapshot);
}

// ──────────────────────────────────────────────────────────────────────
// PoolClass — mirrors the C enum macros
// ──────────────────────────────────────────────────────────────────────

/// Allocation size classes. The first four are pool-managed; the last
/// (`Oversize`) bypasses the pool and direct-allocates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u32)]
pub enum PoolClass {
    Small = 0,
    Medium = 1,
    Large = 2,
    Huge = 3,
    Oversize = 4,
}

impl PoolClass {
    /// Capacity in bytes. `Oversize` returns `usize::MAX` (sentinel for
    /// "user-supplied size" — actual slab capacity is the requested size).
    pub fn capacity(self) -> usize {
        // SAFETY: pure helper; no pointer ops.
        unsafe { axon_csys_buffer_pool_class_capacity(self as u32) }
    }

    pub fn slug(self) -> &'static str {
        match self {
            PoolClass::Small => "small",
            PoolClass::Medium => "medium",
            PoolClass::Large => "large",
            PoolClass::Huge => "huge",
            PoolClass::Oversize => "oversize",
        }
    }

    pub fn for_size(requested: usize) -> Self {
        // SAFETY: pure helper.
        let raw = unsafe { axon_csys_buffer_pool_class_for_size(requested) };
        Self::from_raw(raw)
    }

    pub fn non_oversize() -> [PoolClass; 4] {
        [
            PoolClass::Small,
            PoolClass::Medium,
            PoolClass::Large,
            PoolClass::Huge,
        ]
    }

    fn from_raw(raw: u32) -> Self {
        match raw {
            0 => PoolClass::Small,
            1 => PoolClass::Medium,
            2 => PoolClass::Large,
            3 => PoolClass::Huge,
            4 => PoolClass::Oversize,
            other => unreachable!("axon_csys returned unknown pool class {other}; ABI drift",),
        }
    }
}

// ──────────────────────────────────────────────────────────────────────
// Tenant accounting — lives in Rust per founder pillar split
// ──────────────────────────────────────────────────────────────────────

#[derive(Debug)]
struct TenantAccount {
    soft_limit_bytes: u64,
    live_bytes: u64,
    soft_limit_exceeded_total: u64,
}

impl TenantAccount {
    fn new(soft_limit_bytes: u64) -> Self {
        TenantAccount {
            soft_limit_bytes,
            live_bytes: 0,
            soft_limit_exceeded_total: 0,
        }
    }
}

// ──────────────────────────────────────────────────────────────────────
// BufferPool — public API
// ──────────────────────────────────────────────────────────────────────

/// Slab-allocating pool with cache-line-aligned storage.
///
/// One pool is shared across all tenants in a process; per-tenant
/// accounting is metric-only (no quota enforcement at the allocation
/// site — soft-limit exceeded just increments a counter). Matches the
/// posture of `axon-rs/src/buffer/pool.rs` and the OTS no-coercion
/// principle (the pool observes; the operator decides).
pub struct BufferPool {
    handle: NonNull<c_void>,
    tenants: Mutex<HashMap<Arc<str>, TenantAccount>>,
    default_tenant_soft_limit_bytes: u64,
}

// SAFETY: the C pool is internally synchronised (per-class mutex +
// C11 atomics on counters); the Rust-side `tenants` field is wrapped
// in Mutex. So &BufferPool can be shared across threads.
unsafe impl Send for BufferPool {}
unsafe impl Sync for BufferPool {}

/// Point-in-time snapshot for metric export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BufferPoolSnapshot {
    pub pool_hits: HashMap<PoolClass, u64>,
    pub pool_misses: HashMap<PoolClass, u64>,
    pub oversize_allocations_total: u64,
    pub live_bytes: u64,
    pub huge_page_allocations_total: u64,
    pub huge_page_fallbacks_total: u64,
    pub tenant_live_bytes: HashMap<String, u64>,
    pub tenant_soft_limit_exceeded_total: HashMap<String, u64>,
}

impl BufferPool {
    /// Construct a pool. `enable_hugepages` is honoured only for Large +
    /// Huge classes; even then it's a hint that may fall back to
    /// regular cache-line-aligned allocs if the OS isn't configured
    /// (Linux nr_hugepages > 0; Windows SeLockMemoryPrivilege).
    pub fn new(default_tenant_soft_limit_bytes: u64, enable_hugepages: bool) -> Self {
        // SAFETY: create returns a non-null handle on success; we panic
        // on the rare OOM path matching the Rust reference posture.
        let raw = unsafe { axon_csys_buffer_pool_create(enable_hugepages) };
        let handle = NonNull::new(raw).expect("axon_csys_buffer_pool_create OOM");
        BufferPool {
            handle,
            tenants: Mutex::new(HashMap::new()),
            default_tenant_soft_limit_bytes,
        }
    }

    /// Configure a per-tenant soft-limit override. Unknown tenants get
    /// the constructor default on their first recorded allocation.
    pub fn set_tenant_soft_limit(&self, tenant_id: impl Into<Arc<str>>, soft_limit_bytes: u64) {
        let arc: Arc<str> = tenant_id.into();
        let mut guard = self.tenants.lock().expect("tenant map poisoned");
        guard
            .entry(arc)
            .or_insert_with(|| TenantAccount::new(soft_limit_bytes))
            .soft_limit_bytes = soft_limit_bytes;
    }

    /// Acquire a slab of at least `requested` bytes.
    ///
    /// The returned [`Slab`] is bound to the pool's lifetime so it cannot
    /// outlive the pool. On Drop the slab is automatically released.
    pub fn acquire(&self, requested: usize) -> Slab<'_> {
        // SAFETY: handle is valid for the pool's lifetime; C kernel is
        // internally thread-safe.
        let raw = unsafe { axon_csys_buffer_pool_acquire(self.handle.as_ptr(), requested) };
        if raw.ptr.is_null() {
            panic!(
                "axon_csys_buffer_pool_acquire OOM (requested {requested} bytes, class {})",
                PoolClass::from_raw(raw.pool_class).slug(),
            );
        }
        Slab { pool: self, raw }
    }

    /// Record `bytes` of live buffer allocation against `tenant_id`.
    /// Increments soft-limit-exceeded counter when applicable; does
    /// NOT block or refuse the allocation.
    pub fn record_tenant_allocation(&self, tenant_id: impl Into<Arc<str>>, bytes: u64) {
        let arc: Arc<str> = tenant_id.into();
        let default = self.default_tenant_soft_limit_bytes;
        let mut guard = self.tenants.lock().expect("tenant map poisoned");
        let entry = guard
            .entry(arc)
            .or_insert_with(|| TenantAccount::new(default));
        entry.live_bytes = entry.live_bytes.saturating_add(bytes);
        if entry.live_bytes > entry.soft_limit_bytes {
            entry.soft_limit_exceeded_total += 1;
        }
    }

    /// Symmetric to [`Self::record_tenant_allocation`]; called when a
    /// tenant-attributed buffer drops.
    pub fn record_tenant_release(&self, tenant_id: impl Into<Arc<str>>, bytes: u64) {
        let arc: Arc<str> = tenant_id.into();
        let mut guard = self.tenants.lock().expect("tenant map poisoned");
        if let Some(entry) = guard.get_mut(&arc) {
            entry.live_bytes = entry.live_bytes.saturating_sub(bytes);
        }
    }

    /// Sample all C-side counters + Rust-side tenant state into a
    /// snapshot for metric export. Internally consistent per field.
    pub fn snapshot(&self) -> BufferPoolSnapshot {
        let mut raw = RawSnapshot::default();
        // SAFETY: handle valid; raw is a stack-allocated out-pointer.
        unsafe {
            axon_csys_buffer_pool_snapshot(self.handle.as_ptr(), &mut raw);
        }
        let mut pool_hits = HashMap::with_capacity(4);
        let mut pool_misses = HashMap::with_capacity(4);
        for cls in PoolClass::non_oversize() {
            let idx = cls as usize;
            pool_hits.insert(cls, raw.pool_hits[idx]);
            pool_misses.insert(cls, raw.pool_misses[idx]);
        }
        let guard = self.tenants.lock().expect("tenant map poisoned");
        let tenant_live_bytes: HashMap<String, u64> = guard
            .iter()
            .map(|(k, v)| (k.to_string(), v.live_bytes))
            .collect();
        let tenant_soft_limit_exceeded_total: HashMap<String, u64> = guard
            .iter()
            .map(|(k, v)| (k.to_string(), v.soft_limit_exceeded_total))
            .collect();
        BufferPoolSnapshot {
            pool_hits,
            pool_misses,
            oversize_allocations_total: raw.oversize_allocations_total,
            live_bytes: raw.live_bytes,
            huge_page_allocations_total: raw.huge_page_allocations_total,
            huge_page_fallbacks_total: raw.huge_page_fallbacks_total,
            tenant_live_bytes,
            tenant_soft_limit_exceeded_total,
        }
    }
}

impl Default for BufferPool {
    /// Default per-tenant soft limit: 256 MiB (matches Rust ref).
    /// Huge-pages disabled by default — opt-in via [`BufferPool::new`].
    fn default() -> Self {
        BufferPool::new(256 * 1024 * 1024, false)
    }
}

impl Drop for BufferPool {
    fn drop(&mut self) {
        // SAFETY: handle is valid; the lifetime system guarantees no
        // outstanding Slabs (they borrow &self).
        unsafe { axon_csys_buffer_pool_destroy(self.handle.as_ptr()) };
    }
}

// ──────────────────────────────────────────────────────────────────────
// Slab — RAII handle bound to pool lifetime
// ──────────────────────────────────────────────────────────────────────

/// A slab acquired from a [`BufferPool`]. Released automatically on
/// Drop. The lifetime parameter ensures the slab cannot outlive the
/// pool (Rust borrow checker enforces this at compile time).
#[must_use = "slabs are released on Drop; ignoring the handle leaks the slab back to the pool"]
pub struct Slab<'pool> {
    pool: &'pool BufferPool,
    raw: RawSlab,
}

impl<'pool> Slab<'pool> {
    /// Returns the slab capacity in bytes (always ≥ requested).
    pub fn capacity(&self) -> usize {
        self.raw.capacity
    }

    /// Returns the size class this slab was drawn from.
    pub fn class(&self) -> PoolClass {
        PoolClass::from_raw(self.raw.pool_class)
    }

    /// True iff this slab was direct-allocated (not pool-managed).
    /// Happens for Oversize requests OR when the class's 64 slots are
    /// all in use.
    pub fn is_direct_alloc(&self) -> bool {
        self.raw.slot_id == -1
    }

    /// Read view over the slab's memory.
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: ptr is non-null (acquire panics on null) and capacity
        // bytes are usable. The slice borrow ties the lifetime to &self.
        unsafe { std::slice::from_raw_parts(self.raw.ptr, self.raw.capacity) }
    }

    /// Write view over the slab's memory.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: see as_slice; mut slice ties to &mut self.
        unsafe { std::slice::from_raw_parts_mut(self.raw.ptr, self.raw.capacity) }
    }

    /// Address of the underlying memory. Cache-line aligned (64 bytes).
    pub fn as_ptr(&self) -> *const u8 {
        self.raw.ptr
    }
}

impl<'pool> Drop for Slab<'pool> {
    fn drop(&mut self) {
        // SAFETY: pool outlives slab via lifetime; raw was returned by
        // the pool's acquire call; release is idempotent on null.
        unsafe {
            axon_csys_buffer_pool_release(self.pool.handle.as_ptr(), &self.raw);
        }
    }
}
