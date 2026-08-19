/*
 * v1.19.0 — Cache-line-aligned slab allocator with bitmap free-list.
 *
 * Direct port of `axon-rs/src/buffer/pool.rs`. Three metal-side upgrades
 * documented in pool.h: _Alignas(64) cache-line alignment, bitmap free-
 * list with __builtin_ctzll / _BitScanForward64, huge-pages opt-in.
 *
 * Per-tenant accounting (HashMap<Arc<str>, TenantAccount>) lives in the
 * Rust shim per founder ratification — C handles slabs; symbolic
 * bookkeeping is Rust territory.
 */

#include "pool.h"

#include <stdatomic.h>
#include <stdlib.h>
#include <string.h>

/* ───── Platform detection + abstractions ────────────────────────────── */

#if defined(_WIN32)
#  define AXON_CSYS_POOL_OS_WINDOWS 1
#  include <windows.h>
#  include <intrin.h>
#elif defined(__APPLE__)
#  define AXON_CSYS_POOL_OS_MACOS 1
#  include <pthread.h>
#  include <unistd.h>
#elif defined(__linux__)
#  define AXON_CSYS_POOL_OS_LINUX 1
#  include <pthread.h>
#  include <sys/mman.h>
#  include <unistd.h>
#  ifndef MAP_HUGETLB
#    define MAP_HUGETLB 0  /* graceful no-op on kernels without huge pages */
#  endif
#else
#  define AXON_CSYS_POOL_OS_OTHER 1
#  include <pthread.h>
#endif

/* ───── Mutex abstraction ────────────────────────────────────────────── */

#if defined(AXON_CSYS_POOL_OS_WINDOWS)
typedef SRWLOCK axon_csys_pool_mutex_t;
static inline void axon_csys_pool_mutex_init(axon_csys_pool_mutex_t *m) {
    InitializeSRWLock(m);
}
static inline void axon_csys_pool_mutex_lock(axon_csys_pool_mutex_t *m) {
    AcquireSRWLockExclusive(m);
}
static inline void axon_csys_pool_mutex_unlock(axon_csys_pool_mutex_t *m) {
    ReleaseSRWLockExclusive(m);
}
static inline void axon_csys_pool_mutex_destroy(axon_csys_pool_mutex_t *m) {
    /* SRWLOCK has no destroy — it owns no kernel handle. */
    (void)m;
}
#else
typedef pthread_mutex_t axon_csys_pool_mutex_t;
static inline void axon_csys_pool_mutex_init(axon_csys_pool_mutex_t *m) {
    pthread_mutex_init(m, NULL);
}
static inline void axon_csys_pool_mutex_lock(axon_csys_pool_mutex_t *m) {
    pthread_mutex_lock(m);
}
static inline void axon_csys_pool_mutex_unlock(axon_csys_pool_mutex_t *m) {
    pthread_mutex_unlock(m);
}
static inline void axon_csys_pool_mutex_destroy(axon_csys_pool_mutex_t *m) {
    pthread_mutex_destroy(m);
}
#endif

/* ───── Bitmap intrinsic — find lowest set bit ───────────────────────── */

/* Returns the index (0..63) of the lowest set bit in `bits`. Undefined
 * if `bits` is 0 — callers must guard. */
static inline uint32_t axon_csys_pool_ctz64(uint64_t bits) {
#if defined(_MSC_VER) && !defined(__clang__)
    unsigned long idx;
    /* `_BitScanForward64` is the MSVC intrinsic mirroring __builtin_ctzll. */
    _BitScanForward64(&idx, bits);
    return (uint32_t)idx;
#else
    return (uint32_t)__builtin_ctzll(bits);
#endif
}

/* ───── Aligned allocation primitives ────────────────────────────────── */

#define AXON_CSYS_POOL_CACHE_LINE 64u

/* Allocate `size` bytes aligned to AXON_CSYS_POOL_CACHE_LINE.
 * Returns NULL on failure. */
static uint8_t *axon_csys_pool_alloc_cache_aligned(size_t size) {
#if defined(AXON_CSYS_POOL_OS_WINDOWS)
    return (uint8_t *)_aligned_malloc(size, AXON_CSYS_POOL_CACHE_LINE);
#else
    void *p = NULL;
    /* posix_memalign requires alignment to be a power of 2 + a multiple
     * of sizeof(void*). 64 satisfies both on every supported target. */
    if (posix_memalign(&p, AXON_CSYS_POOL_CACHE_LINE, size) != 0) {
        return NULL;
    }
    return (uint8_t *)p;
#endif
}

static void axon_csys_pool_free_cache_aligned(uint8_t *p) {
    if (p == NULL) return;
#if defined(AXON_CSYS_POOL_OS_WINDOWS)
    _aligned_free(p);
#else
    free(p);
#endif
}

/* ───── Huge-page allocation (opt-in, with graceful fallback) ────────── */

/* Try to allocate `size` bytes backed by huge pages.
 * On success, `*out_used_hugepages` is set to true. On fallback,
 * returns a regular cache-line-aligned allocation and sets
 * `*out_used_hugepages` to false. Returns NULL only on hard OOM. */
static uint8_t *axon_csys_pool_alloc_hugepages(
    size_t size,
    bool *out_used_hugepages
) {
    *out_used_hugepages = false;

#if defined(AXON_CSYS_POOL_OS_LINUX)
    /* MAP_HUGETLB requires /proc/sys/vm/nr_hugepages > 0 OR a huge-page-
     * enabled cgroup. Most adopter machines don't have this configured,
     * so we expect to fall back the majority of the time — that's the
     * documented posture. */
    if (MAP_HUGETLB != 0) {
        void *p = mmap(NULL, size, PROT_READ | PROT_WRITE,
                       MAP_PRIVATE | MAP_ANONYMOUS | MAP_HUGETLB, -1, 0);
        if (p != MAP_FAILED) {
            *out_used_hugepages = true;
            return (uint8_t *)p;
        }
    }
    /* Fall through to regular alignment. */

#elif defined(AXON_CSYS_POOL_OS_WINDOWS)
    /* MEM_LARGE_PAGES requires SeLockMemoryPrivilege, which most processes
     * lack. The size MUST be a multiple of GetLargePageMinimum() which
     * is typically 2 MiB on x86-64. We round up, attempt, and fall back
     * on any failure. */
    SIZE_T large_page_size = GetLargePageMinimum();
    if (large_page_size > 0 && size >= large_page_size) {
        SIZE_T rounded = ((size + large_page_size - 1u) / large_page_size)
                       * large_page_size;
        void *p = VirtualAlloc(
            NULL, rounded,
            MEM_COMMIT | MEM_RESERVE | MEM_LARGE_PAGES,
            PAGE_READWRITE
        );
        if (p != NULL) {
            *out_used_hugepages = true;
            return (uint8_t *)p;
        }
    }
    /* Fall through. */

#else
    /* macOS + other: no huge-pages path in 25.d. Fall through. */
    (void)size;
#endif

    return axon_csys_pool_alloc_cache_aligned(size);
}

static void axon_csys_pool_free_hugepages(
    uint8_t *p,
    size_t size,
    bool used_hugepages
) {
    if (p == NULL) return;
#if defined(AXON_CSYS_POOL_OS_LINUX)
    if (used_hugepages) {
        munmap(p, size);
        return;
    }
#elif defined(AXON_CSYS_POOL_OS_WINDOWS)
    if (used_hugepages) {
        (void)size;
        VirtualFree(p, 0, MEM_RELEASE);
        return;
    }
#else
    (void)size;
    (void)used_hugepages;
#endif
    axon_csys_pool_free_cache_aligned(p);
}

/* ───── Class capacity table ─────────────────────────────────────────── */

static const size_t AXON_CSYS_POOL_CLASS_CAPACITY[AXON_CSYS_POOL_CLASS_COUNT] = {
    /* Small    */ 4u * 1024u,
    /* Medium   */ 64u * 1024u,
    /* Large    */ 1024u * 1024u,
    /* Huge     */ 10u * 1024u * 1024u,
    /* Oversize */ SIZE_MAX,
};

uint32_t axon_csys_buffer_pool_class_for_size(size_t requested_bytes) {
    if (requested_bytes <= AXON_CSYS_POOL_CLASS_CAPACITY[AXON_CSYS_POOL_CLASS_SMALL]) {
        return AXON_CSYS_POOL_CLASS_SMALL;
    }
    if (requested_bytes <= AXON_CSYS_POOL_CLASS_CAPACITY[AXON_CSYS_POOL_CLASS_MEDIUM]) {
        return AXON_CSYS_POOL_CLASS_MEDIUM;
    }
    if (requested_bytes <= AXON_CSYS_POOL_CLASS_CAPACITY[AXON_CSYS_POOL_CLASS_LARGE]) {
        return AXON_CSYS_POOL_CLASS_LARGE;
    }
    if (requested_bytes <= AXON_CSYS_POOL_CLASS_CAPACITY[AXON_CSYS_POOL_CLASS_HUGE]) {
        return AXON_CSYS_POOL_CLASS_HUGE;
    }
    return AXON_CSYS_POOL_CLASS_OVERSIZE;
}

size_t axon_csys_buffer_pool_class_capacity(uint32_t pool_class) {
    if (pool_class >= AXON_CSYS_POOL_CLASS_COUNT) {
        return 0;  /* Unknown class — defensive default. */
    }
    return AXON_CSYS_POOL_CLASS_CAPACITY[pool_class];
}

/* ───── Per-class pool ───────────────────────────────────────────────── */

/* Each non-oversize class owns one of these. Lazy slot allocation:
 * `slots[i]` is NULL until the i-th slot is first acquired. The
 * allocated_bitmap tracks which slots have backing memory; the
 * in_use_bitmap tracks which of those are currently checked out. */
typedef struct {
    uint8_t                *slots[AXON_CSYS_POOL_SLOTS_PER_CLASS];
    /* Whether each slot's memory was huge-page-backed (so release knows
     * which free path to take). One byte per slot for simplicity. */
    bool                    slot_used_hugepages[AXON_CSYS_POOL_SLOTS_PER_CLASS];
    uint64_t                allocated_bitmap;  /* bit i = 1 → slots[i] is non-NULL */
    uint64_t                in_use_bitmap;     /* bit i = 1 → caller holds slots[i] */
    axon_csys_pool_mutex_t  lock;
} axon_csys_pool_class_t;

/* ───── Pool root ────────────────────────────────────────────────────── */

struct axon_csys_buffer_pool {
    axon_csys_pool_class_t  classes[4];      /* Small, Medium, Large, Huge */
    bool                    enable_hugepages;

    /* Atomic metric counters — sampled by snapshot(). */
    _Atomic uint64_t        pool_hits[4];
    _Atomic uint64_t        pool_misses[4];
    _Atomic uint64_t        oversize_allocations_total;
    _Atomic uint64_t        live_bytes;
    _Atomic uint64_t        huge_page_allocations_total;
    _Atomic uint64_t        huge_page_fallbacks_total;
};

/* ───── Lifecycle ────────────────────────────────────────────────────── */

axon_csys_buffer_pool_t *axon_csys_buffer_pool_create(bool enable_hugepages) {
    axon_csys_buffer_pool_t *pool = (axon_csys_buffer_pool_t *)
        calloc(1, sizeof(axon_csys_buffer_pool_t));
    if (pool == NULL) {
        return NULL;
    }
    pool->enable_hugepages = enable_hugepages;
    for (uint32_t c = 0; c < 4u; ++c) {
        axon_csys_pool_mutex_init(&pool->classes[c].lock);
    }
    /* _Atomic counters initialise to 0 via calloc — relaxed init is fine
     * because no other thread can observe the pool until create() returns. */
    return pool;
}

void axon_csys_buffer_pool_destroy(axon_csys_buffer_pool_t *pool) {
    if (pool == NULL) return;
    for (uint32_t c = 0; c < 4u; ++c) {
        axon_csys_pool_class_t *cp = &pool->classes[c];
        size_t cap = AXON_CSYS_POOL_CLASS_CAPACITY[c];
        for (uint32_t s = 0; s < AXON_CSYS_POOL_SLOTS_PER_CLASS; ++s) {
            if (cp->slots[s] != NULL) {
                axon_csys_pool_free_hugepages(
                    cp->slots[s], cap, cp->slot_used_hugepages[s]
                );
                cp->slots[s] = NULL;
            }
        }
        axon_csys_pool_mutex_destroy(&cp->lock);
    }
    free(pool);
}

/* ───── Acquire / release ────────────────────────────────────────────── */

/* Decide whether huge-pages should be attempted for this class. Smaller
 * classes never use huge-pages (would waste 2 MiB per 4 KiB request). */
static bool axon_csys_pool_class_uses_hugepages(uint32_t class) {
    return class == AXON_CSYS_POOL_CLASS_LARGE
        || class == AXON_CSYS_POOL_CLASS_HUGE;
}

axon_csys_buffer_slab_t axon_csys_buffer_pool_acquire(
    axon_csys_buffer_pool_t *pool,
    size_t requested_bytes
) {
    axon_csys_buffer_slab_t out = {
        .ptr = NULL,
        .capacity = 0,
        .pool_class = 0,
        .slot_id = AXON_CSYS_POOL_DIRECT_ALLOC_SLOT,
    };

    uint32_t cls = axon_csys_buffer_pool_class_for_size(requested_bytes);
    out.pool_class = cls;

    /* ── Oversize path: always direct alloc, no pooling ─────────────── */
    if (cls == AXON_CSYS_POOL_CLASS_OVERSIZE) {
        out.ptr = axon_csys_pool_alloc_cache_aligned(requested_bytes);
        out.capacity = (out.ptr != NULL) ? requested_bytes : 0;
        if (out.ptr != NULL) {
            atomic_fetch_add_explicit(
                &pool->oversize_allocations_total, 1u, memory_order_relaxed
            );
            atomic_fetch_add_explicit(
                &pool->live_bytes, requested_bytes, memory_order_relaxed
            );
        }
        return out;
    }

    size_t cap = AXON_CSYS_POOL_CLASS_CAPACITY[cls];
    out.capacity = cap;
    axon_csys_pool_class_t *cp = &pool->classes[cls];

    axon_csys_pool_mutex_lock(&cp->lock);

    /* Try to find an allocated-but-free slot first (HIT path). */
    uint64_t free_alloc = cp->allocated_bitmap & ~cp->in_use_bitmap;
    if (free_alloc != 0u) {
        uint32_t slot = axon_csys_pool_ctz64(free_alloc);
        cp->in_use_bitmap |= (1ull << slot);
        out.ptr = cp->slots[slot];
        out.slot_id = (int32_t)slot;
        axon_csys_pool_mutex_unlock(&cp->lock);
        atomic_fetch_add_explicit(
            &pool->pool_hits[cls], 1u, memory_order_relaxed
        );
        atomic_fetch_add_explicit(
            &pool->live_bytes, cap, memory_order_relaxed
        );
        return out;
    }

    /* No free allocated slot — try to allocate a new one (MISS path).
     * The slot mask is the lower SLOTS_PER_CLASS bits. We rely on the
     * compile-time invariant SLOTS_PER_CLASS == 64 (the bitmap width
     * is uint64_t — they are the same constant by construction). A
     * static_assert here pins the invariant so any future bump bumps
     * both consistently. */
    _Static_assert(
        AXON_CSYS_POOL_SLOTS_PER_CLASS == 64,
        "pool.c assumes the slot bitmap is exactly 64 bits wide; "
        "if you change AXON_CSYS_POOL_SLOTS_PER_CLASS in pool.h, widen "
        "the bitmap fields in axon_csys_pool_class_t accordingly."
    );
    uint64_t unallocated = ~cp->allocated_bitmap;  /* full 64-bit mask */
    if (unallocated != 0u) {
        uint32_t slot = axon_csys_pool_ctz64(unallocated);
        bool used_hugepages = false;
        uint8_t *mem;
        if (pool->enable_hugepages && axon_csys_pool_class_uses_hugepages(cls)) {
            mem = axon_csys_pool_alloc_hugepages(cap, &used_hugepages);
            if (used_hugepages) {
                atomic_fetch_add_explicit(
                    &pool->huge_page_allocations_total, 1u,
                    memory_order_relaxed
                );
            } else if (mem != NULL) {
                atomic_fetch_add_explicit(
                    &pool->huge_page_fallbacks_total, 1u,
                    memory_order_relaxed
                );
            }
        } else {
            mem = axon_csys_pool_alloc_cache_aligned(cap);
        }
        if (mem == NULL) {
            axon_csys_pool_mutex_unlock(&cp->lock);
            out.ptr = NULL;
            out.capacity = 0;
            return out;
        }
        cp->slots[slot] = mem;
        cp->slot_used_hugepages[slot] = used_hugepages;
        cp->allocated_bitmap |= (1ull << slot);
        cp->in_use_bitmap |= (1ull << slot);
        out.ptr = mem;
        out.slot_id = (int32_t)slot;
        axon_csys_pool_mutex_unlock(&cp->lock);
        atomic_fetch_add_explicit(
            &pool->pool_misses[cls], 1u, memory_order_relaxed
        );
        atomic_fetch_add_explicit(
            &pool->live_bytes, cap, memory_order_relaxed
        );
        return out;
    }

    /* All 64 slots in use — fall back to direct alloc.
     * Mirrors the Rust impl's "if free.len() < 64 ... else drop(slab)"
     * cap by counting this as a MISS but with slot_id = DIRECT. */
    axon_csys_pool_mutex_unlock(&cp->lock);
    out.ptr = axon_csys_pool_alloc_cache_aligned(cap);
    if (out.ptr != NULL) {
        atomic_fetch_add_explicit(
            &pool->pool_misses[cls], 1u, memory_order_relaxed
        );
        atomic_fetch_add_explicit(
            &pool->live_bytes, cap, memory_order_relaxed
        );
    } else {
        out.capacity = 0;
    }
    /* slot_id stays DIRECT_ALLOC_SLOT — release will free() instead. */
    return out;
}

void axon_csys_buffer_pool_release(
    axon_csys_buffer_pool_t *pool,
    const axon_csys_buffer_slab_t *slab
) {
    if (pool == NULL || slab == NULL || slab->ptr == NULL) {
        return;
    }

    /* Decrement live_bytes first — it's atomic and order-independent. */
    atomic_fetch_sub_explicit(
        &pool->live_bytes, slab->capacity, memory_order_relaxed
    );

    /* Oversize path: always free directly. */
    if (slab->pool_class == AXON_CSYS_POOL_CLASS_OVERSIZE) {
        axon_csys_pool_free_cache_aligned(slab->ptr);
        return;
    }

    /* Direct-allocated overflow slab (slot_id == -1): free directly. */
    if (slab->slot_id == AXON_CSYS_POOL_DIRECT_ALLOC_SLOT) {
        axon_csys_pool_free_cache_aligned(slab->ptr);
        return;
    }

    /* Pool-managed slot: clear the in-use bit. The slot's memory is
     * NOT freed — it remains in the pool for the next acquire(). */
    if (slab->pool_class >= 4u
        || (uint32_t)slab->slot_id >= AXON_CSYS_POOL_SLOTS_PER_CLASS) {
        return;  /* defensive — bad handle, nothing safe to do */
    }
    axon_csys_pool_class_t *cp = &pool->classes[slab->pool_class];
    axon_csys_pool_mutex_lock(&cp->lock);
    cp->in_use_bitmap &= ~(1ull << (uint32_t)slab->slot_id);
    axon_csys_pool_mutex_unlock(&cp->lock);
}

/* ───── Snapshot ─────────────────────────────────────────────────────── */

void axon_csys_buffer_pool_snapshot(
    const axon_csys_buffer_pool_t *pool,
    axon_csys_buffer_pool_snapshot_t *out
) {
    if (pool == NULL || out == NULL) return;
    /* The atomic counters are mutable in the source; the const-qualified
     * pointer here is a contract for the caller, not a guarantee that the
     * pool itself is read-only. Cast away const to read the atomics. */
    axon_csys_buffer_pool_t *p = (axon_csys_buffer_pool_t *)pool;
    for (uint32_t c = 0; c < 4u; ++c) {
        out->pool_hits[c] = atomic_load_explicit(
            &p->pool_hits[c], memory_order_relaxed
        );
        out->pool_misses[c] = atomic_load_explicit(
            &p->pool_misses[c], memory_order_relaxed
        );
    }
    out->oversize_allocations_total = atomic_load_explicit(
        &p->oversize_allocations_total, memory_order_relaxed
    );
    out->live_bytes = atomic_load_explicit(
        &p->live_bytes, memory_order_relaxed
    );
    out->huge_page_allocations_total = atomic_load_explicit(
        &p->huge_page_allocations_total, memory_order_relaxed
    );
    out->huge_page_fallbacks_total = atomic_load_explicit(
        &p->huge_page_fallbacks_total, memory_order_relaxed
    );
}
