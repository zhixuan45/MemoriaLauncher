# Desktop data sources

The desktop starts with an empty library and task queues, then scans the configured Minecraft directory. It no longer seeds sample instances, accounts, download progress, play time, or AI tasks. Static feature cards are usage guides, and the changelog describes the current development build without fabricated release dates.

## Local games and launching

Open **设置 → 环境配置**, enter the directory containing `versions`, `libraries`, and `assets`, set the Java executable, and click **保存并重新扫描**. The scanner also accepts the legacy `version` directory. The default Windows game directory is `%APPDATA%\.minecraft`.

Java requirements come from installed version metadata. Mod counts include JAR files in each isolated version's `mods` directory. A present client JAR is not a complete dependency check: launching validates the selected account, Java, libraries, assets, and natives through `mc-core::GameLauncher`. The UI reports preparation errors, the process ID and log path, and the final exit code. Play time remains “未记录” because no play-time database exists.

Environment settings are saved to `%LOCALAPPDATA%\MemoriaLauncher\environment.json`. `MC_LAUNCHER_GAME_DIR` and `MC_LAUNCHER_JAVA` override saved settings at startup. `MEMORIA_DATA_DIR` overrides the launcher's data directory for development and isolated verification.

## Microsoft accounts

The core already implements PKCE authorization, Minecraft ownership checks, and token refresh. The desktop now supplies browser opening, a loopback HTTP callback, account selection, persistence, and logout.

For a locally registered application, open **设置 → 账户登录**, enter the application client ID and matching callback URI, and click **保存登录配置**. The configuration takes effect immediately and is reloaded after restart from `microsoft-login.json` in the launcher data directory. The account dialog's **配置 Microsoft 登录** button opens this page. Invalid input and save failures preserve the last active configuration. Changing the client ID requires signing in existing accounts again.

Saved local configuration takes priority over runtime environment variables, which take priority over the publisher's ID embedded through `MEMORIA_MICROSOFT_CLIENT_ID` at build time. Release builds can still embed the publisher's ID so players only click the Microsoft login button. Without saved local configuration, developers can also configure the application at runtime:

```powershell
$env:MC_LAUNCHER_CLIENT_ID = "your-application-client-id"
$env:MC_LAUNCHER_REDIRECT_URI = "http://localhost:53682/oauth/callback"
cargo run --locked -p desktop
```

The redirect URI defaults to the value above when omitted. Desktop callbacks support HTTP on `localhost` or `127.0.0.1`; the listener binds only to loopback, validates the callback path and OAuth state, and times out after three minutes. Login can be cancelled from the account dialog. Missing configuration, occupied ports, rejected authorization, and credential-storage errors are displayed in that dialog.

Account names, UUIDs, and the active account are saved in `accounts.json` in the data directory. Refresh tokens are stored separately in Windows Credential Manager under `MemoriaLauncher/Microsoft/<account-id>`. Access tokens remain transient. Logout deletes the corresponding vault entry. Other operating systems currently report that secure credential storage is unavailable.

## Official version catalogue and unavailable services

The download page loads Mojang's official version manifest asynchronously with a 30-second request timeout. Release IDs, snapshot IDs, publication timestamps, and metadata URLs come from that response. Unavailable descriptions, Java requirements, third-party loader compatibility, and hardware requirements are not synthesized. Failed requests display an error and can be retried without falling back to samples.

Game installation and desktop third-party resource catalogues are still unavailable. AI supports optional OpenAI-compatible chat using a configured endpoint, model, and in-memory API key. The separate `mc-mcp` server exposes read-only real catalogue and local scan tools to external agents. Web search and AI installation/local mutation remain unavailable; actions never insert simulated work into the queues. General setting switches outside environment/account management retain their existing session-only behavior.

## Verification

```powershell
cargo check --workspace --locked --offline
cargo test --locked --offline -p desktop -p mc-core -- --test-threads=1
cargo test --locked --offline -p mc-core official_catalog_is_reachable -- --ignored
```

The ordinary tests cover file-backed scanning, missing game directories/JARs, account-index replacement and corruption, callback state validation over a real loopback connection, Windows vault save/refresh/delete, account controls, launch-without-account behavior, and the existing page interactions. The explicitly ignored catalogue test requires network access. Full Microsoft login and game startup require a registered application, an eligible user account, Java, and installed game files; offline tests do not establish that end-to-end outcome.
