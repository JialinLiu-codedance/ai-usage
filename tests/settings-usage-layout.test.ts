import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

test("Git usage panel does not include the duplicate standalone title", async () => {
  const appSource = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");

  assert.equal(appSource.includes("Git 提交代码行数统计"), false);
  assert.equal(appSource.includes("提交概览"), true);
});

test("Token usage trend title matches the split statistics design", async () => {
  const appSource = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");

  assert.equal(appSource.includes("每日 Token 用量趋势"), false);
  assert.equal(appSource.includes("<h2>Token 用量趋势</h2>"), true);
});

test("Token model ranking renders all models with token component details", async () => {
  const appSource = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");
  const tokenPanelStart = appSource.indexOf("function TokenUsagePanel");
  const tokenSummaryStart = appSource.indexOf("function TokenUsageSummary");
  const tokenPanelSource = appSource.slice(tokenPanelStart, tokenSummaryStart);

  assert.equal(tokenPanelSource.includes("modelUsageRows(report, 3)"), false);
  assert.match(tokenPanelSource, /const modelRows = report \? modelUsageRows\(report\) : \[\]/);
  assert.match(tokenPanelSource, />\s*输入\s*</);
  assert.match(tokenPanelSource, />\s*输出\s*</);
  assert.match(tokenPanelSource, />\s*缓存命中\s*</);
  assert.match(tokenPanelSource, />\s*存储缓存\s*</);
});

test("Token trend legend does not limit the chart to top three models", async () => {
  const appSource = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");
  const tokenPanelStart = appSource.indexOf("function TokenUsagePanel");
  const tokenSummaryStart = appSource.indexOf("function TokenUsageSummary");
  const tokenPanelSource = appSource.slice(tokenPanelStart, tokenSummaryStart);

  assert.equal(tokenPanelSource.includes("buildTokenUsageChartLegend(report, 3)"), false);
  assert.match(tokenPanelSource, /const chartLegend = report \? buildTokenUsageChartLegend\(report\) : \[\]/);
});

test("settings panel caps resize height and scrolls overflowing content", async () => {
  const [appSource, styleSource] = await Promise.all([
    readFile(new URL("../src/App.tsx", import.meta.url), "utf8"),
    readFile(new URL("../src/styles.css", import.meta.url), "utf8"),
  ]);

  assert.match(appSource, /PANEL_HEIGHT_MARGIN/);
  assert.match(appSource, /Math\.min\(contentHeight, maxHeight\)/);
  assert.match(appSource, /--panel-max-height/);
  assert.match(styleSource, /\.settings-panel\s*\{[^}]*max-height:\s*var\(--panel-max-height/s);
  assert.match(styleSource, /\.settings-panel\s*\{[^}]*overflow-y:\s*auto/s);
});

test("Git repository ranking renders all counted repositories", async () => {
  const appSource = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");
  const gitSectionStart = appSource.indexOf("function GitUsageSection");
  const tokenSummaryStart = appSource.indexOf("function TokenUsageSummary");
  const gitSectionSource = appSource.slice(gitSectionStart, tokenSummaryStart);

  assert.equal(gitSectionSource.includes("repositoryUsageRows(report, 3)"), false);
  assert.match(gitSectionSource, /const repositoryRows = report \? repositoryUsageRows\(report\) : \[\]/);
});

test("statistics range selector follows the custom range design", async () => {
  const appSource = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");

  assert.equal(appSource.includes("近3天"), false);
  assert.doesNotMatch(appSource, /tokenUsageRangeOptions:[^\n]+last3Days/);
  assert.match(
    appSource,
    /tokenUsageRangeOptions:[^\n]+\["thisMonth", "thisWeek", "today", "custom"\]/,
  );
  assert.equal(appSource.includes('type="date"'), true);
  assert.equal(appSource.includes('aria-label="开始日期"'), true);
  assert.equal(appSource.includes('aria-label="结束日期"'), true);
  assert.match(appSource, />\s*查询\s*</);
});

test("settings account messages are scoped to the quota tab", async () => {
  const appSource = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");
  const settingsPanelStart = appSource.indexOf("function SettingsPanel");
  const tokenUsagePanelStart = appSource.indexOf("function TokenUsagePanel");
  const settingsPanelSource = appSource.slice(settingsPanelStart, tokenUsagePanelStart);

  assert.equal(
    settingsPanelSource.includes('{message ? <div className="settings-message">{message}</div> : null}'),
    false,
  );
  assert.match(
    settingsPanelSource,
    /\{activeTab === "quota" && message \? <div className="settings-message">\{message\}<\/div> : null\}/,
  );
});

test("Git usage path control is rendered above the refresh footer instead of above the summary card", async () => {
  const appSource = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");
  const gitSectionStart = appSource.indexOf("function GitUsageSection");
  const tokenSummaryStart = appSource.indexOf("function TokenUsageSummary");
  const gitSectionSource = appSource.slice(gitSectionStart, tokenSummaryStart);

  const rootFieldIndex = gitSectionSource.indexOf('<div className="git-root-field">');
  const summaryCardIndex = gitSectionSource.indexOf('<section className="token-card git-summary-card">');
  const footerIndex = gitSectionSource.indexOf('<div className="token-footer">');

  assert.notEqual(rootFieldIndex, -1);
  assert.notEqual(summaryCardIndex, -1);
  assert.notEqual(footerIndex, -1);
  assert.ok(rootFieldIndex > summaryCardIndex);
  assert.ok(rootFieldIndex < footerIndex);
});

test("Git trend chart plots line-count metrics only", async () => {
  const appSource = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");
  const gitSectionStart = appSource.indexOf("function GitUsageSection");
  const tokenSummaryStart = appSource.indexOf("function TokenUsageSummary");
  const gitSectionSource = appSource.slice(gitSectionStart, tokenSummaryStart);

  assert.equal(gitSectionSource.includes("git-chart-changed"), false);
  assert.equal(gitSectionSource.includes("git-changed-legend"), false);
});

test("statistics panel includes the KPI subtab and preserves the shared range selector", async () => {
  const appSource = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");

  assert.match(appSource, />\s*KPI 分析\s*</);
  assert.match(appSource, /type SettingsUsageTab = "token" \| "git" \| "kpi"/);
  assert.match(appSource, /const \[usageRangeUiState, setUsageRangeUiState\]/);
});

test("Token and Git trend charts render every returned bucket", async () => {
  const appSource = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");

  assert.equal(appSource.includes("slice(-7)"), false);
  assert.equal(appSource.includes("visibleChartRows"), false);
  assert.match(appSource, /\{chartRows\.map\(\(row\) => \(/);
});

test("KPI radar labels render the raw metric values under each axis title", async () => {
  const appSource = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");

  assert.match(appSource, /\{axis\.displayValue\}/);
});

test("KPI overview explanation is moved behind a hover help trigger", async () => {
  const appSource = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");

  assert.match(appSource, /className="kpi-summary-heading"/);
  assert.match(appSource, /className="kpi-summary-help"/);
  assert.equal(appSource.includes('className="kpi-summary-caption"'), false);
});

test("KPI metric explanation is moved behind the radar help trigger", async () => {
  const appSource = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");

  assert.match(appSource, /className="kpi-radar-heading"/);
  assert.match(appSource, /className="kpi-radar-help"/);
  assert.equal(appSource.includes('className="kpi-description-title"'), false);
  assert.equal(appSource.includes('className="kpi-description-list"'), false);
});
