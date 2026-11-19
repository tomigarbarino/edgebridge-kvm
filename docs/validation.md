# Windows validation

## Automated coverage

- Fixed-size wire frames round-trip in order. Unknown, malformed, and truncated frames are rejected.
- A localhost TLS session accepts the invitation, reconnects, rejects a different receiver certificate, and rejects a wrong token under the correct certificate.
- Geometry tests verify the default primary-left to secondary-right route, the return path, proportional placement, and vertical configurations.
- Control tests cover offline behavior, held-input guards, return and disconnect.
- A simulated receiving desktop verifies entry and return, prevents a bounce from entry momentum, and checks that teardown releases keys and mouse buttons.

A single-PC Windows smoke test also opened the built WebView2 app, checked the default edge selection, started the receiver through the UI, and verified that Stop closed the TCP listener.

The automated Rust tests use a simulated input sink or localhost network traffic. The cases below require two physical PCs.

## Two-PC test matrix

Use two Windows PCs on the same private Wi-Fi. Put the primary PC on the right, open a disposable text editor on both machines, and launch EdgeBridge on both. Start with the default edge layout.

| Case | Expected result |
| --- | --- |
| Discover the secondary and pair | The primary finds the receiver. Both show connected, and the primary displays LAN RTT. |
| Incorrect code and manual IPv4 | The wrong code cannot control the secondary. With the correct code, manual IPv4 works even when broadcast discovery is unavailable. |
| Move to the primary's left edge | The cursor appears just inside the secondary's right edge at the corresponding height. The opposite edge of the primary does not activate the default connection. |
| Move inward on the secondary, then cross the right edge | Control returns just inside the primary's left edge, preserving relative height. Sustained motion toward the entry edge immediately after entering does not bounce. |
| Type, click, drag within the secondary, scroll, and use keyboard shortcuts | The secondary responds. The primary editor receives no forwarded typing or mouse events. Check both wheel axes and extra mouse buttons if available. |
| Change resolutions, scaling and edge selections | Try 1920×1080 at 100% and 2560×1440 at 150%. Verify placement at multiple heights. Repeat with top and bottom. |
| Press Ctrl + Alt + F12 during remote control | The primary recovers immediately. The secondary releases any held modifier or mouse button. |
| Disconnect Wi-Fi or stop the receiver while forwarding | Local input returns when Windows reports the drop or the heartbeat expires, normally within about 1.5 seconds. The receiver releases remote keys and buttons once it observes the failure. |
| Restore Wi-Fi without restarting either app | The primary retries and reconnects to the same address. It waits for a new edge crossing before forwarding input. |
| Close an app window while forwarding | The session closes and orderly cleanup releases held remote input. |
| Open UAC on the secondary or target an elevated app | EdgeBridge respects Windows restrictions and resumes ordinary operation after reconnecting. |
| Pair a v0.1.0 node with the post-MVP candidate, then reverse roles | Both roles accept the original invitation and preserve edge, Ping/Pong and release behavior. |
| Share a text sample, bitmap screenshot and regular file with clipboard sharing enabled on both PCs | The copied item can be pasted in both directions. Mouse and key events remain responsive while the independent channel transfers the file. |
| Disable clipboard sharing on either peer | Keyboard and mouse still connect. Clipboard items remain local. |
| Run a high polling-rate mouse and log a typing session | Verify smooth pointer movement and responsive typing without missed releases or unexpected local clicks. Record the two PCs’ RTT and polling rates. |

Record observations for Raw Input capture, Windows hook suppression, SendInput delivery, firewall scope, and recovery on both PCs. A successful build does not exercise those behaviors.
