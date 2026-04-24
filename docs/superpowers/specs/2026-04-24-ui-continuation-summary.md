# AI Usage UI Continuation Summary

Date: 2026-04-24

## Context

This session continued UI implementation against the Pencil design file:

- Design source: `/Users/liujialin/project/ai-usage/UI.pen`
- Active project: `/Users/liujialin/project/ai-usage`
- Main target files:
  - `src/App.tsx`
  - `src/styles.css`
  - `src/components/ui/switch.tsx`
  - `src/lib/tauri.ts`
  - `src/vite-env.d.ts`

The goal was to make the app UI match the Pencil design as closely as possible, and list undeveloped functionality separately.

## Pencil Frames Read

Top-level design frames inspected:

- `b90hh`: 额度总览面板
- `NfTuP`: 设置页
- `4DOP1`: 额度总览面板 - 低额度
- `SFXrz`: 额度总览面板 - 错误
- `34hLr`: 多账号总览面板
- `n8k4r`: OpenAI 账户授权
- `wKHLt`: OpenAI 账户授权 - 链接已生成
- `QMfgj`: 添加账号 - 选择平台
- `wjegZ`: LLM Icon Library

Important design tokens observed:

- Panel widths: overview `320px`; settings/add-account/auth `420px`
- Panel radius: `12px`
- Card radius: `8px`
- Primary color: `#18181B`
- Border: `#E4E4E7`
- Muted background: `#F4F4F5`
- Text primary: `#09090B`
- Text muted: `#71717A`
- Danger: `#EF4444`
- Success: `#22C55E`
- Warning: `#F59E0B`

## Implemented UI Changes

### Overview Panel

- Matched the 320px panel layout from `b90hh`.
- Header uses C logo, refresh icon, settings icon.
- Quota card uses account name/email row and 5H / 7D rows.
- Corrected quota bar semantics:
  - Progress bar fill uses `used_percent`.
  - Right-side label displays `remaining_percent`.
- Tone mapping:
  - `remaining <= threshold`: danger
  - `remaining <= 50`: warning
  - otherwise success

### Settings Page

Implemented settings page matching `NfTuP`:

- Title: 设置
- Connected account section:
  - Add account button
  - Account card with OpenAI, email, 已授权
  - 重新授权 and 删除 buttons
- Refresh settings:
  - 自动刷新周期 is now an actual select control.
  - Options: `5 分钟`, `15 分钟`, `30 分钟`, `1 小时`.
  - Selection updates `refresh_interval_minutes` via existing `saveSettings` path.
- Notification settings:
  - 低额度提醒 uses Radix/shadcn `Switch`.
  - 提醒阈值 is now editable.
  - 重置提醒 uses Radix/shadcn `Switch`.

### Switch Component

The project uses the Radix/shadcn `Switch` component in `src/components/ui/switch.tsx`.

A new size variant was added:

- `size="panel"`
- Root: `44px × 24px`, `padding: 2px`
- Thumb: `20px × 20px`
- Checked translation: `translate-x-5`

Settings page uses:

```tsx
<Switch
  aria-label="低额度提醒"
  className="settings-switch"
  size="panel"
  checked={form.notify_on_low_quota}
  onCheckedChange={...}
/>
```

This replaced an earlier temporary custom button implementation, so the current switch is back on the expected Radix/shadcn component.

### Threshold Input

The `提醒阈值` field was changed from a static pill to an editable percent input.

Behavior:

- Displays as percent, e.g. `25%`.
- Internal input accepts digits only.
- Non-digit input is rejected.
- On blur, value normalizes to a positive integer in the range `1-100`.
- Updates `low_quota_threshold_percent` through existing `saveSettings`.
- Styled as a compact pill matching the design:
  - `80px × 32px`
  - white background
  - `8px` radius
  - centered content

### Add Account Page

Implemented `QMfgj`:

- Header with back icon and 添加账号 title.
- Platform list:
  - OpenAI
  - Anthropic
  - Kimi
  - GLM
  - MiniMax
  - Qwen
  - XiaoMi
  - Custom
- Each row uses 48px height, 8px radius, 12px horizontal padding.
- OpenAI is selected by default.
- Next button routes OpenAI to the auth flow.
- Non-OpenAI providers currently show “该平台的接入流程尚未实现”.

### Provider Icons

All provider icons use existing files in `icons/extracted`.

Important mappings:

- OpenAI: `icons/extracted/openai.svg`
- Anthropic: `icons/extracted/anthropic.svg`
- Kimi: `icons/extracted/kimi.svg`
- GLM: `icons/extracted/zhipu.svg`
- MiniMax: `icons/extracted/minimax.svg`
- Qwen: `icons/extracted/qwen.svg`
- XiaoMi: `icons/extracted/xiaomimimo.svg`
- Custom: `icons/extracted/openrouter.svg`

Some icons are rendered via CSS mask to avoid broken-image behavior and match black single-color design usage.

### OpenAI Authorization Flow

Implemented `n8k4r` and `wKHLt` UI states:

- Header with back arrow and title.
- Subtitle: Authorization Method.
- Three auth step cards:
  1. Generate auth link.
  2. Open link in browser and complete auth.
  3. Paste callback URL or code.
- Link-generated state shows:
  - Link box
  - Copy button
  - Regenerate action
- Completion button calls `completeOpenAIOAuth`.

Existing backend functions connected:

- `startOpenAIOAuth`
- `completeOpenAIOAuth`
- `getCurrentQuota`
- `getSettings`

## Browser/Dev Fallback

`src/lib/tauri.ts` now includes a non-Tauri mock fallback for browser/Vite preview:

- Detects Tauri runtime with `__TAURI_INTERNALS__`.
- In browser-only Vite dev, returns mock settings/status and no-ops `resizePanel`.
- Tauri runtime still uses real `invoke`.

This was added so the UI can be tested in the in-app browser without crashing on missing Tauri runtime.

## Verification Performed

Commands run successfully:

```bash
npm run build
```

Browser checks performed through the in-app browser:

- Overview page loads.
- Settings page opens from gear icon.
- Add account page opens from 添加账号.
- OpenAI auth page opens from 下一步.
- Generate auth link state renders.
- Auth code textarea accepts input.
- Complete auth path returns to settings in mock mode.
- Refresh interval select can change to `30 分钟`.
- Threshold input accepts `25` and displays `25%`.
- Threshold input rejects non-digit input such as `abc`.
- Switches are Radix components and visually match `44×24` / `20px` thumb.

## Review Comments Addressed

1. GLM icon incorrect.
   - Fixed icon mapping.

2. XiaoMi icon incorrect/broken.
   - Fixed icon mapping and rendering.

3. Custom icon incorrect.
   - Fixed to OpenRouter icon.

4. Switch style misaligned.
   - Added `panel` size variant to Radix/shadcn switch.

5. Refresh interval should be options.
   - Implemented select with 5/15/30/60-minute options.

6. Threshold should be numeric input.
   - Implemented positive-integer percent input.

7. Threshold looked ugly and should display percentage.
   - Restyled as percent pill input and added strict digit handling.

## Known Unfinished Functionality

Needs product/backend confirmation before implementation:

- Non-OpenAI provider setup flows:
  - Anthropic OAuth
  - Kimi API Key
  - GLM API Key
  - MiniMax API Key
  - Qwen API Key
  - XiaoMi API Key
  - Custom API Key
- Multi-account data model and overview panel:
  - Design has `34hLr` 多账号总览面板.
  - Current app state/types are still single-account oriented.
- Account deletion:
  - Delete button exists visually.
  - No delete behavior implemented because this is destructive and needs exact expected behavior.
- Refresh interval and threshold currently save immediately on change.
  - Confirm whether this should remain auto-save or require an explicit save/apply action.
- Threshold input currently clamps to `1-100`.
  - Confirm whether `0%` should be allowed. The latest request said positive integer, so current minimum is `1`.

## Notes For Next Session

Start by checking current git status because the workspace already had many unrelated modifications before this work.

Useful commands:

```bash
git status --short
npm run build
npm run dev -- --host 127.0.0.1
```

If testing in browser, use:

```text
http://127.0.0.1:1420/
```

The latest focused area is the settings page, especially matching `NfTuP` exactly:

- refresh select visual polish
- threshold input visual polish
- Radix switch `panel` variant

Do not revert unrelated dirty worktree changes unless explicitly requested.
