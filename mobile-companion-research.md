# Pretorium mobile companion: feasibility research

Research date: 2026-10-12. Based on the current local codebase and the official documentation linked below. This document proposes an approach; no mobile functionality has been implemented or tested.

## Conclusion

A mobile companion that controls agents running in desktop Pretorium is feasible. Repositories, credentials, and agent processes would remain on the Mac. The phone would display their state and send commands over an authenticated connection.

Pretorium already has most of the underlying terminal operations. The main work is adding remote connectivity, supporting simultaneous desktop and mobile clients, and designing a usable mobile interface.

## Useful initial scope

- List agents across workspaces and show which are working or need attention.
- Open an agent's live terminal and read its output.
- Send instructions, answer visible prompts, and interrupt an agent.
- Receive notifications when an agent needs attention or finishes.

Later additions could include starting agents in existing projects and reviewing file changes.

Terminal control fits the existing architecture. A polished chat interface and reliable, provider-independent approval buttons require additional integration with each agent.

## Existing foundation and gaps

The [Rust terminal backend](src-tauri/src/lib.rs) exposes `pty_open`, `pty_write`, `pty_resize`, `pty_kill`, and `pty_status` through local Tauri commands. Sessions live in the backend and can survive workspace or window changes while the application remains running.

The [terminal frontend](src/lib/Terminal.svelte) uses xterm.js and Tauri channels for input and output. The [fleet model](src/lib/fleet.ts) already combines workspace tabs and shell state into agent rows.

| Current behavior | Required change for mobile |
|---|---|
| Controls are local Tauri calls; no remote control server was found | Add an authenticated network API and live output stream |
| Each terminal has one output subscriber; reattachment replaces it | Broadcast output to desktop and phone simultaneously |
| Attaching also resizes the terminal | Define which client controls terminal dimensions |
| Reattachment replays roughly 512 KB of raw output | Add reliable reconnection and terminal screen restoration; the byte tail is not a complete screen snapshot |
| Workspace names and layouts live in webview local storage | Make an appropriate workspace and session inventory available to the backend API |
| Desktop frontend polling drives status updates and attention events | Monitor sessions in the backend independently of frontend polling |
| Quitting Pretorium ends agent sessions | Keep the app running, or introduce a separate background service if independent lifetime is required |

Workspace persistence is described in [workspaces.svelte.ts](src/lib/workspaces.svelte.ts). Agent polling is implemented in [agents.svelte.ts](src/lib/agents.svelte.ts).

## Connectivity options

### Private connection through Tailscale

Best fit: personal use and an early beta.

Tailscale Serve can expose a local service over HTTPS to devices within a private Tailscale network. Tailscale supports direct connections and relay fallback, so the phone and desktop do not need to share Wi-Fi.

Pretorium would still need its own remote API and access controls. Tailscale supplies connectivity, not the agent-control interface. Users would need to install and configure Tailscale on their devices.

Sources: [Tailscale Serve](https://tailscale.com/docs/features/tailscale-serve), [connection types](https://tailscale.com/docs/reference/connection-types).

### Pretorium-operated relay

Best fit: a broadly distributed companion app with straightforward onboarding.

Proposed architecture: both desktop and phone establish outbound connections to a hosted relay. The user pairs devices, potentially by scanning a QR code, without configuring router port forwarding.

This would require hosted infrastructure, device authentication, pairing, revocation, and ongoing operations. End-to-end encryption should be part of the protocol design so that the relay need not read terminal content or commands. It is not an automatic property of using a relay.

Because terminal input grants substantial control over the Mac, the remote interface should expose deliberate, authenticated operations rather than indiscriminately forwarding all existing Tauri commands.

## Mobile delivery options

### Tauri mobile companion

Tauri 2 supports Android and iOS and accepts the existing Svelte frontend stack. A dedicated companion could share frontend code and data types with desktop, while desktop-specific process and window functionality stays on the Mac.

This does not mean the current desktop app can be shipped unchanged on mobile. It needs a focused mobile interface, a network transport, and platform-specific integration where appropriate.

Source: [Tauri platform support](https://v2.tauri.app/).

### Installable mobile web app

A mobile web app added to the home screen is a reasonable first delivery route. On iOS and iPadOS, home-screen web apps support Web Push from version 16.4 onward, with user permission. Notifications therefore do not inherently require an App Store app.

Push still needs a sender and delivery integration; installing a web app alone does not provide notifications.

Source: [WebKit: Web Push for Web Apps on iOS and iPadOS](https://webkit.org/blog/13878/web-push-for-web-apps-on-ios-and-ipados/).

## Important constraints

### Desktop availability

The Mac must be awake, online, and running Pretorium. The current `quit` implementation explicitly terminates terminal sessions. A separate background service would be required if agents must outlive the desktop app.

Pretorium has a Stay Awake feature using `caffeinate`, but this should not be treated as a guarantee of availability through every sleep, lid-close, shutdown, or network-loss scenario.

### Mobile background execution

A phone app cannot depend on an indefinitely running background terminal connection. iOS normally suspends background apps. The desktop should continue monitoring agents, send push notifications when appropriate, and allow the phone to reconnect and recover current state when opened.

Source: [Apple: Extending your app's background execution time](https://developer.apple.com/documentation/uikit/extending-your-app-s-background-execution-time).

### Approval semantics

The current [status detection](src-tauri/src/status.rs) recognizes terminal text and notification escape sequences. It does not receive a structured approval object from every provider.

Answering a visible terminal prompt is feasible. A universal Approve button needs reliable provider integration and a way to verify that the pending request is still the same request when the response arrives. Heuristic attention detection alone is insufficient for that behavior.

### Concurrent control and reconnection

Desktop and phone should be able to observe the same session. Input and terminal resizing need an explicit ownership policy to avoid competing keystrokes and disruptive layout changes.

Reconnection should recover output and current state without blindly replaying input. In particular, an interrupted connection must not cause a submitted command or approval response to execute twice.

## Recommended direction

1. Establish a small remote protocol around session inventory, status, output subscription, and terminal input.
2. Add multiple output subscribers and explicit input/resize ownership to the desktop backend.
3. Build a focused mobile Svelte interface: fleet list, one-agent terminal view, and instruction entry.
4. Use Tailscale for initial private connectivity.
5. Add backend-driven attention monitoring and push delivery.
6. Evaluate a hosted relay and streamlined pairing for wider distribution.
7. Add structured conversations and approval controls only where provider integrations support them reliably.

The remote protocol and mobile interface can support either a web app or a Tauri mobile app. Those decisions need not be coupled to the choice of private networking versus a hosted relay.

## Remaining decisions and validation

- Personal companion or feature distributed to all Pretorium users?
- iOS, Android, or both?
- Is installing Tailscale acceptable for the initial audience?
- Must agents continue running after Pretorium quits?
- Is terminal interaction sufficient, or is a structured chat interface required initially?
- Validate terminal input, touch selection, keyboard handling, and rendering on actual phones.
- Validate reconnection, simultaneous desktop/mobile use, stale prompts, and notification delivery before treating the companion as dependable.

This research establishes architectural feasibility, not a delivery estimate or a tested mobile prototype.
