use crate::{
    app_time,
    models::{
        GitUsageBucket, GitUsageReport, GitUsageRepository, GitUsageTotals, LocalTokenUsageRange,
        CUSTOM_USAGE_WINDOW_DAYS,
    },
};
use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone)]
struct GitCommitStat {
    commit_hash: String,
    timestamp: DateTime<Utc>,
    repository_name: String,
    repository_path: String,
    added_lines: u64,
    deleted_lines: u64,
    changed_files: u64,
}

#[derive(Debug, Clone, Default)]
struct GitBucketStats {
    added_lines: u64,
    deleted_lines: u64,
    changed_files: u64,
}

#[derive(Debug, Clone, Copy)]
enum BucketGranularity {
    Day,
    Hour,
    ThreeHours,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitUsageCache {
    #[serde(default)]
    pub root_path: String,
    #[serde(with = "crate::app_time::local_datetime_serde")]
    pub generated_at: DateTime<Utc>,
    pub today: GitUsageReport,
    pub last3_days: GitUsageReport,
    pub this_week: GitUsageReport,
    pub this_month: GitUsageReport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_window_start: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_window_end: Option<NaiveDate>,
    #[serde(default)]
    pub custom_days: Vec<GitUsageCachedDay>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GitUsageCachedDay {
    pub date: String,
    pub totals: GitUsageTotals,
    #[serde(default)]
    pub repositories: Vec<GitUsageRepository>,
}

impl GitUsageCache {
    pub fn report(&self, range: LocalTokenUsageRange) -> GitUsageReport {
        let mut report = match range {
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
        };
        report
            .warnings
            .retain(|warning| !is_stale_worktree_warning(warning));
        report
    }

    pub fn covers_custom_range(&self, start_date: NaiveDate, end_date: NaiveDate) -> bool {
        matches!(
            (self.custom_window_start, self.custom_window_end),
            (Some(window_start), Some(window_end))
                if start_date >= window_start && end_date <= window_end && !self.custom_days.is_empty()
        )
    }

    pub fn custom_report(&self, start_date: NaiveDate, end_date: NaiveDate) -> GitUsageReport {
        let days_by_date = self
            .custom_days
            .iter()
            .map(|day| (day.date.as_str(), day))
            .collect::<HashMap<_, _>>();
        let mut current = start_date;
        let mut totals = GitUsageTotals::default();
        let mut repositories_by_path = HashMap::<String, GitUsageRepository>::new();
        let mut buckets = Vec::new();

        while current <= end_date {
            let date = current.format("%Y-%m-%d").to_string();
            let cached_day = days_by_date.get(date.as_str());
            let day_totals = cached_day.map(|day| day.totals.clone()).unwrap_or_default();
            totals.added_lines = totals.added_lines.saturating_add(day_totals.added_lines);
            totals.deleted_lines = totals
                .deleted_lines
                .saturating_add(day_totals.deleted_lines);
            totals.changed_files = totals
                .changed_files
                .saturating_add(day_totals.changed_files);

            for repository in cached_day
                .map(|day| day.repositories.iter())
                .into_iter()
                .flatten()
            {
                let entry = repositories_by_path
                    .entry(repository.path.clone())
                    .or_insert_with(|| GitUsageRepository {
                        name: repository.name.clone(),
                        path: repository.path.clone(),
                        added_lines: 0,
                        deleted_lines: 0,
                        changed_files: 0,
                    });
                entry.added_lines = entry.added_lines.saturating_add(repository.added_lines);
                entry.deleted_lines = entry.deleted_lines.saturating_add(repository.deleted_lines);
                entry.changed_files = entry.changed_files.saturating_add(repository.changed_files);
            }

            buckets.push(GitUsageBucket {
                date,
                added_lines: day_totals.added_lines,
                deleted_lines: day_totals.deleted_lines,
                changed_files: day_totals.changed_files,
            });
            current += Duration::days(1);
        }

        let mut repositories = repositories_by_path
            .into_values()
            .filter(|repository| {
                repository
                    .added_lines
                    .saturating_add(repository.deleted_lines)
                    .saturating_add(repository.changed_files)
                    > 0
            })
            .collect::<Vec<_>>();
        sort_repositories(&mut repositories);

        GitUsageReport {
            range: LocalTokenUsageRange::Custom,
            start_date: Some(start_date.format("%Y-%m-%d").to_string()),
            end_date: Some(end_date.format("%Y-%m-%d").to_string()),
            pending: false,
            totals,
            buckets,
            repositories,
            repository_count: self.this_month.repository_count,
            missing_sources: Vec::new(),
            warnings: filter_stale_worktree_warnings(self.this_month.warnings.clone()),
            generated_at: self.generated_at,
        }
    }
}

pub fn build_cache(root: PathBuf) -> Result<GitUsageCache, String> {
    let now = Utc::now();
    let root_path = root.to_string_lossy().to_string();
    let (repositories, mut warnings) = match discover_git_repositories(&root) {
        Ok(repositories) => (repositories, Vec::new()),
        Err(error) => (Vec::new(), vec![error]),
    };
    let since = earliest_cached_start(now);
    let mut stats = Vec::new();

    for repository in &repositories {
        match load_repository_stats(repository, since) {
            Ok(mut next) => stats.append(&mut next),
            Err(error) => warnings.push(format!("{}: {error}", repository.display())),
        }
    }

    Ok(build_cache_from_stats(
        now,
        root_path,
        stats,
        repositories.len(),
        warnings,
    ))
}

pub fn empty_report(range: LocalTokenUsageRange, warning: Option<String>) -> GitUsageReport {
    if range == LocalTokenUsageRange::Custom {
        let today = Utc::now().date_naive();
        return aggregate_custom_git_stats(
            Utc::now(),
            today,
            today,
            Vec::new(),
            0,
            warning.into_iter().collect(),
        );
    }
    aggregate_git_stats(
        range,
        Utc::now(),
        Vec::new(),
        0,
        warning.into_iter().collect(),
    )
}

pub fn pending_report(range: LocalTokenUsageRange, warning: Option<String>) -> GitUsageReport {
    let mut report = empty_report(range, warning);
    report.pending = true;
    report
}

pub fn pending_custom_report(
    start_date: NaiveDate,
    end_date: NaiveDate,
    warning: Option<String>,
) -> GitUsageReport {
    let mut report = aggregate_custom_git_stats(
        Utc::now(),
        start_date,
        end_date,
        Vec::new(),
        0,
        warning.into_iter().collect(),
    );
    report.pending = true;
    report
}

fn build_cache_from_stats(
    now: DateTime<Utc>,
    root_path: String,
    stats: Vec<GitCommitStat>,
    repository_count: usize,
    warnings: Vec<String>,
) -> GitUsageCache {
    let stats = dedupe_git_stats_by_commit_hash(stats);
    let offset = app_time::local_offset();
    let (custom_window_start, custom_window_end, custom_days) =
        build_custom_days(now, &stats, offset);
    GitUsageCache {
        root_path,
        generated_at: now,
        today: aggregate_git_stats(
            LocalTokenUsageRange::Today,
            now,
            stats.clone(),
            repository_count,
            warnings.clone(),
        ),
        last3_days: aggregate_git_stats(
            LocalTokenUsageRange::Last3Days,
            now,
            stats.clone(),
            repository_count,
            warnings.clone(),
        ),
        this_week: aggregate_git_stats(
            LocalTokenUsageRange::ThisWeek,
            now,
            stats.clone(),
            repository_count,
            warnings.clone(),
        ),
        this_month: aggregate_git_stats(
            LocalTokenUsageRange::ThisMonth,
            now,
            stats,
            repository_count,
            warnings,
        ),
        custom_window_start: Some(custom_window_start),
        custom_window_end: Some(custom_window_end),
        custom_days,
    }
}

fn build_custom_days(
    now: DateTime<Utc>,
    stats: &[GitCommitStat],
    offset: FixedOffset,
) -> (NaiveDate, NaiveDate, Vec<GitUsageCachedDay>) {
    let window_end = app_time::local_date(now, offset);
    let window_start = window_end - Duration::days(CUSTOM_USAGE_WINDOW_DAYS - 1);
    let start = app_time::local_start_of_day_utc(window_start, offset);
    let end = app_time::local_end_of_day_utc(window_end, offset);
    let mut by_day = HashMap::<String, GitBucketStats>::new();
    let mut repositories_by_day = HashMap::<String, HashMap<String, GitUsageRepository>>::new();

    for stat in stats {
        if stat.timestamp < start || stat.timestamp > end {
            continue;
        }
        let date = bucket_key(BucketGranularity::Day, stat.timestamp, offset);
        add_stat_to_bucket(by_day.entry(date.clone()).or_default(), stat);
        let repository_key = if stat.repository_path.is_empty() {
            "unknown".to_string()
        } else {
            stat.repository_path.clone()
        };
        let repository = repositories_by_day
            .entry(date)
            .or_default()
            .entry(repository_key.clone())
            .or_insert_with(|| GitUsageRepository {
                name: if stat.repository_name.is_empty() {
                    "repository".to_string()
                } else {
                    stat.repository_name.clone()
                },
                path: repository_key,
                added_lines: 0,
                deleted_lines: 0,
                changed_files: 0,
            });
        repository.added_lines = repository.added_lines.saturating_add(stat.added_lines);
        repository.deleted_lines = repository.deleted_lines.saturating_add(stat.deleted_lines);
        repository.changed_files = repository.changed_files.saturating_add(stat.changed_files);
    }

    let mut days = Vec::new();
    let mut current = window_start;
    while current <= window_end {
        let date = app_time::local_day_key(current);
        let day_totals = by_day.remove(&date).unwrap_or_default();
        let mut repositories = repositories_by_day
            .remove(&date)
            .unwrap_or_default()
            .into_values()
            .collect::<Vec<_>>();
        sort_repositories(&mut repositories);
        days.push(GitUsageCachedDay {
            date,
            totals: GitUsageTotals {
                added_lines: day_totals.added_lines,
                deleted_lines: day_totals.deleted_lines,
                changed_files: day_totals.changed_files,
            },
            repositories,
        });
        current += Duration::days(1);
    }

    (window_start, window_end, days)
}

pub fn discover_git_repositories(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut repositories = Vec::new();
    let mut seen = HashSet::new();
    discover_git_repositories_inner(root, &mut repositories, &mut seen, true)?;
    repositories.sort();
    Ok(repositories)
}

fn discover_git_repositories_inner(
    root: &Path,
    repositories: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    strict: bool,
) -> Result<(), String> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if strict => return Err(format!("读取目录失败（{}）: {error}", root.display())),
        Err(_) => return Ok(()),
    };

    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name();
        let name = name.to_string_lossy();

        if name == ".git" && valid_git_marker(&path, &file_type) && !is_linked_worktree(root) {
            let repository = root.to_path_buf();
            if seen.insert(repository.clone()) {
                repositories.push(repository);
            }
            continue;
        }

        if file_type.is_dir() && !should_skip_directory(&path, &name) {
            discover_git_repositories_inner(&path, repositories, seen, false)?;
        }
    }

    Ok(())
}

fn should_skip_directory(path: &Path, name: &str) -> bool {
    if name.ends_with(".app") || name.ends_with(".framework") || name.ends_with(".bundle") {
        return true;
    }

    if name == ".worktrees"
        || (name == "worktrees"
            && path
                .parent()
                .and_then(|parent| parent.file_name())
                .and_then(|parent| parent.to_str())
                == Some(".claude"))
    {
        return true;
    }

    matches!(
        name,
        ".git"
            | ".cache"
            | ".Trash"
            | ".cargo"
            | ".codex"
            | ".docker"
            | ".local"
            | ".npm"
            | ".nvm"
            | ".rustup"
            | "Application Support"
            | "Applications"
            | "Caches"
            | "Library"
            | "node_modules"
            | "target"
            | "dist"
            | "build"
            | ".next"
            | ".nuxt"
    )
}

fn is_linked_worktree(repository: &Path) -> bool {
    let Ok(output) = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(["rev-parse", "--absolute-git-dir", "--git-common-dir"])
        .output()
    else {
        return false;
    };

    if !output.status.success() {
        return false;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();
    let Some(git_dir) = lines.next().map(str::trim).filter(|line| !line.is_empty()) else {
        return false;
    };
    let Some(common_dir) = lines.next().map(str::trim).filter(|line| !line.is_empty()) else {
        return false;
    };

    resolve_git_path(repository, git_dir) != resolve_git_path(repository, common_dir)
}

fn resolve_git_path(repository: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        repository.join(path)
    };
    resolved.canonicalize().unwrap_or(resolved)
}

fn valid_git_marker(path: &Path, file_type: &fs::FileType) -> bool {
    if file_type.is_dir() {
        return true;
    }

    if !file_type.is_file() {
        return false;
    }

    resolve_gitdir_marker(path)
        .map(|resolved| resolved.exists())
        .unwrap_or(false)
}

fn resolve_gitdir_marker(path: &Path) -> Option<PathBuf> {
    let Ok(content) = fs::read_to_string(path) else {
        return None;
    };
    let Some(gitdir) = content.trim().strip_prefix("gitdir:") else {
        return None;
    };
    let gitdir = gitdir.trim();
    if gitdir.is_empty() {
        return None;
    }

    let gitdir_path = Path::new(gitdir);
    Some(if gitdir_path.is_absolute() {
        gitdir_path.to_path_buf()
    } else {
        path.parent()
            .unwrap_or_else(|| Path::new("."))
            .join(gitdir_path)
    })
}

fn is_stale_worktree_warning(warning: &str) -> bool {
    let Some((repository, gitdir)) = warning.split_once(": fatal: not a git repository: ") else {
        return false;
    };
    let git_file = Path::new(repository).join(".git");
    if !git_file.is_file() {
        return false;
    }
    let Some(resolved) = resolve_gitdir_marker(&git_file) else {
        return false;
    };
    if resolved.exists() {
        return false;
    }
    let reported = Path::new(gitdir.trim());
    reported.is_absolute() && reported == resolved
}

fn load_repository_stats(
    repository: &Path,
    since: DateTime<Utc>,
) -> Result<Vec<GitCommitStat>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args([
            "log",
            "--date=iso-strict",
            "--pretty=format:commit%x09%H%x09%cI",
            "--numstat",
            "--branches",
            "--tags",
            &format!("--since={}", since.to_rfc3339()),
        ])
        .arg("--")
        .arg(".")
        .arg(":(exclude)node_modules/**")
        .arg(":(exclude,glob)**/node_modules/**")
        .output()
        .map_err(|error| format!("执行 git log 失败: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "git log 返回非零状态".into()
        } else {
            stderr
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let repository_name = repository
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("repository")
        .to_string();
    let repository_path = repository.to_string_lossy().to_string();
    Ok(parse_git_log_numstat_output(&stdout)
        .into_iter()
        .map(|mut stat| {
            stat.repository_name = repository_name.clone();
            stat.repository_path = repository_path.clone();
            stat
        })
        .collect())
}

fn parse_git_log_numstat_output(output: &str) -> Vec<GitCommitStat> {
    let mut stats = Vec::new();
    let mut current: Option<GitCommitStat> = None;

    for line in output.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }

        if let Some(commit) = line.strip_prefix("commit\t") {
            if let Some(stat) = current.take() {
                stats.push(stat);
            }
            let (commit_hash, timestamp) = parse_commit_header(commit).unwrap_or(("", commit));
            current = DateTime::parse_from_rfc3339(timestamp)
                .ok()
                .map(|timestamp| GitCommitStat {
                    commit_hash: commit_hash.to_string(),
                    timestamp: timestamp.with_timezone(&Utc),
                    repository_name: String::new(),
                    repository_path: String::new(),
                    added_lines: 0,
                    deleted_lines: 0,
                    changed_files: 0,
                });
            continue;
        }

        let Some(stat) = current.as_mut() else {
            continue;
        };
        let Some((added, deleted)) = parse_numstat_line(line) else {
            continue;
        };
        stat.added_lines = stat.added_lines.saturating_add(added);
        stat.deleted_lines = stat.deleted_lines.saturating_add(deleted);
        if added > 0 && deleted > 0 {
            stat.changed_files = stat.changed_files.saturating_add(1);
        }
    }

    if let Some(stat) = current {
        stats.push(stat);
    }

    stats
}

fn parse_commit_header(header: &str) -> Option<(&str, &str)> {
    let (commit_hash, timestamp) = header.split_once('\t')?;
    Some((commit_hash.trim(), timestamp.trim()))
}

fn dedupe_git_stats_by_commit_hash(stats: Vec<GitCommitStat>) -> Vec<GitCommitStat> {
    let mut seen = HashSet::new();
    stats
        .into_iter()
        .filter(|stat| stat.commit_hash.is_empty() || seen.insert(stat.commit_hash.clone()))
        .collect()
}

fn parse_numstat_line(line: &str) -> Option<(u64, u64)> {
    let mut parts = line.splitn(3, '\t');
    let added = parts.next()?.trim();
    let deleted = parts.next()?.trim();
    if added == "-" || deleted == "-" {
        return None;
    }
    Some((added.parse().ok()?, deleted.parse().ok()?))
}

fn aggregate_git_stats(
    range: LocalTokenUsageRange,
    now: DateTime<Utc>,
    stats: Vec<GitCommitStat>,
    repository_count: usize,
    warnings: Vec<String>,
) -> GitUsageReport {
    let offset = app_time::local_offset();
    let granularity = bucket_granularity(range);
    let starts = range_bucket_keys(range, now, offset);
    let start = range_start(range, now, offset);
    aggregate_git_stats_for_window(
        range,
        now,
        start,
        now,
        granularity,
        starts,
        None,
        None,
        stats,
        repository_count,
        warnings,
        offset,
    )
}

fn aggregate_custom_git_stats(
    now: DateTime<Utc>,
    start_date: NaiveDate,
    end_date: NaiveDate,
    stats: Vec<GitCommitStat>,
    repository_count: usize,
    warnings: Vec<String>,
) -> GitUsageReport {
    let offset = app_time::local_offset();
    let start = app_time::local_start_of_day_utc(start_date, offset);
    let inclusive_end = app_time::local_end_of_day_utc(end_date, offset);
    let end = if inclusive_end > now {
        now
    } else {
        inclusive_end
    };
    aggregate_git_stats_for_window(
        LocalTokenUsageRange::Custom,
        now,
        start,
        end,
        BucketGranularity::Day,
        day_bucket_keys(start_date, end_date),
        Some(start_date.format("%Y-%m-%d").to_string()),
        Some(end_date.format("%Y-%m-%d").to_string()),
        stats,
        repository_count,
        warnings,
        offset,
    )
}

fn aggregate_git_stats_for_window(
    range: LocalTokenUsageRange,
    now: DateTime<Utc>,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    granularity: BucketGranularity,
    starts: Vec<String>,
    start_date: Option<String>,
    end_date: Option<String>,
    stats: Vec<GitCommitStat>,
    repository_count: usize,
    warnings: Vec<String>,
    offset: FixedOffset,
) -> GitUsageReport {
    let mut buckets_by_key = starts
        .iter()
        .map(|timestamp| (timestamp.clone(), GitBucketStats::default()))
        .collect::<HashMap<_, _>>();
    let mut totals = GitUsageTotals::default();
    let mut repositories_by_path = HashMap::<String, GitUsageRepository>::new();

    for stat in stats {
        if stat.timestamp < start || stat.timestamp > end {
            continue;
        }
        let key = bucket_key(granularity, stat.timestamp, offset);
        let Some(bucket) = buckets_by_key.get_mut(&key) else {
            continue;
        };
        add_stat_to_bucket(bucket, &stat);
        totals.added_lines = totals.added_lines.saturating_add(stat.added_lines);
        totals.deleted_lines = totals.deleted_lines.saturating_add(stat.deleted_lines);
        totals.changed_files = totals.changed_files.saturating_add(stat.changed_files);

        let repository_key = if stat.repository_path.is_empty() {
            "unknown".to_string()
        } else {
            stat.repository_path.clone()
        };
        let repository = repositories_by_path
            .entry(repository_key.clone())
            .or_insert_with(|| GitUsageRepository {
                name: if stat.repository_name.is_empty() {
                    "repository".to_string()
                } else {
                    stat.repository_name.clone()
                },
                path: repository_key,
                added_lines: 0,
                deleted_lines: 0,
                changed_files: 0,
            });
        repository.added_lines = repository.added_lines.saturating_add(stat.added_lines);
        repository.deleted_lines = repository.deleted_lines.saturating_add(stat.deleted_lines);
        repository.changed_files = repository.changed_files.saturating_add(stat.changed_files);
    }

    let buckets = starts
        .into_iter()
        .map(|date| {
            let stats = buckets_by_key.remove(&date).unwrap_or_default();
            GitUsageBucket {
                date,
                added_lines: stats.added_lines,
                deleted_lines: stats.deleted_lines,
                changed_files: stats.changed_files,
            }
        })
        .collect();
    let mut repositories = repositories_by_path
        .into_values()
        .filter(|repository| {
            repository
                .added_lines
                .saturating_add(repository.deleted_lines)
                .saturating_add(repository.changed_files)
                > 0
        })
        .collect::<Vec<_>>();
    sort_repositories(&mut repositories);

    GitUsageReport {
        range,
        start_date,
        end_date,
        pending: false,
        totals,
        buckets,
        repositories,
        repository_count,
        missing_sources: Vec::new(),
        warnings: filter_stale_worktree_warnings(warnings),
        generated_at: now,
    }
}

fn add_stat_to_bucket(bucket: &mut GitBucketStats, stat: &GitCommitStat) {
    bucket.added_lines = bucket.added_lines.saturating_add(stat.added_lines);
    bucket.deleted_lines = bucket.deleted_lines.saturating_add(stat.deleted_lines);
    bucket.changed_files = bucket.changed_files.saturating_add(stat.changed_files);
}

fn sort_repositories(repositories: &mut [GitUsageRepository]) {
    repositories.sort_by(|a, b| {
        let a_total = a.added_lines.saturating_add(a.deleted_lines);
        let b_total = b.added_lines.saturating_add(b.deleted_lines);
        b_total
            .cmp(&a_total)
            .then_with(|| b.changed_files.cmp(&a.changed_files))
            .then_with(|| a.name.cmp(&b.name))
    });
}

fn filter_stale_worktree_warnings(mut warnings: Vec<String>) -> Vec<String> {
    warnings.retain(|warning| !is_stale_worktree_warning(warning));
    warnings
}

fn earliest_cached_start(now: DateTime<Utc>) -> DateTime<Utc> {
    let offset = app_time::local_offset();
    [
        LocalTokenUsageRange::Today,
        LocalTokenUsageRange::Last3Days,
        LocalTokenUsageRange::ThisWeek,
        LocalTokenUsageRange::ThisMonth,
    ]
    .into_iter()
    .map(|range| range_start(range, now, offset))
    .chain(std::iter::once(app_time::local_start_of_day_utc(
        app_time::local_date(now, offset) - Duration::days(CUSTOM_USAGE_WINDOW_DAYS - 1),
        offset,
    )))
    .min()
    .unwrap_or_else(|| range_start(LocalTokenUsageRange::ThisMonth, now, offset))
}

fn range_start(range: LocalTokenUsageRange, now: DateTime<Utc>, offset: FixedOffset) -> DateTime<Utc> {
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
    app_time::local_start_of_day_utc(start_date, offset)
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

fn range_bucket_keys(range: LocalTokenUsageRange, now: DateTime<Utc>, offset: FixedOffset) -> Vec<String> {
    let today = app_time::local_date(now, offset);
    match bucket_granularity(range) {
        BucketGranularity::Day => {
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
            let end_date = match range {
                LocalTokenUsageRange::ThisWeek => start_date + Duration::days(6),
                LocalTokenUsageRange::ThisMonth => app_time::month_end_date(today),
                _ => today,
            };
            day_bucket_keys(start_date, end_date)
        }
        BucketGranularity::Hour => (0..24)
            .map(|hour| app_time::local_hour_bucket_key(today, hour, offset))
            .collect(),
        BucketGranularity::ThreeHours => {
            let start_date = today - Duration::days(2);
            let end_local = now.with_timezone(&offset);
            let end_date = end_local.date_naive();
            let last_hour = end_local.hour() - (end_local.hour() % 3);
            let mut keys = Vec::new();
            let mut current = start_date;
            while current <= end_date {
                let max_hour = if current == end_date { last_hour } else { 21 };
                for hour in (0..=max_hour).step_by(3usize) {
                    keys.push(app_time::local_hour_bucket_key(current, hour, offset));
                }
                current += Duration::days(1);
            }
            keys
        }
    }
}

fn day_bucket_keys(start_date: NaiveDate, end_date: NaiveDate) -> Vec<String> {
    let mut current = start_date;
    let mut starts = Vec::new();
    while current <= end_date {
        starts.push(app_time::local_day_key(current));
        current = current + Duration::days(1);
    }
    starts
}

fn bucket_key(granularity: BucketGranularity, timestamp: DateTime<Utc>, offset: FixedOffset) -> String {
    match granularity {
        BucketGranularity::Day => app_time::local_bucket_key(timestamp, None, offset),
        BucketGranularity::Hour => app_time::local_bucket_key(timestamp, Some(1), offset),
        BucketGranularity::ThreeHours => app_time::local_bucket_key(timestamp, Some(3), offset),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::LocalTokenUsageRange;
    use chrono::{Local, Offset, TimeZone, Utc};
    use std::{fs, path::PathBuf, process::Command};

    fn temp_dir(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("ai-usage-git-usage-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn git_stat(timestamp: &str, added: u64, deleted: u64, changed_files: u64) -> GitCommitStat {
        git_stat_with_hash(
            &format!("hash-{timestamp}-{added}-{deleted}-{changed_files}"),
            timestamp,
            added,
            deleted,
            changed_files,
            "ai-usage",
            "/tmp/ai-usage",
        )
    }

    fn git_stat_with_hash(
        commit_hash: &str,
        timestamp: &str,
        added: u64,
        deleted: u64,
        changed_files: u64,
        repository_name: &str,
        repository_path: &str,
    ) -> GitCommitStat {
        GitCommitStat {
            commit_hash: commit_hash.to_string(),
            timestamp: chrono::DateTime::parse_from_rfc3339(timestamp)
                .unwrap()
                .with_timezone(&Utc),
            repository_name: repository_name.to_string(),
            repository_path: repository_path.to_string(),
            added_lines: added,
            deleted_lines: deleted,
            changed_files,
        }
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

    fn local_bucket_key_string(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        offset: chrono::FixedOffset,
    ) -> String {
        offset
            .with_ymd_and_hms(year, month, day, hour, 0, 0)
            .unwrap()
            .format("%Y-%m-%dT%H:00:00%:z")
            .to_string()
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

    #[test]
    fn discover_git_repositories_excludes_worktree_directories_and_linked_worktrees() {
        let root = temp_dir("discover");
        let regular_repo = root.join("project").join("repo-a");
        fs::create_dir_all(regular_repo.join(".git")).unwrap();

        let claude_worktree = root.join(".claude").join("worktrees").join("repo-b");
        fs::create_dir_all(claude_worktree.join(".git")).unwrap();

        let dot_worktree = root.join(".worktrees").join("repo-c");
        fs::create_dir_all(dot_worktree.join(".git")).unwrap();

        let main_repo = root.join("main-repo");
        fs::create_dir_all(&main_repo).unwrap();
        run_git(&main_repo, &["init"]);
        run_git(&main_repo, &["config", "user.email", "test@example.com"]);
        run_git(&main_repo, &["config", "user.name", "Test User"]);
        fs::write(main_repo.join("README.md"), "hello\n").unwrap();
        run_git(&main_repo, &["add", "README.md"]);
        run_git(&main_repo, &["commit", "-m", "init"]);
        let linked_worktree = root.join("linked-worktree");
        let linked_worktree_arg = linked_worktree.to_string_lossy().to_string();
        run_git(
            &main_repo,
            &["worktree", "add", &linked_worktree_arg, "-b", "feature"],
        );

        let ignored_dependency = root
            .join("project")
            .join("repo-a")
            .join("node_modules")
            .join("dep");
        fs::create_dir_all(ignored_dependency.join(".git")).unwrap();

        let repos = discover_git_repositories(&root).unwrap();

        assert_eq!(repos, vec![main_repo, regular_repo]);
    }

    #[test]
    fn load_repository_stats_excludes_node_modules_from_committed_history() {
        let root = temp_dir("exclude-node-modules-history");
        let repository = root.join("repo");
        fs::create_dir_all(repository.join("src")).unwrap();
        fs::create_dir_all(repository.join("node_modules").join("dep")).unwrap();
        run_git(&repository, &["init"]);
        run_git(&repository, &["config", "user.email", "test@example.com"]);
        run_git(&repository, &["config", "user.name", "Test User"]);
        fs::write(repository.join("src").join("app.ts"), "one\ntwo\nthree\n").unwrap();
        fs::write(
            repository.join("node_modules").join("dep").join("index.js"),
            "ignored\n".repeat(100),
        )
        .unwrap();
        run_git(&repository, &["add", "."]);
        run_git(&repository, &["commit", "-m", "init"]);

        let stats = load_repository_stats(
            &repository,
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        )
        .unwrap();

        assert_eq!(stats.len(), 1);
        assert!(!stats[0].commit_hash.is_empty());
        assert_eq!(stats[0].added_lines, 3);
        assert_eq!(stats[0].deleted_lines, 0);
    }

    #[test]
    fn discover_git_repositories_skips_stale_worktree_gitdir_files() {
        let root = temp_dir("stale-worktree");
        let regular_repo = root.join("repo-a");
        fs::create_dir_all(regular_repo.join(".git")).unwrap();

        let stale_worktree = root.join("backup").join(".worktrees").join("old-branch");
        fs::create_dir_all(&stale_worktree).unwrap();
        fs::write(
            stale_worktree.join(".git"),
            format!(
                "gitdir: {}",
                root.join("missing").join("old-branch").display()
            ),
        )
        .unwrap();

        let repos = discover_git_repositories(&root).unwrap();

        assert_eq!(repos, vec![regular_repo]);
    }

    #[test]
    fn build_cache_ignores_stale_worktrees_without_warnings() {
        let root = temp_dir("stale-worktree-cache");
        let stale_worktree = root.join("backup").join(".worktrees").join("glm-v1");
        fs::create_dir_all(&stale_worktree).unwrap();
        fs::write(
            stale_worktree.join(".git"),
            format!(
                "gitdir: {}",
                root.join("missing")
                    .join(".git")
                    .join("worktrees")
                    .join("glm-v1")
                    .display()
            ),
        )
        .unwrap();

        let cache = build_cache(root).unwrap();
        let report = cache.report(LocalTokenUsageRange::ThisMonth);

        assert_eq!(report.repository_count, 0);
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn cached_report_filters_previous_stale_worktree_warnings() {
        let root = temp_dir("stale-worktree-existing-cache");
        let stale_worktree = root.join("backup").join(".worktrees").join("glm-v1");
        let missing_gitdir = root
            .join("missing")
            .join(".git")
            .join("worktrees")
            .join("glm-v1");
        fs::create_dir_all(&stale_worktree).unwrap();
        fs::write(
            stale_worktree.join(".git"),
            format!("gitdir: {}", missing_gitdir.display()),
        )
        .unwrap();
        let stale_warning = format!(
            "{}: fatal: not a git repository: {}",
            stale_worktree.display(),
            missing_gitdir.display()
        );

        let cache = GitUsageCache {
            root_path: root.to_string_lossy().to_string(),
            generated_at: Utc::now(),
            today: empty_report(LocalTokenUsageRange::Today, None),
            last3_days: empty_report(LocalTokenUsageRange::Last3Days, None),
            this_week: empty_report(LocalTokenUsageRange::ThisWeek, None),
            this_month: empty_report(LocalTokenUsageRange::ThisMonth, Some(stale_warning)),
            custom_window_start: None,
            custom_window_end: None,
            custom_days: vec![],
        };

        let report = cache.report(LocalTokenUsageRange::ThisMonth);

        assert!(report.warnings.is_empty());
    }

    #[test]
    fn discover_git_repositories_skips_noisy_application_and_library_roots() {
        let root = temp_dir("skip-noisy");
        let regular_repo = root.join("project").join("repo-a");
        fs::create_dir_all(regular_repo.join(".git")).unwrap();

        let library_repo = root
            .join("Library")
            .join("Application Support")
            .join("app-cache")
            .join("repo-b");
        fs::create_dir_all(library_repo.join(".git")).unwrap();

        let application_repo = root
            .join("Applications")
            .join("Example.app")
            .join("Contents")
            .join("repo-c");
        fs::create_dir_all(application_repo.join(".git")).unwrap();

        let repos = discover_git_repositories(&root).unwrap();

        assert_eq!(repos, vec![regular_repo]);
    }

    #[cfg(unix)]
    #[test]
    fn discover_git_repositories_does_not_follow_directory_symlinks() {
        use std::os::unix::fs as unix_fs;

        let root = temp_dir("skip-symlink-root");
        let external = temp_dir("skip-symlink-external");
        let regular_repo = root.join("repo-a");
        let external_repo = external.join("repo-b");
        fs::create_dir_all(regular_repo.join(".git")).unwrap();
        fs::create_dir_all(external_repo.join(".git")).unwrap();
        unix_fs::symlink(&external, root.join("linked-external")).unwrap();

        let repos = discover_git_repositories(&root).unwrap();

        assert_eq!(repos, vec![regular_repo]);
    }

    #[test]
    fn parse_git_log_numstat_output_sums_lines_changed_files_and_ignores_binary_entries() {
        let output = "\
commit\t1111111111111111111111111111111111111111\t2026-04-27T08:00:00+00:00
12\t3\tsrc/app.ts
8\t0\tsrc/new.ts
0\t5\tsrc/delete.ts
-\t-\tassets/logo.png

commit\t2222222222222222222222222222222222222222\t2026-04-26T20:30:00+00:00
2\t2\tsrc/lib.rs
";

        let stats = parse_git_log_numstat_output(output);

        assert_eq!(stats.len(), 2);
        assert_eq!(
            stats[0].commit_hash,
            "1111111111111111111111111111111111111111"
        );
        assert_eq!(stats[0].added_lines, 20);
        assert_eq!(stats[0].deleted_lines, 8);
        assert_eq!(stats[0].changed_files, 1);
        assert_eq!(
            stats[1].commit_hash,
            "2222222222222222222222222222222222222222"
        );
        assert_eq!(stats[1].added_lines, 2);
        assert_eq!(stats[1].deleted_lines, 2);
        assert_eq!(stats[1].changed_files, 1);
    }

    #[test]
    fn aggregate_git_stats_uses_requested_range_and_bucket_granularity() {
        let offset = local_offset();
        let now = local_time_utc(2026, 4, 27, 15, 30, 0);
        let stats = vec![
            git_stat(&local_timestamp_rfc3339(2026, 4, 27, 9, 10, 0), 10, 2, 1),
            git_stat(&local_timestamp_rfc3339(2026, 4, 27, 14, 10, 0), 20, 5, 2),
            git_stat(&local_timestamp_rfc3339(2026, 4, 25, 3, 30, 0), 7, 1, 1),
            git_stat(&local_timestamp_rfc3339(2026, 3, 31, 23, 30, 0), 999, 999, 999),
        ];

        let today = aggregate_git_stats(LocalTokenUsageRange::Today, now, stats.clone(), 3, vec![]);
        assert_eq!(today.totals.added_lines, 30);
        assert_eq!(today.totals.deleted_lines, 7);
        assert_eq!(today.totals.changed_files, 3);
        assert_eq!(
            today.buckets.first().map(|bucket| bucket.date.as_str()),
            Some(local_bucket_key_string(2026, 4, 27, 0, offset).as_str())
        );
        assert_eq!(
            today.buckets.last().map(|bucket| bucket.date.as_str()),
            Some(local_bucket_key_string(2026, 4, 27, 23, offset).as_str())
        );
        assert_eq!(today.buckets.len(), 24);

        let last3_days = aggregate_git_stats(
            LocalTokenUsageRange::Last3Days,
            now,
            stats.clone(),
            3,
            vec![],
        );
        assert_eq!(
            last3_days
                .buckets
                .first()
                .map(|bucket| bucket.date.as_str()),
            Some(local_bucket_key_string(2026, 4, 25, 0, offset).as_str())
        );
        assert_eq!(
            last3_days.buckets.last().map(|bucket| bucket.date.as_str()),
            Some(local_bucket_key_string(2026, 4, 27, 15, offset).as_str())
        );
        assert_eq!(last3_days.buckets.len(), 22);
        assert_eq!(last3_days.totals.added_lines, 37);

        let this_week = aggregate_git_stats(
            LocalTokenUsageRange::ThisWeek,
            now,
            stats.clone(),
            3,
            vec![],
        );
        assert_eq!(
            this_week
                .buckets
                .iter()
                .map(|bucket| bucket.date.as_str())
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

        let this_month =
            aggregate_git_stats(LocalTokenUsageRange::ThisMonth, now, stats, 3, vec![]);
        assert_eq!(
            this_month
                .buckets
                .first()
                .map(|bucket| bucket.date.as_str()),
            Some("2026-04-01")
        );
        assert_eq!(
            this_month.buckets.last().map(|bucket| bucket.date.as_str()),
            Some("2026-04-30")
        );
        assert_eq!(this_month.buckets.len(), 30);
        assert_eq!(this_month.totals.added_lines, 37);
        assert_eq!(this_month.repository_count, 3);
        assert_eq!(this_month.repositories.len(), 1);
        assert_eq!(this_month.repositories[0].name, "ai-usage");
        assert_eq!(this_month.repositories[0].added_lines, 37);
    }

    #[test]
    fn aggregate_custom_git_stats_returns_daily_buckets_for_inclusive_dates() {
        let now = local_time_utc(2026, 4, 27, 15, 30, 0);
        let start = NaiveDate::from_ymd_opt(2026, 4, 20).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 4, 20).unwrap();
        let stats = vec![
            git_stat(&local_timestamp_rfc3339(2026, 4, 19, 23, 30, 0), 999, 999, 999),
            git_stat(&local_timestamp_rfc3339(2026, 4, 20, 0, 30, 0), 10, 2, 1),
            git_stat(&local_timestamp_rfc3339(2026, 4, 21, 0, 30, 0), 20, 5, 2),
            git_stat(&local_timestamp_rfc3339(2026, 4, 21, 1, 30, 0), 999, 999, 999),
        ];

        let report = aggregate_custom_git_stats(now, start, end, stats, 3, vec![]);

        assert_eq!(report.range, LocalTokenUsageRange::Custom);
        assert_eq!(report.start_date.as_deref(), Some("2026-04-20"));
        assert_eq!(report.end_date.as_deref(), Some("2026-04-20"));
        assert_eq!(report.totals.added_lines, 10);
        assert_eq!(report.totals.deleted_lines, 2);
        assert_eq!(report.totals.changed_files, 1);
        assert_eq!(
            report
                .buckets
                .iter()
                .map(|bucket| bucket.date.as_str())
                .collect::<Vec<_>>(),
            vec!["2026-04-20"]
        );
        assert_eq!(report.repositories.len(), 1);
        assert_eq!(report.repositories[0].added_lines, 10);
    }

    #[test]
    fn cache_from_git_stats_precomputes_each_range() {
        let now = Utc.with_ymd_and_hms(2026, 4, 27, 7, 30, 0).unwrap();
        let stats = vec![
            git_stat("2026-04-27T06:10:00Z", 20, 5, 1),
            git_stat("2026-04-26T06:10:00Z", 30, 7, 2),
            git_stat("2026-04-01T06:10:00Z", 40, 9, 3),
        ];

        let cache = build_cache_from_stats(now, "/tmp/workspace".into(), stats, 4, vec![]);

        assert_eq!(cache.root_path, "/tmp/workspace");
        assert_eq!(
            cache.report(LocalTokenUsageRange::Today).totals.added_lines,
            20
        );
        assert_eq!(
            cache
                .report(LocalTokenUsageRange::Last3Days)
                .totals
                .added_lines,
            50
        );
        assert_eq!(
            cache
                .report(LocalTokenUsageRange::ThisMonth)
                .totals
                .added_lines,
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
        let now = Utc.with_ymd_and_hms(2026, 4, 28, 7, 30, 0).unwrap();
        let cache = GitUsageCache {
            root_path: "/tmp/workspace".into(),
            generated_at: now,
            today: empty_report(LocalTokenUsageRange::Today, None),
            last3_days: empty_report(LocalTokenUsageRange::Last3Days, None),
            this_week: empty_report(LocalTokenUsageRange::ThisWeek, None),
            this_month: empty_report(LocalTokenUsageRange::ThisMonth, None),
            custom_window_start: Some(NaiveDate::from_ymd_opt(2026, 1, 29).unwrap()),
            custom_window_end: Some(NaiveDate::from_ymd_opt(2026, 4, 28).unwrap()),
            custom_days: vec![
                GitUsageCachedDay {
                    date: "2026-04-20".into(),
                    totals: GitUsageTotals {
                        added_lines: 10,
                        deleted_lines: 3,
                        changed_files: 1,
                    },
                    repositories: vec![GitUsageRepository {
                        name: "repo-a".into(),
                        path: "/tmp/repo-a".into(),
                        added_lines: 10,
                        deleted_lines: 3,
                        changed_files: 1,
                    }],
                },
                GitUsageCachedDay {
                    date: "2026-04-21".into(),
                    totals: GitUsageTotals::default(),
                    repositories: vec![],
                },
                GitUsageCachedDay {
                    date: "2026-04-22".into(),
                    totals: GitUsageTotals {
                        added_lines: 20,
                        deleted_lines: 5,
                        changed_files: 2,
                    },
                    repositories: vec![GitUsageRepository {
                        name: "repo-b".into(),
                        path: "/tmp/repo-b".into(),
                        added_lines: 20,
                        deleted_lines: 5,
                        changed_files: 2,
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
        assert_eq!(report.totals.added_lines, 30);
        assert_eq!(report.buckets.len(), 3);
        assert_eq!(report.repositories.len(), 2);
        assert_eq!(report.repositories[0].name, "repo-b");
    }

    #[test]
    fn cache_from_git_stats_dedupes_commits_across_repositories_by_hash() {
        let now = Utc.with_ymd_and_hms(2026, 4, 27, 7, 30, 0).unwrap();
        let stats = vec![
            git_stat_with_hash(
                "duplicate-commit",
                "2026-04-27T06:10:00Z",
                20,
                5,
                1,
                "repo-a",
                "/tmp/repo-a",
            ),
            git_stat_with_hash(
                "duplicate-commit",
                "2026-04-27T06:10:00Z",
                20,
                5,
                1,
                "repo-b",
                "/tmp/repo-b",
            ),
            git_stat_with_hash(
                "unique-commit",
                "2026-04-27T06:20:00Z",
                7,
                2,
                1,
                "repo-b",
                "/tmp/repo-b",
            ),
        ];

        let report = build_cache_from_stats(now, "/tmp/workspace".into(), stats, 2, vec![])
            .report(LocalTokenUsageRange::Today);

        assert_eq!(report.totals.added_lines, 27);
        assert_eq!(report.totals.deleted_lines, 7);
        assert_eq!(report.totals.changed_files, 2);
        assert_eq!(report.repositories.len(), 2);
        assert_eq!(report.repositories[0].name, "repo-a");
        assert_eq!(report.repositories[0].added_lines, 20);
        assert_eq!(report.repositories[1].name, "repo-b");
        assert_eq!(report.repositories[1].added_lines, 7);
    }
}
