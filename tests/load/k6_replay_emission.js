// ReplayToken emission overhead — how much does a token cost?
//
// Hits a /api/v1/flow/run endpoint that invokes one token-emitting
// effect inside a flow. The server records the ReplayToken latency
// as a server-side header (x-axon-replay-emit-ms) so we can assert
// < 2ms p99 without measuring network + flow overhead.

import http from 'k6/http';
import { check } from 'k6';
import { Trend, Rate } from 'k6/metrics';

const BASE_URL = __ENV.BASE_URL || 'http://localhost:8000';
const BEARER = __ENV.BEARER || '';

const emitLatency = new Trend('replay_token_emit_ms');
const errors = new Rate('replay_emit_errors');

export const options = {
    scenarios: {
        steady: {
            executor: 'constant-arrival-rate',
            rate: 200,
            timeUnit: '1s',
            duration: '1m',
            preAllocatedVUs: 50,
            maxVUs: 200,
        },
    },
    thresholds: {
        'replay_token_emit_ms': ['p(95)<1', 'p(99)<2'],
        'replay_emit_errors': ['rate<0.001'],
    },
};

export default function () {
    const res = http.post(
        `${BASE_URL}/api/v1/flow/run/sample_token_effect`,
        JSON.stringify({ input: { x: 1 } }),
        {
            headers: {
                'Content-Type': 'application/json',
                ...(BEARER ? { Authorization: `Bearer ${BEARER}` } : {}),
            },
        },
    );
    const emitHeader = res.headers['X-Axon-Replay-Emit-Ms'];
    if (emitHeader) {
        emitLatency.add(parseFloat(emitHeader));
    }
    if (!check(res, { '200': (r) => r.status === 200 })) {
        errors.add(1);
    }
}
