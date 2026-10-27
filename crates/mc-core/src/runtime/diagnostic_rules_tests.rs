use super::super::diagnostics::analyze_with_rules;
use super::*;
use serde_json::json;
#[test]
fn custom_rule_import_preserves_invalid_updates_and_keeps_a_reset_backup() {
    let root = std::env::temp_dir().join(format!("memoria-rules-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let source = root.join("import.json");
    let saved = root.join("diagnostic-rules.json");
    std::fs::write(
        &source,
        serde_json::to_vec(&json!({"version":1,"rules":[rule()]})).unwrap(),
    )
    .unwrap();
    import_custom(&source, &saved).unwrap();
    let original = std::fs::read(&saved).unwrap();
    assert_eq!(
        RuleSet::load(&saved).unwrap().len(),
        RuleSet::builtin().len() + 1
    );
    std::fs::write(&source, b"broken json").unwrap();
    assert!(import_custom(&source, &saved).is_err());
    assert_eq!(std::fs::read(&saved).unwrap(), original);
    restore_builtin(&saved).unwrap();
    assert!(!saved.exists());
    assert_eq!(
        RuleSet::load(&saved).unwrap().len(),
        RuleSet::builtin().len()
    );
    assert!(std::fs::read_dir(&root).unwrap().any(|entry| entry
        .unwrap()
        .path()
        .extension()
        .is_some_and(|extension| extension == "bak")));
    std::fs::remove_dir_all(root).unwrap();
}
fn rule() -> serde_json::Value {
    json!({"id":"fixture_rule","category":"fixture","title":"示例异常","action":"人工核对","severity":"warning","confidence":"medium","priority":90,"any":["(?m)^ERROR (?P<mod_id>[a-z]+)$"],"all":["Caused by:"],"exclude":["optional"],"context_lines":1,"capture_fields":["mod_id"]})
}
#[test]
fn rule_packs_support_context_all_exclusions_named_captures_and_line_evidence() {
    let rules =
        RuleSet::custom(&serde_json::to_vec(&json!({"version":1,"rules":[rule()]})).unwrap())
            .unwrap();
    let report = analyze_with_rules("preface\nERROR example\nCaused by: fixture\n", None, &rules);
    let finding = report
        .findings
        .iter()
        .find(|f| f.rule_id == "fixture_rule")
        .unwrap();
    assert_eq!(finding.mod_id.as_deref(), Some("example"));
    assert_eq!(finding.source_lines, vec![2]);
    assert_eq!(finding.severity, "warning");
    let report = analyze_with_rules("ERROR example\nCaused by: optional fixture\n", None, &rules);
    assert!(report.findings.iter().all(|f| f.rule_id != "fixture_rule"));
    assert!(analyze_with_rules("ERROR example\nno cause", None, &rules)
        .findings
        .iter()
        .all(|f| f.rule_id != "fixture_rule"));
}
#[test]
fn invalid_rule_versions_regexes_duplicate_ids_fields_and_budgets_are_rejected() {
    let mut invalid_regex = rule();
    invalid_regex["any"] = json!(["(?<=unsupported)regex"]);
    assert!(RuleSet::custom(
        &serde_json::to_vec(&json!({"version":1,"rules":[invalid_regex]})).unwrap()
    )
    .is_err());
    for value in [
        json!({"version":2,"rules":[]}),
        json!({"version":1,"rules":[rule(),rule()]}),
        json!({"version":1,"rules":[{"any":["(?<=invalid)regex"]}]}),
        json!({"version":1,"rules":[],"execute":"shell"}),
    ] {
        assert!(RuleSet::custom(&serde_json::to_vec(&value).unwrap()).is_err());
    }
    let mut bad = rule();
    bad["capture_fields"] = json!(["password"]);
    assert!(
        RuleSet::custom(&serde_json::to_vec(&json!({"version":1,"rules":[bad]})).unwrap()).is_err()
    );
    assert!(RuleSet::custom(&vec![b' '; 128 * 1024 + 1]).is_err());
}
#[test]
fn structured_forge_dependencies_and_match_limits_do_not_claim_complete_analysis() {
    let text="Missing or unsupported mandatory dependencies:\nMod ID: 'dependency', Requested by: 'main', Expected range: '[2,)', Actual version: '[MISSING]'";
    let report = super::super::diagnostics::analyze_text(text, None);
    let f = report
        .findings
        .iter()
        .find(|f| f.rule_id == "forge_missing_dependency")
        .unwrap();
    assert_eq!(f.mod_id.as_deref(), Some("main"));
    assert_eq!(f.required_mod_id.as_deref(), Some("dependency"));
    assert_eq!(f.required_version.as_deref(), Some("[2,)"));
    let report = super::super::diagnostics::analyze_text(&"OutOfMemoryError\n".repeat(1024), None);
    assert!(report.rule_limit_reached);
    assert!(report.findings.iter().all(|f| f.evidence.len() <= 3));
}
