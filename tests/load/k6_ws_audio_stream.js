// WebSocket audio streaming — end-to-end SLO for the v1.4.0 stack.
//
// Exercises 11.a Stream<T> + 11.b ZeroCopyBuffer ingest + 11.e OTS
// pipeline (mulaw8 → pcm16 → pcm16_16k) all in one flow. The
// threshold assertion is on the ROUND-TRIP: client sends a μ-law
// frame, server OTS-transcodes, transcriber returns a textual
// chunk, client observes the RTT.
//
// Thresholds enforce v1.2.0 GA SLOs from
// docs/SECURITY_AUDIT_v1_2_0.md.

import ws from 'k6/ws';
import { check } from 'k6';
import { Trend, Rate } from 'k6/metrics';

const BASE_URL = __ENV.BASE_URL || 'ws://localhost:8000';
const BEARER = __ENV.BEARER || '';

const frameLatency = new Trend('audio_frame_rtt_ms');
const errors = new Rate('audio_stream_errors');

export const options = {
    scenarios: {
        steady: {
            executor: 'constant-arrival-rate',
            rate: 50,                // 50 connections per second...
            timeUnit: '1s',
            duration: '2m',
            preAllocatedVUs: 50,
            maxVUs: 200,
        },
    },
    thresholds: {
        'audio_frame_rtt_ms': ['p(95)<300', 'p(99)<500'],
        'audio_stream_errors': ['rate<0.005'],
    },
};

// Synthesise ~1s of μ-law 8 kHz audio (8000 bytes). Fixed payload
// so throughput numbers aren't skewed by random sizes.
function mulawFrame() {
    const buf = new Uint8Array(8000);
    for (let i = 0; i < buf.length; i++) buf[i] = (i * 3) & 0xFF;
    return buf.buffer;
}

export default function () {
    const frame = mulawFrame();
    const url = `${BASE_URL}/api/v1/transcribe/stream`;
    const headers = BEARER ? { Authorization: `Bearer ${BEARER}` } : {};

    const res = ws.connect(url, { headers }, function (socket) {
        const sentAt = new Map();

        socket.on('open', () => {
            const key = `${__VU}-${Date.now()}`;
            sentAt.set(key, Date.now());
            socket.sendBinary(frame);
        });

        socket.on('message', (msg) => {
            // Server echoes { frame_key, transcript } once OTS +
            // transcribe finish.
            let envelope;
            try { envelope = JSON.parse(msg); } catch (_) { return; }
            const startedAt = sentAt.get(envelope.frame_key);
            if (startedAt) {
                frameLatency.add(Date.now() - startedAt);
                sentAt.delete(envelope.frame_key);
                socket.close();
            }
        });

        socket.on('error', () => {
            errors.add(1);
            socket.close();
        });

        socket.setTimeout(() => socket.close(), 2000);
    });

    check(res, { 'socket connect 101': (r) => r && r.status === 101 });
}
