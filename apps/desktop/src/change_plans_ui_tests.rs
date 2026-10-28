use super::*;
use crate::change_plans;
use iced::futures::StreamExt;
use mc_core::service::{
    instance_write::{self, Change},
    mcp_policy::McpPolicy,
};
struct Fixture {
    root: std::path::PathBuf,
    preview: instance_write::PlanPreview,
}
impl Fixture {
    async fn new() -> Self {
        let root = std::env::temp_dir().join(format!("memoria-plan-ui-{}", uuid::Uuid::new_v4()));
        let base = root.join("versions/test");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(
            base.join("test.json"),
            br#"{"id":"test","mainClass":"net.minecraft.client.main.Main","libraries":[]}"#,
        )
        .unwrap();
        std::fs::write(base.join("test.jar"), b"fixture").unwrap();
        let mut policy = McpPolicy {
            enabled: true,
            game_root: root.clone(),
            allowed_instances: vec!["versions/test".into()],
            ..Default::default()
        };
        policy.permissions.write_configs = true;
        let preview = instance_write::prepare(
            &policy,
            "versions/test",
            &Change::Config {
                path: "config/demo.json".into(),
                content: "{\"enabled\":true}".into(),
            },
        )
        .await
        .unwrap();
        Self { root, preview }
    }
    async fn app(&self, size: Size) -> Launcher {
        let (mut app, _) = Launcher::new();
        app.environment.game_root = self.root.clone();
        app.environment.game_roots = vec![self.root.clone()];
        app.viewport = size;
        let _ = app.update(Message::Loaded(Ok(crate::local::scan(self.root.clone())
            .await
            .unwrap())));
        app.accounts.busy = false;
        app
    }
    async fn open_review(&self, app: &mut Launcher) {
        let task = app.update(Message::EditInstance);
        complete(app, task).await;
        settle(app);
        for message in click(app, "变更计划与恢复") {
            let task = app.update(message);
            complete(app, task).await;
        }
        settle(app);
        assert_eq!(app.modal, Some(crate::app::Modal::ChangePlans));
        let title = format!("配置 · 1 个文件 · 待批准 · {}", self.preview.plan_id);
        for message in click(app, &title) {
            let task = app.update(message);
            complete(app, task).await;
        }
        settle(app);
        assert!(app.change_plans.selected.is_some());
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn settle(app: &mut Launcher) {
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::OPEN_DURATION + crate::motion::PAGE_DURATION,
    ));
}
async fn complete(app: &mut Launcher, task: iced::Task<Message>) {
    let mut pending = vec![task];
    while let Some(task) = pending.pop() {
        if let Some(mut stream) = iced_test::runtime::task::into_stream(task) {
            while let Some(action) = stream.next().await {
                if let iced_test::runtime::Action::Output(message) = action {
                    pending.push(app.update(message));
                }
            }
        }
    }
}
#[tokio::test]
async fn local_plan_approval_uses_real_controls_at_both_sizes_and_preserves_mcp_policy() {
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        let fixture = Fixture::new().await;
        let mut app = fixture.app(size).await;
        let policy = app.mcp_settings.saved.clone();
        fixture.open_review(&mut app).await;
        assert!(!fixture.root.join("versions/test/config/demo.json").exists());
        assert!(simulator(&app).find("批准并应用本次计划").is_ok());
        for message in click(&mut app, "批准并应用本次计划") {
            let task = app.update(message);
            complete(&mut app, task).await;
        }
        assert!(
            std::fs::read_to_string(fixture.root.join("versions/test/config/demo.json"))
                .unwrap()
                .contains("true")
        );
        assert_eq!(app.mcp_settings.saved, policy);
        assert!(app
            .change_plans
            .history
            .iter()
            .any(|entry| entry.state == "committed"));
    }
}
#[tokio::test]
async fn stale_generations_running_games_and_external_file_edits_prevent_approval() {
    let fixture = Fixture::new().await;
    let mut app = fixture.app(Size::new(1024., 680.)).await;
    fixture.open_review(&mut app).await;
    app.launching = true;
    assert_eq!(
        app.update(Message::ChangePlans(change_plans::Message::Approve))
            .units(),
        0
    );
    assert!(!fixture.root.join("versions/test/config/demo.json").exists());
    app.launching = false;
    let generation = app.change_plans.generation;
    let _ = app.update(Message::ChangePlans(change_plans::Message::Done(
        generation + 1,
        Ok("stale".into()),
    )));
    assert!(app.change_plans.selected.is_some());
    let path = fixture.root.join("versions/test/config/demo.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"external edit").unwrap();
    for message in click(&mut app, "批准并应用本次计划") {
        let task = app.update(message);
        complete(&mut app, task).await;
    }
    assert_eq!(std::fs::read(&path).unwrap(), b"external edit");
    assert!(app.change_plans.notice.contains("PLAN_STALE"));
}
#[tokio::test]
#[ignore = "writes plan approval screenshots"]
async fn capture_change_plan_review() {
    let directory = std::env::var("MEMORIA_UI_CAPTURE_DIR").unwrap();
    std::fs::create_dir_all(&directory).unwrap();
    let fixture = Fixture::new().await;
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        let mut app = fixture.app(size).await;
        fixture.open_review(&mut app).await;
        simulator(&app)
            .snapshot(&app.theme())
            .unwrap()
            .matches_image(format!("{directory}/plan-review-{}.png", size.width))
            .unwrap();
    }
}
#[tokio::test]
async fn startup_scan_recovers_before_presenting_instances_and_preserves_unknown_external_edits() {
    let fixture = Fixture::new().await;
    let base = fixture.root.join("versions/test");
    let id = uuid::Uuid::new_v4().to_string();
    let directory = base.join(format!(".memoria/transactions/{id}"));
    std::fs::create_dir_all(directory.join("before")).unwrap();
    std::fs::create_dir_all(base.join("config")).unwrap();
    std::fs::write(directory.join("before/0"), b"old").unwrap();
    std::fs::write(base.join("config/demo.json"), b"new").unwrap();
    std::fs::write(directory.join("journal.json"),serde_json::json!({"version":1,"id":id,"root":std::fs::canonicalize(&base).unwrap(),"origin":"startup_fixture","created":0,"entries":[{"path":"config/demo.json","before":instance_write::digest(b"old"),"after":instance_write::digest(b"new")}]}).to_string()).unwrap();
    let data = crate::local::scan(fixture.root.clone()).await.unwrap();
    assert_eq!(
        std::fs::read(base.join("config/demo.json")).unwrap(),
        b"old"
    );
    assert!(data.notice.contains("已恢复 1 个中断事务"));
    std::fs::remove_file(directory.join("rolled_back.json")).unwrap();
    std::fs::write(base.join("config/demo.json"), b"external").unwrap();
    let data = crate::local::scan(fixture.root.clone()).await.unwrap();
    assert!(data.notice.contains("恢复未完成"));
    assert_eq!(
        std::fs::read(base.join("config/demo.json")).unwrap(),
        b"external"
    );
}
