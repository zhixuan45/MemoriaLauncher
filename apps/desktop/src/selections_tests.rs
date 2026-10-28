use super::*;
use crate::{
    app::{Message, Page},
    resource_install, workspace,
};
use iced::futures::StreamExt;

struct Fixture {
    directory: PathBuf,
    root: PathBuf,
    path: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let directory =
            std::env::temp_dir().join(format!("memoria-selections-{}", uuid::Uuid::new_v4()));
        let root = directory.join("game");
        for id in ["a", "b", "c"] {
            create_instance(&root, id);
        }
        Self {
            path: directory.join("data/selections.json"),
            root,
            directory,
        }
    }
    async fn app(&self) -> Launcher {
        let (mut app, _) = Launcher::new();
        app.environment.game_root = self.root.clone();
        app.environment.game_roots = vec![self.root.clone()];
        app.selections = State::load_from(self.path.clone());
        app.accounts.busy = false;
        let _ = app.update(Message::Loaded(Ok(crate::local::scan(self.root.clone())
            .await
            .unwrap())));
        app
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}
fn create_instance(root: &Path, id: &str) {
    let base = root.join("versions").join(id);
    fs::create_dir_all(&base).unwrap();
    fs::write(base.join(format!("{id}.json")),serde_json::json!({"id":id,"clientVersion":"1.21.1","mainClass":"net.minecraft.client.main.Main","libraries":[]}).to_string()).unwrap();
    fs::write(base.join(format!("{id}.jar")), b"fixture").unwrap();
}
fn installation(app: &mut Launcher) -> iced::Task<Message> {
    let _ = app.download.update(crate::download::Message::Category(1));
    app.download.resources.page=Some(serde_json::from_value(serde_json::json!({"hits":[{"project_id":"main","slug":"sample","title":"示例资源","description":"测试","author":"fixture","downloads":0,"categories":[],"versions":[]}],"total_hits":1,"offset":0,"limit":20})).unwrap());
    app.download.resources.selected = Some("main".into());
    app.download.resources.versions=vec![serde_json::from_value(serde_json::json!({"id":"mainV","name":"示例版本","version_number":"1","version_type":"release","game_versions":["1.21.1"],"loaders":[],"files":[],"dependencies":[]})).unwrap()];
    app.update(Message::Download(crate::download::Message::Resources(
        crate::resources::Message::Install("mainV".into()),
    )))
}
async fn complete(app: &mut Launcher, task: iced::Task<Message>) {
    let mut tasks = vec![task];
    while let Some(task) = tasks.pop() {
        if let Some(mut stream) = iced_test::runtime::task::into_stream(task) {
            while let Some(action) = stream.next().await {
                if let iced_test::runtime::Action::Output(message) = action {
                    tasks.push(app.update(message));
                }
            }
        }
    }
}

#[tokio::test]
async fn restart_restores_independent_home_and_ai_choices_and_uses_last_launch_for_mods() {
    let fixture = Fixture::new();
    let mut app = fixture.app().await;
    let _ = app.update(Message::SelectInstance("versions/a".into()));
    let _ = app.update(Message::Launch); // Records the explicit target even when account validation cannot continue.
    assert!(!app.launching);
    let _ = app.update(Message::SelectInstance("versions/c".into()));
    let _ = app.update(Message::Workspace(workspace::Message::OpenProject(
        "versions/b".into(),
    )));
    assert_eq!(app.active, "versions/c");
    assert_eq!(app.page_instance().unwrap().instance.id, "versions/b");
    let _ = app.update(Message::Workspace(workspace::Message::NewChat));
    assert_eq!(
        app.workspace.current.project.as_ref().unwrap().instance_id,
        "versions/b"
    );
    let _ = app.update(Message::Quit);
    let mut restarted = fixture.app().await;
    assert_eq!(restarted.active, "versions/c");
    assert_eq!(
        restarted
            .workspace
            .current
            .project
            .as_ref()
            .unwrap()
            .instance_id,
        "versions/b"
    );
    assert!(restarted.ai.project_context.as_ref().unwrap().contains('b'));
    assert_eq!(
        restarted
            .ai
            .attachments
            .target
            .as_ref()
            .unwrap()
            .instance_id,
        "versions/b"
    );
    let _ = restarted.update(Message::Navigate(Page::Home));
    assert_eq!(restarted.page_instance().unwrap().instance.id, "versions/c");
    let task = installation(&mut restarted);
    complete(&mut restarted, task).await;
    assert_eq!(
        restarted
            .resource_install
            .request
            .as_ref()
            .unwrap()
            .instance,
        "versions/a"
    );
    assert_eq!(restarted.resource_install.root, fixture.root);
    // Neither opening the installer nor a later unrelated selection changes its pinned target.
    let _ = restarted.update(Message::SelectInstance("versions/b".into()));
    assert_eq!(
        restarted
            .resource_install
            .request
            .as_ref()
            .unwrap()
            .instance,
        "versions/a"
    );
}

#[tokio::test]
async fn collection_identity_and_last_launch_target_survive_same_ids_in_other_folders() {
    let fixture = Fixture::new();
    let other = fixture.directory.join("other");
    create_instance(&other, "a");
    let mut app = fixture.app().await;
    let _ = app.update(Message::SelectInstance("versions/c".into()));
    let _ = app.update(Message::Workspace(workspace::Message::OpenProject(
        "versions/b".into(),
    )));
    app.selections.launched(&other, "versions/a").unwrap();
    app.environment.game_roots.push(other.clone());
    let task = installation(&mut app);
    assert!(app.resource_install.busy);
    complete(&mut app, task).await;
    assert!(!app.resource_install.busy);
    let request = app.resource_install.request.as_ref().unwrap();
    assert_eq!(request.root, other);
    assert_eq!(request.instance, "versions/a");
    assert_eq!(app.resource_install.targets.len(), 4);
    let home = app
        .resource_install
        .targets
        .iter()
        .find(|target| target.id == "versions/a" && target.root == fixture.root)
        .unwrap()
        .clone();
    let _ = app.update(Message::ResourceInstall(resource_install::Message::Target(
        home,
    )));
    assert_eq!(
        app.resource_install.request.as_ref().unwrap().root,
        fixture.root
    );
    let generation = app.resource_install.generation;
    let _ = app.update(Message::ResourceInstall(
        resource_install::Message::LastTargetLoaded(
            generation + 1,
            Reference {
                root: other.clone(),
                instance_id: "versions/a".into(),
            },
            Err("stale".into()),
        ),
    ));
    assert_eq!(
        app.resource_install.request.as_ref().unwrap().root,
        fixture.root
    );
    let mut records = State::load_from(fixture.path.clone());
    assert_eq!(records.home(&fixture.root), Some("versions/c"));
    assert_eq!(records.home(&other), None);
    records.remember(&other, "versions/a", None).unwrap();
    assert_eq!(
        State::load_from(fixture.path.clone()).home(&fixture.root),
        Some("versions/c")
    );
}

#[tokio::test]
async fn missing_instances_fall_back_without_changing_a_different_collections_same_id() {
    let fixture = Fixture::new();
    let mut records = State::load_from(fixture.path.clone());
    records
        .remember(&fixture.root, "versions/missing", Some("versions/missing"))
        .unwrap();
    records.launched(&fixture.root, "versions/missing").unwrap();
    let mut app = fixture.app().await;
    assert!(app.current().is_some());
    assert!(app.workspace.current.project.is_none());
    let active = app.active.clone();
    let task = installation(&mut app);
    complete(&mut app, task).await;
    assert_eq!(
        app.resource_install.request.as_ref().unwrap().instance,
        active
    );
    assert!(app.resource_install.notice.contains("回退"));
    let unconfigured = fixture.directory.join("removed-collection");
    create_instance(&unconfigured, "a");
    app.selections
        .launched(&unconfigured, "versions/a")
        .unwrap();
    let task = installation(&mut app);
    complete(&mut app, task).await;
    assert_eq!(
        app.resource_install.request.as_ref().unwrap().root,
        fixture.root
    );
}

#[tokio::test]
async fn closing_before_scan_does_not_erase_saved_selections_and_bad_files_are_preserved() {
    let fixture = Fixture::new();
    let mut records = State::load_from(fixture.path.clone());
    records
        .remember(&fixture.root, "versions/c", Some("versions/b"))
        .unwrap();
    let original = fs::read(&fixture.path).unwrap();
    let (mut app, _) = Launcher::new();
    app.environment.game_root = fixture.root.clone();
    app.selections = State::load_from(fixture.path.clone());
    let _ = app.update(Message::Close);
    assert_eq!(fs::read(&fixture.path).unwrap(), original);
    fs::write(&fixture.path, b"invalid JSON").unwrap();
    let mut broken = State::load_from(fixture.path.clone());
    assert!(broken.notice.is_some());
    broken.remember(&fixture.root, "versions/a", None).unwrap();
    assert_eq!(fs::read(&fixture.path).unwrap(), b"invalid JSON");
    fs::write(&fixture.path, vec![b' '; LIMIT + 1]).unwrap();
    assert!(State::load_from(fixture.path.clone()).notice.is_some());
}
