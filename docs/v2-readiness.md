# V2 readiness

The post-MVP patch separates session liveness from the Windows callback, caps pre-authentication work and defines a least-privilege renderer boundary. It keeps usable v1 sessions and the default right-hand primary / left-hand secondary.

| Planned feature | Current seam | Dependency before implementation | Acceptance evidence |
| --- | --- | --- | --- |
| Universal App Switcher | Rust owns OS operations separately from the WebView | Split local app enumeration and activation from capture. Define opt-in metadata scopes and a typed command schema. Avoid titles by default and respect Windows foreground restrictions. | Opt-in tests, no cross-peer metadata before grant, known behavior where activation fails. |
| Smart Handoff | `Control`, normalized geometry, receiver-held ledger | Introduce a handoff epoch and acknowledgement with explicit terminal cleanup. Add authenticated topology and readiness negotiation before multi-peer routing. | Delay/reorder simulations plus fault tests at handoff and key release. |
| Smart Edge | `Screen` and edge eligibility | Model eligible physical monitor segments, origins and DPI awareness through documented Windows monitor APIs. Add a deterministic edge policy for adjacency and deliberate intent. | Two-topology geometry fixtures and mixed-DPI hardware tests. |
| Privacy Profiles | Single session owner and local command capability | Define capability grants per peer/session and event class. Store peer identities securely in the local user profile with explicit revocation. Derive UI visibility and backend enforcement from the same policy. | Tests for deny-by-default, expiry, revocation, data retention and isolated WebViews. |
| Self-Healing Connection | Correlated liveness, queue age, retry supervision | Add explicit reconnect reasons, delay with a cap, network-change and resume signals, and discovery-assisted address changes validated by the enrolled identity. Keep deterministic clocks in tests. | Disconnect and recovery percentiles on real Windows nodes, plus tests for address changes and expired permissions. |

## Protocol and privilege evolution

V1 is identified by the existing TLS ALPN and bounded fixed-size frames. A future incompatible envelope should negotiate an explicit major version, capability intersection and limits before forwarding. A peer that supports only v1 must continue to take the v1 path. Unknown v2 controls fail closed or are skipped only if explicitly defined as optional and length-bounded. Add a session epoch before adding any unordered lane. Retire old epochs on reconnect and preserve ordered, reliable keyboard/button release semantics.

Keep input, topology control and bulk data permissioned separately. If bulk features are ever requested, keep their scheduling and limits away from live input. Use authenticated user-level IPC for any eventual daemon. Start with the current in-process boundary and measure its cost before adding a background service. Windows admin privileges and secure-desktop interaction are outside these V2 plans.

## Sequence

1. Complete the [physical Windows matrix](validation.md) and record input/failure timings.
2. Resolve dependency notice packaging for binary redistribution and test upgrade compatibility with v0.1.0.
3. Add a version/capability contract and identity lifecycle with unit and mixed-version tests.
4. Model topology and generation-aware handoff. Add features behind explicit grants after the foundations pass.

This branch delivers session, permission and reliability foundations. Implement each V2 feature after its dependencies and acceptance tests are in place.
