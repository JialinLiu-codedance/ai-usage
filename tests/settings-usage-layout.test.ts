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

test("desktop window keeps a fixed default height and lets users resize it", async () => {
  const tauriConfig = JSON.parse(
    await readFile(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"),
  );
  const mainWindow = tauriConfig.app.windows[0];

  assert.equal(mainWindow.width, 420);
  assert.equal(mainWindow.height, 640);
  assert.equal(mainWindow.minWidth, 420);
  assert.equal(mainWindow.minHeight, 240);
  assert.equal(mainWindow.resizable, true);
});

test("app no longer auto-resizes the desktop window from content height", async () => {
  const [appSource, tauriBridgeSource, mainSource, commandsSource] = await Promise.all([
    readFile(new URL("../src/App.tsx", import.meta.url), "utf8"),
    readFile(new URL("../src/lib/tauri.ts", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/src/main.rs", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/src/commands.rs", import.meta.url), "utf8"),
  ]);

  assert.equal(appSource.includes("ResizeObserver"), false);
  assert.equal(appSource.includes("resizePanel"), false);
  assert.equal(appSource.includes("panelMaxHeight"), false);
  assert.equal(appSource.includes("--panel-max-height"), false);
  assert.equal(tauriBridgeSource.includes("resizePanel"), false);
  assert.equal(commandsSource.includes("resize_main_panel"), false);
  assert.equal(mainSource.includes("commands::resize_main_panel"), false);
});

test("panel layout fills the window and scrolls overflowing content internally", async () => {
  const [appSource, styleSource] = await Promise.all([
    readFile(new URL("../src/App.tsx", import.meta.url), "utf8"),
    readFile(new URL("../src/styles.css", import.meta.url), "utf8"),
  ]);

  assert.match(appSource, /className="panel-root panel-root-overview"/);
  assert.match(appSource, /className=\{`panel-root panel-root-\$\{view\}`\}/);
  assert.match(styleSource, /html,\s*body,\s*#root\s*\{[^}]*height:\s*100%/s);
  assert.match(styleSource, /\.panel-root\s*\{[^}]*height:\s*100%/s);
  assert.match(styleSource, /\.overview-panel,\s*\.settings-panel\s*\{[^}]*flex:\s*1 1 auto/s);
  assert.match(styleSource, /\.overview-panel,\s*\.settings-panel\s*\{[^}]*min-height:\s*0/s);
  assert.match(styleSource, /\.overview-panel,\s*\.settings-panel\s*\{[^}]*overflow-y:\s*auto/s);
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

test("settings quota tab includes the launch-at-login toggle", async () => {
  const appSource = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");
  const settingsPanelStart = appSource.indexOf("function SettingsPanel");
  const tokenUsagePanelStart = appSource.indexOf("function TokenUsagePanel");
  const settingsPanelSource = appSource.slice(settingsPanelStart, tokenUsagePanelStart);

  assert.match(settingsPanelSource, />\s*登录时自动启动\s*</);
  assert.match(settingsPanelSource, /checked=\{form\.launch_at_login\}/);
  assert.match(
    settingsPanelSource,
    /onCheckedChange=\{\(checked\) => onChange\(\{ \.\.\.form, launch_at_login: checked \}\)\}/,
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

test("Git commit detail is merged into repository ranking with collapsible project rows", async () => {
  const appSource = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");
  const gitSectionStart = appSource.indexOf("function GitUsageSection");
  const tokenSummaryStart = appSource.indexOf("function TokenUsageSummary");
  const gitSectionSource = appSource.slice(gitSectionStart, tokenSummaryStart);

  assert.equal(gitSectionSource.includes("git-commit-section"), false);
  assert.equal(gitSectionSource.includes(">提交明细<"), false);
  assert.match(gitSectionSource, /const commitGroups = report \? commitDetailGroups\(report\) : \[\]/);
  assert.match(gitSectionSource, /const commitGroupsByPath = new Map\(commitGroups\.map\(\(group\) => \[group\.path, group\]\)\)/);
  assert.match(gitSectionSource, /const commitGroup = commitGroupsByPath\.get\(repository\.path\);/);
  assert.match(gitSectionSource, /<details className="git-repository-details" key=\{repository\.path\}>/);
  assert.match(gitSectionSource, /className="git-repository-summary"/);
  assert.match(gitSectionSource, /className="git-commit-list"/);
  assert.match(gitSectionSource, /className="git-commit-group-details"/);
  assert.match(gitSectionSource, /className="git-commit-added"/);
  assert.match(gitSectionSource, /className="git-commit-deleted"/);
  assert.match(gitSectionSource, /className="git-commit-role-badge"/);
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
  const [appSource, styleSource] = await Promise.all([
    readFile(new URL("../src/App.tsx", import.meta.url), "utf8"),
    readFile(new URL("../src/styles.css", import.meta.url), "utf8"),
  ]);

  assert.match(appSource, />\s*KPI 分析\s*</);
  assert.match(appSource, />\s*默认分支\s*</);
  assert.match(appSource, /type SettingsUsageTab = "token" \| "git" \| "kpi" \| "branch"/);
  assert.match(appSource, /const \[usageRangeUiState, setUsageRangeUiState\]/);
  assert.match(
    styleSource,
    /\.usage-subtabs\s*\{[^}]*grid-template-columns:\s*minmax\(0,\s*1fr\)\s*1px\s*minmax\(0,\s*1fr\)\s*1px\s*minmax\(0,\s*1fr\)\s*1px\s*minmax\(0,\s*1fr\)/s,
  );
});

test("statistics panel includes a dedicated default branch management section", async () => {
  const appSource = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");

  assert.match(appSource, /getGitBranchManagement/);
  assert.match(appSource, /const \[branchManagementState, setBranchManagementState\]/);
  assert.match(appSource, /function BranchManagementSection/);
  assert.match(appSource, /const \[openBranchPickerByPath, setOpenBranchPickerByPath\]/);
  assert.match(appSource, /const \[branchFilterByPath, setBranchFilterByPath\]/);
  assert.match(appSource, /GitHub 默认分支：/);
  assert.match(appSource, /当前生效分支：/);
  assert.match(appSource, /className="git-branch-combobox"/);
  assert.match(appSource, /className="git-branch-combobox-panel"/);
  assert.match(appSource, /placeholder="搜索分支"/);
  assert.match(appSource, /candidate\.display_name\.toLowerCase\(\)\.includes\(filterKeyword\)/);
  assert.match(appSource, />\s*恢复自动识别\s*</);
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
  assert.match(appSource, /缓存命中\s*\/\s*10/);
  assert.equal(appSource.includes('className="kpi-summary-caption"'), false);
});

test("KPI metric explanation is moved behind the radar help trigger", async () => {
  const appSource = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");

  assert.match(appSource, /className="kpi-radar-heading"/);
  assert.match(appSource, /className="kpi-radar-help"/);
  assert.equal(appSource.includes('className="kpi-description-title"'), false);
  assert.equal(appSource.includes('className="kpi-description-list"'), false);
});
