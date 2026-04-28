use crate::models::{
    LocalTokenUsageDay, LocalTokenUsageModel, LocalTokenUsageRange, LocalTokenUsageReport,
    LocalTokenUsageTool, LocalTokenUsageTotals, CUSTOM_USAGE_WINDOW_DAYS,
};
use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Timelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs,
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    time::SystemTime,
};

const TOOL_CLAUDE: &str = "claude";
const TOOL_CODEX: &str = "codex";
const TOOL_OPENCODE: &str = "opencode";
const TOOL_KIMI: &str = "kimi";

#[derive(Debug, Clone)]
struct LocalUsageEvent {
    tool: String,
    model: Option<String>,
    session_id: String,
    timestamp: DateTime<Utc>,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_creation_tokens: u64,
}

#[derive(Debug, Clone, Default)]
struct TokenStats {
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_creation_tokens: u64,
}

#[derive(Debug, Clone, Copy)]
enum BucketGranularity {
    Day,
    Hour,
    ThreeHours,
}

#[derive(Debug, Clone, Default)]
struct RawCodexUsage {
    input_tokens: u64,
    cached_input_tokens: u64,
    output_tokens: u64,
    reasoning_output_tokens: u64,
    total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalTokenUsageCache {
    pub generated_at: DateTime<Utc>,
    pub today: LocalTokenUsageReport,
    pub last3_days: LocalTokenUsageReport,
    pub this_week: LocalTokenUsageReport,
    pub this_month: LocalTokenUsageReport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_window_start: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_window_end: Option<NaiveDate>,
    #[serde(default)]
    pub custom_days: Vec<LocalTokenUsageCachedDay>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LocalTokenUsageCachedDay {
    pub date: String,
    pub totals: LocalTokenUsageTotals,
    #[serde(default)]
    pub models: Vec<LocalTokenUsageModel>,
    #[serde(default)]
    pub tools: Vec<LocalTokenUsageTool>,
}

impl LocalTokenUsageCache {
    pub fn report(&self, range: LocalTokenUsageRange) -> LocalTokenUsageReport {
        match range {
            LocalTokenUsageRange::Today => self.today.clone(),
            LocalTokenUsageRange::Last3Days => self.last3_days.clone(),
            LocalTokenUsageRange::ThisWeek => self.this_week.clone(),
            LocalTokenUsageRange::ThisMonth => self.this_month.clone(),
            LocalTokenUsageRange::Custom => {
                let mut report = self.this_month.clone();
                report.range = LocalTokenUsageRange::Custom;
                report.start_date = None;
                report.end_date = None;
                report
            }
        }
    }

    pub fn covers_custom_range(&self, start_date: NaiveDate, end_date: NaiveDate) -> bool {
        matches!(
            (self.custom_window_start, self.custom_window_end),
            (Some(window_start), Some(window_end))
                if start_date >= window_start && end_date <= window_end && !self.custom_days.is_empty()
        )
    }

    pub fn custom_report(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> LocalTokenUsageReport {
        let days_by_date = self
            .custom_days
            .iter()
            .map(|day| (day.date.as_str(), day))
            .collect::<HashMap<_, _>>();
        let mut current = start_date;
        let mut totals = TokenStats::default();
        let mut by_model = HashMap::<String, TokenStats>::new();
        let mut by_tool = HashMap::<String, TokenStats>::new();
        let mut days = Vec::new();

        while current <= end_date {
            let date = current.format("%Y-%m-%d").to_string();
            let cached_day = days_by_date.get(date.as_str());
            let day_totals = cached_day.map(|day| &day.totals);
            let input_tokens = day_totals.map(|totals| totals.input_tokens).unwrap_or(0);
            let output_tokens = day_totals.map(|totals| totals.output_tokens).unwrap_or(0);
            let cache_read_tokens = day_totals
                .map(|totals| totals.cache_read_tokens)
                .unwrap_or(0);
            let cache_creation_tokens = day_totals
                .map(|totals| totals.cache_creation_tokens)
                .unwrap_or(0);
            let total_tokens = input_tokens
                .saturating_add(output_tokens)
                .saturating_add(cache_read_tokens)
                .saturating_add(cache_creation_tokens);
            let models = cached_day.map(|day| day.models.clone()).unwrap_or_default();

            totals.input_tokens = totals.input_tokens.saturating_add(input_tokens);
            totals.output_tokens = totals.output_tokens.saturating_add(output_tokens);
            totals.cache_read_tokens = totals.cache_read_tokens.saturating_add(cache_read_tokens);
            totals.cache_creation_tokens = totals
                .cache_creation_tokens
                .saturating_add(cache_creation_tokens);

            for model in &models {
                let current_model = by_model.entry(model.model.clone()).or_default();
                current_model.input_tokens = current_model
                    .input_tokens
                    .saturating_add(model.input_tokens);
                current_model.output_tokens = current_model
                    .output_tokens
                    .saturating_add(model.output_tokens);
                current_model.cache_read_tokens = current_model
                    .cache_read_tokens
                    .saturating_add(model.cache_read_tokens);
                current_model.cache_creation_tokens = current_model
                    .cache_creation_tokens
                    .saturating_add(model.cache_creation_tokens);
            }

            for tool in cached_day.map(|day| day.tools.iter()).into_iter().flatten() {
                let current_tool = by_tool.entry(tool.tool.clone()).or_default();
                current_tool.input_tokens =
                    current_tool.input_tokens.saturating_add(tool.input_tokens);
                current_tool.output_tokens = current_tool
                    .output_tokens
                    .saturating_add(tool.output_tokens);
                current_tool.cache_read_tokens = current_tool
                    .cache_read_tokens
                    .saturating_add(tool.cache_read_tokens);
                current_tool.cache_creation_tokens = current_tool
                    .cache_creation_tokens
                    .saturating_add(tool.cache_creation_tokens);
            }

            days.push(LocalTokenUsageDay {
                date,
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_creation_tokens,
                total_tokens,
                models,
            });
            current += Duration::days(1);
        }

        let mut models = by_model
            .into_iter()
            .map(|(model, stats)| model_usage(model, stats))
            .collect::<Vec<_>>();
        models.sort_by(|a, b| {
            b.total_tokens
                .cmp(&a.total_tokens)
                .then_with(|| a.model.cmp(&b.model))
        });

        let mut tools = by_tool
            .into_iter()
            .map(|(tool, stats)| LocalTokenUsageTool {
                tool,
                input_tokens: stats.input_tokens,
                output_tokens: stats.output_tokens,
                cache_read_tokens: stats.cache_read_tokens,
                cache_creation_tokens: stats.cache_creation_tokens,
                total_tokens: stats.total_tokens(),
            })
            .collect::<Vec<_>>();
        tools.sort_by(|a, b| {
            b.total_tokens
                .cmp(&a.total_tokens)
                .then_with(|| a.tool.cmp(&b.tool))
        });

        LocalTokenUsageReport {
            range: LocalTokenUsageRange::Custom,
            start_date: Some(start_date.format("%Y-%m-%d").to_string()),
            end_date: Some(end_date.format("%Y-%m-%d").to_string()),
            pending: false,
            totals: LocalTokenUsageTotals {
                input_tokens: totals.input_tokens,
                output_tokens: totals.output_tokens,
                cache_read_tokens: totals.cache_read_tokens,
                cache_creation_tokens: totals.cache_creation_tokens,
                total_tokens: totals.total_tokens(),
                cache_hit_rate_percent: cache_hit_rate(&totals),
            },
            days,
            models,
            tools,
            missing_sources: self.this_month.missing_sources.clone(),
            warnings: self.this_month.warnings.clone(),
            generated_at: self.generated_at,
        }
    }
}

pub fn build_cache() -> Result<LocalTokenUsageCache, String> {
    let now = Utc::now();
    let (events, missing_sources, warnings) = load_events()?;
    Ok(build_cache_from_events(
        now,
        events,
        missing_sources,
        warnings,
    ))
}

pub fn empty_report(range: LocalTokenUsageRange, warning: Option<String>) -> LocalTokenUsageReport {
    let warnings = warning.into_iter().collect::<Vec<_>>();
    if range == LocalTokenUsageRange::Custom {
        let today = Utc::now().date_naive();
        return aggregate_custom_events(Utc::now(), today, today, Vec::new(), Vec::new(), warnings);
    }
    aggregate_events(range, Utc::now(), Vec::new(), Vec::new(), warnings)
}

pub fn pending_report(
    range: LocalTokenUsageRange,
    warning: Option<String>,
) -> LocalTokenUsageReport {
    let mut report = empty_report(range, warning);
    report.pending = true;
    report
}

pub fn pending_custom_report(
    start_date: NaiveDate,
    end_date: NaiveDate,
    warning: Option<String>,
) -> LocalTokenUsageReport {
    let mut report = aggregate_custom_events(
        Utc::now(),
        start_date,
        end_date,
        Vec::new(),
        Vec::new(),
        warning.into_iter().collect(),
    );
    report.pending = true;
    report
}

fn load_events() -> Result<(Vec<LocalUsageEvent>, Vec<String>, Vec<String>), String> {
    let mut events = Vec::new();
    let mut missing_sources = Vec::new();
    let mut warnings = Vec::new();

    let claude_roots = claude_projects_roots();
    let existing_claude_roots = existing_roots("Claude Code", &claude_roots, &mut missing_sources);
    match load_claude_events_from_roots(&existing_claude_roots) {
        Ok(mut next) => events.append(&mut next),
        Err(error) => warnings.push(format!("Claude Code 日志解析失败: {error}")),
    }

    let codex_roots = codex_session_roots();
    let existing_codex_roots = existing_roots("Codex CLI", &codex_roots, &mut missing_sources);
    match load_codex_events_from_roots(&existing_codex_roots) {
        Ok(mut next) => events.append(&mut next),
        Err(error) => warnings.push(format!("Codex CLI 日志解析失败: {error}")),
    }

    let opencode_roots = opencode_roots();
    let existing_opencode_roots = existing_roots("OpenCode", &opencode_roots, &mut missing_sources);
    match load_opencode_events_from_roots(&existing_opencode_roots) {
        Ok(mut next) => events.append(&mut next),
        Err(error) => warnings.push(format!("OpenCode 日志解析失败: {error}")),
    }

    let kimi_root = kimi_root();
    if kimi_root.is_dir() {
        match load_kimi_events_from_root(&kimi_root) {
            Ok((mut next, mut next_warnings)) => {
                events.append(&mut next);
                warnings.append(&mut next_warnings);
            }
            Err(error) => warnings.push(format!("Kimi CLI 日志解析失败: {error}")),
        }
    } else {
        missing_sources.push(format!("Kimi CLI: {}", kimi_root.display()));
    }

    Ok((events, missing_sources, warnings))
}

fn build_cache_from_events(
    now: DateTime<Utc>,
    events: Vec<LocalUsageEvent>,
    missing_sources: Vec<String>,
    warnings: Vec<String>,
) -> LocalTokenUsageCache {
    let (custom_window_start, custom_window_end, custom_days) = build_custom_days(now, &events);
    LocalTokenUsageCache {
        generated_at: now,
        today: aggregate_events(
            LocalTokenUsageRange::Today,
            now,
            events.clone(),
            missing_sources.clone(),
            warnings.clone(),
        ),
        last3_days: aggregate_events(
            LocalTokenUsageRange::Last3Days,
            now,
            events.clone(),
            missing_sources.clone(),
            warnings.clone(),
        ),
        this_week: aggregate_events(
            LocalTokenUsageRange::ThisWeek,
            now,
            events.clone(),
            missing_sources.clone(),
            warnings.clone(),
        ),
        this_month: aggregate_events(
            LocalTokenUsageRange::ThisMonth,
            now,
            events,
            missing_sources,
            warnings,
        ),
        custom_window_start: Some(custom_window_start),
        custom_window_end: Some(custom_window_end),
        custom_days,
    }
}

fn build_custom_days(
    now: DateTime<Utc>,
    events: &[LocalUsageEvent],
) -> (NaiveDate, NaiveDate, Vec<LocalTokenUsageCachedDay>) {
    let window_end = now.date_naive();
    let window_start = window_end - Duration::days(CUSTOM_USAGE_WINDOW_DAYS - 1);
    let start = Utc.from_utc_datetime(&window_start.and_hms_opt(0, 0, 0).unwrap());
    let end = Utc.from_utc_datetime(&window_end.and_hms_opt(23, 59, 59).unwrap());
    let mut by_day = HashMap::<String, TokenStats>::new();
    let mut by_day_model = HashMap::<String, HashMap<String, TokenStats>>::new();
    let mut by_day_tool = HashMap::<String, HashMap<String, TokenStats>>::new();

    for event in events {
        if event.timestamp < start || event.timestamp > end {
            continue;
        }
        let date = bucket_key(
            BucketGranularity::Day,
            bucket_start_for_event(BucketGranularity::Day, event.timestamp),
        );
        add_event(by_day.entry(date.clone()).or_default(), event);
        add_event(
            by_day_model
                .entry(date.clone())
                .or_default()
                .entry(event.model.clone().unwrap_or_else(|| "unknown".into()))
                .or_default(),
            event,
        );
        add_event(
            by_day_tool
                .entry(date)
                .or_default()
                .entry(event.tool.clone())
                .or_default(),
            event,
        );
    }

    let mut days = Vec::new();
    let mut current = window_start;
    while current <= window_end {
        let date = current.format("%Y-%m-%d").to_string();
        let stats = by_day.remove(&date).unwrap_or_default();
        let mut models = by_day_model
            .remove(&date)
            .unwrap_or_default()
            .into_iter()
            .map(|(model, stats)| model_usage(model, stats))
            .collect::<Vec<_>>();
        models.sort_by(|a, b| {
            b.total_tokens
                .cmp(&a.total_tokens)
                .then_with(|| a.model.cmp(&b.model))
        });
        let mut tools = by_day_tool
            .remove(&date)
            .unwrap_or_default()
            .into_iter()
            .map(|(tool, stats)| LocalTokenUsageTool {
                tool,
                input_tokens: stats.input_tokens,
                output_tokens: stats.output_tokens,
                cache_read_tokens: stats.cache_read_tokens,
                cache_creation_tokens: stats.cache_creation_tokens,
                total_tokens: stats.total_tokens(),
            })
            .collect::<Vec<_>>();
        tools.sort_by(|a, b| {
            b.total_tokens
                .cmp(&a.total_tokens)
                .then_with(|| a.tool.cmp(&b.tool))
        });
        days.push(LocalTokenUsageCachedDay {
            date,
            totals: LocalTokenUsageTotals {
                input_tokens: stats.input_tokens,
                output_tokens: stats.output_tokens,
                cache_read_tokens: stats.cache_read_tokens,
                cache_creation_tokens: stats.cache_creation_tokens,
                total_tokens: stats.total_tokens(),
                cache_hit_rate_percent: cache_hit_rate(&stats),
            },
            models,
            tools,
        });
        current += Duration::days(1);
    }

    (window_start, window_end, days)
}

fn existing_roots(
    label: &str,
    roots: &[PathBuf],
    missing_sources: &mut Vec<String>,
) -> Vec<PathBuf> {
    let existing = roots
        .iter()
        .filter(|root| root.is_dir())
        .cloned()
        .collect::<Vec<_>>();
    if existing.is_empty() {
        missing_sources.extend(
            roots
                .iter()
                .map(|root| format!("{label}: {}", root.display())),
        );
    }
    existing
}

fn claude_projects_roots() -> Vec<PathBuf> {
    if let Ok(raw) = std::env::var("CLAUDE_CONFIG_DIR") {
        let roots = raw
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| {
                let path = PathBuf::from(value);
                if path.file_name().and_then(|name| name.to_str()) == Some("projects") {
                    path
                } else {
                    path.join("projects")
                }
            })
            .collect::<Vec<_>>();
        if !roots.is_empty() {
            return roots;
        }
    }

    let home = home_dir();
    vec![
        home.join(".config").join("claude").join("projects"),
        home.join(".claude").join("projects"),
    ]
}

fn codex_session_roots() -> Vec<PathBuf> {
    let codex_home = std::env::var("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(".codex"));
    vec![
        codex_home.join("sessions"),
        codex_home.join("archived_sessions"),
    ]
}

fn opencode_roots() -> Vec<PathBuf> {
    if let Ok(raw) = std::env::var("OPENCODE_DATA_DIR") {
        let path = PathBuf::from(raw.trim());
        if !path.as_os_str().is_empty() {
            return vec![path];
        }
    }
    vec![home_dir().join(".local").join("share").join("opencode")]
}

fn kimi_root() -> PathBuf {
    std::env::var("KIMI_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(".kimi"))
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn load_claude_events_from_roots(roots: &[PathBuf]) -> Result<Vec<LocalUsageEvent>, String> {
    let mut events = Vec::new();
    let mut seen = HashSet::new();
    for root in roots {
        for file in collect_files(root, &|path| {
            path.extension().and_then(|ext| ext.to_str()) == Some("jsonl")
        })? {
            let session_id = file
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("unknown")
                .to_string();
            for value in read_jsonl_values(&file)? {
                let Some(usage) = value
                    .get("message")
                    .and_then(|message| message.get("usage"))
                    .and_then(Value::as_object)
                else {
                    continue;
                };
                let timestamp = match value.get("timestamp").and_then(parse_rfc3339) {
                    Some(timestamp) => timestamp,
                    None => continue,
                };
                let message_id = value
                    .get("message")
                    .and_then(|message| message.get("id"))
                    .and_then(Value::as_str);
                let request_id = value.get("requestId").and_then(Value::as_str);
                if let (Some(message_id), Some(request_id)) = (message_id, request_id) {
                    let key = format!("{message_id}:{request_id}");
                    if !seen.insert(key) {
                        continue;
                    }
                }

                let event = LocalUsageEvent {
                    tool: TOOL_CLAUDE.to_string(),
                    model: value
                        .get("message")
                        .and_then(|message| message.get("model"))
                        .and_then(as_non_empty_string)
                        .map(str::to_string),
                    session_id: value
                        .get("sessionId")
                        .and_then(as_non_empty_string)
                        .unwrap_or(&session_id)
                        .to_string(),
                    timestamp,
                    input_tokens: object_u64(usage, "input_tokens"),
                    output_tokens: object_u64(usage, "output_tokens"),
                    cache_read_tokens: object_u64(usage, "cache_read_input_tokens"),
                    cache_creation_tokens: object_u64(usage, "cache_creation_input_tokens"),
                };
                if !event.is_zero() {
                    events.push(event);
                }
            }
        }
    }
    Ok(events)
}

fn load_codex_events_from_roots(roots: &[PathBuf]) -> Result<Vec<LocalUsageEvent>, String> {
    let mut events = Vec::new();
    for root in roots {
        for file in collect_files(root, &|path| {
            path.extension().and_then(|ext| ext.to_str()) == Some("jsonl")
        })? {
            let mut previous_totals: Option<RawCodexUsage> = None;
            let mut current_model: Option<String> = None;
            let session_id = file
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("unknown")
                .trim_end_matches(".jsonl")
                .to_string();

            for value in read_jsonl_values(&file)? {
                let entry_type = value.get("type").and_then(Value::as_str);
                if entry_type == Some("turn_context") {
                    if let Some(model) = extract_model(value.get("payload")) {
                        current_model = Some(model);
                    }
                    continue;
                }

                if entry_type != Some("event_msg") {
                    continue;
                }

                let Some(payload) = value.get("payload") else {
                    continue;
                };
                if payload.get("type").and_then(Value::as_str) != Some("token_count") {
                    continue;
                }
                let timestamp = match value.get("timestamp").and_then(parse_rfc3339) {
                    Some(timestamp) => timestamp,
                    None => continue,
                };
                let info = payload.get("info");
                let last_usage = info
                    .and_then(|info| info.get("last_token_usage"))
                    .and_then(parse_codex_usage);
                let total_usage = info
                    .and_then(|info| info.get("total_token_usage"))
                    .and_then(parse_codex_usage);
                let raw = last_usage.or_else(|| {
                    total_usage
                        .as_ref()
                        .map(|total| subtract_codex_usage(total, previous_totals.as_ref()))
                });
                if let Some(total_usage) = total_usage {
                    previous_totals = Some(total_usage);
                }
                let Some(raw) = raw else {
                    continue;
                };

                let cached = raw.cached_input_tokens.min(raw.input_tokens);
                let event = LocalUsageEvent {
                    tool: TOOL_CODEX.to_string(),
                    model: extract_model(Some(payload))
                        .or_else(|| current_model.clone())
                        .or_else(|| Some("gpt-5".to_string())),
                    session_id: session_id.clone(),
                    timestamp,
                    input_tokens: raw.input_tokens.saturating_sub(cached),
                    output_tokens: raw.output_tokens,
                    cache_read_tokens: cached,
                    cache_creation_tokens: 0,
                };
                if !event.is_zero() {
                    events.push(event);
                }
            }
        }
    }
    Ok(events)
}

fn load_opencode_events_from_roots(roots: &[PathBuf]) -> Result<Vec<LocalUsageEvent>, String> {
    let mut events = Vec::new();
    let mut seen = HashSet::new();
    for root in roots {
        let messages_dir = root.join("storage").join("message");
        if !messages_dir.is_dir() {
            continue;
        }
        for file in collect_files(&messages_dir, &|path| {
            path.extension().and_then(|ext| ext.to_str()) == Some("json")
        })? {
            let content = match fs::read_to_string(&file) {
                Ok(content) => content,
                Err(_) => continue,
            };
            let value = match serde_json::from_str::<Value>(&content) {
                Ok(value) => value,
                Err(_) => continue,
            };
            let Some(message_id) = value.get("id").and_then(as_non_empty_string) else {
                continue;
            };
            if !seen.insert(message_id.to_string()) {
                continue;
            }
            let Some(model) = value.get("modelID").and_then(as_non_empty_string) else {
                continue;
            };
            let Some(tokens) = value.get("tokens").and_then(Value::as_object) else {
                continue;
            };
            let timestamp = value
                .get("time")
                .and_then(|time| time.get("created"))
                .and_then(as_f64)
                .and_then(epoch_millis_to_utc)
                .unwrap_or_else(|| file_timestamp(&file));
            let session_id = value
                .get("sessionID")
                .and_then(as_non_empty_string)
                .map(str::to_string)
                .or_else(|| {
                    file.parent()
                        .and_then(|parent| parent.file_name())
                        .and_then(|name| name.to_str())
                        .map(str::to_string)
                })
                .unwrap_or_else(|| "unknown".into());
            let cache = value
                .get("tokens")
                .and_then(|tokens| tokens.get("cache"))
                .and_then(Value::as_object);
            let event = LocalUsageEvent {
                tool: TOOL_OPENCODE.to_string(),
                model: Some(model.to_string()),
                session_id,
                timestamp,
                input_tokens: object_u64(tokens, "input"),
                output_tokens: object_u64(tokens, "output"),
                cache_read_tokens: cache.map(|cache| object_u64(cache, "read")).unwrap_or(0),
                cache_creation_tokens: cache.map(|cache| object_u64(cache, "write")).unwrap_or(0),
            };
            if !event.is_zero() {
                events.push(event);
            }
        }
    }
    Ok(events)
}

fn load_kimi_events_from_root(root: &Path) -> Result<(Vec<LocalUsageEvent>, Vec<String>), String> {
    let sessions_dir = root.join("sessions");
    if !sessions_dir.is_dir() {
        return Ok((Vec::new(), Vec::new()));
    }

    let wire_files = collect_files(&sessions_dir, &|path| {
        path.file_name().and_then(|name| name.to_str()) == Some("wire.jsonl")
    })?;
    if !wire_files.is_empty() {
        let mut events = Vec::new();
        for file in wire_files {
            events.extend(load_kimi_wire_file(&file)?);
        }
        return Ok((events, Vec::new()));
    }

    let context_files = collect_files(&sessions_dir, &|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.starts_with("context") && name.ends_with(".jsonl"))
            .unwrap_or(false)
    })?;
    let mut events = Vec::new();
    for file in context_files {
        events.extend(load_kimi_context_file(&file)?);
    }
    let warnings = if events.is_empty() {
        Vec::new()
    } else {
        vec!["Kimi CLI 未找到 wire.jsonl，已使用 context token_count 正增量估算".into()]
    };
    Ok((events, warnings))
}

fn load_kimi_wire_file(file: &Path) -> Result<Vec<LocalUsageEvent>, String> {
    let session_id = parent_name(file).unwrap_or_else(|| "unknown".into());
    let mut events = Vec::new();
    for value in read_jsonl_values(file)? {
        let payload = value
            .get("message")
            .filter(|message| message.get("type").and_then(Value::as_str) == Some("StatusUpdate"))
            .and_then(|message| message.get("payload"));
        let Some(token_usage) = payload
            .and_then(|payload| payload.get("token_usage"))
            .and_then(Value::as_object)
        else {
            continue;
        };
        let timestamp = value
            .get("timestamp")
            .and_then(as_f64)
            .and_then(epoch_seconds_to_utc)
            .unwrap_or_else(|| file_timestamp(file));
        let event = LocalUsageEvent {
            tool: TOOL_KIMI.to_string(),
            model: Some("kimi-cli".into()),
            session_id: session_id.clone(),
            timestamp,
            input_tokens: object_u64(token_usage, "input_other"),
            output_tokens: object_u64(token_usage, "output"),
            cache_read_tokens: object_u64(token_usage, "input_cache_read"),
            cache_creation_tokens: object_u64(token_usage, "input_cache_creation"),
        };
        if !event.is_zero() {
            events.push(event);
        }
    }
    Ok(events)
}

fn load_kimi_context_file(file: &Path) -> Result<Vec<LocalUsageEvent>, String> {
    let session_id = parent_name(file).unwrap_or_else(|| "unknown".into());
    let timestamp = file_timestamp(file);
    let mut previous: Option<u64> = None;
    let mut events = Vec::new();
    for value in read_jsonl_values(file)? {
        if value.get("role").and_then(Value::as_str) != Some("_usage") {
            continue;
        }
        let Some(current) = value.get("token_count").and_then(as_u64) else {
            continue;
        };
        if let Some(previous_value) = previous {
            if current > previous_value {
                events.push(LocalUsageEvent {
                    tool: TOOL_KIMI.to_string(),
                    model: Some("kimi-cli".into()),
                    session_id: session_id.clone(),
                    timestamp,
                    input_tokens: current - previous_value,
                    output_tokens: 0,
                    cache_read_tokens: 0,
                    cache_creation_tokens: 0,
                });
            }
        }
        previous = Some(current);
    }
    Ok(events)
}

fn aggregate_events(
    range: LocalTokenUsageRange,
    now: DateTime<Utc>,
    events: Vec<LocalUsageEvent>,
    missing_sources: Vec<String>,
    warnings: Vec<String>,
) -> LocalTokenUsageReport {
    let start = range_start(range, now);
    let granularity = bucket_granularity(range);
    let bucket_starts = range_bucket_starts(range, now);
    aggregate_events_for_window(
        range,
        now,
        start,
        now,
        granularity,
        bucket_starts,
        None,
        None,
        events,
        missing_sources,
        warnings,
    )
}

fn aggregate_custom_events(
    now: DateTime<Utc>,
    start_date: NaiveDate,
    end_date: NaiveDate,
    events: Vec<LocalUsageEvent>,
    missing_sources: Vec<String>,
    warnings: Vec<String>,
) -> LocalTokenUsageReport {
    let start = Utc.from_utc_datetime(&start_date.and_hms_opt(0, 0, 0).unwrap());
    let inclusive_end = Utc.from_utc_datetime(&end_date.and_hms_opt(23, 59, 59).unwrap());
    let end = if inclusive_end > now {
        now
    } else {
        inclusive_end
    };
    aggregate_events_for_window(
        LocalTokenUsageRange::Custom,
        now,
        start,
        end,
        BucketGranularity::Day,
        day_bucket_starts(start_date, end_date),
        Some(start_date.format("%Y-%m-%d").to_string()),
        Some(end_date.format("%Y-%m-%d").to_string()),
        events,
        missing_sources,
        warnings,
    )
}

fn aggregate_events_for_window(
    range: LocalTokenUsageRange,
    now: DateTime<Utc>,
    range_start: DateTime<Utc>,
    range_end: DateTime<Utc>,
    granularity: BucketGranularity,
    bucket_starts: Vec<DateTime<Utc>>,
    start_date: Option<String>,
    end_date: Option<String>,
    events: Vec<LocalUsageEvent>,
    missing_sources: Vec<String>,
    warnings: Vec<String>,
) -> LocalTokenUsageReport {
    let filtered = events
        .into_iter()
        .filter(|event| event.timestamp >= range_start && event.timestamp <= range_end)
        .collect::<Vec<_>>();

    let mut totals = TokenStats::default();
    let mut by_bucket: HashMap<String, TokenStats> = HashMap::new();
    let mut by_bucket_model: HashMap<String, HashMap<String, TokenStats>> = HashMap::new();
    let mut by_model: HashMap<String, TokenStats> = HashMap::new();
    let mut by_tool: HashMap<String, TokenStats> = HashMap::new();
    let mut sessions_seen = HashSet::new();

    for event in &filtered {
        add_event(&mut totals, event);
        let bucket = bucket_key(
            granularity,
            bucket_start_for_event(granularity, event.timestamp),
        );
        add_event(by_bucket.entry(bucket.clone()).or_default(), event);
        add_event(
            by_bucket_model
                .entry(bucket)
                .or_default()
                .entry(event.model.clone().unwrap_or_else(|| "unknown".into()))
                .or_default(),
            event,
        );
        add_event(
            by_model
                .entry(event.model.clone().unwrap_or_else(|| "unknown".into()))
                .or_default(),
            event,
        );
        add_event(by_tool.entry(event.tool.clone()).or_default(), event);
        sessions_seen.insert((event.tool.as_str(), event.session_id.as_str()));
    }

    let days = bucket_starts
        .into_iter()
        .map(|bucket_start| {
            let date = bucket_key(granularity, bucket_start);
            let stats = by_bucket.remove(&date).unwrap_or_default();
            let mut models = by_bucket_model
                .remove(&date)
                .unwrap_or_default()
                .into_iter()
                .map(|(model, stats)| model_usage(model, stats))
                .collect::<Vec<_>>();
            models.sort_by(|a, b| {
                b.total_tokens
                    .cmp(&a.total_tokens)
                    .then_with(|| a.model.cmp(&b.model))
            });
            LocalTokenUsageDay {
                date,
                input_tokens: stats.input_tokens,
                output_tokens: stats.output_tokens,
                cache_read_tokens: stats.cache_read_tokens,
                cache_creation_tokens: stats.cache_creation_tokens,
                total_tokens: stats.total_tokens(),
                models,
            }
        })
        .collect::<Vec<_>>();

    let mut models = by_model
        .into_iter()
        .map(|(model, stats)| model_usage(model, stats))
        .collect::<Vec<_>>();
    models.sort_by(|a, b| {
        b.total_tokens
            .cmp(&a.total_tokens)
            .then_with(|| a.model.cmp(&b.model))
    });

    let mut tools = by_tool
        .into_iter()
        .map(|(tool, stats)| LocalTokenUsageTool {
            tool,
            input_tokens: stats.input_tokens,
            output_tokens: stats.output_tokens,
            cache_read_tokens: stats.cache_read_tokens,
            cache_creation_tokens: stats.cache_creation_tokens,
            total_tokens: stats.total_tokens(),
        })
        .collect::<Vec<_>>();
    tools.sort_by(|a, b| {
        b.total_tokens
            .cmp(&a.total_tokens)
            .then_with(|| a.tool.cmp(&b.tool))
    });

    LocalTokenUsageReport {
        range,
        start_date,
        end_date,
        pending: false,
        totals: LocalTokenUsageTotals {
            input_tokens: totals.input_tokens,
            output_tokens: totals.output_tokens,
            cache_read_tokens: totals.cache_read_tokens,
            cache_creation_tokens: totals.cache_creation_tokens,
            total_tokens: totals.total_tokens(),
            cache_hit_rate_percent: cache_hit_rate(&totals),
        },
        days,
        models,
        tools,
        missing_sources,
        warnings,
        generated_at: now,
    }
}

fn add_event(stats: &mut TokenStats, event: &LocalUsageEvent) {
    stats.input_tokens = stats.input_tokens.saturating_add(event.input_tokens);
    stats.output_tokens = stats.output_tokens.saturating_add(event.output_tokens);
    stats.cache_read_tokens = stats
        .cache_read_tokens
        .saturating_add(event.cache_read_tokens);
    stats.cache_creation_tokens = stats
        .cache_creation_tokens
        .saturating_add(event.cache_creation_tokens);
}

fn cache_hit_rate(stats: &TokenStats) -> f64 {
    let denominator = stats.input_tokens + stats.cache_read_tokens;
    if denominator == 0 {
        0.0
    } else {
        (stats.cache_read_tokens as f64 / denominator as f64) * 100.0
    }
}

fn model_usage(model: String, stats: TokenStats) -> LocalTokenUsageModel {
    LocalTokenUsageModel {
        model,
        input_tokens: stats.input_tokens,
        output_tokens: stats.output_tokens,
        cache_read_tokens: stats.cache_read_tokens,
        cache_creation_tokens: stats.cache_creation_tokens,
        total_tokens: stats.total_tokens(),
    }
}

fn range_start(range: LocalTokenUsageRange, now: DateTime<Utc>) -> DateTime<Utc> {
    let today = now.date_naive();
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
    Utc.from_utc_datetime(&start_date.and_hms_opt(0, 0, 0).unwrap())
}

fn bucket_granularity(range: LocalTokenUsageRange) -> BucketGranularity {
    match range {
        LocalTokenUsageRange::Today => BucketGranularity::Hour,
        LocalTokenUsageRange::Last3Days => BucketGranularity::ThreeHours,
        LocalTokenUsageRange::ThisWeek
        | LocalTokenUsageRange::ThisMonth
        | LocalTokenUsageRange::Custom => BucketGranularity::Day,
    }
}

fn range_bucket_starts(range: LocalTokenUsageRange, now: DateTime<Utc>) -> Vec<DateTime<Utc>> {
    let granularity = bucket_granularity(range);
    let step = match granularity {
        BucketGranularity::Day => Duration::days(1),
        BucketGranularity::Hour => Duration::hours(1),
        BucketGranularity::ThreeHours => Duration::hours(3),
    };
    let mut current = range_start(range, now);
    let end = match range {
        LocalTokenUsageRange::Today => current + Duration::hours(23),
        LocalTokenUsageRange::ThisWeek => current + Duration::days(6),
        LocalTokenUsageRange::ThisMonth => {
            let month_end = month_end_date(now.date_naive());
            Utc.from_utc_datetime(&month_end.and_hms_opt(0, 0, 0).unwrap())
        }
        _ => now,
    };
    let mut starts = Vec::new();
    while current <= end {
        starts.push(current);
        current = current + step;
    }
    starts
}

fn month_end_date(date: NaiveDate) -> NaiveDate {
    let (next_year, next_month) = if date.month() == 12 {
        (date.year() + 1, 1)
    } else {
        (date.year(), date.month() + 1)
    };
    NaiveDate::from_ymd_opt(next_year, next_month, 1).unwrap_or(date) - Duration::days(1)
}

fn day_bucket_starts(start_date: NaiveDate, end_date: NaiveDate) -> Vec<DateTime<Utc>> {
    let mut current = start_date;
    let mut starts = Vec::new();
    while current <= end_date {
        starts.push(Utc.from_utc_datetime(&current.and_hms_opt(0, 0, 0).unwrap()));
        current = current + Duration::days(1);
    }
    starts
}

fn bucket_start_for_event(
    granularity: BucketGranularity,
    timestamp: DateTime<Utc>,
) -> DateTime<Utc> {
    let date = timestamp.date_naive();
    let hour = match granularity {
        BucketGranularity::Day => 0,
        BucketGranularity::Hour => timestamp.hour(),
        BucketGranularity::ThreeHours => timestamp.hour() - (timestamp.hour() % 3),
    };
    Utc.from_utc_datetime(&date.and_hms_opt(hour, 0, 0).unwrap())
}

fn bucket_key(granularity: BucketGranularity, timestamp: DateTime<Utc>) -> String {
    match granularity {
        BucketGranularity::Day => timestamp.date_naive().format("%Y-%m-%d").to_string(),
        BucketGranularity::Hour | BucketGranularity::ThreeHours => {
            timestamp.format("%Y-%m-%dT%H:00:00Z").to_string()
        }
    }
}

fn collect_files(root: &Path, predicate: &dyn Fn(&Path) -> bool) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    collect_files_inner(root, predicate, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_files_inner(
    root: &Path,
    predicate: &dyn Fn(&Path) -> bool,
    files: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) => return Err(format!("读取目录失败（{}）: {error}", root.display())),
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.is_dir() {
            collect_files_inner(&path, predicate, files)?;
        } else if predicate(&path) {
            files.push(path);
        }
    }
    Ok(())
}

fn read_jsonl_values(file: &Path) -> Result<Vec<Value>, String> {
    let handle =
        File::open(file).map_err(|error| format!("读取文件失败（{}）: {error}", file.display()))?;
    let reader = BufReader::new(handle);
    let mut values = Vec::new();
    for line in reader.lines() {
        let Ok(line) = line else {
            continue;
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
            values.push(value);
        }
    }
    Ok(values)
}

fn extract_model(value: Option<&Value>) -> Option<String> {
    let value = value?;
    if let Some(info) = value.get("info") {
        if let Some(model) = info.get("model").and_then(as_non_empty_string) {
            return Some(model.to_string());
        }
        if let Some(model) = info.get("model_name").and_then(as_non_empty_string) {
            return Some(model.to_string());
        }
        if let Some(model) = info
            .get("metadata")
            .and_then(|metadata| metadata.get("model"))
            .and_then(as_non_empty_string)
        {
            return Some(model.to_string());
        }
    }
    if let Some(model) = value.get("model").and_then(as_non_empty_string) {
        return Some(model.to_string());
    }
    value
        .get("metadata")
        .and_then(|metadata| metadata.get("model"))
        .and_then(as_non_empty_string)
        .map(str::to_string)
}

fn parse_codex_usage(value: &Value) -> Option<RawCodexUsage> {
    let value = value.as_object()?;
    Some(RawCodexUsage {
        input_tokens: object_u64(value, "input_tokens"),
        cached_input_tokens: object_u64(value, "cached_input_tokens")
            .max(object_u64(value, "cache_read_input_tokens")),
        output_tokens: object_u64(value, "output_tokens"),
        reasoning_output_tokens: object_u64(value, "reasoning_output_tokens"),
        total_tokens: object_u64(value, "total_tokens"),
    })
}

fn subtract_codex_usage(
    current: &RawCodexUsage,
    previous: Option<&RawCodexUsage>,
) -> RawCodexUsage {
    RawCodexUsage {
        input_tokens: current
            .input_tokens
            .saturating_sub(previous.map(|usage| usage.input_tokens).unwrap_or(0)),
        cached_input_tokens: current
            .cached_input_tokens
            .saturating_sub(previous.map(|usage| usage.cached_input_tokens).unwrap_or(0)),
        output_tokens: current
            .output_tokens
            .saturating_sub(previous.map(|usage| usage.output_tokens).unwrap_or(0)),
        reasoning_output_tokens: current.reasoning_output_tokens.saturating_sub(
            previous
                .map(|usage| usage.reasoning_output_tokens)
                .unwrap_or(0),
        ),
        total_tokens: current
            .total_tokens
            .saturating_sub(previous.map(|usage| usage.total_tokens).unwrap_or(0)),
    }
}

fn object_u64(map: &serde_json::Map<String, Value>, key: &str) -> u64 {
    map.get(key).and_then(as_u64).unwrap_or(0)
}

fn as_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|number| u64::try_from(number).ok()))
        .or_else(|| value.as_f64().map(|number| number.max(0.0).floor() as u64))
        .or_else(|| value.as_str()?.trim().parse::<u64>().ok())
}

fn as_f64(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_i64().map(|number| number as f64))
        .or_else(|| value.as_str()?.trim().parse::<f64>().ok())
}

fn as_non_empty_string(value: &Value) -> Option<&str> {
    let value = value.as_str()?.trim();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn parse_rfc3339(value: &Value) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value.as_str()?)
        .ok()
        .map(|timestamp| timestamp.with_timezone(&Utc))
}

fn epoch_seconds_to_utc(seconds: f64) -> Option<DateTime<Utc>> {
    if !seconds.is_finite() {
        return None;
    }
    let whole = seconds.floor() as i64;
    let nanos = ((seconds - whole as f64) * 1_000_000_000.0).round() as u32;
    Utc.timestamp_opt(whole, nanos.min(999_999_999)).single()
}

fn epoch_millis_to_utc(millis: f64) -> Option<DateTime<Utc>> {
    epoch_seconds_to_utc(millis / 1000.0)
}

fn file_timestamp(path: &Path) -> DateTime<Utc> {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map(DateTime::<Utc>::from)
        .unwrap_or_else(|_| DateTime::<Utc>::from(SystemTime::now()))
}

fn parent_name(path: &Path) -> Option<String> {
    path.parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        .map(str::to_string)
}

impl LocalUsageEvent {
    fn is_zero(&self) -> bool {
        self.input_tokens == 0
            && self.output_tokens == 0
            && self.cache_read_tokens == 0
            && self.cache_creation_tokens == 0
    }
}

impl TokenStats {
    fn total_tokens(&self) -> u64 {
        self.input_tokens
            .saturating_add(self.output_tokens)
            .saturating_add(self.cache_read_tokens)
            .saturating_add(self.cache_creation_tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::{fs, path::Path};

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "ai-usage-local-usage-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    #[test]
    fn existing_roots_skips_missing_candidates_when_at_least_one_exists() {
        let root = temp_dir("existing-root-candidates");
        let missing = root.join("missing");
        let existing = root.join("existing");
        fs::create_dir_all(&existing).unwrap();
        let mut missing_sources = Vec::new();

        let roots = existing_roots(
            "Claude Code",
            &[missing.clone(), existing.clone()],
            &mut missing_sources,
        );

        assert_eq!(roots, vec![existing]);
        assert!(missing_sources.is_empty());
    }

    #[test]
    fn claude_loader_dedupes_by_message_and_request_ids() {
        let root = temp_dir("claude");
        let file = root
            .join("projects")
            .join("-Users-test-project")
            .join("session.jsonl");
        let line = r#"{"timestamp":"2026-04-27T01:00:00Z","requestId":"req-1","message":{"id":"msg-1","model":"claude-sonnet-4-5","usage":{"input_tokens":100,"output_tokens":40,"cache_creation_input_tokens":7,"cache_read_input_tokens":13}}}"#;
        write_file(&file, &format!("{line}\n{line}\n"));

        let events = load_claude_events_from_roots(&[root.join("projects")]).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].tool, "claude");
        assert_eq!(events[0].model.as_deref(), Some("claude-sonnet-4-5"));
        assert_eq!(events[0].input_tokens, 100);
        assert_eq!(events[0].output_tokens, 40);
        assert_eq!(events[0].cache_creation_tokens, 7);
        assert_eq!(events[0].cache_read_tokens, 13);
    }

    #[test]
    fn codex_loader_uses_last_usage_and_splits_cached_input() {
        let root = temp_dir("codex");
        let file = root
            .join("2026")
            .join("04")
            .join("27")
            .join("rollout.jsonl");
        write_file(
            &file,
            r#"{"timestamp":"2026-04-27T01:00:00Z","type":"turn_context","payload":{"model":"gpt-5.3-codex"}}
{"timestamp":"2026-04-27T01:01:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":120,"cached_input_tokens":50,"output_tokens":30,"reasoning_output_tokens":10,"total_tokens":150}}}}
"#,
        );

        let events = load_codex_events_from_roots(&[root]).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].model.as_deref(), Some("gpt-5.3-codex"));
        assert_eq!(events[0].input_tokens, 70);
        assert_eq!(events[0].cache_read_tokens, 50);
        assert_eq!(events[0].output_tokens, 30);
    }

    #[test]
    fn opencode_loader_dedupes_message_ids_and_reads_cache_tokens() {
        let root = temp_dir("opencode");
        let message_dir = root.join("storage").join("message").join("session-1");
        let body = r#"{"id":"msg-1","sessionID":"session-1","providerID":"anthropic","modelID":"claude-3.5","time":{"created":1777248000000},"tokens":{"input":20,"output":8,"cache":{"read":5,"write":2}}}"#;
        write_file(&message_dir.join("a.json"), body);
        write_file(&message_dir.join("b.json"), body);

        let events = load_opencode_events_from_roots(&[root]).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].tool, "opencode");
        assert_eq!(events[0].cache_read_tokens, 5);
        assert_eq!(events[0].cache_creation_tokens, 2);
    }

    #[test]
    fn kimi_loader_prefers_wire_status_updates() {
        let root = temp_dir("kimi");
        let session = root.join("sessions").join("project").join("session");
        write_file(
            &session.join("wire.jsonl"),
            r#"{"timestamp":1777248000,"message":{"type":"StatusUpdate","payload":{"token_usage":{"input_other":80,"input_cache_read":40,"input_cache_creation":10,"output":20}}}}"#,
        );
        write_file(
            &session.join("context.jsonl"),
            "{\"role\":\"_usage\",\"token_count\":1000}\n{\"role\":\"_usage\",\"token_count\":1500}\n",
        );

        let (events, warnings) = load_kimi_events_from_root(&root).unwrap();

        assert!(warnings.is_empty());
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].input_tokens, 80);
        assert_eq!(events[0].cache_read_tokens, 40);
        assert_eq!(events[0].cache_creation_tokens, 10);
        assert_eq!(events[0].output_tokens, 20);
    }

    #[test]
    fn kimi_loader_falls_back_to_positive_context_deltas() {
        let root = temp_dir("kimi-context");
        let session = root.join("sessions").join("project").join("session");
        write_file(
            &session.join("context.jsonl"),
            "{\"role\":\"_usage\",\"token_count\":100}\n{\"role\":\"_usage\",\"token_count\":150}\n{\"role\":\"_usage\",\"token_count\":140}\n{\"role\":\"_usage\",\"token_count\":175}\n",
        );

        let (events, warnings) = load_kimi_events_from_root(&root).unwrap();

        assert_eq!(warnings.len(), 1);
        assert_eq!(events.len(), 2);
        assert_eq!(
            events.iter().map(|event| event.input_tokens).sum::<u64>(),
            85
        );
    }

    #[test]
    fn report_filters_this_month_and_calculates_cache_hit_rate() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 4, 27, 12, 0, 0).unwrap();
        let events = vec![
            usage_event("claude", "gpt-a", "2026-04-01T00:00:00Z", 70, 20, 30, 0),
            usage_event("codex", "gpt-b", "2026-03-31T23:00:00Z", 999, 999, 999, 999),
        ];

        let report = aggregate_events(LocalTokenUsageRange::ThisMonth, now, events, vec![], vec![]);

        assert_eq!(report.totals.input_tokens, 70);
        assert_eq!(report.totals.output_tokens, 20);
        assert_eq!(report.totals.cache_read_tokens, 30);
        assert_eq!(report.totals.total_tokens, 120);
        assert_eq!(report.totals.cache_hit_rate_percent, 30.0);
        assert_eq!(report.models[0].model, "gpt-a");
    }

    #[test]
    fn report_this_month_returns_each_calendar_day_with_model_breakdown() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 4, 3, 12, 0, 0).unwrap();
        let events = vec![
            usage_event(
                "claude",
                "claude-sonnet",
                "2026-04-01T10:00:00Z",
                10,
                5,
                0,
                0,
            ),
            usage_event("codex", "gpt-codex", "2026-04-03T08:00:00Z", 20, 10, 5, 0),
        ];

        let report = aggregate_events(LocalTokenUsageRange::ThisMonth, now, events, vec![], vec![]);

        assert_eq!(report.days.len(), 30);
        assert_eq!(
            report
                .days
                .iter()
                .take(3)
                .map(|day| day.date.as_str())
                .collect::<Vec<_>>(),
            vec!["2026-04-01", "2026-04-02", "2026-04-03"]
        );
        assert_eq!(
            report.days.last().map(|day| day.date.as_str()),
            Some("2026-04-30")
        );
        assert_eq!(report.days[0].models[0].model, "claude-sonnet");
        assert_eq!(report.days[0].models[0].total_tokens, 15);
        assert!(report.days[1].models.is_empty());
        assert_eq!(report.days[2].models[0].model, "gpt-codex");
        assert!(report.days[29].models.is_empty());
    }

    #[test]
    fn report_today_returns_hourly_buckets_for_full_day() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 4, 27, 3, 30, 0).unwrap();
        let events = vec![
            usage_event(
                "claude",
                "claude-sonnet",
                "2026-04-27T00:10:00Z",
                10,
                0,
                0,
                0,
            ),
            usage_event("codex", "gpt-codex", "2026-04-27T03:10:00Z", 20, 0, 0, 0),
        ];

        let report = aggregate_events(LocalTokenUsageRange::Today, now, events, vec![], vec![]);

        assert_eq!(
            report
                .days
                .iter()
                .take(4)
                .map(|day| day.date.as_str())
                .collect::<Vec<_>>(),
            vec![
                "2026-04-27T00:00:00Z",
                "2026-04-27T01:00:00Z",
                "2026-04-27T02:00:00Z",
                "2026-04-27T03:00:00Z",
            ]
        );
        assert_eq!(report.days.len(), 24);
        assert_eq!(
            report.days.last().map(|day| day.date.as_str()),
            Some("2026-04-27T23:00:00Z")
        );
        assert_eq!(report.days[0].models[0].model, "claude-sonnet");
        assert!(report.days[1].models.is_empty());
        assert!(report.days[2].models.is_empty());
        assert_eq!(report.days[3].models[0].model, "gpt-codex");
    }

    #[test]
    fn report_this_week_returns_monday_to_sunday() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 4, 29, 12, 0, 0).unwrap();
        let report = aggregate_events(LocalTokenUsageRange::ThisWeek, now, vec![], vec![], vec![]);

        assert_eq!(
            report
                .days
                .iter()
                .map(|day| day.date.as_str())
                .collect::<Vec<_>>(),
            vec![
                "2026-04-27",
                "2026-04-28",
                "2026-04-29",
                "2026-04-30",
                "2026-05-01",
                "2026-05-02",
                "2026-05-03",
            ]
        );
    }

    #[test]
    fn report_last_three_days_returns_three_hour_buckets() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 4, 27, 7, 30, 0).unwrap();
        let events = vec![usage_event(
            "codex",
            "gpt-codex",
            "2026-04-27T06:10:00Z",
            20,
            0,
            0,
            0,
        )];

        let report = aggregate_events(LocalTokenUsageRange::Last3Days, now, events, vec![], vec![]);

        assert_eq!(
            report.days.first().map(|day| day.date.as_str()),
            Some("2026-04-25T00:00:00Z")
        );
        assert_eq!(
            report.days.last().map(|day| day.date.as_str()),
            Some("2026-04-27T06:00:00Z")
        );
        assert_eq!(report.days.len(), 19);
        assert_eq!(report.days[18].models[0].model, "gpt-codex");
    }

    #[test]
    fn report_custom_range_returns_daily_buckets_for_inclusive_dates() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 4, 27, 7, 30, 0).unwrap();
        let start = NaiveDate::from_ymd_opt(2026, 4, 20).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 4, 22).unwrap();
        let events = vec![
            usage_event("codex", "gpt-codex", "2026-04-19T23:59:00Z", 999, 0, 0, 0),
            usage_event("codex", "gpt-codex", "2026-04-20T00:00:00Z", 10, 0, 0, 0),
            usage_event("kimi", "kimi-cli", "2026-04-22T23:59:00Z", 20, 0, 0, 0),
            usage_event(
                "claude",
                "claude-sonnet",
                "2026-04-23T00:00:00Z",
                999,
                0,
                0,
                0,
            ),
        ];

        let report = aggregate_custom_events(now, start, end, events, vec![], vec![]);

        assert_eq!(report.range, LocalTokenUsageRange::Custom);
        assert_eq!(report.start_date.as_deref(), Some("2026-04-20"));
        assert_eq!(report.end_date.as_deref(), Some("2026-04-22"));
        assert_eq!(report.totals.total_tokens, 30);
        assert_eq!(
            report
                .days
                .iter()
                .map(|day| day.date.as_str())
                .collect::<Vec<_>>(),
            vec!["2026-04-20", "2026-04-21", "2026-04-22"]
        );
        assert_eq!(report.days[0].models[0].model, "gpt-codex");
        assert!(report.days[1].models.is_empty());
        assert_eq!(report.days[2].models[0].model, "kimi-cli");
    }

    #[test]
    fn cache_from_events_precomputes_each_token_range() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 4, 27, 7, 30, 0).unwrap();
        let events = vec![
            usage_event("codex", "gpt-codex", "2026-04-27T06:10:00Z", 20, 0, 0, 0),
            usage_event("kimi", "kimi-cli", "2026-04-26T06:10:00Z", 30, 0, 0, 0),
            usage_event(
                "claude",
                "claude-sonnet",
                "2026-04-01T06:10:00Z",
                40,
                0,
                0,
                0,
            ),
        ];

        let cache = build_cache_from_events(now, events, vec![], vec![]);

        assert_eq!(
            cache
                .report(LocalTokenUsageRange::Today)
                .totals
                .total_tokens,
            20
        );
        assert_eq!(
            cache
                .report(LocalTokenUsageRange::Last3Days)
                .totals
                .total_tokens,
            50
        );
        assert_eq!(
            cache
                .report(LocalTokenUsageRange::ThisMonth)
                .totals
                .total_tokens,
            90
        );
        assert_eq!(
            cache.report(LocalTokenUsageRange::ThisWeek).generated_at,
            now
        );
        assert_eq!(
            cache.custom_window_start,
            Some(NaiveDate::from_ymd_opt(2026, 1, 28).unwrap())
        );
        assert_eq!(
            cache.custom_window_end,
            Some(NaiveDate::from_ymd_opt(2026, 4, 27).unwrap())
        );
        assert_eq!(cache.custom_days.len(), 90);
    }

    #[test]
    fn custom_report_aggregates_cached_daily_rows() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 4, 28, 7, 30, 0).unwrap();
        let cache = LocalTokenUsageCache {
            generated_at: now,
            today: empty_report(LocalTokenUsageRange::Today, None),
            last3_days: empty_report(LocalTokenUsageRange::Last3Days, None),
            this_week: empty_report(LocalTokenUsageRange::ThisWeek, None),
            this_month: empty_report(LocalTokenUsageRange::ThisMonth, None),
            custom_window_start: Some(NaiveDate::from_ymd_opt(2026, 1, 29).unwrap()),
            custom_window_end: Some(NaiveDate::from_ymd_opt(2026, 4, 28).unwrap()),
            custom_days: vec![
                LocalTokenUsageCachedDay {
                    date: "2026-04-20".into(),
                    totals: LocalTokenUsageTotals {
                        input_tokens: 10,
                        output_tokens: 5,
                        cache_read_tokens: 2,
                        cache_creation_tokens: 1,
                        total_tokens: 18,
                        cache_hit_rate_percent: 16.7,
                    },
                    models: vec![LocalTokenUsageModel {
                        model: "gpt-4.1".into(),
                        input_tokens: 10,
                        output_tokens: 5,
                        cache_read_tokens: 2,
                        cache_creation_tokens: 1,
                        total_tokens: 18,
                    }],
                    tools: vec![LocalTokenUsageTool {
                        tool: "codex".into(),
                        input_tokens: 10,
                        output_tokens: 5,
                        cache_read_tokens: 2,
                        cache_creation_tokens: 1,
                        total_tokens: 18,
                    }],
                },
                LocalTokenUsageCachedDay {
                    date: "2026-04-21".into(),
                    totals: LocalTokenUsageTotals::default(),
                    models: vec![],
                    tools: vec![],
                },
                LocalTokenUsageCachedDay {
                    date: "2026-04-22".into(),
                    totals: LocalTokenUsageTotals {
                        input_tokens: 20,
                        output_tokens: 10,
                        cache_read_tokens: 0,
                        cache_creation_tokens: 0,
                        total_tokens: 30,
                        cache_hit_rate_percent: 0.0,
                    },
                    models: vec![LocalTokenUsageModel {
                        model: "claude-sonnet".into(),
                        input_tokens: 20,
                        output_tokens: 10,
                        cache_read_tokens: 0,
                        cache_creation_tokens: 0,
                        total_tokens: 30,
                    }],
                    tools: vec![LocalTokenUsageTool {
                        tool: "claude".into(),
                        input_tokens: 20,
                        output_tokens: 10,
                        cache_read_tokens: 0,
                        cache_creation_tokens: 0,
                        total_tokens: 30,
                    }],
                },
            ],
        };

        let report = cache.custom_report(
            NaiveDate::from_ymd_opt(2026, 4, 20).unwrap(),
            NaiveDate::from_ymd_opt(2026, 4, 22).unwrap(),
        );

        assert_eq!(report.range, LocalTokenUsageRange::Custom);
        assert_eq!(report.start_date.as_deref(), Some("2026-04-20"));
        assert_eq!(report.end_date.as_deref(), Some("2026-04-22"));
        assert_eq!(report.totals.total_tokens, 48);
        assert_eq!(report.days.len(), 3);
        assert_eq!(report.tools.len(), 2);
        assert_eq!(report.models[0].model, "claude-sonnet");
    }

    fn usage_event(
        tool: &str,
        model: &str,
        timestamp: &str,
        input_tokens: u64,
        output_tokens: u64,
        cache_read_tokens: u64,
        cache_creation_tokens: u64,
    ) -> LocalUsageEvent {
        LocalUsageEvent {
            tool: tool.to_string(),
            model: Some(model.to_string()),
            session_id: "session".into(),
            timestamp: chrono::DateTime::parse_from_rfc3339(timestamp)
                .unwrap()
                .with_timezone(&chrono::Utc),
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_creation_tokens,
        }
    }
}
