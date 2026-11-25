# EdgeBridge KVM

EdgeBridge shares one mouse and keyboard between two Windows computers on the same local network. Both PCs keep their own displays, apps, and Windows sessions. EdgeBridge sends input events over a direct TLS connection.

| Secondary PC | | Primary PC |
| --- | :---: | --- |
| **Right edge** | ⇄ | **Left edge** · mouse and keyboard connected here |

Move the cursor to the **left edge of the primary PC**. It enters near the right edge of the secondary PC. Move into the secondary display first, then push against its **right edge** to return. Press **Ctrl + Alt + F12** on the primary keyboard to return immediately.

![EdgeBridge KVM primary setup showing the default left and right edges](docs/screenshots/primary.png)

## Install and pair

1. On each PC, run the x64 setup executable from [Releases](../../releases). Use Windows 11 x64 with Microsoft Edge WebView2 and a mouse that reports relative movement. Keep the app open on both PCs.
2. On the PC you want to control, select **Secondary PC**, then **Start receiver**. If Windows Firewall prompts, allow EdgeBridge on private networks. Select **Start receiver when EdgeBridge opens** if you want it ready on future app launches.
3. On the PC whose mouse and keyboard you want to use, select **Primary PC**. Set where the secondary sits with **Left**, **Right**, **Above**, or **Below**, then select **Pair with** and the other PC’s name.
4. With v0.4.0 or later on both PCs, compare the eight digits shown on both screens. Select **Numbers match** on both PCs if the numbers match. On v0.5.0 or later, select **Remember this PC for future connections** on both to skip the number check on later connections.
5. Check that both PCs show **Connected**. The primary PC displays the measured LAN round-trip time. Use **Disconnect** or **Stop receiver** to end the session.

If discovery is unavailable or one PC is on an older version, use **Show manual pairing code** on the secondary and **Manual pairing and custom edges** on the primary. Enter the secondary’s IPv4 address and its pairing code privately, then select **Connect with code**. The secondary shows a suggested IPv4 address when available.

For a remembered device, select **Connect to** and its name while its receiver is ready. EdgeBridge stores one remembered PC per Windows user. The two PCs prove possession of the saved credential across fresh TLS sessions. The device name helps find the saved entry. To revoke trust, stop the session on the PC where you want to revoke it and select **Forget PC**.

To change which mouse and keyboard are shared, use **Make this PC primary** on one PC and **Make this PC secondary** on the other. This ends the active session on each PC. Start the new receiver, check the layout on the new primary, and connect again. A remembered PC works with either role. Roles and edges are saved locally.

If pairing stalls, select **Diagnose connection** on either PC. The primary tests LAN discovery by interface and checks TCP port 46387 when it finds one receiver or you enter an IPv4 address. The secondary reports whether its receiver and discovery are listening. If approval is in progress or the PCs are connected, the diagnostic reports that state without sending test traffic. **Copy diagnostic** includes the app version, role and test results. Pairing codes, device names and IP addresses are omitted.

### Move a shared HDMI monitor

In Windows Display settings, use **Extend** and place the monitor on the side where it physically sits. Keep the EdgeBridge position set to where the other PC sits. With v0.4.1 or later on both PCs, EdgeBridge recalculates the crossing edge when Windows detects a screen being attached or removed. If a display change happens while controlling the other PC, move the mouse to return control locally, then cross the new outer edge.

For a left laptop, shared middle monitor and right laptop, set the secondary's position according to the laptops. If the middle monitor is attached to the right laptop, the left crossing starts at the middle monitor's left edge. If it is attached to the left laptop, crossing from the right laptop enters the middle monitor on its right edge. A monitor or HDMI switch that keeps the same displays active in Windows does not report the selected video input to EdgeBridge.

The v0.1 installer is unsigned. Check its SHA-256 against the published release checksum if you download a copy from somewhere other than the release page. WebView2 may require a one-time download during installation if it is missing.

EdgeBridge v0.2.0 and later check GitHub Releases for signed updates on startup. Upgrading from v0.1.0 requires running the new installer once on each PC. [Windows updates](docs/updates.md)

### Quick install with an AI assistant

Paste this prompt into the assistant on each PC. Start with the secondary PC.

```text
Help me install or update EdgeBridge KVM on this Windows 11 x64 PC.
This PC is [secondary / primary]. I want clipboard sharing [on / off].
Ask me which option applies if I leave either choice open.

1. Use the latest stable release from
   https://github.com/tomigarbarino/edgebridge-kvm/releases/latest.
   Check the installed version. If it needs installing or updating, download
   the x64 setup and SHA256SUMS.txt and verify the setup with Get-FileHash
   -Algorithm SHA256. If the checksum matches, close EdgeBridge and run the
   setup. If a session is active, let me know before disconnecting it.
2. Open EdgeBridge and verify the installed version.
3. On the secondary PC, select Secondary PC and Start receiver.
   On the primary PC, select Primary PC, choose the secondary's position
   and select Pair with the other PC's name. Ask me to compare the number
   on both screens before accepting on each PC for first-time pairing.
   If I want to trust these PCs for later sessions, select Remember this PC
   for future connections on both. Use Connect to for a remembered PC.
   If discovery is unavailable, let me enter the manual code in the app.
4. Confirm Connected on both PCs. If I want clipboard sharing, enable
   Allow clipboard sharing on this PC on both computers, then confirm
   Clipboard sharing active and test with fresh text between two Notepad windows.

If you can operate only one PC, set up that PC and tell me where to continue
on the other. If you cannot run commands or use the desktop, guide me through
the steps. Tell me which checks actually passed.
```

## Share clipboard

With v0.5.0 or later on both PCs, turn on **Allow clipboard sharing on this PC** on each computer before or during the connection. Turn it off on either PC to pause sharing while keeping the mouse and keyboard connected. The checkbox starts off each time the app opens. When **Clipboard sharing active** appears, copy fresh text with Ctrl+C and paste it on the other PC with Ctrl+V.

With a PC on v0.3.0 through v0.4.4, set the clipboard option on both PCs before connecting. Reconnect to change the setting on those versions.

Clipboard sharing sends Unicode text, compatible Windows bitmap images and up to 32 regular files copied from a local drive. Each transfer can be up to 64 MiB. Folders, cloud placeholders and network paths are outside this version. Paste files before disconnecting. The receiving PC makes a temporary copy so Explorer can paste it into the chosen destination.

## Features and current limits

- Mouse movement, five mouse buttons, vertical and horizontal wheel input, and keyboard scan codes.
- Configurable exit and entry edges across each PC's active Windows displays. Crossing uses the outer edge in the selected direction. Position along the edge is proportional across resolutions and Windows display scale settings. Pointer movement uses relative mouse counts and can feel different from Windows pointer acceleration.
- IPv4 discovery sends a UDP broadcast on each active LAN interface. Wi-Fi and virtual adapters such as Hyper-V can coexist. If no receiver replies, check that both PCs are on the same LAN and use manual IPv4 if needed. Guest Wi-Fi and client isolation can prevent direct connections. The receiver uses TCP port **46387** and UDP port **46388**.
- If the pointer becomes invisible on either PC, open EdgeBridge on that PC and choose **Restore cursor**. You can reach the button with Tab and press Enter. It reloads the Windows cursor scheme while keeping the EdgeBridge session available.
- In-session reconnection to the configured IPv4 address. TCP failures and heartbeat timeouts restore local input. Automatic reconnection uses the current pairing code until either app is stopped or the receiver is restarted.
- Transition waits for held keys and mouse buttons to be released. Emergency return and orderly disconnect send releases for remote keys and buttons still held. Dragging across screens is outside this release.
- Normal Windows user-session permissions apply. Windows can block input to elevated apps, the sign-in screen, and secure desktops. Scan codes follow the secondary PC’s active keyboard layout. Absolute pointing devices and applications that require raw hardware input are outside this release.

## Pairing and privacy

Starting the receiver explicitly enables one connected client at a time. LAN discovery advertises a computer name and certificate fingerprint. Verify the matching number on both screens before approving a request. EdgeBridge issues a temporary credential after both PCs accept. The manual pairing code includes a separate 256-bit secret and the receiver’s TLS certificate fingerprint. Keep the manual code private. Stopping the receiver revokes both forms of access.

Key and mouse events are processed in memory during forwarding. EdgeBridge has no telemetry and sends neither input events nor screenshots to external servers. The app saves the selected edge settings locally. Pairing material lives in memory for the session.

## Build from source

Install [the Windows prerequisites for Tauri](https://v2.tauri.app/start/prerequisites/), including a stable Rust MSVC toolchain, Visual Studio C++ Build Tools, the Windows SDK, WebView2, and Node.js 22.12 or newer. Run these commands from the repository root in an x64 developer shell.

```powershell
npm ci
npm run tauri dev
```

For a production installer, run `npm.cmd run tauri build -- --bundles nsis --ci -- --locked`. The setup executable is written under `src-tauri/target/release/bundle/nsis/`.

## Architecture and validation

The Tauri commands in `service.rs` own a single session and expose status to the React UI. `discovery.rs` handles LAN discovery, `pairing.rs` pins the TLS receiver, and `network.rs` runs the framed Tokio session with bounded writes and heartbeats. `state.rs` owns the control transitions and display geometry. `native/capture.rs` handles Raw Input and Windows low-level hooks on a dedicated message thread. `receiver.rs` tracks remote input state and uses `native/injection.rs` to deliver Windows input and release held keys and buttons on session exit.

The Windows CI workflow checks TypeScript, builds the UI, runs Rust format and lint checks, tests protocol and control logic, and builds the NSIS installer. See [Validation](docs/validation.md) for automated coverage and the two-computer test procedure.

See the [architecture](docs/architecture.md), [benchmark](docs/benchmark.md), [open-source comparison](docs/open-source-comparison.md), [threat model](docs/security/threat-model.md), and [V2 readiness plan](docs/v2-readiness.md).

Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md). EdgeBridge KVM is released under the [MIT License](LICENSE).
