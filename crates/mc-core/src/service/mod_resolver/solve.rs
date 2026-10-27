use super::version::Match;
use super::*;
use std::collections::{BTreeSet, HashSet};

const MAX_STEPS: usize = 20000;
fn constraints_valid(value: &Constraint, depth: usize) -> bool {
    if depth > 16 {
        return false;
    }
    match value {
        Constraint::AnyOf(v) | Constraint::AllOf(v) => {
            !v.is_empty() && v.len() <= 32 && v.iter().all(|c| constraints_valid(c, depth + 1))
        }
        Constraint::Range { expression, .. }
        | Constraint::Exact(expression)
        | Constraint::Unknown(expression) => expression.len() <= 1024,
        _ => true,
    }
}
pub fn solve(candidates: &[Candidate], roots: &[Root], environment: Environment) -> Report {
    let empty = |status, issues| Report {
        status,
        selected: vec![],
        issues,
        warnings: vec![],
        edges: vec![],
        cycles: vec![],
        changes: vec![],
        steps: 0,
    };
    if candidates.len() > 2048 || roots.len() > 256 {
        return empty(
            Status::LimitReached,
            vec![issue("input_limit", "", "候选或根依赖超出求解上限", vec![])],
        );
    }
    if candidates.iter().any(|c| {
        !identifier(&c.id)
            || c.version.len() > 200
            || c.source.len() > 1024
            || c.origin.len() > 1024
            || c.provides.len() > 128
            || c.provided_versions.len() > 128
            || c.provided_versions
                .iter()
                .any(|(id, version)| !c.provides.contains(id) || id == &c.id || version.len() > 200)
            || c.provides.iter().any(|id| !identifier(id))
            || c.dependencies.len() > 256
            || c.dependencies
                .iter()
                .any(|d| !identifier(&d.id) || !constraints_valid(&d.constraint, 0))
    }) || roots
        .iter()
        .any(|r| !identifier(&r.id) || !constraints_valid(&r.constraint, 0))
    {
        return empty(
            Status::Unknown,
            vec![issue(
                "invalid_metadata",
                "",
                "候选标识、约束或依赖元数据无效",
                vec![],
            )],
        );
    }
    let mut state = Search {
        candidates,
        roots,
        environment,
        steps: 0,
        work: 0,
        last: vec![],
        limit: false,
    };
    let Some(selected) = state.visit(vec![]) else {
        let mut report = empty(
            if state.limit {
                Status::LimitReached
            } else {
                Status::Conflict
            },
            state.last,
        );
        report.steps = state.steps;
        return report;
    };
    let chosen: Vec<_> = selected.iter().map(|i| candidates[*i].clone()).collect();
    let mut warnings = vec![];
    let mut edges = vec![];
    for candidate in &chosen {
        if !candidate.metadata_complete || candidate.environment == Environment::Unknown {
            warnings.push(issue(
                "unknown_metadata",
                &candidate.id,
                "部分元数据或运行环境不明确，不能保证兼容",
                vec![candidate.source.clone()],
            ));
        }
        for dependency in &candidate.dependencies {
            edges.push(Edge {
                from: candidate.id.clone(),
                to: dependency.id.clone(),
                relation: dependency.relation.clone(),
                constraint: dependency.constraint.clone(),
            });
            let present = chosen.iter().find(|c| c.supplies(&dependency.id));
            let matched = present
                .map(|c| version::matches(c.version_for(&dependency.id), &dependency.constraint));
            if matched == Some(Match::Unknown) {
                warnings.push(issue(
                    "unknown_constraint",
                    &dependency.id,
                    "无法可靠判断版本表达式",
                    vec![candidate.id.clone(), format!("{:?}", dependency.constraint)],
                ));
            }
            if matches!(
                dependency.relation,
                Relation::Optional | Relation::Recommended
            ) && (present.is_none() || matched != Some(Match::Yes))
            {
                warnings.push(issue(
                    "optional_dependency",
                    &dependency.id,
                    "可选或推荐前置未满足，未强制添加",
                    vec![candidate.id.clone(), dependency.reason.clone()],
                ));
            }
            if dependency.relation == Relation::Discouraged && matched == Some(Match::Yes) {
                warnings.push(issue(
                    "soft_conflict",
                    &dependency.id,
                    "元数据声明不推荐共存，需人工确认",
                    vec![candidate.id.clone(), dependency.reason.clone()],
                ));
            }
        }
    }
    for root in roots {
        if chosen
            .iter()
            .find(|c| c.supplies(&root.id))
            .is_some_and(|c| {
                version::matches(c.version_for(&root.id), &root.constraint) == Match::Unknown
            })
        {
            warnings.push(issue(
                "unknown_constraint",
                &root.id,
                "根版本约束无法确定",
                vec![root.reason.clone()],
            ));
        }
    }
    let unknown = warnings
        .iter()
        .any(|w| matches!(w.code.as_str(), "unknown_metadata" | "unknown_constraint"));
    let cycles = cycles(&chosen);
    let changes = chosen
        .iter()
        .map(|c| Change {
            action: if c.installed { "keep" } else { "add" }.into(),
            id: c.id.clone(),
            version: c.version.clone(),
            source: c.source.clone(),
            reason: roots
                .iter()
                .find(|r| c.supplies(&r.id))
                .map(|r| r.reason.clone())
                .unwrap_or_else(|| "满足传递前置依赖".into()),
        })
        .collect();
    Report {
        status: if unknown {
            Status::Unknown
        } else {
            Status::Resolved
        },
        selected: chosen,
        issues: vec![],
        warnings,
        edges,
        cycles,
        changes,
        steps: state.steps,
    }
}
struct Search<'a> {
    candidates: &'a [Candidate],
    roots: &'a [Root],
    environment: Environment,
    steps: usize,
    work: usize,
    last: Vec<Issue>,
    limit: bool,
}
impl Search<'_> {
    fn visit(&mut self, selected: Vec<usize>) -> Option<Vec<usize>> {
        self.steps += 1;
        if self.steps > MAX_STEPS || selected.len() > 256 {
            self.limit = true;
            self.last = vec![issue(
                "search_limit",
                "",
                "求解达到预算，未证明有解或无解",
                vec![],
            )];
            return None;
        }
        let mut requirements: Vec<(String, Constraint, String)> = self
            .roots
            .iter()
            .map(|r| (r.id.clone(), r.constraint.clone(), r.reason.clone()))
            .collect();
        for index in &selected {
            let candidate = &self.candidates[*index];
            for dependency in &candidate.dependencies {
                if dependency.relation == Relation::Required {
                    requirements.push((
                        dependency.id.clone(),
                        dependency.constraint.clone(),
                        format!(
                            "{} {} → {}：{}",
                            candidate.id, candidate.version, dependency.id, dependency.reason
                        ),
                    ));
                }
                if dependency.relation == Relation::Conflict {
                    if let Some(other) = selected.iter().map(|i| &self.candidates[*i]).find(|c| {
                        c.supplies(&dependency.id)
                            && version::matches(
                                c.version_for(&dependency.id),
                                &dependency.constraint,
                            ) == Match::Yes
                    }) {
                        self.last = vec![issue(
                            "incompatible",
                            &dependency.id,
                            format!(
                                "{} {} 与 {} {} 不兼容",
                                candidate.id, candidate.version, other.id, other.version
                            ),
                            vec![dependency.reason.clone()],
                        )];
                        return None;
                    }
                }
            }
        }
        let mut missing = vec![];
        for (id, constraint, reason) in &requirements {
            if let Some(present) = selected
                .iter()
                .map(|i| &self.candidates[*i])
                .find(|c| c.supplies(id))
            {
                if version::matches(present.version_for(id), constraint) == Match::No {
                    self.last = vec![issue(
                        "version_conflict",
                        id,
                        format!(
                            "已选择 {} {}，不满足 {:?}",
                            present.id, present.version, constraint
                        ),
                        requirements
                            .iter()
                            .filter(|(key, _, _)| key == id)
                            .map(|(_, _, r)| r.clone())
                            .collect(),
                    )];
                    return None;
                }
            } else {
                missing.push((id.clone(), constraint.clone(), reason.clone()));
            }
        }
        if missing.is_empty() {
            return Some(selected);
        }
        let mut next: Option<(String, Vec<usize>, Vec<String>)> = None;
        let mut seen = HashSet::new();
        for (id, _, _) in &missing {
            if !seen.insert(id.clone()) {
                continue;
            }
            let needs: Vec<_> = requirements
                .iter()
                .filter(|(key, _, _)| key == id)
                .collect();
            self.work = self.work.saturating_add(
                self.candidates
                    .len()
                    .saturating_mul(needs.len() + selected.len() + 1),
            );
            if self.work > 10_000_000 {
                self.limit = true;
                self.last = vec![issue(
                    "search_limit",
                    id,
                    "候选检查达到工作预算，未证明无解",
                    vec![],
                )];
                return None;
            }
            let mut options: Vec<_> = self
                .candidates
                .iter()
                .enumerate()
                .filter(|(index, c)| {
                    !selected.contains(index)
                        && c.supplies(id)
                        && self.environment_allowed(c)
                        && needs.iter().all(|(_, constraint, _)| {
                            version::matches(c.version_for(id), constraint) != Match::No
                        })
                        && !selected.iter().any(|i| overlap(c, &self.candidates[*i]))
                })
                .map(|(i, _)| i)
                .collect();
            options.sort_by_key(|index| {
                let c = &self.candidates[*index];
                (
                    needs.iter().any(|(_, requirement, _)| {
                        version::matches(c.version_for(id), requirement) == Match::Unknown
                    }),
                    !c.installed,
                    !c.metadata_complete,
                )
            });
            if next
                .as_ref()
                .is_none_or(|(_, old, _)| options.len() < old.len())
            {
                next = Some((
                    id.clone(),
                    options,
                    needs.iter().map(|(_, _, reason)| reason.clone()).collect(),
                ));
            }
            if next
                .as_ref()
                .is_some_and(|(_, options, _)| options.len() <= 1)
            {
                break;
            }
        }
        let (id, options, reasons) = next.unwrap();
        if options.is_empty() {
            self.last = vec![issue(
                "missing_or_incompatible",
                &id,
                "候选集中没有同时满足全部版本、运行环境及身份约束的前置",
                reasons,
            )];
            return None;
        }
        for option in options {
            let mut branch = selected.clone();
            let candidate = &self.candidates[option];
            let bundle: Vec<_> = if candidate.source == "environment" || candidate.source.is_empty()
            {
                vec![option]
            } else {
                self.candidates
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.source == candidate.source && c.origin == candidate.origin)
                    .map(|(i, _)| i)
                    .collect()
            };
            let mut valid = true;
            for index in bundle {
                if branch.contains(&index) {
                    continue;
                }
                if !self.environment_allowed(&self.candidates[index])
                    || branch
                        .iter()
                        .any(|i| overlap(&self.candidates[index], &self.candidates[*i]))
                {
                    valid = false;
                    break;
                }
                branch.push(index);
            }
            if !valid {
                self.last = vec![issue(
                    "bundle_conflict",
                    &id,
                    "同一个 JAR 内的多个 Mod 必须作为整体选择，不能混用不同文件的身份",
                    reasons.clone(),
                )];
                continue;
            }
            if let Some(answer) = self.visit(branch) {
                return Some(answer);
            }
            if self.limit {
                return None;
            }
        }
        None
    }
    fn environment_allowed(&self, candidate: &Candidate) -> bool {
        matches!(
            (&self.environment, &candidate.environment),
            (
                Environment::Client,
                Environment::Any | Environment::Client | Environment::Unknown
            ) | (
                Environment::Server,
                Environment::Any | Environment::Server | Environment::Unknown
            ) | (Environment::Any | Environment::Unknown, _)
        )
    }
}
fn overlap(a: &Candidate, b: &Candidate) -> bool {
    a.supplies(&b.id) || b.supplies(&a.id) || a.provides.iter().any(|id| b.supplies(id))
}
fn cycles(candidates: &[Candidate]) -> Vec<Vec<String>> {
    fn walk(
        id: &str,
        candidates: &[Candidate],
        path: &mut Vec<String>,
        done: &mut HashSet<String>,
        found: &mut BTreeSet<Vec<String>>,
    ) {
        if let Some(start) = path.iter().position(|key| key == id) {
            let mut cycle = path[start..].to_vec();
            cycle.sort();
            found.insert(cycle);
            return;
        }
        if !done.insert(id.into()) {
            return;
        }
        path.push(id.into());
        if let Some(candidate) = candidates.iter().find(|c| c.id == id) {
            for dep in candidate
                .dependencies
                .iter()
                .filter(|d| d.relation == Relation::Required)
            {
                if let Some(target) = candidates.iter().find(|c| c.supplies(&dep.id)) {
                    walk(&target.id, candidates, path, done, found);
                }
            }
        }
        path.pop();
    }
    let mut found = BTreeSet::new();
    let mut done = HashSet::new();
    for candidate in candidates {
        walk(
            &candidate.id,
            candidates,
            &mut vec![],
            &mut done,
            &mut found,
        );
    }
    found.into_iter().collect()
}
