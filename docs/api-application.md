# Memoria Launcher — API application and review notes

This document describes the implemented application and its intended Microsoft / Minecraft API use. It does not assert that an application ID has been approved or that live authentication has been independently verified.

## Project

| Item | Details |
| --- | --- |
| Application | Memoria Launcher |
| Source and maintainer | [zhixuan45/MemoriaLauncher](https://github.com/zhixuan45/MemoriaLauncher) |
| Contact | [Repository issues](https://github.com/zhixuan45/MemoriaLauncher/issues) |
| Application type | Independent native desktop launcher for Minecraft Java Edition |
| Implementation | Rust, iced, Tokio; Windows is the current secure-credential-storage target |
| Intended users | Users signing into their own Microsoft accounts with eligible Minecraft Java Edition access |
| Purpose | Discover local Minecraft installations, authenticate the selected account, verify Java Edition entitlements/profile, and start the installed game |

## Description for an application form

> Memoria Launcher is an independent native desktop launcher for Minecraft Java Edition. Users authenticate with their own Microsoft accounts in the system browser using OAuth authorization code flow with PKCE. The launcher exchanges the resulting authorization through Xbox Live, XSTS and Minecraft Services, verifies Minecraft Java Edition entitlements, obtains the user's Java profile, and launches an installed local game using the authenticated session. It does not collect Microsoft account passwords. Windows refresh tokens are stored in the operating system credential vault. Optional AI chat and read-only MCP integrations do not receive account credentials or launch authenticated sessions on behalf of external agents.

## Authentication and permissions

The application uses Microsoft consumer-account authorization code flow with PKCE S256 and a per-login OAuth state value. Requested scopes are `XboxLive.signin offline_access`. Offline access supports refreshing the user's session before an authenticated local game launch.

The default registered callback is `http://localhost:53682/oauth/callback`. The listener binds only to IPv4 loopback (`127.0.0.1`) and verifies the callback path, origin and state. It accepts an authorization code or an OAuth error, can be cancelled, and times out after three minutes while waiting for the callback. The launcher is a public desktop client and does not embed a client secret.

The client ID belongs to the application's maintainer. It may be embedded at build time, or supplied once in **Settings → 账户登录** and persisted locally. The application does not reuse another launcher's client ID. An Azure/Entra app registration alone does not establish eligibility to access every Xbox/Minecraft service; the maintainer must complete the relevant application review and supply their registered ID in the actual application form.

## Data handling

| Data | Use and storage |
| --- | --- |
| Microsoft password and browser sign-in | Entered on Microsoft's sign-in pages in the system browser; not collected by the launcher |
| Authorization code / PKCE verifier | Used for the authorization exchange; not written to the account index or project documentation |
| Refresh token | Stored in Windows Credential Manager under `MemoriaLauncher/Microsoft/<account-id>`; removed on local logout |
| Access tokens | Used transiently for the identity/game service exchanges and authenticated game startup; excluded from serializable account records |
| Java username and UUID | Stored locally in `accounts.json` to list and select accounts |
| Application client ID and callback URI | Stored locally in `microsoft-login.json`; contains no account tokens or client secret |
| Local game paths | Used for scanning and launching local installations; not sent to the optional AI provider |
| AI API Key | User-supplied and retained only in the running desktop process; sent only to the configured compatible API endpoint |
| AI prompt | Sent to the user-selected AI provider with target game version and output language; not accompanied by Microsoft/Minecraft credentials |

Authenticated game startup passes the required session parameters to the Java game process. Game log forwarding redacts known session secrets. The current project does not provide a hosted account service or a credential-collection backend. Local logout deletes the launcher's credential entry; server-side consent revocation remains under the user's Microsoft account controls.

## Separation from AI / MCP

The optional AI chat adapter returns text recommendations. It has no installer, shell or local filesystem tools. The separate MCP stdio server offers four read-only tools: Mojang version listing, local instance listing, Modrinth search, and Modrinth project metadata. The MCP server does not expose account tokens, Microsoft sign-in or game launching, and cannot apply installation changes.

## Code locations for review

| Area | Source |
| --- | --- |
| Browser login, callback validation, account selection and local app configuration | [accounts.rs](../apps/desktop/src/accounts.rs) |
| Windows secure credential storage | [credentials.rs](../apps/desktop/src/credentials.rs) |
| Microsoft / Xbox / XSTS / Minecraft exchanges, entitlements and refresh | [core authentication](../crates/mc-core/src/runtime/auth.rs) |
| OAuth helper and PKCE construction | [Microsoft OAuth helper](../crates/vendor/mc-launcher-core/src/auth/microsoft_account.rs) |
| Java game launch preparation | [launch.rs](../crates/mc-core/src/runtime/launch.rs) |
| Process/log handling | [process.rs](../crates/mc-core/src/runtime/process.rs) |
| Optional user-configured AI requests | [api_client.rs](../apps/desktop/src/api_client.rs) |
| Read-only external agent tools | [MCP server](../crates/mc-mcp/src/main.rs) |

## Current verification boundary

Automated checks cover local file scanning, account-index persistence, saved client ID configuration and restart, callback validation over loopback, Windows credential vault operations, UI actions, and read-only service contracts. Mojang manifest reachability is checked separately with a live network test.

A registered application, service approval where required, an eligible Microsoft account, compatible Java and installed game files are necessary for a complete live sign-in/refresh/game-start validation. No screenshot or passing unit test in this repository should be interpreted as proof of an approval decision or a completed live authentication review.
