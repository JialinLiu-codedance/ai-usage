use crate::{
    app_time, git_usage,
    models::{
        GitUsageReport, LocalTokenUsageRange, LocalTokenUsageReport, PrKpiMetric, PrKpiMetricKey,
        PrKpiOverview, PrKpiReport, CUSTOM_USAGE_WINDOW_DAYS,
    },
};
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use reqwest::{
    blocking::Client,
    header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT},
    StatusCode,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    process::Command,
    time::Duration as StdDuration,
};

const GITHUB_API_BASE: &str = "https://api.github.com";
const GITHUB_ACCEPT: &str = "application/vnd.github+json";
const GITHUB_API_VERSION: &str = "2022-11-28";
const GITHUB_USER_AGENT: &str = "ai-usage-pr-kpi/0.1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrKpiCache {
    #[serde(default)]
    pub root_path: String,
    #[serde(with = "crate::app_time::local_datetime_serde")]
    pub generated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_login: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_window_start: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_window_end: Option<NaiveDate>,
    #[serde(default)]
    pub pull_requests: Vec<PrKpiPullRequestRecord>,
    #[serde(default)]
    pub missing_sources: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrKpiPullRequestRecord {
    pub repository_owner: String,
    pub repository_name: String,
    pub repository_path: String,
    pub number: u64,
    #[serde(with = "crate::app_time::local_datetime_serde")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "crate::app_time::local_datetime_serde")]
    pub merged_at: DateTime<Utc>,
    pub review_comments: u64,
    pub additions: u64,
    pub test_additions: u64,
    #[serde(default)]
    pub is_ai_assisted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_stability: Option<PrKpiLocalStability>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PrKpiLocalStability {
    pub added_lines: u64,
    pub reworked_lines: u64,
    pub retained_lines: u64,
}

#[derive(Debug, Clone)]
struct RepositorySource {
    owner: String,
    name: String,
    path: PathBuf,
}

#[derive(Debug, Clone, Copy)]
struct DateRangeWindow {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    inclusive_days: i64,
}

#[derive(Debug, Clone, Copy)]
enum DiffRangeSide {
    Old,
    New,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LineRange {
    start: usize,
    count: usize,
}

#[derive(Debug, Deserialize)]
struct GithubViewer {
    login: String,
}

#[derive(Debug, Deserialize)]
struct GithubSearchResponse {
    items: Vec<GithubSearchItem>,
}

#[derive(Debug, Deserialize)]
struct GithubSearchItem {
    number: u64,
}

#[derive(Debug, Deserialize)]
struct GithubPullRequestDetail {
    number: u64,
    created_at: DateTime<Utc>,
    merged_at: Option<DateTime<Utc>>,
    review_comments: u64,
    #[serde(default)]
    merge_commit_sha: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GithubPullRequestFile {
    filename: String,
    additions: u64,
}

impl PrKpiCache {
    pub fn report(&self, range: LocalTokenUsageRange, overview: PrKpiOverview) -> PrKpiReport {
        let window = range_window(range, self.generated_at);
        build_report(
            &self.pull_requests,
            range,
            window.start,
            window.end,
            window.inclusive_days,
            None,
            None,
            overview,
            self.generated_at,
            self.missing_sources.clone(),
            self.warnings.clone(),
        )
    }

    pub fn covers_custom_range(&self, start_date: NaiveDate, end_date: NaiveDate) -> bool {
        matches!(
            (self.custom_window_start, self.custom_window_end),
            (Some(window_start), Some(window_end))
                if start_date >= window_start && end_date <= window_end
        )
    }

    pub fn custom_report(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        overview: PrKpiOverview,
    ) -> PrKpiReport {
        let offset = app_time::local_offset();
        let start = app_time::local_start_of_day_utc(start_date, offset);
        let end = app_time::local_end_of_day_utc(end_date, offset);
        build_report(
            &self.pull_requests,
            LocalTokenUsageRange::Custom,
            start,
            end,
            (end_date - start_date).num_days() + 1,
            Some(start_date.format("%Y-%m-%d").to_string()),
            Some(end_date.format("%Y-%m-%d").to_string()),
            overview,
            self.generated_at,
            self.missing_sources.clone(),
            self.warnings.clone(),
        )
    }
}

pub fn build_cache(root: PathBuf, github_token: Option<String>) -> Result<PrKpiCache, String> {
    let now = Utc::now();
    let offset = app_time::local_offset();
    let window_end = app_time::local_date(now, offset);
    let window_start = window_end - Duration::days(CUSTOM_USAGE_WINDOW_DAYS - 1);
    let root_path = root.to_string_lossy().to_string();
    let repositories = git_usage::discover_git_repositories(&root)
        .map_err(|error| format!("扫描本地 Git 仓库失败: {error}"))?;
    let sources = discover_github_repository_sources(repositories);
    let mut warnings = BTreeSet::new();
    let missing_sources = BTreeSet::new();

    let Some(token) = github_token.filter(|value| !value.trim().is_empty()) else {
        warnings
            .insert("未配置 GitHub Token，KPI 分析仅能展示本地概览，PR 质量雷达会显示 N/A".into());
        return Ok(PrKpiCache {
            root_path,
            generated_at: now,
            github_login: None,
            custom_window_start: Some(window_start),
            custom_window_end: Some(window_end),
            pull_requests: Vec::new(),
            missing_sources: missing_sources.into_iter().collect(),
            warnings: warnings.into_iter().collect(),
        });
    };

    let client = build_github_client(&token)?;
    let login = match fetch_viewer_login(&client) {
        Ok(login) => login,
        Err(error) => {
            warnings.insert(error);
            return Ok(PrKpiCache {
                root_path,
                generated_at: now,
                github_login: None,
                custom_window_start: Some(window_start),
                custom_window_end: Some(window_end),
                pull_requests: Vec::new(),
                missing_sources: missing_sources.into_iter().collect(),
                warnings: warnings.into_iter().collect(),
            });
        }
    };

    let mut pull_requests = Vec::new();
    for repository in &sources {
        match fetch_repository_pull_request_records(
            &client,
            &login,
            repository,
            window_start,
            window_end,
        ) {
            Ok((mut records, repository_warnings)) => {
                pull_requests.append(&mut records);
                for warning in repository_warnings {
                    warnings.insert(warning);
                }
            }
            Err(error) => {
                warnings.insert(format!("{}/{}: {error}", repository.owner, repository.name));
            }
        }
    }

    pull_requests.sort_by(|left, right| {
        right
            .merged_at
            .cmp(&left.merged_at)
            .then_with(|| right.number.cmp(&left.number))
    });

    Ok(PrKpiCache {
        root_path,
        generated_at: now,
        github_login: Some(login),
        custom_window_start: Some(window_start),
        custom_window_end: Some(window_end),
        pull_requests,
        missing_sources: missing_sources.into_iter().collect(),
        warnings: warnings.into_iter().collect(),
    })
}

pub fn build_overview(
    token_report: &LocalTokenUsageReport,
    git_report: &GitUsageReport,
) -> PrKpiOverview {
    let token_total = token_report.totals.total_tokens;
    let code_lines = git_report
        .totals
        .added_lines
        .saturating_add(git_report.totals.deleted_lines);
    let net_lines = git_report.totals.added_lines as f64 - git_report.totals.deleted_lines as f64;
    let output_ratio = if token_total == 0 {
        None
    } else {
        Some(net_lines / (token_total as f64 / 1_000.0))
    };

    PrKpiOverview {
        token_total,
        code_lines,
        output_ratio,
    }
}

pub fn empty_report(
    range: LocalTokenUsageRange,
    overview: PrKpiOverview,
    warning: Option<String>,
) -> PrKpiReport {
    let now = Utc::now();
    let mut report = build_report(
        &[],
        range,
        range_window(range, now).start,
        now,
        range_window(range, now).inclusive_days,
        None,
        None,
        overview,
        now,
        Vec::new(),
        warning.into_iter().collect(),
    );
    report.pending = false;
    report
}

pub fn pending_report(
    range: LocalTokenUsageRange,
    overview: PrKpiOverview,
    warning: Option<String>,
) -> PrKpiReport {
    let mut report = empty_report(range, overview, warning);
    report.pending = true;
    report
}

pub fn pending_custom_report(
    start_date: NaiveDate,
    end_date: NaiveDate,
    overview: PrKpiOverview,
    warning: Option<String>,
) -> PrKpiReport {
    let now = Utc::now();
    let offset = app_time::local_offset();
    let mut report = build_report(
        &[],
        LocalTokenUsageRange::Custom,
        app_time::local_start_of_day_utc(start_date, offset),
        app_time::local_end_of_day_utc(end_date, offset),
        (end_date - start_date).num_days() + 1,
        Some(start_date.format("%Y-%m-%d").to_string()),
        Some(end_date.format("%Y-%m-%d").to_string()),
        overview,
        now,
        Vec::new(),
        warning.into_iter().collect(),
    );
    report.pending = true;
    report
}

pub fn parse_github_remote_owner_repo(remote: &str) -> Option<(String, String)> {
    let trimmed = remote.trim().trim_end_matches('/');
    let normalized = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    let path = if let Some(value) = normalized.strip_prefix("git@github.com:") {
        value
    } else if let Some(value) = normalized.strip_prefix("ssh://git@github.com/") {
        value
    } else if let Some(value) = normalized.strip_prefix("https://github.com/") {
        value
    } else if let Some(value) = normalized.strip_prefix("http://github.com/") {
        value
    } else {
        return None;
    };

    let mut segments = path.split('/');
    let owner = segments.next()?.trim();
    let repo = segments.next()?.trim();
    if owner.is_empty() || repo.is_empty() {
        return None;
    }

    Some((owner.to_string(), repo.to_string()))
}

pub fn is_test_file_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    normalized.starts_with("test/")
        || normalized.starts_with("tests/")
        || normalized.starts_with("__tests__/")
        || normalized.starts_with("spec/")
        || normalized.starts_with("specs/")
        || normalized.contains("/test/")
        || normalized.contains("/tests/")
        || normalized.contains("/__tests__/")
        || normalized.contains("/spec/")
        || normalized.contains("/specs/")
        || normalized.ends_with(".test.ts")
        || normalized.ends_with(".test.tsx")
        || normalized.ends_with(".test.js")
        || normalized.ends_with(".test.jsx")
        || normalized.ends_with(".test.rs")
        || normalized.ends_with(".spec.ts")
        || normalized.ends_with(".spec.tsx")
        || normalized.ends_with(".spec.js")
        || normalized.ends_with(".spec.jsx")
        || normalized.ends_with(".spec.rs")
}

fn build_report(
    pull_requests: &[PrKpiPullRequestRecord],
    range: LocalTokenUsageRange,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    inclusive_days: i64,
    start_date: Option<String>,
    end_date: Option<String>,
    overview: PrKpiOverview,
    generated_at: DateTime<Utc>,
    missing_sources: Vec<String>,
    mut warnings: Vec<String>,
) -> PrKpiReport {
    let selected = pull_requests
        .iter()
        .filter(|record| record.merged_at >= start && record.merged_at <= end)
        .collect::<Vec<_>>();

    let pr_count = selected.len();
    let range_days = inclusive_days.max(1) as f64;
    let cycle_hours = average_cycle_time_hours(&selected);
    let merged_per_week = if pr_count == 0 {
        None
    } else {
        Some(pr_count as f64 * 7.0 / range_days)
    };
    let review_comments_per_pr = average_review_comments(&selected);
    let test_added_ratio = aggregate_test_added_ratio(&selected);
    let local_stability = aggregate_local_stability(&selected);

    if pr_count == 0 {
        warnings.push("当前时间范围暂无已合入 PR，雷达指标会显示 N/A".into());
    } else {
        match local_stability {
            Some((available_prs, _, _, _)) if available_prs < pr_count => warnings.push(format!(
                "7 日稳定性仅基于 {available_prs}/{pr_count} 个可本地分析的 PR，返工控制与代码保留为部分样本"
            )),
            None => warnings.push("本地默认分支历史不足或缺少合入提交，7 日稳定性指标显示 N/A".into()),
            _ => {}
        }
    }

    let rework_rate = local_stability.and_then(|(_, added_lines, reworked_lines, _)| {
        if added_lines == 0 {
            None
        } else {
            Some(reworked_lines as f64 / added_lines as f64)
        }
    });
    let retention_rate = rework_rate.map(|value| 1.0 - value);

    let metrics = vec![
        build_metric(PrKpiMetricKey::CycleTimeAi, cycle_hours),
        build_metric(PrKpiMetricKey::MergedAiPrsPerWeek, merged_per_week),
        build_metric(PrKpiMetricKey::ReviewCommentsPerPr, review_comments_per_pr),
        build_metric(PrKpiMetricKey::TestAddedRatio, test_added_ratio),
        build_metric(PrKpiMetricKey::SevenDayReworkRate, rework_rate),
        build_metric(PrKpiMetricKey::SevenDayRetentionRate, retention_rate),
    ];
    let available_scores = metrics
        .iter()
        .filter_map(|metric| metric.score)
        .collect::<Vec<_>>();
    let overall_score = if available_scores.is_empty() {
        None
    } else {
        Some(available_scores.iter().sum::<f64>() / available_scores.len() as f64)
    };

    PrKpiReport {
        range,
        start_date,
        end_date,
        pending: false,
        overview,
        metrics,
        overall_score,
        missing_sources,
        warnings: unique_sorted_strings(warnings),
        generated_at,
    }
}

fn build_metric(key: PrKpiMetricKey, raw_value: Option<f64>) -> PrKpiMetric {
    let label = metric_label(key).to_string();
    let score = raw_value.and_then(|value| metric_score(key, value));
    let display_value = raw_value
        .map(|value| format_metric_value(key, value))
        .unwrap_or_else(|| "N/A".into());
    PrKpiMetric {
        key,
        label,
        score,
        raw_value,
        display_value,
        is_missing: raw_value.is_none(),
    }
}

fn metric_label(key: PrKpiMetricKey) -> &'static str {
    match key {
        PrKpiMetricKey::CycleTimeAi => "合入周期",
        PrKpiMetricKey::MergedAiPrsPerWeek => "合入频率",
        PrKpiMetricKey::ReviewCommentsPerPr => "评审负担",
        PrKpiMetricKey::TestAddedRatio => "测试占比",
        PrKpiMetricKey::SevenDayReworkRate => "返工控制",
        PrKpiMetricKey::SevenDayRetentionRate => "代码保留",
    }
}

fn metric_score(key: PrKpiMetricKey, value: f64) -> Option<f64> {
    let clamped = match key {
        PrKpiMetricKey::CycleTimeAi => interpolate_lower_is_better(value, 24.0, 24.0 * 7.0),
        PrKpiMetricKey::MergedAiPrsPerWeek => interpolate_higher_is_better(value, 0.0, 5.0),
        PrKpiMetricKey::ReviewCommentsPerPr => interpolate_lower_is_better(value, 2.0, 15.0),
        PrKpiMetricKey::TestAddedRatio => interpolate_higher_is_better(value, 0.0, 0.30),
        PrKpiMetricKey::SevenDayReworkRate => interpolate_lower_is_better(value, 0.05, 0.40),
        PrKpiMetricKey::SevenDayRetentionRate => interpolate_higher_is_better(value, 0.60, 0.95),
    };
    Some((clamped * 100.0).round() / 100.0)
}

fn interpolate_lower_is_better(value: f64, best: f64, worst: f64) -> f64 {
    if value <= best {
        return 100.0;
    }
    if value >= worst {
        return 0.0;
    }
    ((worst - value) / (worst - best) * 100.0).clamp(0.0, 100.0)
}

fn interpolate_higher_is_better(value: f64, worst: f64, best: f64) -> f64 {
    if value <= worst {
        return 0.0;
    }
    if value >= best {
        return 100.0;
    }
    ((value - worst) / (best - worst) * 100.0).clamp(0.0, 100.0)
}

fn format_metric_value(key: PrKpiMetricKey, value: f64) -> String {
    match key {
        PrKpiMetricKey::CycleTimeAi => {
            if value >= 48.0 {
                format!("{:.1}d", value / 24.0)
            } else {
                format!("{:.0}h", value)
            }
        }
        PrKpiMetricKey::MergedAiPrsPerWeek => format!("{:.1} / 周", round_one_decimal(value)),
        PrKpiMetricKey::ReviewCommentsPerPr => format!("{:.1} / PR", round_one_decimal(value)),
        PrKpiMetricKey::TestAddedRatio
        | PrKpiMetricKey::SevenDayReworkRate
        | PrKpiMetricKey::SevenDayRetentionRate => {
            format!("{:.0}%", round_one_decimal(value * 100.0))
        }
    }
}

fn average_cycle_time_hours(records: &[&PrKpiPullRequestRecord]) -> Option<f64> {
    if records.is_empty() {
        return None;
    }
    let total_seconds = records
        .iter()
        .map(|record| (record.merged_at - record.created_at).num_seconds().max(0) as f64)
        .sum::<f64>();
    Some(total_seconds / records.len() as f64 / 3600.0)
}

fn average_review_comments(records: &[&PrKpiPullRequestRecord]) -> Option<f64> {
    if records.is_empty() {
        return None;
    }
    Some(
        records
            .iter()
            .map(|record| record.review_comments as f64)
            .sum::<f64>()
            / records.len() as f64,
    )
}

fn aggregate_test_added_ratio(records: &[&PrKpiPullRequestRecord]) -> Option<f64> {
    let additions = records.iter().map(|record| record.additions).sum::<u64>();
    if additions == 0 {
        return None;
    }
    let test_additions = records
        .iter()
        .map(|record| record.test_additions)
        .sum::<u64>();
    Some(test_additions as f64 / additions as f64)
}

fn aggregate_local_stability(
    records: &[&PrKpiPullRequestRecord],
) -> Option<(usize, u64, u64, u64)> {
    let available = records
        .iter()
        .filter_map(|record| record.local_stability.as_ref())
        .collect::<Vec<_>>();
    if available.is_empty() {
        return None;
    }

    let added_lines = available.iter().map(|item| item.added_lines).sum::<u64>();
    let reworked_lines = available
        .iter()
        .map(|item| item.reworked_lines)
        .sum::<u64>();
    let retained_lines = available
        .iter()
        .map(|item| item.retained_lines)
        .sum::<u64>();
    Some((available.len(), added_lines, reworked_lines, retained_lines))
}

fn discover_github_repository_sources(repositories: Vec<PathBuf>) -> Vec<RepositorySource> {
    let mut by_repo = HashMap::<(String, String), PathBuf>::new();
    for repository in repositories {
        let Some((owner, name)) = github_repository_for_path(&repository) else {
            continue;
        };
        by_repo.entry((owner, name)).or_insert(repository);
    }

    let mut sources = by_repo
        .into_iter()
        .map(|((owner, name), path)| RepositorySource { owner, name, path })
        .collect::<Vec<_>>();
    sources.sort_by(|left, right| {
        left.owner
            .cmp(&right.owner)
            .then_with(|| left.name.cmp(&right.name))
    });
    sources
}

fn github_repository_for_path(repository: &Path) -> Option<(String, String)> {
    if let Ok(remote) = git_output(repository, &["remote", "get-url", "origin"]) {
        if let Some(parsed) = parse_github_remote_owner_repo(&remote) {
            return Some(parsed);
        }
    }

    let remotes = git_output(repository, &["remote"]).ok()?;
    for remote in remotes
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let remote_url = git_output(repository, &["remote", "get-url", remote]).ok()?;
        if let Some(parsed) = parse_github_remote_owner_repo(&remote_url) {
            return Some(parsed);
        }
    }
    None
}

fn build_github_client(token: &str) -> Result<Client, String> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static(GITHUB_ACCEPT));
    headers.insert(USER_AGENT, HeaderValue::from_static(GITHUB_USER_AGENT));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", token.trim()))
            .map_err(|error| format!("GitHub Token 无效: {error}"))?,
    );
    headers.insert(
        "x-github-api-version",
        HeaderValue::from_static(GITHUB_API_VERSION),
    );

    Client::builder()
        .default_headers(headers)
        .timeout(StdDuration::from_secs(20))
        .build()
        .map_err(|error| format!("创建 GitHub 客户端失败: {error}"))
}

fn fetch_viewer_login(client: &Client) -> Result<String, String> {
    let response = client
        .get(format!("{GITHUB_API_BASE}/user"))
        .send()
        .map_err(|error| format!("读取 GitHub 当前用户失败: {error}"))?;
    if response.status() == StatusCode::UNAUTHORIZED || response.status() == StatusCode::FORBIDDEN {
        return Err("GitHub 认证失败，请重新运行 gh auth login 或提供可用 Token".into());
    }
    if !response.status().is_success() {
        return Err(format!("读取 GitHub 当前用户失败: {}", response.status()));
    }
    response
        .json::<GithubViewer>()
        .map(|viewer| viewer.login)
        .map_err(|error| format!("解析 GitHub 当前用户失败: {error}"))
}

fn fetch_repository_pull_request_records(
    client: &Client,
    login: &str,
    repository: &RepositorySource,
    window_start: NaiveDate,
    window_end: NaiveDate,
) -> Result<(Vec<PrKpiPullRequestRecord>, Vec<String>), String> {
    let pull_numbers = search_repository_pull_request_numbers(
        client,
        login,
        &repository.owner,
        &repository.name,
        window_start,
        window_end,
    )?;
    let mut warnings = Vec::new();
    let mut records = Vec::new();

    for number in pull_numbers {
        let detail =
            fetch_pull_request_detail(client, &repository.owner, &repository.name, number)?;
        let Some(merged_at) = detail.merged_at else {
            continue;
        };
        let files = fetch_pull_request_files(client, &repository.owner, &repository.name, number)?;
        let additions = files.iter().map(|file| file.additions).sum::<u64>();
        let test_additions = files
            .iter()
            .filter(|file| is_test_file_path(&file.filename))
            .map(|file| file.additions)
            .sum::<u64>();
        let local_stability = detail
            .merge_commit_sha
            .as_deref()
            .and_then(|merge_commit_sha| {
                match analyze_local_stability(&repository.path, merge_commit_sha, merged_at) {
                    Ok(result) => Some(result),
                    Err(error) => {
                        warnings.push(format!("{}/#{}: {error}", repository.name, detail.number));
                        None
                    }
                }
            });

        records.push(PrKpiPullRequestRecord {
            repository_owner: repository.owner.clone(),
            repository_name: repository.name.clone(),
            repository_path: repository.path.to_string_lossy().to_string(),
            number: detail.number,
            created_at: detail.created_at,
            merged_at,
            review_comments: detail.review_comments,
            additions,
            test_additions,
            is_ai_assisted: true,
            local_stability,
        });
    }

    Ok((records, warnings))
}

fn search_repository_pull_request_numbers(
    client: &Client,
    login: &str,
    owner: &str,
    repository: &str,
    window_start: NaiveDate,
    window_end: NaiveDate,
) -> Result<Vec<u64>, String> {
    let search_start = window_start - Duration::days(1);
    let search_end = window_end + Duration::days(1);
    let query = format!(
        "repo:{owner}/{repository} is:pr is:merged author:{login} merged:{}..{}",
        search_start.format("%Y-%m-%d"),
        search_end.format("%Y-%m-%d")
    );
    let mut page = 1;
    let mut numbers = Vec::new();
    loop {
        let page_param = page.to_string();
        let response = client
            .get(format!("{GITHUB_API_BASE}/search/issues"))
            .query(&[
                ("q", query.as_str()),
                ("sort", "updated"),
                ("order", "desc"),
                ("per_page", "100"),
                ("page", page_param.as_str()),
            ])
            .send()
            .map_err(|error| format!("搜索 GitHub PR 失败: {error}"))?;

        if response.status() == StatusCode::UNAUTHORIZED
            || response.status() == StatusCode::FORBIDDEN
        {
            return Err("GitHub PR 查询鉴权失败".into());
        }
        if !response.status().is_success() {
            return Err(format!("GitHub PR 查询失败: {}", response.status()));
        }

        let parsed = response
            .json::<GithubSearchResponse>()
            .map_err(|error| format!("解析 GitHub PR 搜索结果失败: {error}"))?;
        if parsed.items.is_empty() {
            break;
        }
        numbers.extend(parsed.items.into_iter().map(|item| item.number));
        if numbers.len() % 100 != 0 {
            break;
        }
        page += 1;
    }
    Ok(numbers)
}

fn fetch_pull_request_detail(
    client: &Client,
    owner: &str,
    repository: &str,
    number: u64,
) -> Result<GithubPullRequestDetail, String> {
    let response = client
        .get(format!(
            "{GITHUB_API_BASE}/repos/{owner}/{repository}/pulls/{number}"
        ))
        .send()
        .map_err(|error| format!("读取 PR 详情失败: {error}"))?;

    if !response.status().is_success() {
        return Err(format!("读取 PR 详情失败: {}", response.status()));
    }

    response
        .json::<GithubPullRequestDetail>()
        .map_err(|error| format!("解析 PR 详情失败: {error}"))
}

fn fetch_pull_request_files(
    client: &Client,
    owner: &str,
    repository: &str,
    number: u64,
) -> Result<Vec<GithubPullRequestFile>, String> {
    let mut page = 1;
    let mut files = Vec::new();
    loop {
        let page_param = page.to_string();
        let response = client
            .get(format!(
                "{GITHUB_API_BASE}/repos/{owner}/{repository}/pulls/{number}/files"
            ))
            .query(&[("per_page", "100"), ("page", page_param.as_str())])
            .send()
            .map_err(|error| format!("读取 PR 文件列表失败: {error}"))?;
        if !response.status().is_success() {
            return Err(format!("读取 PR 文件列表失败: {}", response.status()));
        }
        let page_items = response
            .json::<Vec<GithubPullRequestFile>>()
            .map_err(|error| format!("解析 PR 文件列表失败: {error}"))?;
        if page_items.is_empty() {
            break;
        }
        let page_len = page_items.len();
        files.extend(page_items);
        if page_len < 100 {
            break;
        }
        page += 1;
    }
    Ok(files)
}

fn analyze_local_stability(
    repository: &Path,
    merge_commit_sha: &str,
    merged_at: DateTime<Utc>,
) -> Result<PrKpiLocalStability, String> {
    let default_ref = resolve_default_branch_ref(repository)
        .ok_or_else(|| "未找到可用于分析的默认分支".to_string())?;
    let cutoff_at = merged_at + Duration::days(7);
    let tip_timestamp = git_commit_timestamp(repository, &default_ref)?;
    if tip_timestamp < cutoff_at {
        return Err(format!(
            "本地默认分支历史仅到 {}，不足以分析合入后 7 天稳定性",
            tip_timestamp.format("%Y-%m-%d")
        ));
    }

    let cutoff_sha = git_output(
        repository,
        &[
            "rev-list",
            "-1",
            &format!("--before={}", cutoff_at.to_rfc3339()),
            &default_ref,
        ],
    )?;
    if cutoff_sha.trim().is_empty() {
        return Err("未找到 7 天窗口内的截止提交".into());
    }

    let added_ranges = diff_line_ranges(
        repository,
        &format!("{merge_commit_sha}^1"),
        merge_commit_sha,
        DiffRangeSide::New,
    )?;
    let total_added = added_ranges
        .values()
        .flat_map(|ranges| ranges.iter())
        .map(|range| range.count as u64)
        .sum::<u64>();
    if total_added == 0 {
        return Ok(PrKpiLocalStability::default());
    }

    let changed_ranges = diff_line_ranges(
        repository,
        merge_commit_sha,
        &cutoff_sha,
        DiffRangeSide::Old,
    )?;
    let reworked_lines = intersect_range_maps(&added_ranges, &changed_ranges);
    let retained_lines = total_added.saturating_sub(reworked_lines);
    Ok(PrKpiLocalStability {
        added_lines: total_added,
        reworked_lines,
        retained_lines,
    })
}

fn diff_line_ranges(
    repository: &Path,
    base_ref: &str,
    head_ref: &str,
    side: DiffRangeSide,
) -> Result<HashMap<String, Vec<LineRange>>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args([
            "diff",
            "--unified=0",
            "--no-color",
            "--no-renames",
            base_ref,
            head_ref,
            "--",
        ])
        .output()
        .map_err(|error| format!("执行 git diff 失败: {error}"))?;

    if !output.status.success() {
        return Err(format!(
            "git diff 返回非零状态: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(parse_diff_line_ranges(
        &String::from_utf8_lossy(&output.stdout),
        side,
    ))
}

fn parse_diff_line_ranges(output: &str, side: DiffRangeSide) -> HashMap<String, Vec<LineRange>> {
    let mut result = HashMap::<String, Vec<LineRange>>::new();
    let mut current_old_path: Option<String> = None;
    let mut current_new_path: Option<String> = None;

    for line in output.lines() {
        if let Some(path) = line.strip_prefix("--- ") {
            current_old_path = normalize_diff_path(path);
            continue;
        }
        if let Some(path) = line.strip_prefix("+++ ") {
            current_new_path = normalize_diff_path(path);
            continue;
        }
        if !line.starts_with("@@") {
            continue;
        }

        let selected_path = match side {
            DiffRangeSide::Old => current_old_path.clone(),
            DiffRangeSide::New => current_new_path.clone(),
        };
        let Some(path) = selected_path else {
            continue;
        };

        let Some(range) = parse_diff_hunk_range(line, side) else {
            continue;
        };
        if range.count == 0 {
            continue;
        }
        result.entry(path).or_default().push(range);
    }

    for ranges in result.values_mut() {
        *ranges = merge_line_ranges(ranges);
    }
    result
}

fn normalize_diff_path(path: &str) -> Option<String> {
    let trimmed = path.trim();
    if trimmed == "/dev/null" {
        return None;
    }
    let normalized = trimmed
        .strip_prefix("a/")
        .or_else(|| trimmed.strip_prefix("b/"))
        .unwrap_or(trimmed);
    Some(normalized.to_string())
}

fn parse_diff_hunk_range(line: &str, side: DiffRangeSide) -> Option<LineRange> {
    let rest = line.strip_prefix("@@ ")?;
    let (range_part, _) = rest.split_once(" @@").unwrap_or((rest, ""));
    let mut parts = range_part.split(' ');
    let old = parts.next()?;
    let new = parts.next()?;
    let target = match side {
        DiffRangeSide::Old => old,
        DiffRangeSide::New => new,
    };
    parse_hunk_range(target)
}

fn parse_hunk_range(raw: &str) -> Option<LineRange> {
    let trimmed = raw.trim();
    let trimmed = trimmed
        .strip_prefix('-')
        .or_else(|| trimmed.strip_prefix('+'))
        .unwrap_or(trimmed);
    let (start, count) = match trimmed.split_once(',') {
        Some((start, count)) => (start, count),
        None => (trimmed, "1"),
    };
    Some(LineRange {
        start: start.parse().ok()?,
        count: count.parse().ok()?,
    })
}

fn merge_line_ranges(ranges: &[LineRange]) -> Vec<LineRange> {
    if ranges.is_empty() {
        return Vec::new();
    }
    let mut sorted = ranges.to_vec();
    sorted.sort_by_key(|range| range.start);
    let mut merged = Vec::with_capacity(sorted.len());
    let mut current = sorted[0];

    for next in sorted.into_iter().skip(1) {
        let current_end = current.start + current.count;
        if next.start <= current_end {
            let next_end = next.start + next.count;
            current.count = current.count.max(next_end.saturating_sub(current.start));
        } else {
            merged.push(current);
            current = next;
        }
    }
    merged.push(current);
    merged
}

fn intersect_range_maps(
    added_ranges: &HashMap<String, Vec<LineRange>>,
    changed_ranges: &HashMap<String, Vec<LineRange>>,
) -> u64 {
    added_ranges
        .iter()
        .map(|(path, added)| {
            let changed = changed_ranges.get(path).cloned().unwrap_or_default();
            intersect_line_ranges(added, &changed)
        })
        .sum()
}

fn intersect_line_ranges(left: &[LineRange], right: &[LineRange]) -> u64 {
    let left = merge_line_ranges(left);
    let right = merge_line_ranges(right);
    let mut total = 0_u64;
    let mut left_index = 0;
    let mut right_index = 0;

    while left_index < left.len() && right_index < right.len() {
        let left_range = left[left_index];
        let right_range = right[right_index];
        let left_end = left_range.start + left_range.count;
        let right_end = right_range.start + right_range.count;
        let start = left_range.start.max(right_range.start);
        let end = left_end.min(right_end);
        if end > start {
            total = total.saturating_add((end - start) as u64);
        }
        if left_end <= right_end {
            left_index += 1;
        } else {
            right_index += 1;
        }
    }
    total
}

fn resolve_default_branch_ref(repository: &Path) -> Option<String> {
    let mut candidates = Vec::new();

    if let Ok(symbolic) = git_output(repository, &["symbolic-ref", "refs/remotes/origin/HEAD"]) {
        let symbolic = symbolic.trim();
        if !symbolic.is_empty() {
            candidates.push(symbolic.to_string());
        }
    }

    for candidate in [
        "refs/heads/main",
        "refs/heads/master",
        "refs/heads/dev",
        "refs/heads/rc",
        "refs/heads/development",
        "refs/remotes/origin/main",
        "refs/remotes/origin/master",
        "refs/remotes/origin/dev",
        "refs/remotes/origin/rc",
        "refs/remotes/origin/development",
    ] {
        if git_ref_exists(repository, candidate) {
            candidates.push(candidate.to_string());
        }
    }

    let best = candidates
        .into_iter()
        .filter_map(|candidate| {
            git_commit_timestamp(repository, &candidate)
                .ok()
                .and_then(|timestamp| {
                    branch_priority(&candidate).map(|priority| (candidate, priority, timestamp))
                })
        })
        .min_by(|left, right| left.1.cmp(&right.1).then_with(|| right.2.cmp(&left.2)))
        .map(|(candidate, _, _)| candidate);

    if best.is_some() {
        return best;
    }

    if git_ref_exists(repository, "HEAD") {
        return Some("HEAD".to_string());
    }

    None
}

fn git_ref_exists(repository: &Path, reference: &str) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(["show-ref", "--verify", "--quiet", reference])
        .status()
        .ok()
        .is_some_and(|status| status.success())
}

fn branch_priority(reference: &str) -> Option<u8> {
    let normalized = reference.trim().to_ascii_lowercase();
    if normalized.ends_with("/dev")
        || normalized.ends_with("/rc")
        || normalized.ends_with("/development")
    {
        return Some(0);
    }
    if normalized.ends_with("/main") || normalized.ends_with("/master") {
        return Some(1);
    }
    None
}

fn git_commit_timestamp(repository: &Path, reference: &str) -> Result<DateTime<Utc>, String> {
    let value = git_output(repository, &["show", "-s", "--format=%cI", reference])?;
    DateTime::parse_from_rfc3339(value.trim())
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|error| format!("解析 git 提交时间失败: {error}"))
}

fn git_output(repository: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .output()
        .map_err(|error| format!("执行 git 命令失败: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn range_window(range: LocalTokenUsageRange, now: DateTime<Utc>) -> DateRangeWindow {
    let offset = app_time::local_offset();
    let today = app_time::local_date(now, offset);
    let start_date = match range {
        LocalTokenUsageRange::Today => today,
        LocalTokenUsageRange::Last3Days => today - Duration::days(2),
        LocalTokenUsageRange::ThisWeek => {
            today - Duration::days(i64::from(today.weekday().num_days_from_monday()))
        }
        LocalTokenUsageRange::ThisMonth => {
            NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap_or(today)
        }
        LocalTokenUsageRange::Custom => today,
    };
    DateRangeWindow {
        start: app_time::local_start_of_day_utc(start_date, offset),
        end: now,
        inclusive_days: (today - start_date).num_days() + 1,
    }
}

fn round_one_decimal(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn unique_sorted_strings(values: Vec<String>) -> Vec<String> {
    let mut items = values
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    items.sort();
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, Offset, TimeZone};
    use std::{fs, path::PathBuf};

    fn sample_record(
        created_at: &str,
        merged_at: &str,
        review_comments: u64,
        additions: u64,
        test_additions: u64,
        local_stability: Option<PrKpiLocalStability>,
    ) -> PrKpiPullRequestRecord {
        PrKpiPullRequestRecord {
            repository_owner: "openai".into(),
            repository_name: "codex".into(),
            repository_path: "/tmp/openai-codex".into(),
            number: 1,
            created_at: DateTime::parse_from_rfc3339(created_at)
                .unwrap()
                .with_timezone(&Utc),
            merged_at: DateTime::parse_from_rfc3339(merged_at)
                .unwrap()
                .with_timezone(&Utc),
            review_comments,
            additions,
            test_additions,
            is_ai_assisted: true,
            local_stability,
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("ai-usage-pr-kpi-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn run_git(repository: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(args)
            .output()
            .unwrap_or_else(|error| panic!("failed to run git {args:?}: {error}"));
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn run_git_env(repository: &Path, args: &[&str], timestamp: &str) {
        let output = Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(args)
            .env("GIT_AUTHOR_DATE", timestamp)
            .env("GIT_COMMITTER_DATE", timestamp)
            .output()
            .unwrap_or_else(|error| panic!("failed to run git {args:?}: {error}"));
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn local_offset() -> chrono::FixedOffset {
        Local::now().offset().fix()
    }

    fn local_time_utc(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
    ) -> chrono::DateTime<Utc> {
        local_offset()
            .with_ymd_and_hms(year, month, day, hour, minute, second)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn local_timestamp_rfc3339(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
    ) -> String {
        local_time_utc(year, month, day, hour, minute, second).to_rfc3339()
    }

    #[test]
    fn build_report_scores_metrics_and_warns_on_partial_local_stability() {
        let cache = PrKpiCache {
            root_path: "/tmp/workspace".into(),
            generated_at: Utc.with_ymd_and_hms(2026, 4, 28, 7, 30, 0).unwrap(),
            github_login: Some("octocat".into()),
            custom_window_start: Some(NaiveDate::from_ymd_opt(2026, 1, 30).unwrap()),
            custom_window_end: Some(NaiveDate::from_ymd_opt(2026, 4, 28).unwrap()),
            pull_requests: vec![
                sample_record(
                    "2026-04-20T00:00:00Z",
                    "2026-04-20T12:00:00Z",
                    1,
                    100,
                    20,
                    Some(PrKpiLocalStability {
                        added_lines: 80,
                        reworked_lines: 8,
                        retained_lines: 72,
                    }),
                ),
                sample_record(
                    "2026-04-22T00:00:00Z",
                    "2026-04-24T00:00:00Z",
                    5,
                    50,
                    5,
                    None,
                ),
            ],
            missing_sources: vec![],
            warnings: vec![],
        };

        let report = cache.custom_report(
            NaiveDate::from_ymd_opt(2026, 4, 20).unwrap(),
            NaiveDate::from_ymd_opt(2026, 4, 27).unwrap(),
            PrKpiOverview {
                token_total: 1_240_000,
                code_lines: 8_432,
                output_ratio: Some(6.8),
            },
        );

        assert_eq!(report.metrics.len(), 6);
        assert!(report.overall_score.is_some());
        let cycle_time = report
            .metrics
            .iter()
            .find(|metric| metric.key == PrKpiMetricKey::CycleTimeAi)
            .unwrap();
        assert_eq!(cycle_time.display_value, "30h");
        let rework = report
            .metrics
            .iter()
            .find(|metric| metric.key == PrKpiMetricKey::SevenDayReworkRate)
            .unwrap();
        assert_eq!(rework.display_value, "10%");
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.contains("1/2 个可本地分析的 PR")));
    }

    #[test]
    fn custom_report_filters_pull_requests_by_local_day_boundaries() {
        let cache = PrKpiCache {
            root_path: "/tmp/workspace".into(),
            generated_at: local_time_utc(2026, 4, 28, 15, 30, 0),
            github_login: Some("octocat".into()),
            custom_window_start: Some(NaiveDate::from_ymd_opt(2026, 1, 30).unwrap()),
            custom_window_end: Some(NaiveDate::from_ymd_opt(2026, 4, 28).unwrap()),
            pull_requests: vec![
                sample_record(
                    &local_timestamp_rfc3339(2026, 4, 19, 22, 0, 0),
                    &local_timestamp_rfc3339(2026, 4, 20, 0, 30, 0),
                    1,
                    100,
                    20,
                    None,
                ),
                sample_record(
                    &local_timestamp_rfc3339(2026, 4, 20, 12, 0, 0),
                    &local_timestamp_rfc3339(2026, 4, 19, 23, 30, 0),
                    2,
                    40,
                    5,
                    None,
                ),
            ],
            missing_sources: vec![],
            warnings: vec![],
        };

        let report = cache.custom_report(
            NaiveDate::from_ymd_opt(2026, 4, 20).unwrap(),
            NaiveDate::from_ymd_opt(2026, 4, 20).unwrap(),
            PrKpiOverview::default(),
        );

        let cycle_time = report
            .metrics
            .iter()
            .find(|metric| metric.key == PrKpiMetricKey::CycleTimeAi)
            .unwrap();
        assert_eq!(cycle_time.raw_value, Some(2.5));
        assert!(!report
            .warnings
            .iter()
            .any(|warning| warning.contains("暂无已合入 PR")));
    }

    #[test]
    fn parse_github_remote_owner_repo_supports_https_and_ssh() {
        assert_eq!(
            parse_github_remote_owner_repo("git@github.com:openai/codex.git"),
            Some(("openai".into(), "codex".into()))
        );
        assert_eq!(
            parse_github_remote_owner_repo("https://github.com/openai/codex.git"),
            Some(("openai".into(), "codex".into()))
        );
        assert_eq!(
            parse_github_remote_owner_repo("ssh://git@github.com/openai/codex"),
            Some(("openai".into(), "codex".into()))
        );
    }

    #[test]
    fn detect_test_file_path_matches_common_test_conventions() {
        assert!(is_test_file_path("src/foo.test.ts"));
        assert!(is_test_file_path("tests/foo.ts"));
        assert!(is_test_file_path("__tests__/foo.js"));
        assert!(is_test_file_path("spec/helpers.rb"));
        assert!(!is_test_file_path("src/foo.ts"));
    }

    #[test]
    fn analyze_local_stability_counts_reworked_lines_within_seven_days() {
        let repository = temp_dir("stability");
        fs::create_dir_all(repository.join("src")).unwrap();
        run_git(&repository, &["init"]);
        run_git(&repository, &["branch", "-m", "main"]);
        run_git(&repository, &["config", "user.email", "test@example.com"]);
        run_git(&repository, &["config", "user.name", "Test User"]);

        fs::write(repository.join("src").join("app.ts"), "base\n").unwrap();
        run_git(&repository, &["add", "."]);
        run_git_env(
            &repository,
            &["commit", "-m", "base"],
            "2026-04-01T00:00:00Z",
        );

        fs::write(
            repository.join("src").join("app.ts"),
            "base\none\ntwo\nthree\n",
        )
        .unwrap();
        run_git(&repository, &["add", "."]);
        run_git_env(
            &repository,
            &["commit", "-m", "merge"],
            "2026-04-02T00:00:00Z",
        );
        let merge_sha = git_output(&repository, &["rev-parse", "HEAD"]).unwrap();

        fs::write(
            repository.join("src").join("app.ts"),
            "base\none\nchanged\nthree\n",
        )
        .unwrap();
        run_git(&repository, &["add", "."]);
        run_git_env(
            &repository,
            &["commit", "-m", "rework"],
            "2026-04-05T00:00:00Z",
        );

        fs::write(repository.join("README.md"), "history marker\n").unwrap();
        run_git(&repository, &["add", "."]);
        run_git_env(
            &repository,
            &["commit", "-m", "after-cutoff"],
            "2026-04-10T00:00:00Z",
        );

        let stability = analyze_local_stability(
            &repository,
            &merge_sha,
            DateTime::parse_from_rfc3339("2026-04-02T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        )
        .unwrap();

        assert_eq!(stability.added_lines, 3);
        assert_eq!(stability.reworked_lines, 1);
        assert_eq!(stability.retained_lines, 2);
    }

    #[test]
    fn analyze_local_stability_requires_history_to_cutoff() {
        let repository = temp_dir("stability-insufficient");
        fs::create_dir_all(repository.join("src")).unwrap();
        run_git(&repository, &["init"]);
        run_git(&repository, &["branch", "-m", "main"]);
        run_git(&repository, &["config", "user.email", "test@example.com"]);
        run_git(&repository, &["config", "user.name", "Test User"]);

        fs::write(repository.join("src").join("app.ts"), "base\n").unwrap();
        run_git(&repository, &["add", "."]);
        run_git_env(
            &repository,
            &["commit", "-m", "base"],
            "2026-04-01T00:00:00Z",
        );

        fs::write(
            repository.join("src").join("app.ts"),
            "base\none\ntwo\nthree\n",
        )
        .unwrap();
        run_git(&repository, &["add", "."]);
        run_git_env(
            &repository,
            &["commit", "-m", "merge"],
            "2026-04-02T00:00:00Z",
        );
        let merge_sha = git_output(&repository, &["rev-parse", "HEAD"]).unwrap();

        let error = analyze_local_stability(
            &repository,
            &merge_sha,
            DateTime::parse_from_rfc3339("2026-04-02T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        )
        .unwrap_err();

        assert!(error.contains("历史仅到"));
    }

    #[test]
    fn resolve_default_branch_ref_prefers_newer_local_main_over_stale_origin_main() {
        let repository = temp_dir("default-branch-preference");
        fs::create_dir_all(repository.join("src")).unwrap();
        run_git(&repository, &["init"]);
        run_git(&repository, &["branch", "-m", "main"]);
        run_git(&repository, &["config", "user.email", "test@example.com"]);
        run_git(&repository, &["config", "user.name", "Test User"]);

        fs::write(repository.join("src").join("app.ts"), "base\n").unwrap();
        run_git(&repository, &["add", "."]);
        run_git_env(
            &repository,
            &["commit", "-m", "base"],
            "2026-03-12T00:00:00Z",
        );
        let base_sha = git_output(&repository, &["rev-parse", "HEAD"]).unwrap();

        run_git(
            &repository,
            &["update-ref", "refs/remotes/origin/main", base_sha.trim()],
        );

        fs::write(repository.join("src").join("app.ts"), "base\nnext\n").unwrap();
        run_git(&repository, &["add", "."]);
        run_git_env(
            &repository,
            &["commit", "-m", "fresh-local"],
            "2026-04-28T00:00:00Z",
        );

        assert_eq!(
            resolve_default_branch_ref(&repository).as_deref(),
            Some("refs/heads/main")
        );
    }

    #[test]
    fn resolve_default_branch_ref_supports_dev_rc_and_development_candidates() {
        let repository = temp_dir("default-branch-dev-candidates");
        fs::create_dir_all(repository.join("src")).unwrap();
        run_git(&repository, &["init"]);
        run_git(&repository, &["branch", "-m", "main"]);
        run_git(&repository, &["config", "user.email", "test@example.com"]);
        run_git(&repository, &["config", "user.name", "Test User"]);

        fs::write(repository.join("src").join("app.ts"), "base\n").unwrap();
        run_git(&repository, &["add", "."]);
        run_git_env(
            &repository,
            &["commit", "-m", "base-main"],
            "2026-03-12T00:00:00Z",
        );

        run_git(&repository, &["branch", "dev"]);
        run_git(&repository, &["checkout", "dev"]);
        fs::write(repository.join("src").join("app.ts"), "base\ndev\n").unwrap();
        run_git(&repository, &["add", "."]);
        run_git_env(
            &repository,
            &["commit", "-m", "fresh-dev"],
            "2026-04-28T00:00:00Z",
        );

        run_git(&repository, &["branch", "rc", "main"]);
        run_git(&repository, &["branch", "development", "main"]);

        assert_eq!(
            resolve_default_branch_ref(&repository).as_deref(),
            Some("refs/heads/dev")
        );
    }

    #[test]
    fn resolve_default_branch_ref_prefers_dev_family_before_main_master() {
        let repository = temp_dir("default-branch-dev-priority");
        fs::create_dir_all(repository.join("src")).unwrap();
        run_git(&repository, &["init"]);
        run_git(&repository, &["branch", "-m", "main"]);
        run_git(&repository, &["config", "user.email", "test@example.com"]);
        run_git(&repository, &["config", "user.name", "Test User"]);

        fs::write(repository.join("src").join("app.ts"), "base\n").unwrap();
        run_git(&repository, &["add", "."]);
        run_git_env(
            &repository,
            &["commit", "-m", "base-main"],
            "2026-04-28T00:00:00Z",
        );

        run_git(&repository, &["branch", "development"]);
        run_git(&repository, &["checkout", "development"]);
        fs::write(repository.join("src").join("app.ts"), "base\ndevelopment\n").unwrap();
        run_git(&repository, &["add", "."]);
        run_git_env(
            &repository,
            &["commit", "-m", "older-development"],
            "2026-04-20T00:00:00Z",
        );

        assert_eq!(
            resolve_default_branch_ref(&repository).as_deref(),
            Some("refs/heads/development")
        );
    }

    #[test]
    fn pr_kpi_metric_keys_serialize_with_expected_7d_names() {
        assert_eq!(
            serde_json::to_string(&PrKpiMetricKey::SevenDayReworkRate).unwrap(),
            "\"7d_rework_rate\""
        );
        assert_eq!(
            serde_json::to_string(&PrKpiMetricKey::SevenDayRetentionRate).unwrap(),
            "\"7d_retention_rate\""
        );
    }
}
