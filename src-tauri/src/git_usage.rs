use crate::models::{GitUsageBucket, GitUsageReport, GitUsageTotals, LocalTokenUsageRange};
use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone)]
struct GitCommitStat {
    timestamp: DateTime<Utc>,
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
    pub generated_at: DateTime<Utc>,
    pub today: GitUsageReport,
    pub last3_days: GitUsageReport,
    pub this_week: GitUsageReport,
    pub this_month: GitUsageReport,
}

impl GitUsageCache {
    pub fn report(&self, range: LocalTokenUsageRange) -> GitUsageReport {
        match range {
            LocalTokenUsageRange::Today => self.today.clone(),
            LocalTokenUsageRange::Last3Days => self.last3_days.clone(),
            LocalTokenUsageRange::ThisWeek => self.this_week.clone(),
            LocalTokenUsageRange::ThisMonth => self.this_month.clone(),
        }
    }
}

pub fn build_cache() -> Result<GitUsageCache, String> {
    let now = Utc::now();
    let home = home_dir();
    let repositories = discover_git_repositories(&home)?;
    let since = earliest_range_start(now);
    let mut stats = Vec::new();
    let mut warnings = Vec::new();

    for repository in &repositories {
        match load_repository_stats(repository, since) {
            Ok(mut next) => stats.append(&mut next),
            Err(error) => warnings.push(format!("{}: {error}", repository.display())),
        }
    }

    Ok(build_cache_from_stats(
        now,
        stats,
        repositories.len(),
        warnings,
    ))
}

pub fn empty_report(range: LocalTokenUsageRange, warning: Option<String>) -> GitUsageReport {
    aggregate_git_stats(
        range,
        Utc::now(),
        Vec::new(),
        0,
        warning.into_iter().collect(),
    )
}

fn build_cache_from_stats(
    now: DateTime<Utc>,
    stats: Vec<GitCommitStat>,
    repository_count: usize,
    warnings: Vec<String>,
) -> GitUsageCache {
    GitUsageCache {
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
    }
}

fn discover_git_repositories(root: &Path) -> Result<Vec<PathBuf>, String> {
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
        let name = entry.file_name();
        let name = name.to_string_lossy();

        if name == ".git" && (path.is_dir() || path.is_file()) {
            let repository = root.to_path_buf();
            if seen.insert(repository.clone()) {
                repositories.push(repository);
            }
            continue;
        }

        if path.is_dir() && !should_skip_directory(&name) {
            discover_git_repositories_inner(&path, repositories, seen, false)?;
        }
    }

    Ok(())
}

fn should_skip_directory(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".cache"
            | ".Trash"
            | "node_modules"
            | "target"
            | "dist"
            | "build"
            | ".next"
            | ".nuxt"
    )
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
            "--pretty=format:commit%x09%cI",
            "--numstat",
            "--branches",
            "--tags",
            &format!("--since={}", since.to_rfc3339()),
        ])
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
    Ok(parse_git_log_numstat_output(&stdout))
}

fn parse_git_log_numstat_output(output: &str) -> Vec<GitCommitStat> {
    let mut stats = Vec::new();
    let mut current: Option<GitCommitStat> = None;

    for line in output.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }

        if let Some(timestamp) = line.strip_prefix("commit\t") {
            if let Some(stat) = current.take() {
                stats.push(stat);
            }
            current = DateTime::parse_from_rfc3339(timestamp)
                .ok()
                .map(|timestamp| GitCommitStat {
                    timestamp: timestamp.with_timezone(&Utc),
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
    let granularity = bucket_granularity(range);
    let starts = range_bucket_starts(range, now);
    let start = range_start(range, now);
    let mut buckets_by_key = starts
        .iter()
        .map(|timestamp| {
            (
                bucket_key(granularity, *timestamp),
                GitBucketStats::default(),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut totals = GitUsageTotals::default();

    for stat in stats {
        if stat.timestamp < start || stat.timestamp > now {
            continue;
        }
        let key = bucket_key(
            granularity,
            bucket_start_for_event(granularity, stat.timestamp),
        );
        let Some(bucket) = buckets_by_key.get_mut(&key) else {
            continue;
        };
        add_stat_to_bucket(bucket, &stat);
        totals.added_lines = totals.added_lines.saturating_add(stat.added_lines);
        totals.deleted_lines = totals.deleted_lines.saturating_add(stat.deleted_lines);
        totals.changed_files = totals.changed_files.saturating_add(stat.changed_files);
    }

    let buckets = starts
        .into_iter()
        .map(|timestamp| {
            let date = bucket_key(granularity, timestamp);
            let stats = buckets_by_key.remove(&date).unwrap_or_default();
            GitUsageBucket {
                date,
                added_lines: stats.added_lines,
                deleted_lines: stats.deleted_lines,
                changed_files: stats.changed_files,
            }
        })
        .collect();

    GitUsageReport {
        range,
        totals,
        buckets,
        repository_count,
        missing_sources: Vec::new(),
        warnings,
        generated_at: now,
    }
}

fn add_stat_to_bucket(bucket: &mut GitBucketStats, stat: &GitCommitStat) {
    bucket.added_lines = bucket.added_lines.saturating_add(stat.added_lines);
    bucket.deleted_lines = bucket.deleted_lines.saturating_add(stat.deleted_lines);
    bucket.changed_files = bucket.changed_files.saturating_add(stat.changed_files);
}

fn earliest_range_start(now: DateTime<Utc>) -> DateTime<Utc> {
    [
        LocalTokenUsageRange::Today,
        LocalTokenUsageRange::Last3Days,
        LocalTokenUsageRange::ThisWeek,
        LocalTokenUsageRange::ThisMonth,
    ]
    .into_iter()
    .map(|range| range_start(range, now))
    .min()
    .unwrap_or_else(|| range_start(LocalTokenUsageRange::ThisMonth, now))
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
    };
    Utc.from_utc_datetime(&start_date.and_hms_opt(0, 0, 0).unwrap())
}

fn bucket_granularity(range: LocalTokenUsageRange) -> BucketGranularity {
    match range {
        LocalTokenUsageRange::Today => BucketGranularity::Hour,
        LocalTokenUsageRange::Last3Days => BucketGranularity::ThreeHours,
        LocalTokenUsageRange::ThisWeek | LocalTokenUsageRange::ThisMonth => BucketGranularity::Day,
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
    let mut starts = Vec::new();
    while current <= now {
        starts.push(current);
        current = current + step;
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

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::LocalTokenUsageRange;
    use chrono::{TimeZone, Utc};
    use std::{fs, path::PathBuf};

    fn temp_dir(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("ai-usage-git-usage-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn git_stat(timestamp: &str, added: u64, deleted: u64, changed_files: u64) -> GitCommitStat {
        GitCommitStat {
            timestamp: chrono::DateTime::parse_from_rfc3339(timestamp)
                .unwrap()
                .with_timezone(&Utc),
            added_lines: added,
            deleted_lines: deleted,
            changed_files,
        }
    }

    #[test]
    fn discover_git_repositories_finds_git_directories_files_and_hidden_worktrees() {
        let root = temp_dir("discover");
        let regular_repo = root.join("project").join("repo-a");
        fs::create_dir_all(regular_repo.join(".git")).unwrap();

        let worktree_repo = root.join("worktrees").join("repo-b");
        fs::create_dir_all(&worktree_repo).unwrap();
        fs::write(
            worktree_repo.join(".git"),
            "gitdir: /tmp/repo-b/.git/worktrees/repo-b",
        )
        .unwrap();

        let hidden_worktree = root.join(".hidden-worktree");
        fs::create_dir_all(&hidden_worktree).unwrap();
        fs::write(
            hidden_worktree.join(".git"),
            "gitdir: /tmp/hidden/.git/worktrees/hidden",
        )
        .unwrap();

        let ignored_dependency = root
            .join("project")
            .join("repo-a")
            .join("node_modules")
            .join("dep");
        fs::create_dir_all(ignored_dependency.join(".git")).unwrap();

        let repos = discover_git_repositories(&root).unwrap();

        assert_eq!(repos, vec![hidden_worktree, regular_repo, worktree_repo]);
    }

    #[test]
    fn parse_git_log_numstat_output_sums_lines_changed_files_and_ignores_binary_entries() {
        let output = "\
commit\t2026-04-27T08:00:00+00:00
12\t3\tsrc/app.ts
8\t0\tsrc/new.ts
0\t5\tsrc/delete.ts
-\t-\tassets/logo.png

commit\t2026-04-26T20:30:00+00:00
2\t2\tsrc/lib.rs
";

        let stats = parse_git_log_numstat_output(output);

        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].added_lines, 20);
        assert_eq!(stats[0].deleted_lines, 8);
        assert_eq!(stats[0].changed_files, 1);
        assert_eq!(stats[1].added_lines, 2);
        assert_eq!(stats[1].deleted_lines, 2);
        assert_eq!(stats[1].changed_files, 1);
    }

    #[test]
    fn aggregate_git_stats_uses_requested_range_and_bucket_granularity() {
        let now = Utc.with_ymd_and_hms(2026, 4, 27, 7, 30, 0).unwrap();
        let stats = vec![
            git_stat("2026-04-27T01:10:00Z", 10, 2, 1),
            git_stat("2026-04-27T06:10:00Z", 20, 5, 2),
            git_stat("2026-04-25T03:30:00Z", 7, 1, 1),
            git_stat("2026-03-31T23:30:00Z", 999, 999, 999),
        ];

        let today = aggregate_git_stats(LocalTokenUsageRange::Today, now, stats.clone(), 3, vec![]);
        assert_eq!(today.totals.added_lines, 30);
        assert_eq!(today.totals.deleted_lines, 7);
        assert_eq!(today.totals.changed_files, 3);
        assert_eq!(
            today.buckets.first().map(|bucket| bucket.date.as_str()),
            Some("2026-04-27T00:00:00Z")
        );
        assert_eq!(
            today.buckets.last().map(|bucket| bucket.date.as_str()),
            Some("2026-04-27T07:00:00Z")
        );

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
            Some("2026-04-25T00:00:00Z")
        );
        assert_eq!(
            last3_days.buckets.last().map(|bucket| bucket.date.as_str()),
            Some("2026-04-27T06:00:00Z")
        );
        assert_eq!(last3_days.buckets.len(), 19);
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
            vec!["2026-04-27"]
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
            Some("2026-04-27")
        );
        assert_eq!(this_month.totals.added_lines, 37);
        assert_eq!(this_month.repository_count, 3);
    }

    #[test]
    fn cache_from_git_stats_precomputes_each_range() {
        let now = Utc.with_ymd_and_hms(2026, 4, 27, 7, 30, 0).unwrap();
        let stats = vec![
            git_stat("2026-04-27T06:10:00Z", 20, 5, 1),
            git_stat("2026-04-26T06:10:00Z", 30, 7, 2),
            git_stat("2026-04-01T06:10:00Z", 40, 9, 3),
        ];

        let cache = build_cache_from_stats(now, stats, 4, vec![]);

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
    }
}
