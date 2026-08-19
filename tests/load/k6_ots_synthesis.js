// OTS pipeline synthesis — cold vs cached cost.
//
// Two scenarios in one script:
//   cold — unique (from, to) per request so the registry + Dijkstra
//          walk run fresh each time.
//   cached — repeats the same (from, to) so subsequent requests
//            hit the pool / path cache.
//
// Server exposes a debug endpoint /api/v1/ots/synthesize (gated by
// an admin JWT) that returns headers:
//   x-axon-ots-synth-ms: Dijkstra + instantiate cost
//   x-axon-ots-cache: "hit" | "miss"

import http from 'k6/http';
import { check } from 'k6';
import { Trend, Rate } from 'k6/metrics';

const BASE_URL = __ENV.BASE_URL || 'http://localhost:8000';
const BEARER = __ENV.BEARER || '';

const coldLatency = new Trend('ots_synth_cold_ms');
const cachedLatency = new Trend('ots_synth_cached_ms');
const errors = new Rate('ots_synth_errors');

export const options = {
    scenarios: {
        cold_paths: {
            executor: 'per-vu-iterations',
            vus: 20,
            iterations: 10,
            maxDuration: '30s',
            exec: 'cold',
        },
        cached_paths: {
            executor: 'constant-arrival-rate',
            rate: 500,
            timeUnit: '1s',
            duration: '30s',
            preAllocatedVUs: 20,
            maxVUs: 50,
            exec: 'cached',
            startTime: '30s',
        },
    },
    thresholds: {
        'ots_synth_cold_ms': ['p(95)<5', 'p(99)<10'],
        'ots_synth_cached_ms': ['p(95)<0.05', 'p(99)<0.1'],
        'ots_synth_errors': ['rate<0.005'],
    },
};

const headers = {
    'Content-Type': 'application/json',
    ...(BEARER ? { Authorization: `Bearer ${BEARER}` } : {}),
};

export function cold() {
    // Force a fresh path: pick kinds the server isn't likely to
    // have recently computed. Tests use unique (from, to) names
    // seeded with __VU so Dijkstra runs.
    const from = `synthetic_src_${__VU}_${__ITER}`;
    const to = `synthetic_dst_${__VU}_${__ITER}`;
    const res = http.post(
        `${BASE_URL}/api/v1/ots/synthesize`,
        JSON.stringify({ from, to }),
        { headers },
    );
    const synth = res.headers['X-Axon-Ots-Synth-Ms'];
    if (synth) coldLatency.add(parseFloat(synth));
    if (!check(res, { 'cold 200 or 404': (r) => r.status === 200 || r.status === 404 })) {
        errors.add(1);
    }
}

export function cached() {
    // Repeat the same (from, to) so the second+ requests are
    // cache hits.
    const res = http.post(
        `${BASE_URL}/api/v1/ots/synthesize`,
        JSON.stringify({ from: 'mulaw8', to: 'pcm16_16k' }),
        { headers },
    );
    const synth = res.headers['X-Axon-Ots-Synth-Ms'];
    if (synth) cachedLatency.add(parseFloat(synth));
    if (!check(res, { 'cached 200': (r) => r.status === 200 })) {
        errors.add(1);
    }
}
