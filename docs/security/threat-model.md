# Threat model

## Trust boundaries

An operator starts the receiver and privately shares the invitation with the intended primary. The protocol transfers control of the ordinary interactive Windows desktop, so possession of a valid invitation is permission to inject input during that session. The invitation is reusable for reconnection and is revoked when the receiver stops.

The LAN is untrusted. Discovery responses provide only candidates. TLS 1.3 uses a SHA-256 pin obtained out of band. Rustls also verifies the handshake signature. The sender transmits its random 256-bit token inside TLS and the receiver compares it in constant time. The local main WebView can access the five session commands and a snapshot with the receiver code. The capability applies to the packaged local origin.

| Risk | Control in this branch | Remaining boundary |
| --- | --- | --- |
| Discovery spoofing or network interception | A discovered address has no authority. The peer must prove possession of the pinned private key before receiving the token. | Anyone who can replace the out-of-band code can substitute the identity. Verify the code through a trusted channel. |
| Unauthorized control | Invitation token before Ready. Session can be stopped locally. Exactly one receiver-side controller is selected. | A copied invitation can be reused until stop. The design has no persistent identities, replay ledger or per-peer revocation churn. |
| Delayed negotiations | Eight pending accepts with a three-second limit each. Other pending negotiations are cancelled when one peer wins. | A busy LAN can exhaust the finite slots or bandwidth. Restrict incoming ports to private networks. |
| Stuck input or stale connection | One matching probe renews the sender lease. Bounded writes, bounded queue age and a receiver key/button ledger fail toward local control. | Data already in TCP can arrive late. Forced termination on the receiver or a switch to a secure desktop can prevent release injection. Physical tests are required. |
| Untrusted renderer or future local extensions | Tauri capabilities grant only the five commands to the local `main` WebView. Production content is packaged locally and constrained by CSP. | Script execution inside the trusted main WebView would inherit its privileges. Future remote content needs separate windows and capabilities. |
| Scope creep around privacy | Input-only protocol, transient credentials, no screen capture or telemetry. No persistent credential store. | OS and authorized endpoint apps may observe normal input. A compromised host or privileged Windows process sits outside the protection of the bridge. |

## Assumptions and validation

The trust model assumes ordinary Windows user sessions and operators in possession of both endpoints. EdgeBridge respects UIPI and the secure desktop. Logs and diagnostics must exclude keys, typed text, clipboard and pairing codes. IP addresses must be redacted before sharing diagnostics.

Local automated checks cover certificate and token rejection, admission with a pending negotiation, matched and expired heartbeats, stale input, and release of held mouse input after transport close. A native WebView2 smoke test checked the normal session commands and confirmed that an independent loopback origin could not read the session snapshot. [Tauri runtime authority](https://v2.tauri.app/security/runtime-authority/) · [Microsoft SendInput](https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-sendinput)
