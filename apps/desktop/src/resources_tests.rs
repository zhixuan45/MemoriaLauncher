use super::*;
fn page(offset: usize, id: &str) -> ResourcePage {
    ResourcePage {
        hits: vec![ResourceProject {
            project_id: id.into(),
            slug: id.into(),
            title: id.into(),
            description: "Fixture".into(),
            author: "author".into(),
            downloads: 1,
            categories: vec![],
            versions: vec![],
        }],
        offset,
        limit: 20,
        total_hits: 45,
    }
}
#[test]
fn switching_categories_rejects_old_search_and_detail_responses() {
    let mut state = State::default();
    let _ = state.category(1);
    let first = state.generation;
    let _ = state.update(Message::Loaded(first, Ok(page(0, "mod"))));
    let _ = state.update(Message::Select("mod".into()));
    let detail = state.detail_generation;
    assert!(state.detail_loading);
    let _ = state.category(4);
    let _ = state.update(Message::Loaded(first, Ok(page(0, "stale"))));
    let _ = state.update(Message::Versions(detail, "mod".into(), Ok(vec![])));
    assert!(state.loading);
    assert!(state.page.is_none());
    assert!(state.selected.is_none());
    let _ = state.update(Message::Loaded(state.generation, Ok(page(0, "shader"))));
    assert_eq!(state.page.as_ref().unwrap().hits[0].title, "shader");
    assert_eq!(state.kind, ResourceKind::Shader);
    assert!(!state.loading);
}
#[test]
fn pagination_and_details_use_the_applied_filters_and_retry_after_failure() {
    let mut state = State::default();
    let _ = state.update(Message::Query("sodium".into()));
    let _ = state.update(Message::Version("1.21.1".into()));
    let _ = state.update(Message::Loader("Fabric"));
    let _ = state.update(Message::Search);
    let _ = state.update(Message::Loaded(state.generation, Ok(page(0, "sodium"))));
    let _ = state.update(Message::Query("draft".into()));
    let _ = state.update(Message::Page(true));
    assert_eq!(state.applied.as_ref().unwrap().query, "sodium");
    assert_eq!(state.applied.as_ref().unwrap().offset, 20);
    assert_eq!(state.applied.as_ref().unwrap().loader, "fabric");
    let _ = state.update(Message::Loaded(state.generation, Err("HTTP 429".into())));
    assert!(!state.loading);
    assert!(state.notice.contains("429"));
    assert!(state.page.is_none());
    let _ = state.update(Message::Search);
    assert!(state.loading);
    assert_eq!(state.applied.as_ref().unwrap().query, "draft");
    assert_eq!(state.applied.as_ref().unwrap().offset, 0);
    let _ = state.update(Message::Loaded(state.generation, Ok(page(0, "current"))));
    assert_eq!(state.update(Message::Select("absent".into())).units(), 0);
    assert!(state.selected.is_none());
}
