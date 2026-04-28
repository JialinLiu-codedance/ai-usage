import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

interface Rule {
  selectors: string[];
  declarations: Record<string, string>;
  order: number;
}

interface TargetElement {
  element: string;
  ancestorClasses: Set<string>;
}

test("Token and Git summary metric values use the same final font size", async () => {
  const css = await readFile(new URL("../src/styles.css", import.meta.url), "utf8");
  const rules = parseRules(css);
  const tokenValue = {
    element: "strong",
    ancestorClasses: new Set(["token-summary-card", "token-metric"]),
  };
  const gitValue = {
    element: "strong",
    ancestorClasses: new Set(["git-summary-card", "git-summary-grid", "token-metric"]),
  };

  assert.equal(finalDeclaration(rules, tokenValue, "font-size"), "20px");
  assert.equal(finalDeclaration(rules, gitValue, "font-size"), "20px");
  assert.equal(finalDeclaration(rules, gitValue, "font-size"), finalDeclaration(rules, tokenValue, "font-size"));
});

test("Token and Git trend bars share the same dimensions", async () => {
  const css = await readFile(new URL("../src/styles.css", import.meta.url), "utf8");
  const rules = parseRules(css);

  assert.equal(chartDeclaration(rules, "token", "default", "width"), "min(18px, 64%)");
  assert.equal(chartDeclaration(rules, "git", "default", "width"), chartDeclaration(rules, "token", "default", "width"));
  assert.equal(chartDeclaration(rules, "git", "default", "height"), chartDeclaration(rules, "token", "default", "height"));
  assert.equal(chartDeclaration(rules, "git", "default", "border-radius"), chartDeclaration(rules, "token", "default", "border-radius"));

  assert.equal(chartDeclaration(rules, "git", "thisMonth", "width"), chartDeclaration(rules, "token", "thisMonth", "width"));
  assert.equal(
    chartGridDeclaration(rules, "git", "thisMonth", "grid-auto-columns"),
    chartGridDeclaration(rules, "token", "thisMonth", "grid-auto-columns"),
  );

  assert.equal(chartDeclaration(rules, "git", "today", "width"), chartDeclaration(rules, "token", "today", "width"));
  assert.equal(
    chartGridDeclaration(rules, "git", "today", "grid-auto-columns"),
    chartGridDeclaration(rules, "token", "today", "grid-auto-columns"),
  );
});

test("Token and Git dense trend bars keep visible space between columns", async () => {
  const css = await readFile(new URL("../src/styles.css", import.meta.url), "utf8");
  const rules = parseRules(css);

  assert.equal(chartDeclaration(rules, "token", "thisMonth", "width"), "min(12px, 64%)");
  assert.equal(chartDeclaration(rules, "git", "thisMonth", "width"), "min(12px, 64%)");
  assert.equal(chartDeclaration(rules, "token", "today", "width"), "min(16px, 64%)");
  assert.equal(chartDeclaration(rules, "git", "today", "width"), "min(16px, 64%)");
});

test("Token and Git trend charts fit all columns without horizontal scrolling", async () => {
  const css = await readFile(new URL("../src/styles.css", import.meta.url), "utf8");
  const rules = parseRules(css);

  assert.equal(chartGridDeclaration(rules, "token", "default", "grid-template-columns"), "repeat(var(--usage-chart-columns), minmax(0, 1fr))");
  assert.equal(chartGridDeclaration(rules, "git", "default", "grid-template-columns"), "repeat(var(--usage-chart-columns), minmax(0, 1fr))");
  assert.equal(chartGridDeclaration(rules, "token", "default", "overflow-x"), "visible");
  assert.equal(chartGridDeclaration(rules, "git", "default", "overflow-x"), "visible");
});

test("Token chart segment palette does not use black", async () => {
  const css = await readFile(new URL("../src/styles.css", import.meta.url), "utf8");
  const rules = parseRules(css);
  const blackValues = new Set(["#18181b", "#000", "#000000", "black"]);

  for (let index = 0; index < 6; index += 1) {
    const background = finalDeclaration(
      rules,
      {
        element: "span",
        ancestorClasses: new Set([`token-model-color-${index}`, "token-chart-segment"]),
      },
      "background",
    );
    assert.equal(blackValues.has(background ?? ""), false);
  }
});

function parseRules(css: string): Rule[] {
  const rules: Rule[] = [];
  const ruleRegex = /([^{}]+)\{([^{}]+)\}/g;
  let match: RegExpExecArray | null;
  let order = 0;

  while ((match = ruleRegex.exec(css)) !== null) {
    const selectors = match[1]
      .split(",")
      .map((selector) => selector.trim())
      .filter((selector) => selector && !selector.startsWith("@"));
    if (selectors.length === 0) {
      continue;
    }
    rules.push({
      selectors,
      declarations: parseDeclarations(match[2]),
      order,
    });
    order += 1;
  }

  return rules;
}

function parseDeclarations(block: string): Record<string, string> {
  return Object.fromEntries(
    block
      .split(";")
      .map((line) => line.trim())
      .filter(Boolean)
      .map((line) => {
        const [property, ...valueParts] = line.split(":");
        return [property.trim(), valueParts.join(":").trim()];
      }),
  );
}

function finalDeclaration(rules: Rule[], target: TargetElement, property: string): string | null {
  let match: { value: string; specificity: number; order: number } | null = null;

  for (const rule of rules) {
    const value = rule.declarations[property];
    if (!value) {
      continue;
    }
    for (const selector of rule.selectors) {
      if (!selectorMatchesTarget(selector, target)) {
        continue;
      }
      const specificity = selectorSpecificity(selector);
      if (!match || specificity > match.specificity || (specificity === match.specificity && rule.order > match.order)) {
        match = { value, specificity, order: rule.order };
      }
    }
  }

  return match?.value ?? null;
}

function chartDeclaration(
  rules: Rule[],
  chart: "token" | "git",
  range: "default" | "thisMonth" | "today",
  property: string,
): string | null {
  const barClass = chart === "token" ? "token-chart-bar" : "git-chart-bars";
  const classes = new Set([barClass]);
  if (range !== "default") {
    classes.add(`${chart}-chart-range-${range}`);
  }
  return finalDeclaration(rules, { element: "div", ancestorClasses: classes }, property);
}

function chartGridDeclaration(
  rules: Rule[],
  chart: "token" | "git",
  range: "default" | "thisMonth" | "today",
  property: string,
): string | null {
  const classes = new Set([`${chart}-chart`]);
  if (range !== "default") {
    classes.add(`${chart}-chart-range-${range}`);
  }
  return finalDeclaration(
    rules,
    {
      element: "div",
      ancestorClasses: classes,
    },
    property,
  );
}

function selectorMatchesTarget(selector: string, target: TargetElement): boolean {
  const elementName = selector.match(/(^|[\s>+~])([a-z][a-z0-9-]*)$/i)?.[2];
  if (elementName && elementName.toLowerCase() !== target.element) {
    return false;
  }
  return [...selector.matchAll(/\.([a-zA-Z0-9_-]+)/g)].every((match) => target.ancestorClasses.has(match[1]));
}

function selectorSpecificity(selector: string): number {
  const classCount = [...selector.matchAll(/\.([a-zA-Z0-9_-]+)/g)].length;
  const elementCount = selector
    .replace(/\.[a-zA-Z0-9_-]+/g, " ")
    .split(/[\s>+~]+/)
    .filter((token) => /^[a-z][a-z0-9-]*$/i.test(token)).length;
  return classCount * 100 + elementCount;
}
