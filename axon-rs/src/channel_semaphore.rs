//! v2.69.0 — **`capacity` becomes a real bound: a concurrency semaphore per
//! channel.**
//!
//! v2.69.0 derived `resource.capacity` into `ToolEntry::capacity` — the number
//! reached the runtime. But a number that reaches the runtime and bounds nothing
//! is the nominal link this whole line exists to avoid. v2.69.0 is the semaphore
//! that turns the number into a bound.
//!
//! # Why it is held on `ServerState`, keyed by resource
//!
//! Concurrency is inherently a **cross-request** property: two simultaneous
//! requests each making a call to the same vendor is exactly what `capacity`
//! bounds. A per-request semaphore starts full every time and bounds nothing — the
//! same trap the budget gate faces (v2.69.0). So the semaphores are built once, at
//! deploy, and live on `ServerState`.
//!
//! Keyed by **resource**, not one global: a single process-wide semaphore would
//! let one tenant's load exhaust another's channel. OSS is single-tenant so the
//! scope is the deployment; the enterprise keys by tenant through the same map.
//!
//! # Before v2.69.0
//!
//! A tool had **no** concurrency bound. A `par` over N items opened N connections
//! to a vendor that tolerated ten, and nothing in the language could say otherwise.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Semaphore;

use crate::ir_nodes::IRResource;

/// The channel semaphores for one deployed program: `resource name → semaphore`.
///
/// Only resources with a positive `capacity:` get a semaphore; a resource with no
/// capacity is unbounded (the legacy behaviour), so it simply has no entry and a
/// call against it acquires nothing.
#[derive(Clone, Default)]
pub struct ChannelSemaphores {
    by_resource: HashMap<String, Arc<Semaphore>>,
}

impl ChannelSemaphores {
    /// Build the semaphore set from a program's resources. Empty ⇒ nothing to
    /// bound, and the whole feature costs nothing.
    pub fn from_resources(resources: &[IRResource]) -> Self {
        let mut by_resource = HashMap::new();
        for r in resources {
            if let Some(cap) = r.capacity.filter(|c| *c > 0) {
                by_resource.insert(r.name.clone(), Arc::new(Semaphore::new(cap as usize)));
            }
        }
        ChannelSemaphores { by_resource }
    }

    pub fn is_empty(&self) -> bool {
        self.by_resource.is_empty()
    }

    /// The semaphore governing `resource_name`, if it has a capacity bound.
    ///
    /// The caller acquires a permit from it and **holds the guard across the
    /// call**, so at most `capacity` calls are in flight against that channel at
    /// once. `None` ⇒ the resource is unbounded (or the tool names no resource) —
    /// the call proceeds with no wait, byte-identical to pre-v2.69.0.
    pub fn for_resource(&self, resource_name: &str) -> Option<Arc<Semaphore>> {
        self.by_resource.get(resource_name).cloned()
    }

    /// The permit count a resource was built with — for gates that want to prove
    /// `capacity: 8` produced a semaphore of eight.
    pub fn permits_of(&self, resource_name: &str) -> Option<usize> {
        self.by_resource
            .get(resource_name)
            .map(|s| s.available_permits())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn res(name: &str, capacity: Option<i64>) -> IRResource {
        let mut r = IRResource::new(name.into(), 0, 0);
        r.kind = "https".into();
        r.endpoint = "x.y".into();
        r.capacity = capacity;
        r
    }

    #[test]
    fn a_resource_with_capacity_gets_a_semaphore_of_that_size() {
        let s = ChannelSemaphores::from_resources(&[res("Api", Some(8))]);
        assert_eq!(s.permits_of("Api"), Some(8));
        assert!(s.for_resource("Api").is_some());
    }

    #[test]
    fn a_resource_without_capacity_is_unbounded_no_semaphore() {
        let s = ChannelSemaphores::from_resources(&[res("Api", None)]);
        assert_eq!(s.permits_of("Api"), None);
        assert!(s.for_resource("Api").is_none());
        assert!(s.is_empty());
    }

    #[tokio::test]
    async fn the_semaphore_actually_bounds_concurrency() {
        let s = ChannelSemaphores::from_resources(&[res("Api", Some(2))]);
        let sem = s.for_resource("Api").unwrap();

        // Take both permits.
        let _p1 = sem.clone().try_acquire_owned().unwrap();
        let _p2 = sem.clone().try_acquire_owned().unwrap();
        // The third acquisition must fail — the channel is at capacity.
        assert!(
            sem.clone().try_acquire_owned().is_err(),
            "capacity: 2 must permit exactly two in flight; the third waits"
        );
        drop(_p1);
        // Releasing one frees a permit.
        assert!(sem.try_acquire_owned().is_ok());
    }
}
