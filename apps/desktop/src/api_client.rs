//! Optional direct OpenAI-compatible Chat Completions adapter.
//! Not an agent: requests never invoke local installation or filesystem tools.
use reqwest::Url;
use serde_json::{json, Value};
use std::time::Duration;

pub fn completion_url(endpoint: &str) -> Result<Url, String> {
    let mut url = Url::parse(endpoint.trim()).map_err(|_| "API 地址无效，请填写完整 URL".to_owned())?;
    if url.username() != "" || url.password().is_some() || url.query().is_some() || url.fragment().is_some() {
        return Err("API 地址不能包含用户名、密码、查询串或片段".into());
    }
    let host = url.host_str().ok_or("API 地址缺少主机名")?;
    let local = host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" || host == "[::1]";
    if url.scheme() != "https" && !(local && url.scheme() == "http") {
        return Err("远程 API 必须使用 HTTPS；仅本机回环地址允许 HTTP".into());
    }
    let path = url.path().trim_end_matches('/');
    if !path.ends_with("/chat/completions") {
        let updated = if path.is_empty() || path == "/" {
            "/v1/chat/completions".to_owned()
        } else {
            format!("{path}/chat/completions")
        };
        url.set_path(&updated);
    }
    Ok(url)
}

pub async fn chat(
    endpoint: String, model: String, api_key: String,
    prompt: String, version: String, language: String,
) -> Result<String, String> {
    let url = completion_url(&endpoint)?;
    if model.trim().is_empty() {
        return Err("请填写模型名称".into());
    }
    mc_core::service::online_catalog::init_tls_crypto();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::none())
        .build().map_err(|e| format!("初始化 API 客户端失败: {e}"))?;
    let payload = json!({
        "model": model,
        "stream": false,
        "messages": [
            {"role": "system", "content":
                "你是 Memoria Launcher 的 Minecraft 配置顾问。仅提供解释、选型和可供用户审查的计划。你没有本地工具权限，不能声称已下载、安装、汉化或修改任何实例。搜索未接入，不要声称已经在线搜索。"},
            {"role": "user", "content": format!("目标 Minecraft 版本: {version}\n输出语言: {language}\n用户需求:\n{prompt}")}
        ]
    });
    let mut request = client.post(url).json(&payload);
    if !api_key.trim().is_empty() {
        request = request.bearer_auth(api_key.trim());
    }
    let response = request.send().await.map_err(|e| format!("API 请求失败: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("API 返回 HTTP {status}，请检查地址、模型和密钥"));
    }
    let bytes = response.bytes().await.map_err(|e| format!("读取 API 回复失败: {e}"))?;
    if bytes.len() > 512 * 1024 {
        return Err("API 响应过大，已拒绝显示".into());
    }
    let body: Value = serde_json::from_slice(&bytes)
        .map_err(|_| "API 响应不是有效 JSON".to_owned())?;
    body["choices"][0]["message"]["content"].as_str()
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| "API 没有返回有效的 Chat Completions 文本；请确认接口兼容格式".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn url_validation_blocks_insecure_remote_transport() {
        assert!(completion_url("http://example.com/v1").is_err());
        assert!(completion_url("https://user:secret@example.com/v1").is_err());
        assert_eq!(completion_url("https://example.com/v1").unwrap().as_str(),
            "https://example.com/v1/chat/completions");
        assert_eq!(completion_url("http://127.0.0.1:8080/v1/chat/completions").unwrap().as_str(),
            "http://127.0.0.1:8080/v1/chat/completions");
    }
}
