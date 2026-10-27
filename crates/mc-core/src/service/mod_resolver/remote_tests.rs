use super::*;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[tokio::test]
#[ignore = "requires live Modrinth metadata; does not download or install JARs"]
async fn live_modrinth_dependency_graph_is_resolvable() {
    let catalog = crate::ResourceCatalog::new().unwrap();
    let versions = catalog
        .versions("YL57xq9U", "1.21.1", "fabric")
        .await
        .unwrap();
    let version = versions
        .iter()
        .find(|v| v.version_type == "release")
        .unwrap();
    let plan = remote::resolve(
        &catalog,
        &[remote::Pin {
            project: "YL57xq9U".into(),
            version: version.id.clone(),
        }],
        &[],
        "1.21.1",
        "fabric",
    )
    .await
    .unwrap();
    assert_eq!(
        plan.report.status,
        Status::Resolved,
        "{:?}",
        plan.report.issues
    );
    assert!(plan
        .selections
        .iter()
        .any(|pin| pin.project == "YL57xq9U" && pin.version == version.id));
    println!(
        "Resolved Iris {}: {} selected projects, {} metadata requests",
        version.id,
        plan.selections.len(),
        plan.requests
    );
}
struct Server {
    base: String,
    routes: Arc<Mutex<HashMap<String, serde_json::Value>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    async fn new() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let routes = Arc::new(Mutex::new(HashMap::<String, serde_json::Value>::new()));
        let shared = routes.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let shared = shared.clone();
                tokio::spawn(async move {
                    let mut request = vec![];
                    while !request.ends_with(b"\r\n\r\n") {
                        let Ok(byte) = stream.read_u8().await else {
                            return;
                        };
                        request.push(byte);
                        if request.len() > 8192 {
                            return;
                        }
                    }
                    let text = String::from_utf8_lossy(&request);
                    let path = text
                        .split_whitespace()
                        .nth(1)
                        .unwrap()
                        .split('?')
                        .next()
                        .unwrap();
                    let body = shared.lock().unwrap().get(path).cloned();
                    let status = if body.is_some() { 200 } else { 404 };
                    let body = body.unwrap_or(json!({})).to_string();
                    let _=stream.write_all(format!("HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await;
                });
            }
        });
        Self { base, routes, task }
    }
    fn put(&self, path: &str, value: serde_json::Value) {
        self.routes.lock().unwrap().insert(path.into(), value);
    }
    fn project(&self, id: &str) {
        self.put(
            &format!("/project/{id}"),
            json!({"id":id,"project_type":"mod","client_side":"required"}),
        );
    }
}
fn dep(project: &str, version: Option<&str>, kind: &str) -> serde_json::Value {
    json!({"project_id":project,"version_id":version,"dependency_type":kind})
}
fn v(project: &str, id: &str, deps: serde_json::Value) -> serde_json::Value {
    json!({"project_id":project,"id":id,"version_type":"release","game_versions":["1.21.1"],"loaders":["fabric"],"dependencies":deps})
}
#[tokio::test]
async fn remote_solver_backtracks_transitive_versions_preserves_installed_roots_and_checks_reverse_conflicts(
) {
    let server = Server::new().await;
    for project in ["main", "d", "b", "installed"] {
        server.project(project);
    }
    server.put(
        "/version/mainV",
        v("main", "mainV", json!([dep("d", None, "required")])),
    );
    server.put(
        "/version/installedV",
        v(
            "installed",
            "installedV",
            json!([dep("b", Some("b1"), "required")]),
        ),
    );
    server.put(
        "/project/d/version",
        json!([
            v("d", "d2", json!([dep("b", Some("b2"), "required")])),
            v("d", "d1", json!([dep("b", Some("b1"), "required")]))
        ]),
    );
    server.put("/version/b1", v("b", "b1", json!([])));
    server.put("/version/b2", v("b", "b2", json!([])));
    let catalog = crate::ResourceCatalog::with_base(server.base.clone());
    let plan = remote::resolve(
        &catalog,
        &[remote::Pin {
            project: "main".into(),
            version: "mainV".into(),
        }],
        &[remote::Pin {
            project: "installed".into(),
            version: "installedV".into(),
        }],
        "1.21.1",
        "fabric",
    )
    .await
    .unwrap();
    assert_eq!(plan.report.status, Status::Resolved);
    assert!(plan
        .selections
        .iter()
        .any(|p| p.project == "d" && p.version == "d1"));
    assert!(!plan.selections.iter().any(|p| p.version == "b2"));
    server.put(
        "/version/installedV",
        v(
            "installed",
            "installedV",
            json!([dep("main", None, "incompatible")]),
        ),
    );
    let plan = remote::resolve(
        &catalog,
        &[remote::Pin {
            project: "main".into(),
            version: "mainV".into(),
        }],
        &[remote::Pin {
            project: "installed".into(),
            version: "installedV".into(),
        }],
        "1.21.1",
        "fabric",
    )
    .await
    .unwrap();
    assert_eq!(plan.report.status, Status::Conflict);
}
#[tokio::test]
async fn provider_failure_is_unknown_and_does_not_masquerade_as_no_solution() {
    let server = Server::new().await;
    server.project("main");
    server.put(
        "/version/mainV",
        v(
            "main",
            "mainV",
            json!([dep("missing", Some("missingV"), "required")]),
        ),
    );
    let plan = remote::resolve(
        &crate::ResourceCatalog::with_base(server.base.clone()),
        &[remote::Pin {
            project: "main".into(),
            version: "mainV".into(),
        }],
        &[],
        "1.21.1",
        "fabric",
    )
    .await
    .unwrap();
    assert_eq!(plan.report.status, Status::Unknown);
    assert!(plan.selections.is_empty());
    assert!(plan
        .report
        .warnings
        .iter()
        .any(|w| w.code == "provider_unavailable"));
}
