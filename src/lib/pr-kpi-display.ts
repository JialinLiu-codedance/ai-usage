import type { PrKpiMetric, PrKpiReport } from "./types";

export interface PrKpiRadarAxis extends PrKpiMetric {
  displayValue: string;
  angle: number;
  labelX: number;
  labelY: number;
  pointX: number;
  pointY: number;
}

export interface PrKpiRadarModel {
  axes: PrKpiRadarAxis[];
  missingAxes: PrKpiRadarAxis[];
  center: number;
  radius: number;
  polygonPoints: string;
  gridPolygons: string[];
  overallScoreLabel: string | null;
}

export const prKpiMetricDescriptions: Record<PrKpiMetric["key"], string> = {
  cycle_time_ai: "PR 创建到合入的平均时间",
  merged_ai_prs_per_week: "每周合入的 AI-assisted PR 数量",
  review_comments_per_pr: "每个 PR 平均 review comments 数",
  test_added_ratio: "新增测试代码行 / 总新增代码行",
  "7d_rework_rate": "合入后 7 天内被删除或重写的代码比例",
  "7d_retention_rate": "合入后 7 天仍然保留的代码比例",
};

export function buildPrKpiRadarModel(report: PrKpiReport, size = 280): PrKpiRadarModel {
  const center = size / 2;
  const radius = size * 0.26;
  const labelRadius = radius + 28;
  const axes = report.metrics.map((metric, index) => {
    const angle = (-Math.PI / 2) + (Math.PI * 2 * index) / report.metrics.length;
    const normalizedScore = metric.score == null ? 0 : metric.score / 100;
    return {
      ...metric,
      displayValue: metric.display_value,
      angle,
      labelX: center + Math.cos(angle) * labelRadius,
      labelY: center + Math.sin(angle) * labelRadius,
      pointX: center + Math.cos(angle) * radius * normalizedScore,
      pointY: center + Math.sin(angle) * radius * normalizedScore,
    };
  });

  return {
    axes,
    missingAxes: axes.filter((axis) => axis.is_missing),
    center,
    radius,
    polygonPoints: axes.map((axis) => `${axis.pointX},${axis.pointY}`).join(" "),
    gridPolygons: [1, 0.75, 0.5, 0.25].map((ratio) =>
      axes
        .map((axis) => {
          const x = center + Math.cos(axis.angle) * radius * ratio;
          const y = center + Math.sin(axis.angle) * radius * ratio;
          return `${x},${y}`;
        })
        .join(" "),
    ),
    overallScoreLabel:
      report.overall_score == null ? null : String(Math.round(report.overall_score)),
  };
}

export function formatPrKpiOverviewValue(value: number): string {
  const absolute = Math.abs(value);
  if (absolute >= 1_000_000) {
    return `${trimScaled(value / 1_000_000, 2)}M`;
  }
  if (absolute >= 100_000) {
    return `${trimScaled(value / 1_000, 1)}K`;
  }
  return value.toLocaleString("en-US");
}

export function formatPrKpiOutputRatio(value: number | null): string {
  if (value == null || Number.isNaN(value)) {
    return "N/A";
  }
  return trimTrailingZero(value);
}

export function prKpiOutputRatioTone(value: number | null): "default" | "green" | "red" {
  if (value == null || Number.isNaN(value) || value === 0) {
    return "default";
  }
  return value > 0 ? "green" : "red";
}

export function prKpiAxisAnchor(x: number, center: number): "start" | "middle" | "end" {
  if (Math.abs(x - center) < 10) {
    return "middle";
  }
  return x > center ? "start" : "end";
}

function trimTrailingZero(value: number): string {
  return value.toFixed(1).replace(/\.0$/, "");
}

function trimScaled(value: number, decimals: number): string {
  return value.toFixed(decimals).replace(/\.?0+$/, "");
}
