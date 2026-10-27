use crate::{
    app::Message,
    motion_renderer::Element,
    theme::{bold, label, MUTED},
};
use iced::widget::column;
use mc_core::service::mod_resolver::{Report, Status};
pub fn view(report: &Report) -> Element<'_, Message> {
    let mut body = column![
        bold(
            match report.status {
                Status::Resolved => "静态依赖约束可满足",
                Status::Conflict => "前置或冲突约束不满足",
                Status::Unknown => "存在未知项，不能保证兼容",
                Status::LimitReached => "达到求解预算，尚未完成",
            },
            15.
        ),
        label("只读检查与变更建议；此处不会启停、删除或安装 JAR。", 11.).color(MUTED)
    ]
    .spacing(8);
    for issue in report.issues.iter().chain(&report.warnings).take(32) {
        body = body.push(label(format!("{}：{}", issue.subject, issue.message), 12.));
        if !issue.chain.is_empty() {
            body = body.push(label(issue.chain.join(" → "), 10.).color(MUTED));
        }
    }
    for change in report
        .changes
        .iter()
        .filter(|change| change.source != "environment")
        .take(64)
    {
        body = body.push(label(
            format!(
                "{} · {} {} · {}",
                match change.action.as_str() {
                    "keep" => "保留",
                    "replace_review" | "update" => "建议替换，需确认",
                    _ => "建议加入",
                },
                change.id,
                change.version,
                change.source
            ),
            11.,
        ));
    }
    for cycle in &report.cycles {
        body = body
            .push(label(format!("依赖环（已共同满足）：{}", cycle.join(" → ")), 10.).color(MUTED));
    }
    for edge in report
        .edges
        .iter()
        .filter(|edge| edge.relation == mc_core::service::mod_resolver::Relation::Required)
        .take(64)
    {
        body = body.push(
            label(
                format!(
                    "前置：{} → {} · {}",
                    edge.from,
                    edge.to,
                    constraint(&edge.constraint)
                ),
                10.,
            )
            .color(MUTED),
        );
    }
    body.into()
}
fn constraint(value: &mc_core::service::mod_resolver::Constraint) -> String {
    use mc_core::service::mod_resolver::Constraint::*;
    match value {
        Any => "任意版本".into(),
        Exact(v) => format!("固定 {v}"),
        Range { expression, .. } => expression.clone(),
        AnyOf(values) => values
            .iter()
            .map(constraint)
            .collect::<Vec<_>>()
            .join(" 或 "),
        AllOf(values) => values
            .iter()
            .map(constraint)
            .collect::<Vec<_>>()
            .join(" 且 "),
        Unknown(reason) => format!("未知：{reason}"),
    }
}
