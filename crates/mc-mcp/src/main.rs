//! MCP stdio server. stdout carries JSON-RPC only; all diagnostics go to stderr.
use mc_core::{GameLayout, LocalClientService, OnlineCatalog};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};

const PROTOCOL: &str = "2025-06-18";

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("Memoria MCP: {err}");
        std::process::exit(1);
    }
}

fn game_root() -> PathBuf {
    if let Some(arg) = std::env::args().skip(1).find_map(|arg| arg.strip_prefix("--game-root=").map(str::to_owned)) {
        return PathBuf::from(arg);
    }
    if let Some(root) = std::env::var_os("MEMORIA_GAME_ROOT") {
        return PathBuf::from(root);
    }
    #[cfg(windows)]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return PathBuf::from(appdata).join(".minecraft");
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".minecraft");
    }
    PathBuf::from(".minecraft")
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let catalog = OnlineCatalog::new()?;
    let root = game_root();
    let mut lines = BufReader::new(io::stdin()).lines();
    let mut out = io::stdout();
    while let Some(line) = lines.next_line().await? {
        if line.len() > 1024 * 1024 {
            eprintln!("Memoria MCP: input line too large");
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(request) => dispatch(request, &catalog, &root).await,
            Err(_) => Some(error(Value::Null, -32700, "Parse error")),
        };
        if let Some(response) = response {
            let mut data = serde_json::to_vec(&response)?;
            data.push(b'\n');
            out.write_all(&data).await?;
            out.flush().await?;
        }
    }
    Ok(())
}

fn response(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn tool_result(data: Value) -> Value {
    let printed = serde_json::to_string_pretty(&data).unwrap_or_default();
    json!({"content": [{"type": "text", "text": printed}], "structuredContent": data})
}

fn tool_error(code: &str, message: &str) -> Value {
    json!({"isError": true,
        "content": [{"type": "text", "text": format!("{code}: {message}")}],
        "structuredContent": {"code": code, "message": message}})
}

fn tool_definitions() -> Value {
    json!([
        {"name": "minecraft_list_versions",
         "description": "List real Mojang Minecraft version metadata (read-only).",
         "annotations": {"readOnlyHint": true},
         "inputSchema": {"type": "object", "properties": {
             "limit": {"type": "integer", "minimum": 1, "maximum": 100, "default": 20}
         }, "additionalProperties": false}},
        {"name": "minecraft_list_instances",
         "description": "Scan installed local Minecraft versions without reading credentials or writing files.",
         "annotations": {"readOnlyHint": true},
         "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}},
        {"name": "minecraft_search_mods",
         "description": "Search real Modrinth mods by name and optional MC version and loader.",
         "annotations": {"readOnlyHint": true},
         "inputSchema": {"type": "object", "properties": {
             "query": {"type": "string", "minLength": 1, "maxLength": 120},
             "minecraft_version": {"type": "string", "maxLength": 40},
             "loader": {"type": "string", "enum": ["fabric", "forge", "neoforge", "quilt"]},
             "limit": {"type": "integer", "minimum": 1, "maximum": 20, "default": 10}
         }, "required": ["query"], "additionalProperties": false}},
        {"name": "minecraft_get_mod_info",
         "description": "Get Modrinth project metadata and version IDs, read-only. Does not install anything.",
         "annotations": {"readOnlyHint": true},
         "inputSchema": {"type": "object", "properties": {
             "project_id": {"type": "string", "minLength": 1, "maxLength": 100}
         }, "required": ["project_id"], "additionalProperties": false}}
    ])
}

fn string_arg<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

fn usize_arg(v: &Value, key: &str, default: usize, max: usize) -> Result<usize, &'static str> {
    match v.get(key) {
        None => Ok(default),
        Some(v) => v.as_u64().filter(|n| *n > 0 && *n <= max as u64)
            .map(|n| n as usize).ok_or("limit must be a positive integer within range"),
    }
}

fn verify_args(args: &Value, allowed: &[&str]) -> Result<(), &'static str> {
    let o = args.as_object().ok_or("arguments must be an object")?;
    if o.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err("unknown argument");
    }
    Ok(())
}

async fn call_tool(name: &str, args: &Value, catalog: &OnlineCatalog, root: &Path) -> Value {
    let result: Result<Value, (String, String)> = match name {
        "minecraft_list_versions" => {
            let n = verify_args(args, &["limit"]).and_then(|_| usize_arg(args, "limit", 20, 100));
            match n {
                Ok(n) => catalog.list_versions(n).await.map_err(|e| ("CATALOG_UNAVAILABLE".into(), e.to_string())),
                Err(e) => Err(("INVALID_ARGUMENT".into(), e.into())),
            }
        }
        "minecraft_list_instances" => {
            match verify_args(args, &[]) {
                Err(e) => Err(("INVALID_ARGUMENT".into(), e.into())),
                Ok(()) => match GameLayout::new(root) {
                    Err(e) => Err(("LOCAL_SCAN_FAILED".into(), e.to_string())),
                    Ok(layout) => match LocalClientService::new(layout).scan().await {
                        Err(e) => Err(("LOCAL_SCAN_FAILED".into(), e.to_string())),
                        Ok(report) => {
                            let clients: Vec<Value> = report.clients.iter().map(|c| json!({
                                "id": c.instance.id, "name": c.instance.name,
                                "minecraft_version": c.instance.mc_version,
                                "loader": c.instance.loader, "client_jar_present": c.client_jar_present,
                                "java_major": c.java_major
                            })).collect();
                            Ok(json!({"source": "local_scan", "instances": clients,
                                "warning_count": report.issues.len()}))
                        }
                    }
                }
            }
        }
        "minecraft_search_mods" => {
            match verify_args(args, &["query", "minecraft_version", "loader", "limit"]) {
                Err(e) => Err(("INVALID_ARGUMENT".into(), e.into())),
                Ok(()) => match string_arg(args, "query") {
                    Some(q) if q.len() <= 120 => {
                        let version = string_arg(args, "minecraft_version");
                        let loader = string_arg(args, "loader");
                        let allowed_loader = loader.map(|s| matches!(s, "fabric" | "forge" | "neoforge" | "quilt")).unwrap_or(true);
                        if !allowed_loader || version.map(|v| v.len() > 40 || !v.bytes().all(|c| c.is_ascii_alphanumeric() || b".-_".contains(&c))).unwrap_or(false) {
                            Err(("INVALID_ARGUMENT".into(), "invalid game version or loader".into()))
                        } else {
                            match usize_arg(args, "limit", 10, 20) {
                                Ok(n) => catalog.search_mods(q, version, loader, n).await
                                    .map_err(|e| ("CATALOG_UNAVAILABLE".into(), e.to_string())),
                                Err(e) => Err(("INVALID_ARGUMENT".into(), e.into())),
                            }
                        }
                    }
                    _ => Err(("INVALID_ARGUMENT".into(), "query is required (max 120 bytes)".into())),
                }
            }
        }
        "minecraft_get_mod_info" => {
            match verify_args(args, &["project_id"]) {
                Err(e) => Err(("INVALID_ARGUMENT".into(), e.into())),
                Ok(()) => match string_arg(args, "project_id") {
                    Some(id) => catalog.get_mod_info(id).await
                        .map_err(|e| ("CATALOG_UNAVAILABLE".into(), e.to_string())),
                    None => Err(("INVALID_ARGUMENT".into(), "project_id is required".into())),
                }
            }
        }
        _ => Err(("UNKNOWN_TOOL".into(), "Unknown or unavailable tool".into())),
    };
    match result {
        Ok(data) => tool_result(data),
        Err((code, message)) => tool_error(&code, &message),
    }
}

async fn dispatch(request: Value, catalog: &OnlineCatalog, root: &Path) -> Option<Value> {
    let id = match request.get("id") {
        Some(id) => id.clone(),
        None => return None, // MCP notifications never receive replies
    };
    if request.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || request.get("method").and_then(Value::as_str).is_none()
    {
        return Some(error(id, -32600, "Invalid Request"));
    }
    let method = request["method"].as_str().unwrap_or_default();
    let params = &request["params"];
    let reply = match method {
        "initialize" => {
            let requested = params["protocolVersion"].as_str().unwrap_or(PROTOCOL);
            let version = if ["2024-11-05", "2025-03-26", "2025-06-18"].contains(&requested) {
                requested
            } else { PROTOCOL };
            response(id, json!({
                "protocolVersion": version, "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "memoria-mcp", "version": env!("CARGO_PKG_VERSION")}
            }))
        }
        "ping" => response(id, json!({})),
        "tools/list" => response(id, json!({"tools": tool_definitions()})),
        "tools/call" => {
            match (params["name"].as_str(), params.get("arguments")) {
                (Some(name), args) => {
                    let args = args.cloned().unwrap_or_else(|| json!({}));
                    response(id, call_tool(name, &args, catalog, root).await)
                }
                _ => error(id, -32602, "Missing tool name"),
            }
        }
        _ => error(id, -32601, "Method not found"),
    };
    Some(reply)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn handshake_tool_list_and_notifications() {
        let c = OnlineCatalog::new().unwrap();
        let root = Path::new(".minecraft");
        let init = dispatch(json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}), &c, root).await.unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        let tools = dispatch(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}), &c, root).await.unwrap();
        assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 4);
        assert!(dispatch(json!({"jsonrpc":"2.0","method":"notifications/initialized"}), &c, root).await.is_none());
    }
    #[tokio::test]
    async fn unknown_and_invalid_calls_do_not_mutate_local_files() {
        let c = OnlineCatalog::new().unwrap();
        let root = Path::new(".");
        let unknown = dispatch(json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"run_shell"}}), &c, root).await.unwrap();
        assert_eq!(unknown["result"]["structuredContent"]["code"], "UNKNOWN_TOOL");
        let bad = dispatch(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"minecraft_search_mods","arguments":{}}}), &c, root).await.unwrap();
        assert_eq!(bad["result"]["structuredContent"]["code"], "INVALID_ARGUMENT");
        let bad_args = dispatch(json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"minecraft_list_versions","arguments":{"limit":-1}}}), &c, root).await.unwrap();
        assert_eq!(bad_args["result"]["structuredContent"]["code"], "INVALID_ARGUMENT");
    }
}
