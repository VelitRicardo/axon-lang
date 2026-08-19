//! [`Transformer`] trait + [`TransformerRegistry`] + [`Pipeline`]
//! builder.
//!
//! Path finding is Dijkstra over a directed graph whose nodes are
//! [`BufferKind`]s and whose edges are registered transformers.
//! Edge weight = `cost_hint` so adopters can bias toward native
//! paths even when a cheap ffmpeg chain would also work.

use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;

use crate::buffer::{BufferKind, ZeroCopyBuffer};

// ── Errors ───────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum OtsError {
    /// No path exists from source to sink in the current registry.
    NoPath {
        from: BufferKind,
        to: BufferKind,
    },
    /// Transformer execution failed; message carries adopter-supplied
    /// detail.
    TransformFailed(String),
    /// The pipeline hit a kind mismatch mid-flight — typically means
    /// the registry was mutated between path-find and execute.
    KindMismatch {
        expected: BufferKind,
        actual: BufferKind,
    },
}

impl std::fmt::Display for OtsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoPath { from, to } => write!(
                f,
                "no OTS path from {from} to {to} in the current registry"
            ),
            Self::TransformFailed(m) => write!(f, "transform failed: {m}"),
            Self::KindMismatch { expected, actual } => write!(
                f,
                "pipeline kind mismatch — expected {expected}, got {actual}"
            ),
        }
    }
}

impl std::error::Error for OtsError {}

// ── Transformer trait ───────────────────────────────────────────────

/// Backend classification. Consumed by the checker to enforce the
/// `sensitive + legal:HIPAA.* + ots:backend:ffmpeg` rejection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransformerBackend {
    /// Pure-Rust transcoder. No data crosses a process boundary.
    Native,
    /// Subprocess delegation (ffmpeg, sox, …). Data leaves the
    /// address space of the hosting process.
    Subprocess,
}

impl TransformerBackend {
    pub fn slug(self) -> &'static str {
        match self {
            TransformerBackend::Native => "native",
            TransformerBackend::Subprocess => "ffmpeg",
        }
    }
}

/// Stable identifier used for caching + metrics. `"mulaw8->pcm16"`
/// is the canonical format.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TransformerId(pub String);

impl TransformerId {
    pub fn new(from: &BufferKind, to: &BufferKind) -> Self {
        TransformerId(format!("{from}->{to}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The bread-and-butter of OTS. Adopters implement this for each
/// conversion they want to register.
pub trait Transformer: Send + Sync {
    fn source_kind(&self) -> BufferKind;
    fn sink_kind(&self) -> BufferKind;
    fn backend(&self) -> TransformerBackend;

    /// Relative cost. Dijkstra's shortest-path picks the path with
    /// the smallest sum. Native transcoders should be ~1; ffmpeg
    /// subprocess ~10 so native wins when both exist.
    fn cost_hint(&self) -> u32 {
        1
    }

    /// Perform the conversion. `input.kind()` MUST equal
    /// [`Self::source_kind`]; the returned buffer's `kind()` MUST
    /// equal [`Self::sink_kind`]. The pipeline verifies this
    /// invariant to catch registry drift.
    fn transform(
        &self,
        input: &ZeroCopyBuffer,
    ) -> Result<ZeroCopyBuffer, OtsError>;
}

// ── Registry ────────────────────────────────────────────────────────

/// Directed multi-graph of transformers. Lookup returns a cheapest
/// path (Dijkstra). Safe for concurrent reads after construction;
/// writes happen once at startup.
pub struct TransformerRegistry {
    // Each source kind maps to the transformers that ACCEPT it.
    edges: HashMap<BufferKind, Vec<Arc<dyn Transformer>>>,
}

impl TransformerRegistry {
    pub fn new() -> Self {
        TransformerRegistry {
            edges: HashMap::new(),
        }
    }

    pub fn install(&mut self, transformer: Arc<dyn Transformer>) {
        let src = transformer.source_kind();
        self.edges
            .entry(src)
            .or_default()
            .push(transformer);
    }

    pub fn transformers_from(
        &self,
        kind: &BufferKind,
    ) -> &[Arc<dyn Transformer>] {
        self.edges.get(kind).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Return the cheapest sequence of transformers that converts
    /// `from → to`, or [`OtsError::NoPath`] if none exists. An
    /// identity path (`from == to`) returns an empty `Vec`.
    pub fn shortest_path(
        &self,
        from: &BufferKind,
        to: &BufferKind,
    ) -> Result<Vec<Arc<dyn Transformer>>, OtsError> {
        if from == to {
            return Ok(Vec::new());
        }

        // Dijkstra: state = (cost, kind); parent map lets us
        // reconstruct the path once the sink is popped.
        let mut best_cost: HashMap<BufferKind, u32> = HashMap::new();
        let mut parent: HashMap<BufferKind, (BufferKind, Arc<dyn Transformer>)> =
            HashMap::new();
        let mut heap: BinaryHeap<std::cmp::Reverse<(u32, BufferKind)>> =
            BinaryHeap::new();

        best_cost.insert(from.clone(), 0);
        heap.push(std::cmp::Reverse((0, from.clone())));

        while let Some(std::cmp::Reverse((cost, kind))) = heap.pop() {
            if &kind == to {
                // Reconstruct path.
                let mut path = Vec::new();
                let mut cur = kind;
                while let Some((prev, t)) = parent.remove(&cur) {
                    path.push(t);
                    cur = prev;
                }
                path.reverse();
                return Ok(path);
            }
            if cost > *best_cost.get(&kind).unwrap_or(&u32::MAX) {
                continue;
            }
            for t in self.transformers_from(&kind) {
                let next = t.sink_kind();
                let new_cost = cost.saturating_add(t.cost_hint());
                let existing = best_cost.get(&next).copied().unwrap_or(u32::MAX);
                if new_cost < existing {
                    best_cost.insert(next.clone(), new_cost);
                    parent
                        .insert(next.clone(), (kind.clone(), Arc::clone(t)));
                    heap.push(std::cmp::Reverse((new_cost, next)));
                }
            }
        }

        Err(OtsError::NoPath {
            from: from.clone(),
            to: to.clone(),
        })
    }

    pub fn has_path(&self, from: &BufferKind, to: &BufferKind) -> bool {
        self.shortest_path(from, to).is_ok()
    }

    /// List every source kind known to the registry. Sorted for
    /// stable diagnostic output.
    pub fn known_sources(&self) -> Vec<BufferKind> {
        let mut v: Vec<BufferKind> = self.edges.keys().cloned().collect();
        v.sort_by(|a, b| a.slug().cmp(b.slug()));
        v
    }
}

impl Default for TransformerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ── Pipeline (executable chain) ─────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PipelineStep {
    pub id: TransformerId,
    pub backend: TransformerBackend,
}

/// Executable chain. Clone is cheap (Arcs all the way down).
#[derive(Clone)]
pub struct Pipeline {
    steps: Vec<Arc<dyn Transformer>>,
}

impl Pipeline {
    pub fn from_registry(
        registry: &TransformerRegistry,
        from: &BufferKind,
        to: &BufferKind,
    ) -> Result<Self, OtsError> {
        let steps = registry.shortest_path(from, to)?;
        Ok(Pipeline { steps })
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    pub fn steps(&self) -> Vec<PipelineStep> {
        self.steps
            .iter()
            .map(|t| PipelineStep {
                id: TransformerId::new(&t.source_kind(), &t.sink_kind()),
                backend: t.backend(),
            })
            .collect()
    }

    /// True when any step uses a subprocess backend. Used by the
    /// type checker to reject `sensitive + HIPAA + ffmpeg`
    /// combinations.
    pub fn crosses_process_boundary(&self) -> bool {
        self.steps
            .iter()
            .any(|t| t.backend() == TransformerBackend::Subprocess)
    }

    /// Run every step in order. Each step's output is the next
    /// step's input; kind mismatches raise [`OtsError::KindMismatch`].
    pub fn execute(
        &self,
        input: &ZeroCopyBuffer,
    ) -> Result<ZeroCopyBuffer, OtsError> {
        let mut current = input.clone();
        for step in &self.steps {
            let expected = step.source_kind();
            if current.kind() != expected {
                return Err(OtsError::KindMismatch {
                    expected,
                    actual: current.kind(),
                });
            }
            current = step.transform(&current)?;
        }
        Ok(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Identity-ish transformer for path-search tests: converts
    // between named kinds with a fixed cost, copying bytes.
    struct Identity {
        from: BufferKind,
        to: BufferKind,
        cost: u32,
        backend: TransformerBackend,
    }

    impl Transformer for Identity {
        fn source_kind(&self) -> BufferKind {
            self.from.clone()
        }
        fn sink_kind(&self) -> BufferKind {
            self.to.clone()
        }
        fn backend(&self) -> TransformerBackend {
            self.backend
        }
        fn cost_hint(&self) -> u32 {
            self.cost
        }
        fn transform(
            &self,
            input: &ZeroCopyBuffer,
        ) -> Result<ZeroCopyBuffer, OtsError> {
            Ok(input.retag(self.to.clone()))
        }
    }

    fn ident(
        from: &str,
        to: &str,
        cost: u32,
        backend: TransformerBackend,
    ) -> Arc<dyn Transformer> {
        Arc::new(Identity {
            from: BufferKind::new(from),
            to: BufferKind::new(to),
            cost,
            backend,
        })
    }

    #[test]
    fn identity_path_is_empty() {
        let reg = TransformerRegistry::new();
        let a = BufferKind::new("a");
        let path = reg.shortest_path(&a, &a).unwrap();
        assert!(path.is_empty());
    }

    #[test]
    fn single_edge_path() {
        let mut reg = TransformerRegistry::new();
        reg.install(ident("a", "b", 1, TransformerBackend::Native));
        let from = BufferKind::new("a");
        let to = BufferKind::new("b");
        let path = reg.shortest_path(&from, &to).unwrap();
        assert_eq!(path.len(), 1);
    }

    #[test]
    fn multi_edge_picks_lowest_cost_path() {
        let mut reg = TransformerRegistry::new();
        // Two disjoint paths a → c:
        //   1. a → b → c   (cost 1 + 1 = 2, native)
        //   2. a → c       (cost 10, ffmpeg)
        reg.install(ident("a", "b", 1, TransformerBackend::Native));
        reg.install(ident("b", "c", 1, TransformerBackend::Native));
        reg.install(ident("a", "c", 10, TransformerBackend::Subprocess));

        let from = BufferKind::new("a");
        let to = BufferKind::new("c");
        let path = reg.shortest_path(&from, &to).unwrap();
        assert_eq!(path.len(), 2, "cheaper 2-hop native path must win");
        assert_eq!(
            path[0].backend(),
            TransformerBackend::Native
        );
        assert_eq!(
            path[1].backend(),
            TransformerBackend::Native
        );
    }

    #[test]
    fn no_path_returns_typed_error() {
        let mut reg = TransformerRegistry::new();
        reg.install(ident("a", "b", 1, TransformerBackend::Native));
        let from = BufferKind::new("a");
        let to = BufferKind::new("z");
        // v1.4.2 — `.unwrap_err()` requires the Ok variant to be
        // `Debug`, but the Ok type here is `Vec<Arc<dyn Transformer>>`
        // and `dyn Transformer` is not `Debug`. `.err().expect(...)`
        // drops the Ok variant before unwrapping and needs no bound.
        let err = reg
            .shortest_path(&from, &to)
            .err()
            .expect("expected NoPath error");
        matches!(err, OtsError::NoPath { .. });
    }

    #[test]
    fn pipeline_crosses_process_boundary_flag() {
        let mut reg = TransformerRegistry::new();
        reg.install(ident("a", "b", 10, TransformerBackend::Subprocess));

        let from = BufferKind::new("a");
        let to = BufferKind::new("b");
        let p = Pipeline::from_registry(&reg, &from, &to).unwrap();
        assert!(p.crosses_process_boundary());
    }

    #[test]
    fn pipeline_native_only_does_not_cross_boundary() {
        let mut reg = TransformerRegistry::new();
        reg.install(ident("a", "b", 1, TransformerBackend::Native));
        reg.install(ident("b", "c", 1, TransformerBackend::Native));

        let from = BufferKind::new("a");
        let to = BufferKind::new("c");
        let p = Pipeline::from_registry(&reg, &from, &to).unwrap();
        assert!(!p.crosses_process_boundary());
    }

    #[test]
    fn execute_runs_every_step() {
        let mut reg = TransformerRegistry::new();
        reg.install(ident("a", "b", 1, TransformerBackend::Native));
        reg.install(ident("b", "c", 1, TransformerBackend::Native));

        let from = BufferKind::new("a");
        let to = BufferKind::new("c");
        let p = Pipeline::from_registry(&reg, &from, &to).unwrap();

        let input = ZeroCopyBuffer::from_bytes(
            vec![1, 2, 3],
            BufferKind::new("a"),
        );
        let out = p.execute(&input).unwrap();
        assert_eq!(out.kind().slug(), "c");
        // Identity transformer keeps bytes intact.
        assert_eq!(out.as_slice(), &[1, 2, 3]);
    }

    #[test]
    fn execute_detects_kind_mismatch_on_wrong_input() {
        let mut reg = TransformerRegistry::new();
        reg.install(ident("a", "b", 1, TransformerBackend::Native));

        let from = BufferKind::new("a");
        let to = BufferKind::new("b");
        let p = Pipeline::from_registry(&reg, &from, &to).unwrap();

        let wrong_input = ZeroCopyBuffer::from_bytes(
            vec![1],
            BufferKind::new("wrong"),
        );
        let err = p.execute(&wrong_input).unwrap_err();
        matches!(err, OtsError::KindMismatch { .. });
    }
}
