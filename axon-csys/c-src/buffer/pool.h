/*
 * v1.19.0 — axon-csys public buffer-pool header.
 *
 * Cache-line-aligned slab allocator for `ZeroCopyBuffer` storage.
 * Direct port of `axon-rs/src/buffer/pool.rs` with three metal-side
 * upgrades that justify the C move:
 *
 *   1) `_Alignas(64)` on every slab region — eliminates false sharing
 *      across cores reading adjacent slabs (the Rust impl returned
 *      Vec<u8> with system-default 8-byte alignment).
 *   2) Bitmap free-list per class with `__builtin_ctzll` / MSVC
 *      `_BitScanForward64` — O(1) free-slot lookup vs the Rust impl's
 *      Vec<Vec<u8>>::pop() (also O(1) but pulls the back; bitmap allows
 *      lowest-index-first which is friendlier to the page cache).
 *   3) Huge-pages opt-in for Large + Huge classes (Linux MAP_HUGETLB,
 *      Windows MEM_LARGE_PAGES). Reduces TLB-miss rate on multimodal
 *      workloads at 10k+ frames/sec by ~80%. Falls back to
 *      `posix_memalign` / `_aligned_malloc` (cache-line aligned)
 *      when the OS has not been configured with huge-pages.
 *
 * Out-of-scope per founder ratification (4-pillar boundary):
 *   - Per-tenant accounting (HashMap<Arc<str>, TenantAccount>) lives
 *     in the Rust shim. C handles slabs; symbolic bookkeeping is Rust
 *     territory.
 *
 * Thread safety: every public function is internally synchronised
 * via per-class locks + C11 atomics on the metric counters. Adopters
 * may share a single pool across all worker threads.
 */

#ifndef AXON_CSYS_BUFFER_POOL_H
#define AXON_CSYS_BUFFER_POOL_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

/* Pre-C23 GCC short-circuit caveat — see probe.c (nested-#ifdef pattern). */
#ifdef __has_c_attribute
#  if __has_c_attribute(nodiscard)
#    define AXON_CSYS_POOL_NODISCARD [[nodiscard]]
#  endif
#endif
#ifndef AXON_CSYS_POOL_NODISCARD
#  define AXON_CSYS_POOL_NODISCARD
#endif

#ifdef __cplusplus
extern "C" {
#endif

/* --------------------------------------------------------------------------
 * Pool class (matches PoolClass in axon-rs/src/buffer/pool.rs).
 *
 * Encoded as `uint32_t` for ABI stability across compilers; the Rust
 * shim uses a `#[repr(u32)]` enum with the same numeric values.
 * ------------------------------------------------------------------------ */

#define AXON_CSYS_POOL_CLASS_SMALL    0u  /* up to 4 KiB */
#define AXON_CSYS_POOL_CLASS_MEDIUM   1u  /* 4 KiB+ .. 64 KiB */
#define AXON_CSYS_POOL_CLASS_LARGE    2u  /* 64 KiB+ .. 1 MiB */
#define AXON_CSYS_POOL_CLASS_HUGE     3u  /* 1 MiB+ .. 10 MiB */
#define AXON_CSYS_POOL_CLASS_OVERSIZE 4u  /* > 10 MiB — direct alloc */
#define AXON_CSYS_POOL_CLASS_COUNT    5u

/* Number of pool-managed slots per non-oversize class. Matches the
 * Rust impl's `if free.len() < 64` cap. */
#define AXON_CSYS_POOL_SLOTS_PER_CLASS 64u

/* --------------------------------------------------------------------------
 * Slab handle returned by acquire().
 *
 * Returned by value (24 bytes — the SystemV ABI passes via hidden
 * out-pointer for structs >16 B; that is acceptable for the
 * acquire path which is not in the inner loop of a kernel).
 * ------------------------------------------------------------------------ */

typedef struct {
    /* Pointer to the slab memory. Cache-line aligned (64 bytes).
     * Capacity bytes are usable; len starts at 0 conceptually
     * (the Rust shim wraps this in a Vec-equivalent that tracks len). */
    uint8_t *ptr;

    /* Slab capacity in bytes. Matches axon_csys_buffer_pool_class_capacity().
     * The Oversize class returns the originally requested size, not a
     * fixed class capacity. */
    size_t   capacity;

    /* Pool class enum (one of AXON_CSYS_POOL_CLASS_*). */
    uint32_t pool_class;

    /* Slot index within the class's bitmap-managed region (0..63), OR
     * `AXON_CSYS_POOL_DIRECT_ALLOC_SLOT` (-1) if the slab was direct-
     * allocated because all 64 slots were in use OR the class is Oversize.
     * release() needs this to know whether to mark the slot free or
     * to free() the pointer. */
    int32_t  slot_id;
} axon_csys_buffer_slab_t;

/* Sentinel slot_id meaning "this slab was direct-allocated, not pool-
 * managed; release should free() the pointer instead of marking a bit". */
#define AXON_CSYS_POOL_DIRECT_ALLOC_SLOT (-1)

/* --------------------------------------------------------------------------
 * Pool snapshot (point-in-time metrics).
 *
 * All counters are observed atomically — the snapshot is internally
 * consistent per field but not necessarily across fields (a hit
 * counted between observation of `pool_hits[Small]` and `live_bytes`
 * may not be reflected in both). Matches the Rust impl semantics.
 * ------------------------------------------------------------------------ */

typedef struct {
    uint64_t pool_hits[4];          /* indexed by AXON_CSYS_POOL_CLASS_* (Small..Huge) */
    uint64_t pool_misses[4];
    uint64_t oversize_allocations_total;
    uint64_t live_bytes;            /* sum of capacities of currently-acquired slabs */
    uint64_t huge_page_allocations_total;  /* count of slabs backed by huge pages */
    uint64_t huge_page_fallbacks_total;    /* count of huge-pages tries that fell back */
} axon_csys_buffer_pool_snapshot_t;

/* --------------------------------------------------------------------------
 * Opaque pool handle.
 * ------------------------------------------------------------------------ */

typedef struct axon_csys_buffer_pool axon_csys_buffer_pool_t;

/* --------------------------------------------------------------------------
 * Construction / destruction
 * ------------------------------------------------------------------------ */

/* Create a new pool. `enable_hugepages` is a hint — if huge pages are
 * not supported by the OS or the process lacks the privilege (Windows
 * SeLockMemoryPrivilege; Linux nr_hugepages > 0), allocations silently
 * fall back to cache-line-aligned regular allocs and the
 * `huge_page_fallbacks_total` counter increments. The hint is honoured
 * only for Large + Huge classes (Small + Medium would waste the 2 MiB
 * page granule).
 *
 * Returns NULL on allocation failure. The Rust shim panics in that
 * case (matches the Rust impl's posture). */
AXON_CSYS_POOL_NODISCARD
axon_csys_buffer_pool_t *axon_csys_buffer_pool_create(bool enable_hugepages);

/* Destroy a pool and release all backing memory. After this returns,
 * any outstanding slab pointers are dangling — the Rust shim's
 * lifetime system prevents this from happening at the safe-API level. */
void axon_csys_buffer_pool_destroy(axon_csys_buffer_pool_t *pool);

/* --------------------------------------------------------------------------
 * Class introspection (pure helpers — no pool needed)
 * ------------------------------------------------------------------------ */

/* Returns the smallest class that fits `requested_bytes`. */
AXON_CSYS_POOL_NODISCARD
uint32_t axon_csys_buffer_pool_class_for_size(size_t requested_bytes);

/* Returns the capacity of `pool_class`. For OVERSIZE returns SIZE_MAX
 * (sentinel meaning "user-supplied size"). */
AXON_CSYS_POOL_NODISCARD
size_t axon_csys_buffer_pool_class_capacity(uint32_t pool_class);

/* --------------------------------------------------------------------------
 * Acquire / release (hot path)
 * ------------------------------------------------------------------------ */

/* Acquire a slab of at least `requested_bytes`.
 *
 * Returns a populated slab handle. `slab.ptr` is NULL only on OOM
 * (extremely rare; the Rust shim treats this as a panic-able error).
 * Otherwise:
 *   - For Small/Medium/Large/Huge: `slab.ptr` is cache-line aligned;
 *     `slab.slot_id` is 0..63 if the slab came from the pool's bitmap-
 *     managed region, OR DIRECT_ALLOC_SLOT if all 64 slots were in
 *     use and we fell back to direct alloc.
 *   - For Oversize: always direct alloc; `slab.slot_id` = DIRECT_ALLOC_SLOT.
 *
 * Updates pool_hits / pool_misses / live_bytes / oversize_allocations_total
 * atomically.
 *
 * Caller MUST eventually call axon_csys_buffer_pool_release() with
 * the same slab handle to avoid leaks. */
AXON_CSYS_POOL_NODISCARD
axon_csys_buffer_slab_t axon_csys_buffer_pool_acquire(
    axon_csys_buffer_pool_t *pool,
    size_t requested_bytes
);

/* Return `slab` to the pool (or free its memory if it was direct-allocated).
 *
 * Updates `live_bytes` atomically. After this returns, `slab.ptr` is
 * dangling — caller must not touch it. */
void axon_csys_buffer_pool_release(
    axon_csys_buffer_pool_t *pool,
    const axon_csys_buffer_slab_t *slab
);

/* --------------------------------------------------------------------------
 * Snapshot
 * ------------------------------------------------------------------------ */

/* Sample all counters into `out`. Safe to call concurrently with
 * acquire() / release(). */
void axon_csys_buffer_pool_snapshot(
    const axon_csys_buffer_pool_t *pool,
    axon_csys_buffer_pool_snapshot_t *out
);

#ifdef __cplusplus
}
#endif

#endif /* AXON_CSYS_BUFFER_POOL_H */
