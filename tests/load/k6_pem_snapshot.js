// CognitiveState snapshot + restore SLO.
//
// Target: 64 KiB density matrix + belief state + short-term memory,
// end-to-end persist + restore < 50ms p99. Exercises 11.d envelope
// encryption + Postgres write + 11.d read path.

import http from 'k6/http';
import { check } from 'k6';
import { Trend, Rate } from 'k6/metrics';

const BASE_URL = __ENV.BASE_URL || 'http://localhost:8000';
const BEARER = __ENV.BEARER || '';

const persistLatency = new Trend('pem_persist_ms');
const restoreLatency = new Trend('pem_restore_ms');
const errors = new Rate('pem_errors');

export const options = {
    scenarios: {
        steady: {
            executor: 'constant-arrival-rate',
            rate: 50,
            timeUnit: '1s',
            duration: '1m',
            preAllocatedVUs: 20,
            maxVUs: 100,
        },
    },
    thresholds: {
        'pem_persist_ms': ['p(95)<30', 'p(99)<50'],
        'pem_restore_ms': ['p(95)<30', 'p(99)<50'],
        'pem_errors': ['rate<0.005'],
    },
};

function build64KiBState() {
    // ~64 KiB of density-matrix equivalent content.
    const chars = 'abcdefghijklmnopqrstuvwxyz';
    let blob = '';
    for (let i = 0; i < 64 * 1024 / chars.length; i++) blob += chars;
    return blob;
}

const headers = {
    'Content-Type': 'application/json',
    ...(BEARER ? { Authorization: `Bearer ${BEARER}` } : {}),
};

export default function () {
    const sessionId = `sess-load-${__VU}-${__ITER}`;
    const payload = build64KiBState();

    const persistStart = Date.now();
    const persisted = http.post(
        `${BASE_URL}/api/v1/pem/state/${sessionId}`,
        JSON.stringify({ state_json: payload, ttl_seconds: 900 }),
        { headers },
    );
    persistLatency.add(Date.now() - persistStart);
    if (!check(persisted, { 'persist 200': (r) => r.status === 200 })) {
        errors.add(1);
        return;
    }

    const restoreStart = Date.now();
    const restored = http.get(
        `${BASE_URL}/api/v1/pem/state/${sessionId}`,
        { headers },
    );
    restoreLatency.add(Date.now() - restoreStart);
    if (!check(restored, { 'restore 200': (r) => r.status === 200 })) {
        errors.add(1);
    }
}
