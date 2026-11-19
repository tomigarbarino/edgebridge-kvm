# Open-source comparison

Snapshot reviewed on **2026-11-18**. The comparison uses the pinned source below, release metadata and upstream issue reports. Upstream systems were inspected in source. The runtime benchmark used EdgeBridge on one Windows host, so comparisons about architecture and maintenance are qualitative. [Benchmark methodology and results](benchmark.md)

## Source and compatibility

| Project | Source and license evidence | Maintenance and Windows evidence |
| --- | --- | --- |
| EdgeBridge | v0.1.0 at `3d42a3a`, MIT. Post-MVP changes in the associated branch. | Windows x64 CI produced the published preview. One Windows 11 native UI smoke test passed. Two-PC input testing is still pending. [Release](https://github.com/tomigarbarino/edgebridge-kvm/releases/tag/v0.1.0) |
| R-ShareMouse | `d18a0d5`. Top-level file is MIT, while workspace metadata and README declare MIT OR Apache-2.0. File and dependency terms would need a separate review for adoption. [LICENSE][r-license] · [Cargo manifest][r-cargo] | Recent commits and source-level Windows checks. The repository had no GitHub Releases at review time. [Input lifecycle PR](https://github.com/a1112/R-ShareMouse/pull/17) and [peer trust PR](https://github.com/a1112/R-ShareMouse/pull/35) document active engineering work. Platform claims describe the source project. [Windows source][r-win] · [Releases][r-releases] |
| Mouse Without Borders | PowerToys `084281e`. MIT root license and module source. [LICENSE][p-license] · [Module][p-module] | PowerToys v0.102.3101 release dated 2026-11-06 includes Windows x64 and ARM64 installers. Main can be ahead of the released module. [Release][p-release] · [Official support docs][p-doc] |
| Lan Mouse | `69958c6`. Workspace GPL-3.0-or-later with the same explicit license in both input adapter manifests. [Cargo manifest][l-cargo] · [LICENSE][l-license] | v0.11.0 dated 2026-06-12. Source includes Windows capture and emulation, alongside Linux and macOS backends. Its Windows documentation notes a pointer visibility limitation without a physical mouse attached. [Release][l-release] · [README][l-readme] |
| Deskflow | `80221f0`. Central protocol, TLS and Windows hook sources use GPL-2.0-only WITH an OpenSSL exception. The license directory shows additional terms for specific components. [LICENSE][d-license] · [License inventory][d-licenses] · [Windows hook][d-hook] | v1.27.0 dated 2026-10-01 includes Windows x64 and ARM64 portable and MSI builds. Platform-specific issues remain active. [Release][d-release] · [Windows/RDP issue][d-issue] |

## Matrix

| Dimension | EdgeBridge MVP | R-ShareMouse | Mouse Without Borders | Lan Mouse | Deskflow |
| --- | --- | --- | --- | --- | --- |
| Architecture | One Tauri process, one authenticated peer, Win32 worker. | Rust workspace with separate daemon, Tauri client and platform crates. [Desktop client][r-ui] | PowerToys C# module with Windows input hooks, TCP and optional service integration. [Module][p-module] | Rust service with protocol, IPC, capture and emulation crates and GTK frontend. [Workspace][l-cargo] | C++ client/server, GUI and OS adapters. [Server][d-server] · [Client][d-client] |
| Performance | Bounded single FIFO and one TLS write per frame. No file I/O. | Distinct control, reliable input, latest-motion and bulk paths. That gives scheduling options with more queues to supervise. [Transport][r-transport] | Dedicated native input and packet paths, TCP_NODELAY. Runtime cost depends on companion processes and settings. [Sockets][p-socket] | DTLS datagrams and platform event pipelines. Mapping and backend cost varies by OS. [Connect][l-connect] | Coalesces consecutive motion while transport has more ready input. [Client proxy][d-client] |
| Latency | TCP ordering makes a lost segment delay later frames. Local loopback was measured. | QUIC lanes can isolate bulk traffic from live input. Source includes a loopback benchmark. [Transport][r-transport] · [Benchmark][r-bench] | TCP with Nagle disabled. No comparable measurements collected. [Sockets][p-socket] | UDP/DTLS avoids TCP retransmission ordering for independent datagrams. Reliable transition semantics are handled at the application layer. [Protocol][l-proto] | TCP/TLS and motion compression. No comparable measurements collected. [TLS][d-tls] · [Client proxy][d-client] |
| Security | TLS 1.3 server pin and temporary 256-bit invitation token. | QUIC with certificate fingerprints and operator approval. Source distinguishes approval from first-seen provenance. [Trust store][r-trust] · [Handshake][r-handshake] | Shared user key with an AES stream and PBKDF2 derivation visible in the implementation. Support for a service increases the privilege surface. [Encryption][p-crypto] · [Official support docs][p-doc] | DTLS certificate fingerprints and configured inbound authorization. [Listener][l-listen] | OpenSSL TLS with certificate fingerprints and peer-verification options. [TLS][d-tls] |
| Stability | Windows user-session scope and held-input cleanup. Physical and focus cases remain. | Source has platform diagnostics and tests. No released build or independent runtime measurements inspected. [Windows validation][r-validation] | Sleep/resume reconnection and RDP focus appear in upstream reports. [Sleep report][p-issue] · [Known issues][p-doc] | Release/watchdog paths exist. Reports include OS/backend-specific input issues. [Capture state][l-capture] · [Windows service report][l-issue] | Keepalive, pressed-input handling and real focus/topology limitations. [Client proxy][d-client] · [Windows/RDP issue][d-issue] |
| Input and coordinates | Relative Raw Input motion, button/scan-code hooks, one primary display rectangle. | Windows Raw Input and low-level hooks, virtual desktop mapping and SendInput. [Windows source][r-win] | Low-level hooks, virtual/primary screen bounds and conversion to a normalized 16-bit range for SendInput. [Hook][p-hook] · [Coordinates][p-machine] · [Injection][p-inject] | Separate OS backends. Windows capture uses low-level hooks and emulation uses relative SendInput. [Capture][l-win-capture] · [Emulation][l-win-emulate] | Platform-specific input and virtual desktop origins on Windows. Protocol includes absolute and relative events. [Windows desktop][d-win] · [Frame types][d-frames] |
| Reconnection | Same IPv4 and invitation while receiver remains active. Re-entry is deliberate. | Independent daemon supervision. Input messages include session epochs and reliable release. [Transport][r-transport] | Explicit reopen and reconnect paths. Sleep recovery has reported gaps. [Sockets][p-socket] · [Sleep report][p-issue] | Fingerprint-based authorization, watchdog and explicit Leave/Ack. [Listener][l-listen] · [Protocol][l-proto] | Connection supervision and keepalive alarms at client/proxy boundaries. [Client proxy][d-client] |
| Configuration | Two roles, broadcast address suggestions and manual IPv4, four edge choices. | Tauri and CLI, daemon-managed identities and layout. UDP broadcast. A legacy mDNS setting logs a warning and falls back to broadcast. [Desktop client][r-ui] · [Discovery][r-discovery] | Machine matrix, key exchange and troubleshooting actions in PowerToys settings. [Official support docs][p-doc] | GTK, CLI and TOML client placement and fingerprints. [README][l-readme] | Named screens and adjacency in server configuration. [Server config][d-config] |
| Extensibility | Transport and receiver abstractions exist. OS capture and app activation are Windows-only. | Separate platform implementations and input, clipboard, files and display pipelines. [Clipboard][r-clip] · [Files][r-files] | Module fits Windows and PowerToys integration. Porting requires a platform boundary. [Module][p-module] | Multiple capture/injection implementations behind traits and separate IPC/protocol crates. [Workspace][l-cargo] | Protocol revisions and narrow platform adapters carry compatibility requirements. [Client proxy variants][d-proxies] |
| Maintenance complexity | Small app, one peer and two roles. Win32 edge cases dominate maintenance risk. | More processes, capabilities, transports and platform privileges to support. [Workspace][r-cargo] | Additional helpers, service and Windows-specific lifecycle paths. [Module][p-module] | Testing spans display servers, OS permissions and transport state. [README backend matrix][l-readme] | Long-lived protocol versions, GUI, OpenSSL and multiple platform adapters. [Client proxy variants][d-proxies] |

## Decisions for EdgeBridge

| Opportunity | Decision | Evidence and rationale |
| --- | --- | --- |
| Sluggish admission and stale input | **Refactor our code.** Bound concurrent TLS negotiations, correlate heartbeats and close an overdue input session. | EdgeBridge's original listener covered handshake and accept in one await. Lan Mouse has explicit liveness and input release, while R-ShareMouse separates freshness from ordered control. EdgeBridge can get the failure behavior within its existing Tokio runtime. [Lan Mouse capture][l-capture] · [R-ShareMouse transport][r-transport] |
| TLS, TCP and QUIC | **Keep our implementation** using rustls/Tokio. | Two input-only peers do not need multiple traffic lanes today. QUIC and DTLS are worth prototyping if Wi-Fi loss or bulk features make head-of-line blocking measurable. Any experiment must preserve button, key and release reliability. [R-ShareMouse transport][r-transport] · [Lan Mouse protocol][l-proto] |
| Movement aggregation | **Keep exact movement frames for now.** | Deskflow's proxy compresses pending movement. EdgeBridge uses intermediate positions for edge decisions. Before coalescing, add equivalence tests around turns, crossing and button order and measure high-rate mice on LAN. [Deskflow client proxy][d-client] |
| DPI and multi-monitor geometry | **Build independently with documented Win32 APIs.** | PowerToys and Deskflow validate giving coordinate origin and extent explicit representations. An EdgeBridge implementation needs negative origins, physical adjacency and mixed DPI fixtures. Reusing C# or GPL code adds unnecessary coupling. [PowerToys coordinates][p-machine] · [Deskflow Windows desktop][d-win] |
| Renderer and eventual daemon | **Refactor with a built-in capability.** | Tauri already provides a runtime allow-list, so a new privilege service or library is unnecessary. R-ShareMouse's daemon/UI boundary is a useful reference if a measured background ownership need emerges. [Tauri capabilities](https://v2.tauri.app/security/capabilities/) · [R-ShareMouse desktop client][r-ui] |
| File transfer and clipboard | **Defer.** | R-ShareMouse includes digests, cancellation, queueing and clipboard MIME handling. Those paths expand privacy scope and could contend with input. Requirements and per-peer grants come first. [File transfer][r-files] · [Clipboard][r-clip] |

## License and attribution

This branch uses original code and the existing Rust and npm dependency locks. Upstream projects are cited as references for the original changes. R-ShareMouse and PowerToys are references, with actual file and dependency terms to be reviewed before any future reuse. Preserve the copyright and license notice for any code adopted under MIT. Apache-2.0 redistribution has its own license, notice and change-marking obligations. The Deskflow OpenSSL exception has a specific linking scope. Lan Mouse and the GPL parts of Deskflow are references for behavior and architecture only. [MIT text](../LICENSE) · [Apache-2.0 terms](https://www.apache.org/licenses/LICENSE-2.0) · [Deskflow exception][d-licenses]

Existing direct crates are `anyhow`, `base64`, `getrandom`, `rcgen`, `rustls`, `serde`, `sha2`, `subtle`, `tauri`, `tokio`, `tokio-rustls` and `windows-sys`. `tauri-build` is a build dependency. Their pinned versions and transitive packages are in [Cargo.lock](../src-tauri/Cargo.lock). The frontend uses React, React DOM, Tauri API and Tailwind CSS with TypeScript, Vite and the Tauri CLI for development and packaging. The complete npm inventory is in [package-lock.json](../package-lock.json).

For binary redistribution, derive the target-specific dependency set and verify actual packaged components. Preserve MIT/Apache/BSD/ISC notices and all ring files (`LICENSE`, `LICENSE-BoringSSL`, `LICENSE-other-bits`) relevant to included code. The Windows normal-edge graph also contains existing MPL-2.0 packages such as `option-ext`, `cssparser` and `selectors`. That graph also includes compile-time proc macros. A binary attribution review must establish which crates ship. Where MPL-covered code ships in executable form, preserve its notices and tell recipients how to obtain the corresponding covered source. Retain the Unicode data notice where included. Audit WebView2 bootstrapper/runtime terms independently from the EdgeBridge MIT license. The post-MVP installer was rebuilt for local verification. Verify the notice package before a follow-up binary release. [MPL-2.0 section 3.2](https://www.mozilla.org/en-US/MPL/2.0/) · [Tauri Windows bundling](https://v2.tauri.app/distribute/windows-installer/)

To inspect the Windows graph, run `cargo tree --manifest-path src-tauri/Cargo.toml --locked --edges normal --target x86_64-pc-windows-msvc --format "{p} | {l}"`. For npm, `npm ls --all --package-lock-only` shows the locked package graph. Both outputs include upstream metadata and require checking actual notices before linking or redistributing.

[r-license]: https://github.com/a1112/R-ShareMouse/blob/d18a0d5d8d1dfa7d5356267b119307ddc0725146/LICENSE
[r-cargo]: https://github.com/a1112/R-ShareMouse/blob/d18a0d5d8d1dfa7d5356267b119307ddc0725146/Cargo.toml
[r-win]: https://github.com/a1112/R-ShareMouse/blob/d18a0d5d8d1dfa7d5356267b119307ddc0725146/crates/rshare-platform/src/windows.rs#L1665-L1718
[r-ui]: https://github.com/a1112/R-ShareMouse/blob/d18a0d5d8d1dfa7d5356267b119307ddc0725146/apps/rshare-desktop/src-tauri/src/main.rs#L280-L316
[r-transport]: https://github.com/a1112/R-ShareMouse/blob/d18a0d5d8d1dfa7d5356267b119307ddc0725146/crates/rshare-net/src/transport.rs
[r-trust]: https://github.com/a1112/R-ShareMouse/blob/d18a0d5d8d1dfa7d5356267b119307ddc0725146/crates/rshare-net/src/encryption.rs#L208-L288
[r-handshake]: https://github.com/a1112/R-ShareMouse/blob/d18a0d5d8d1dfa7d5356267b119307ddc0725146/crates/rshare-net/src/handshake.rs
[r-files]: https://github.com/a1112/R-ShareMouse/blob/d18a0d5d8d1dfa7d5356267b119307ddc0725146/crates/rshare-net/src/file_transfer.rs
[r-clip]: https://github.com/a1112/R-ShareMouse/blob/d18a0d5d8d1dfa7d5356267b119307ddc0725146/crates/rshare-core/src/engine/clipboard.rs
[r-discovery]: https://github.com/a1112/R-ShareMouse/blob/d18a0d5d8d1dfa7d5356267b119307ddc0725146/crates/rshare-net/src/discovery.rs#L228-L249
[r-bench]: https://github.com/a1112/R-ShareMouse/blob/d18a0d5d8d1dfa7d5356267b119307ddc0725146/crates/rshare-net/benches/quic_loopback.rs
[r-validation]: https://github.com/a1112/R-ShareMouse/blob/d18a0d5d8d1dfa7d5356267b119307ddc0725146/docs/performance/phase1-windows-validation.md
[r-releases]: https://github.com/a1112/R-ShareMouse/releases
[p-license]: https://github.com/microsoft/PowerToys/blob/084281ef6b137aa47a1e6a8e1e7bf79b0fe0378e/LICENSE
[p-module]: https://github.com/microsoft/PowerToys/tree/084281ef6b137aa47a1e6a8e1e7bf79b0fe0378e/src/modules/MouseWithoutBorders
[p-release]: https://github.com/microsoft/PowerToys/releases/tag/v0.102.3101
[p-doc]: https://learn.microsoft.com/windows/powertoys/mouse-without-borders
[p-issue]: https://github.com/microsoft/PowerToys/issues/50583
[p-hook]: https://github.com/microsoft/PowerToys/blob/084281ef6b137aa47a1e6a8e1e7bf79b0fe0378e/src/modules/MouseWithoutBorders/App/Class/InputHook.cs
[p-machine]: https://github.com/microsoft/PowerToys/blob/084281ef6b137aa47a1e6a8e1e7bf79b0fe0378e/src/modules/MouseWithoutBorders/App/Core/MachineStuff.cs#L184-L232
[p-inject]: https://github.com/microsoft/PowerToys/blob/084281ef6b137aa47a1e6a8e1e7bf79b0fe0378e/src/modules/MouseWithoutBorders/App/Class/InputSimulation.cs
[p-socket]: https://github.com/microsoft/PowerToys/blob/084281ef6b137aa47a1e6a8e1e7bf79b0fe0378e/src/modules/MouseWithoutBorders/App/Class/SocketStuff.cs#L1230-L1305
[p-crypto]: https://github.com/microsoft/PowerToys/blob/084281ef6b137aa47a1e6a8e1e7bf79b0fe0378e/src/modules/MouseWithoutBorders/App/Core/Encryption.cs
[l-cargo]: https://github.com/feschber/lan-mouse/blob/69958c625163649b69e27d0a6fb5a3edd596a467/Cargo.toml
[l-license]: https://github.com/feschber/lan-mouse/blob/69958c625163649b69e27d0a6fb5a3edd596a467/LICENSE
[l-release]: https://github.com/feschber/lan-mouse/releases/tag/v0.11.0
[l-readme]: https://github.com/feschber/lan-mouse/blob/69958c625163649b69e27d0a6fb5a3edd596a467/README.md
[l-listen]: https://github.com/feschber/lan-mouse/blob/69958c625163649b69e27d0a6fb5a3edd596a467/src/listen.rs#L72-L114
[l-connect]: https://github.com/feschber/lan-mouse/blob/69958c625163649b69e27d0a6fb5a3edd596a467/src/connect.rs
[l-capture]: https://github.com/feschber/lan-mouse/blob/69958c625163649b69e27d0a6fb5a3edd596a467/src/capture.rs#L390-L453
[l-proto]: https://github.com/feschber/lan-mouse/blob/69958c625163649b69e27d0a6fb5a3edd596a467/lan-mouse-proto/src/lib.rs#L43-L68
[l-win-capture]: https://github.com/feschber/lan-mouse/blob/69958c625163649b69e27d0a6fb5a3edd596a467/input-capture/src/windows/event_thread.rs
[l-win-emulate]: https://github.com/feschber/lan-mouse/blob/69958c625163649b69e27d0a6fb5a3edd596a467/input-emulation/src/windows.rs
[l-issue]: https://github.com/feschber/lan-mouse/issues/207
[d-license]: https://github.com/deskflow/deskflow/blob/80221f0f7b7ebd807fb1ffdf00906bbcfe561011/LICENSE
[d-licenses]: https://github.com/deskflow/deskflow/tree/80221f0f7b7ebd807fb1ffdf00906bbcfe561011/LICENSES
[d-release]: https://github.com/deskflow/deskflow/releases/tag/v1.27.0
[d-server]: https://github.com/deskflow/deskflow/blob/80221f0f7b7ebd807fb1ffdf00906bbcfe561011/src/lib/server/Server.cpp
[d-client]: https://github.com/deskflow/deskflow/blob/80221f0f7b7ebd807fb1ffdf00906bbcfe561011/src/lib/client/ServerProxy.cpp
[d-proxies]: https://github.com/deskflow/deskflow/tree/80221f0f7b7ebd807fb1ffdf00906bbcfe561011/src/lib/server
[d-hook]: https://github.com/deskflow/deskflow/blob/80221f0f7b7ebd807fb1ffdf00906bbcfe561011/src/lib/platform/MSWindowsHook.cpp
[d-win]: https://github.com/deskflow/deskflow/blob/80221f0f7b7ebd807fb1ffdf00906bbcfe561011/src/lib/platform/MSWindowsComputer.cpp#L1428-L1449
[d-tls]: https://github.com/deskflow/deskflow/blob/80221f0f7b7ebd807fb1ffdf00906bbcfe561011/src/lib/net/SecureSocket.cpp
[d-frames]: https://github.com/deskflow/deskflow/blob/80221f0f7b7ebd807fb1ffdf00906bbcfe561011/src/lib/deskflow/ProtocolTypes.cpp
[d-config]: https://github.com/deskflow/deskflow/blob/80221f0f7b7ebd807fb1ffdf00906bbcfe561011/src/lib/gui/config/ServerConfig.cpp
[d-issue]: https://github.com/deskflow/deskflow/issues/10239
