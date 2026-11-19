# Windows benchmark

## Environment and scope

Measured on Windows 11 x64 (build 26300), Ryzen 7 6800H, 16 logical processors, Rust 1.100.0 and the local WebView2 runtime. The workload intentionally bound TCP to localhost and used a simulated receiving desktop. Tests never sent real keystrokes. Numbers cover this host and build. Wi-Fi, Windows hook and injection latency, mouse-to-display delay and competitor performance require separate measurements.

## TLS and input dispatch

The ignored Rust benchmark opens the production pinned TLS session and uses the production frame reader and receiver. It discards 100 warm-up RTTs, measures 1,000 sequential Ping/Pong round trips and then measures 100 bursts of 64 ordered motion events followed by a Ping/Pong acknowledgement. That acknowledgement is processed after the motion frames. The initial handshake is a single observation.

| Workload | Samples | p50 | p95 | p99 |
| --- | ---: | ---: | ---: | ---: |
| Ping/Pong loopback RTT | 1,000 | 44.5 µs | 109.1 µs | 183.7 µs |
| 64 ordered motion events and acknowledgement | 100 | 754.9 µs | 1,336.5 µs | 1,786.8 µs |

TCP connect, pinned TLS handshake and application Ready took **3.28 ms** in this run. The burst uses the same per-frame write as the input sender but excludes hook capture and its queue. These timings provide a baseline for future changes. A latency comparison with v0.1.0 requires running the same transport harness on that baseline.

From the repository root on Windows, with the [build prerequisites](../README.md#build-from-source) available:

```powershell
npm ci
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --locked --release --features tauri/custom-protocol benchmark::tls_loopback_profile -- --ignored --nocapture
```

## Idle process footprint

Measured release-built v0.1.0 followed by the candidate, repeated twice. The app ran disconnected with a fresh temporary WebView2 profile per sample. After four seconds of warm-up, each observation covered ten seconds. CPU is the total user and kernel time of the app and descendant processes divided by elapsed time. Percentages use one logical core as the denominator. Private memory and working set are sums across the tree at sample end. Summed working set can count shared pages more than once.

| Build | Tree size | CPU median, one core | Private memory median | Summed working set median |
| --- | ---: | ---: | ---: | ---: |
| v0.1.0 | 7 | 3.82% | 164.87 MiB | 292.91 MiB |
| Post-MVP | 7 | 2.26% | 165.99 MiB | 348.86 MiB |

Only two samples were collected per build. Baseline working set ranged from 240.35 to 345.47 MiB. Candidate CPU ranged from 1.56% to 2.96% of one core. Treat these as footprint observations. Startup variation and other host activity limit causal conclusions about CPU or shared mappings. The two binaries use the same frontend bundle and WebView2 runtime, and the session queue stays idle while disconnected.

Measure independently with `Get-CimInstance Win32_Process` for the parent process tree and `Get-Process` for accumulated CPU seconds, private memory and working set. Count only samples with a stable process tree. The values above used equivalent counters from psutil 7.2.2.

## Functional verification and next measurements

- The original release passed 10 Rust tests. The branch passed 15 tests, with the benchmark run separately. Coverage includes the left-to-right and return geometry, TLS rejection, pending negotiation fairness, heartbeat correlation, queue age, and held-input release after the stream closes.
- TypeScript, Vite, rustfmt, Clippy and the optimized Windows NSIS build passed. A native WebView2 smoke test confirmed edge defaults and persistence, failed IPv4 validation, and start/stop with the receiver TCP port closing. A separate loopback web origin could not access the session snapshot.
- For a physical comparison, run both versions on the same two PCs with identical mouse polling rates, desktop resolutions, DPI settings and Wi-Fi access point. Record steady RTT and reconnect time distributions, cursor-to-display timing using an external camera, dropped click or release events, and CPU/memory during forwarding. Repeat with a lossy link and both Idle and active profiles.
- Run the candidate against v0.1.0 in each role before switching a real installation. Verify both directions, no local typing leak, emergency return, and recovery after Wi-Fi and sleep. The [physical matrix](validation.md) records the required cases.

The [upstream matrix](open-source-comparison.md) compares source and support evidence. A numeric ranking would require a common test setup.
