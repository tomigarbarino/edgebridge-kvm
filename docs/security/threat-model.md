# Threat model

## Trust boundaries

An operator starts the receiver and either confirms a request on both PCs or shares the manual invitation privately. The protocol transfers control of the ordinary interactive Windows desktop. A valid credential permits input during that session and can be reused for reconnection. Stopping the receiver revokes it.

The LAN is untrusted. In manual pairing, the invitation provides an out-of-band SHA-256 pin. In discovered pairing, the advertised pin is provisional. Each endpoint derives a comparison number from its TLS 1.3 connection and prompts its operator. Operators verify the same number on both physical screens before approving. The receiver then issues a random 256-bit credential over that TLS connection. The sender presents it inside TLS on the control connection and the receiver compares it in constant time. The local main WebView can access session commands and the snapshot. The capability applies to the packaged local origin.

| Risk | Control in this branch | Remaining boundary |
| --- | --- | --- |
| Discovery spoofing or network interception | TLS verifies possession of the candidate's private key. Quick pairing adds matching-number confirmation on both screens before credential issuance. Manual pairing uses a pin delivered separately. | Names and advertised pins are unverified until confirmation. Compare the numbers through direct observation of the intended PCs. |
| Unauthorized control | Invitation token before Ready. Session can be stopped locally. Exactly one receiver-side controller is selected. | A copied invitation can be reused until stop. The design has no persistent identities, replay ledger or per-peer revocation churn. |
| Delayed negotiations | Eight concurrent accept handlers, bounded TLS and credential reads, and one visible approval per app with a 45-second deadline. Other pending negotiations are cancelled when one peer wins. | A busy LAN can exhaust the finite slots or bandwidth. Restrict incoming ports to private networks. |
| Stuck input or stale connection | One matching probe renews the sender lease. Bounded writes, bounded queue age and a receiver key/button ledger fail toward local control. | Data already in TCP can arrive late. Forced termination on the receiver or a switch to a secure desktop can prevent release injection. Physical tests are required. |
| Untrusted renderer or future local extensions | Tauri capabilities grant session and updater commands to the local `main` WebView. Production content is packaged locally and constrained by CSP. | Script execution inside the trusted main WebView would inherit its privileges. Future remote content needs separate windows and capabilities. |
| Scope creep around privacy | Transient credentials, opt-in clipboard on both PCs, and a separate pinned TLS clipboard channel with a session key and transfer limits. Input events use the original channel. | OS and authorized endpoint apps may observe normal input. A compromised host or privileged Windows process sits outside the protection of the bridge. |

## Assumptions and validation

The trust model assumes ordinary Windows user sessions and operators in possession of both endpoints. EdgeBridge respects UIPI and the secure desktop. Logs and diagnostics must exclude keys, typed text, clipboard and pairing codes. IP addresses must be redacted before sharing diagnostics.

Local automated checks cover certificate and token rejection, admission with a pending negotiation, matched and expired heartbeats, stale input, and release of held mouse input after transport close. A native WebView2 smoke test checked the normal session commands and confirmed that an independent loopback origin could not read the session snapshot. [Tauri runtime authority](https://v2.tauri.app/security/runtime-authority/) · [Microsoft SendInput](https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-sendinput)
