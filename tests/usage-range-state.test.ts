import test from "node:test";
import assert from "node:assert/strict";
import {
  applyCustomRangeDraft,
  createUsageRangeUiState,
  customUsageWindowBounds,
  resolveVisibleReportState,
  selectUsageRangeOption,
  updateCustomRangeDraft,
  validateCustomUsageRangeSelection,
} from "../src/lib/usage-range.ts";

test("editing custom draft does not change applied range until apply", () => {
  const initial = createUsageRangeUiState(new Date(2026, 3, 28));
  const customSelected = selectUsageRangeOption(initial, "custom");
  const edited = updateCustomRangeDraft(customSelected, "startDate", "2026-03-01");

  assert.deepEqual(initial.appliedSelection, { kind: "preset", range: "thisMonth" });
  assert.deepEqual(edited.appliedSelection, { kind: "preset", range: "thisMonth" });

  const applied = applyCustomRangeDraft(edited);
  assert.deepEqual(applied.appliedSelection, {
    kind: "custom",
    startDate: "2026-03-01",
    endDate: customSelected.customDraft.endDate,
  });
});

test("custom range validation rejects dates older than the rolling ninety-day window", () => {
  const now = new Date(2026, 3, 28);
  const bounds = customUsageWindowBounds(now);

  assert.deepEqual(bounds, {
    minDate: "2026-01-29",
    maxDate: "2026-04-28",
  });
  assert.equal(
    validateCustomUsageRangeSelection(
      {
        kind: "custom",
        startDate: "2026-01-28",
        endDate: "2026-04-28",
      },
      now,
    ),
    "自定义开始日期不能早于 2026-01-29",
  );
});

test("visible report falls back to the last ready report while the requested report is pending", () => {
  assert.deepEqual(
    resolveVisibleReportState("custom:2026-04-01:2026-04-10", { pending: true }, "thisMonth", { pending: false }),
    {
      visibleKey: "thisMonth",
      showingFallback: true,
    },
  );
  assert.deepEqual(
    resolveVisibleReportState("custom:2026-04-01:2026-04-10", { pending: false }, "thisMonth", { pending: false }),
    {
      visibleKey: "custom:2026-04-01:2026-04-10",
      showingFallback: false,
    },
  );
});
