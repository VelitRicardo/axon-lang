//! v2.42.0 — the `TopologyBackend` port + the OSS reference TDA engine.
//!
//! The savant maps the *shape* of the ingested corpus so it can steer its
//! research toward epistemic gaps rather than toward semantic look-alikes (the
//! failure mode of vector-similarity RAG — paper section 4). It approximates the corpus
//! as a Vietoris–Rips simplicial complex and reads its homology.
//!
//! The OSS reference computes the low-dimensional Betti numbers exactly over the
//! 1-skeleton (graph):
//!   - `β₀` = connected components (union-find) — the number of disjoint
//!     knowledge islands.
//!   - `β₁` = the cyclomatic number `E − V + β₀` — independent 1-cycles (loops
//!     of association with a "hole" in the middle).
//!   - a **cycle-participation centrality**: per vertex, the number of incident
//!     edges that lie on a cycle (i.e. survive 2-core reduction). This is the
//! reference proxy for Persistent-Homology Centrality (PHC, paper section 4.2).
//!
//! Honest bound: the full persistence pairing across a filtration and the `β₂`
//! (void) analysis the paper leans on require the boundary-matrix reduction of
//! the enterprise engine (v2.42.0); the OSS reference is exact for `β₀`/`β₁` on the
//! graph and is the differential-test oracle. No advantage is claimed (v2.23.0) —
//! these are exact combinatorial invariants.

/// The low-dimensional Betti numbers of a graph (1-skeleton).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BettiNumbers {
    /// `β₀` — connected components.
    pub b0: usize,
    /// `β₁` — independent 1-cycles (`E − V + β₀`).
    pub b1: usize,
}

/// Euclidean distance between two equal-length points.
pub fn euclidean(a: &[f64], b: &[f64]) -> f64 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x - y) * (x - y))
        .sum::<f64>()
        .sqrt()
}

/// The Vietoris–Rips 1-skeleton at scale `threshold`: an edge for every pair of
/// points within `threshold`. Returns `(i, j)` with `i < j`.
pub fn vietoris_rips_edges(points: &[Vec<f64>], threshold: f64) -> Vec<(usize, usize)> {
    let mut edges = Vec::new();
    for i in 0..points.len() {
        for j in (i + 1)..points.len() {
            if euclidean(&points[i], &points[j]) <= threshold {
                edges.push((i, j));
            }
        }
    }
    edges
}

// ── Union-Find (for β₀) ──────────────────────────────────────────────────────

struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        UnionFind {
            parent: (0..n).collect(),
        }
    }
    fn find(&mut self, x: usize) -> usize {
        let mut root = x;
        while self.parent[root] != root {
            root = self.parent[root];
        }
        // Path compression.
        let mut cur = x;
        while self.parent[cur] != root {
            let next = self.parent[cur];
            self.parent[cur] = root;
            cur = next;
        }
        root
    }
    fn union(&mut self, a: usize, b: usize) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.parent[ra] = rb;
        }
    }
    fn components(&mut self, n: usize) -> usize {
        let mut roots = std::collections::HashSet::new();
        for i in 0..n {
            let r = self.find(i);
            roots.insert(r);
        }
        roots.len()
    }
}

/// The TDA port (charter split R1). Enterprise mounts the full persistent-
/// homology engine (β₂ voids, persistence pairing) behind this trait (v2.42.0).
pub trait TopologyBackend {
    /// `β₀`/`β₁` of the graph on `n_vertices` with the given edges.
    fn betti(&self, n_vertices: usize, edges: &[(usize, usize)]) -> BettiNumbers;
    /// Per-vertex cycle-participation centrality (PHC proxy): how many incident
    /// edges lie on a cycle.
    fn cycle_centrality(&self, n_vertices: usize, edges: &[(usize, usize)]) -> Vec<usize>;
}

/// The OSS reference: exact `β₀`/`β₁` over the 1-skeleton + a 2-core cycle-
/// participation centrality.
pub struct ReferenceTopology;

impl TopologyBackend for ReferenceTopology {
    fn betti(&self, n_vertices: usize, edges: &[(usize, usize)]) -> BettiNumbers {
        let mut uf = UnionFind::new(n_vertices.max(1));
        for &(a, b) in edges {
            uf.union(a, b);
        }
        let b0 = if n_vertices == 0 {
            0
        } else {
            uf.components(n_vertices)
        };
        // Cyclomatic number: E − V + β₀ (independent cycles of the graph).
        let e = edges.len() as isize;
        let v = n_vertices as isize;
        let b1 = (e - v + b0 as isize).max(0) as usize;
        BettiNumbers { b0, b1 }
    }

    fn cycle_centrality(&self, n_vertices: usize, edges: &[(usize, usize)]) -> Vec<usize> {
        // 2-core: iteratively strip vertices of degree < 2. The surviving edges
        // are exactly the cycle edges; a vertex's centrality is its degree in
        // that core.
        let mut degree = vec![0usize; n_vertices];
        let mut alive_edge = vec![true; edges.len()];
        let mut removed = vec![false; n_vertices];
        for &(a, b) in edges {
            degree[a] += 1;
            degree[b] += 1;
        }
        // Queue of vertices to peel.
        let mut queue: Vec<usize> = (0..n_vertices).filter(|&v| degree[v] < 2).collect();
        while let Some(v) = queue.pop() {
            if removed[v] {
                continue;
            }
            removed[v] = true;
            for (ei, &(a, b)) in edges.iter().enumerate() {
                if !alive_edge[ei] {
                    continue;
                }
                if a == v || b == v {
                    alive_edge[ei] = false;
                    let other = if a == v { b } else { a };
                    if !removed[other] && degree[other] > 0 {
                        degree[other] -= 1;
                        if degree[other] < 2 {
                            queue.push(other);
                        }
                    }
                }
            }
        }
        // Centrality = surviving (cycle) edges incident to each vertex.
        let mut centrality = vec![0usize; n_vertices];
        for (ei, &(a, b)) in edges.iter().enumerate() {
            if alive_edge[ei] {
                centrality[a] += 1;
                centrality[b] += 1;
            }
        }
        centrality
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triangle_has_one_component_one_cycle() {
        let t = ReferenceTopology;
        let edges = vec![(0, 1), (1, 2), (0, 2)];
        assert_eq!(t.betti(3, &edges), BettiNumbers { b0: 1, b1: 1 });
        // Every vertex sits on the single cycle → 2 cycle-edges each.
        assert_eq!(t.cycle_centrality(3, &edges), vec![2, 2, 2]);
    }

    #[test]
    fn two_disjoint_edges() {
        let t = ReferenceTopology;
        let edges = vec![(0, 1), (2, 3)];
        assert_eq!(t.betti(4, &edges), BettiNumbers { b0: 2, b1: 0 });
        assert_eq!(t.cycle_centrality(4, &edges), vec![0, 0, 0, 0]);
    }

    #[test]
    fn square_is_one_cycle() {
        let t = ReferenceTopology;
        let edges = vec![(0, 1), (1, 2), (2, 3), (3, 0)];
        assert_eq!(t.betti(4, &edges), BettiNumbers { b0: 1, b1: 1 });
        assert_eq!(t.cycle_centrality(4, &edges), vec![2, 2, 2, 2]);
    }

    #[test]
    fn path_has_no_cycle() {
        let t = ReferenceTopology;
        let edges = vec![(0, 1), (1, 2), (2, 3)];
        assert_eq!(t.betti(4, &edges), BettiNumbers { b0: 1, b1: 0 });
        assert_eq!(t.cycle_centrality(4, &edges), vec![0, 0, 0, 0]);
    }

    #[test]
    fn tadpole_isolates_the_cycle() {
        // A triangle (0,1,2) with a tail 2-3-4. Only the triangle vertices carry
        // cycle centrality; the tail is peeled by the 2-core.
        let t = ReferenceTopology;
        let edges = vec![(0, 1), (1, 2), (0, 2), (2, 3), (3, 4)];
        assert_eq!(t.betti(5, &edges), BettiNumbers { b0: 1, b1: 1 });
        let c = t.cycle_centrality(5, &edges);
        assert_eq!(c[0], 2);
        assert_eq!(c[1], 2);
        assert_eq!(c[2], 2);
        assert_eq!(c[3], 0);
        assert_eq!(c[4], 0);
    }

    #[test]
    fn isolated_points_are_their_own_components() {
        let t = ReferenceTopology;
        assert_eq!(t.betti(5, &[]), BettiNumbers { b0: 5, b1: 0 });
    }

    #[test]
    fn vietoris_rips_gates_on_threshold() {
        let pts = vec![vec![0.0, 0.0], vec![0.5, 0.0], vec![10.0, 10.0]];
        // Small threshold: only the close pair connects → 2 components.
        let near = vietoris_rips_edges(&pts, 1.0);
        assert_eq!(near, vec![(0, 1)]);
        assert_eq!(ReferenceTopology.betti(3, &near).b0, 2);
        // Large threshold: everything connects.
        let far = vietoris_rips_edges(&pts, 100.0);
        assert_eq!(far.len(), 3);
        assert_eq!(ReferenceTopology.betti(3, &far).b0, 1);
    }
}
