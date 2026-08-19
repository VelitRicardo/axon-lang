---
name: widget_ephemeral_credential
title: Ephemeral widget credential — mint an attenuated, TTL-bounded bearer
summary: "`credential` + `mint` (v2.46.0): the chat-widget-on-any-origin shape. The bootstrap flow mints a 15-minute bearer carrying exactly [chat.invoke] (grants ⊆ minter, `authority_only_attenuates`); the widget presents it against a `requires:`-gated endpoint behind a wildcard-origin `cors` policy. The token is shown once — persisting it is a compile error (axon-T896)."
topic: endpoints
primitives:
  - credential
  - flow
  - axonendpoint
  - cors
---

// The canonical SaaS embed: a chat widget served into ANY third-party
// origin. The widget needs identity — a SLICE of the backend's authority,
// briefly. `credential` declares the slice; `mint` hands it down under the
// attenuation law (grants ⊆ capabilities(minter), fail-closed).

type BootPayload { token_hint: String }

credential WidgetSession {
    ttl:    15m
    grants: [chat.invoke]
}

flow BootstrapWidget() -> BootPayload {
    // The raw bearer binds to `tok` — shown once. Persisting it is
    // axon-T896; the wire audit carries a summary, never the token.
    mint WidgetSession as tok
    step Compose {
        ask: "Compose the widget bootstrap payload carrying ${tok}."
        output: BootPayload
    }
    return Compose.output
}

// Token-in-header + wildcard origin (no credentials) is the spec-correct
// widget pairing — axon-T853 forbids the unsafe combination.
cors AnyOriginWidget {
    allow_origins: ["*"]
    allow_methods: [POST]
    allow_headers: ["Content-Type", "Authorization"]
}

axonendpoint WidgetBoot {
    method:  post
    path:    "/v1/widget/boot"
    execute: BootstrapWidget
    output:  FlowEnvelope<BootPayload>
    cors:    AnyOriginWidget
    // The bootstrap itself is gated: only a principal already holding
    // chat.invoke can mint a bearer that carries it (attenuation).
    requires: [chat.invoke]
}
