//! ffmpeg subprocess wrapper with a warm-process pool.
//!
//! ffmpeg is the fallback for transformations OTS can't handle
//! natively (arbitrary codec combos, format containers, colour-space
//! conversions). The wrapper:
//!
//! 1. Detects ffmpeg at startup — absence is NOT fatal. OTS falls
//!    back to native paths when they exist; flows that need ffmpeg
//!    but can't find it emit `ots:capability_degraded` on first
//!    pipeline synthesis and compile-time warnings on the checker.
//! 2. Maintains a TTL-bounded pool of warm processes keyed by the
//!    pipeline signature (`source_kind → sink_kind` + flag set).
//!    First call pays the spawn cost; subsequent calls reuse within
//!    the TTL.
//! 3. Never returns plaintext of the payload on stderr; adopter
//!    `RUST_LOG` levels control ffmpeg's own verbosity.
//!
//! This module ships the pool + detection + executor plumbing.
//! Concrete `Transformer` implementations that delegate to ffmpeg
//! are adopter-side — they pick the exact ffmpeg args (e.g.
//! `-f s16le -ar 16000 -ac 1 ...`) that match their kind taxonomy.

use std::collections::HashMap;
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::buffer::{BufferKind, ZeroCopyBuffer};
use crate::ots::pipeline::{OtsError, Transformer, TransformerBackend};

// ── Detection ───────────────────────────────────────────────────────

/// Probe once at startup; cache the result for the process lifetime.
/// Adopters who want to re-detect (e.g. after a container upgrade
/// installed ffmpeg) restart the process.
pub fn is_ffmpeg_available() -> bool {
    use std::sync::OnceLock;
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        Command::new("ffmpeg")
            .arg("-version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}

// ── Pipeline description ────────────────────────────────────────────

/// Concrete ffmpeg invocation — codec-pair signature + argv.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FfmpegPipeline {
    pub from: BufferKind,
    pub to: BufferKind,
    /// argv AFTER the `ffmpeg` executable; the wrapper prepends
    /// `-y -hide_banner -loglevel error` for reproducible stderr.
    pub argv: Vec<String>,
}

impl FfmpegPipeline {
    pub fn new(
        from: BufferKind,
        to: BufferKind,
        argv: impl IntoIterator<Item = String>,
    ) -> Self {
        FfmpegPipeline {
            from,
            to,
            argv: argv.into_iter().collect(),
        }
    }
}

// ── Warm pool ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FfmpegPoolConfig {
    /// How long a pipeline entry stays warm after its last use.
    pub ttl: Duration,
    /// Cap total entries so a long-running process doesn't
    /// accumulate unbounded pipelines.
    pub max_entries: usize,
}

impl Default for FfmpegPoolConfig {
    fn default() -> Self {
        FfmpegPoolConfig {
            ttl: Duration::from_secs(60),
            max_entries: 32,
        }
    }
}

#[derive(Debug, Clone)]
struct PoolEntry {
    /// Stored for the warm-reuse read-back leg, which has NOT landed: today
    /// `register` + TTL eviction run but nothing fetches a cached pipeline
    /// back (named dead wire — v2.67.0 discipline, never silently deleted).
    #[allow(dead_code)]
    pipeline: FfmpegPipeline,
    last_used: Instant,
    /// Cumulative invocation count for observability.
    hits: u64,
}

/// TTL-bounded warm cache. The wrapper doesn't keep the ffmpeg
/// process alive across calls (we spawn per-call today) — the
/// pool's value is in caching the resolved pipeline descriptor
/// + metrics, which is where the meaningful cost sits for small
/// audio chunks. A follow-up revision can upgrade to a pipe-in /
/// pipe-out long-running ffmpeg worker without changing this API.
pub struct FfmpegPool {
    entries: Mutex<HashMap<String, PoolEntry>>,
    config: FfmpegPoolConfig,
}

impl FfmpegPool {
    pub fn new(config: FfmpegPoolConfig) -> Self {
        FfmpegPool {
            entries: Mutex::new(HashMap::new()),
            config,
        }
    }

    pub fn register(&self, pipeline: FfmpegPipeline) {
        if !is_ffmpeg_available() {
            return;
        }
        let key = Self::key_for(&pipeline);
        let mut guard = self.entries.lock().expect("pool poisoned");
        self.evict_stale(&mut guard);
        if guard.len() >= self.config.max_entries {
            return;
        }
        guard.insert(
            key,
            PoolEntry {
                pipeline,
                last_used: Instant::now(),
                hits: 0,
            },
        );
    }

    /// Execute ffmpeg for this pipeline. Spawns per-call today;
    /// future revision upgrades to a long-running pipe-in worker.
    pub fn execute(
        &self,
        pipeline: &FfmpegPipeline,
        payload: &[u8],
    ) -> Result<Vec<u8>, OtsError> {
        if !is_ffmpeg_available() {
            return Err(OtsError::TransformFailed(
                "ffmpeg not available on this host; register a native \
                 transformer or install ffmpeg to unlock subprocess \
                 paths"
                    .into(),
            ));
        }

        // Update pool stats (best-effort; contention shouldn't block).
        if let Ok(mut guard) = self.entries.lock() {
            let key = Self::key_for(pipeline);
            let entry = guard.entry(key).or_insert_with(|| PoolEntry {
                pipeline: pipeline.clone(),
                last_used: Instant::now(),
                hits: 0,
            });
            entry.last_used = Instant::now();
            entry.hits += 1;
        }

        let mut args: Vec<String> = vec![
            "-y".into(),
            "-hide_banner".into(),
            "-loglevel".into(),
            "error".into(),
        ];
        args.extend(pipeline.argv.iter().cloned());

        use std::io::Write;
        use std::process::Stdio;
        let mut child = Command::new("ffmpeg")
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                OtsError::TransformFailed(format!("ffmpeg spawn: {e}"))
            })?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(payload).map_err(|e| {
                OtsError::TransformFailed(format!("ffmpeg stdin: {e}"))
            })?;
        }
        let output = child.wait_with_output().map_err(|e| {
            OtsError::TransformFailed(format!("ffmpeg wait: {e}"))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(OtsError::TransformFailed(format!(
                "ffmpeg exited {:?}: {stderr}",
                output.status.code()
            )));
        }
        Ok(output.stdout)
    }

    pub fn snapshot(&self) -> Vec<(String, u64, Duration)> {
        let guard = self.entries.lock().expect("pool poisoned");
        let now = Instant::now();
        guard
            .iter()
            .map(|(k, e)| {
                let age = now.duration_since(e.last_used);
                (k.clone(), e.hits, age)
            })
            .collect()
    }

    fn evict_stale(&self, entries: &mut HashMap<String, PoolEntry>) {
        let now = Instant::now();
        entries.retain(|_, e| now.duration_since(e.last_used) < self.config.ttl);
    }

    fn key_for(pipeline: &FfmpegPipeline) -> String {
        format!(
            "{}->{}|{}",
            pipeline.from,
            pipeline.to,
            pipeline.argv.join(" ")
        )
    }
}

impl Default for FfmpegPool {
    fn default() -> Self {
        FfmpegPool::new(FfmpegPoolConfig::default())
    }
}

// ── Generic subprocess transformer ──────────────────────────────────

/// Transformer that routes through the shared pool. Adopters
/// create one per `FfmpegPipeline` they register.
pub struct FfmpegTransformer {
    pub pipeline: FfmpegPipeline,
    pub pool: std::sync::Arc<FfmpegPool>,
    pub cost_hint: u32,
}

impl Transformer for FfmpegTransformer {
    fn source_kind(&self) -> BufferKind {
        self.pipeline.from.clone()
    }

    fn sink_kind(&self) -> BufferKind {
        self.pipeline.to.clone()
    }

    fn backend(&self) -> TransformerBackend {
        TransformerBackend::Subprocess
    }

    fn cost_hint(&self) -> u32 {
        self.cost_hint
    }

    fn transform(
        &self,
        input: &ZeroCopyBuffer,
    ) -> Result<ZeroCopyBuffer, OtsError> {
        let out_bytes = self.pool.execute(&self.pipeline, input.as_slice())?;
        let mut buf =
            ZeroCopyBuffer::from_bytes(out_bytes, self.sink_kind());
        if let Some(tenant) = input.tenant_id() {
            buf = buf.with_tenant(tenant.to_string());
        }
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_registers_without_crashing_when_ffmpeg_missing() {
        // Regression: calling register() on a host without ffmpeg
        // should be a no-op, not a panic.
        let pool = FfmpegPool::default();
        pool.register(FfmpegPipeline::new(
            BufferKind::new("custom"),
            BufferKind::new("other"),
            std::iter::empty(),
        ));
        // Snapshot is either empty (no ffmpeg) or contains the
        // registration; either is legal.
        let _ = pool.snapshot();
    }

    #[test]
    fn key_for_is_deterministic() {
        let p = FfmpegPipeline::new(
            BufferKind::new("a"),
            BufferKind::new("b"),
            ["-f".into(), "s16le".into()],
        );
        let k1 = FfmpegPool::key_for(&p);
        let k2 = FfmpegPool::key_for(&p);
        assert_eq!(k1, k2);
    }

    #[test]
    fn execute_returns_error_when_ffmpeg_missing() {
        // On a CI runner without ffmpeg the execute() path errors
        // instead of crashing. We skip the assertion when ffmpeg is
        // actually available (the call path then requires valid
        // payload + args, which we don't want to synthesise in unit
        // tests).
        if is_ffmpeg_available() {
            return;
        }
        let pool = FfmpegPool::default();
        let pipeline = FfmpegPipeline::new(
            BufferKind::new("a"),
            BufferKind::new("b"),
            std::iter::empty(),
        );
        let err = pool.execute(&pipeline, b"nothing").unwrap_err();
        matches!(err, OtsError::TransformFailed(_));
    }

    #[test]
    fn transformer_backend_is_subprocess() {
        let pool = std::sync::Arc::new(FfmpegPool::default());
        let t = FfmpegTransformer {
            pipeline: FfmpegPipeline::new(
                BufferKind::new("a"),
                BufferKind::new("b"),
                std::iter::empty(),
            ),
            pool,
            cost_hint: 10,
        };
        assert_eq!(t.backend(), TransformerBackend::Subprocess);
        assert_eq!(t.cost_hint(), 10);
    }
}
