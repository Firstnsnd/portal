# Portal 设计规范 v1.0

本文档是 Portal 应用的设计规范，约束视觉风格、色彩、组件尺寸与状态管理。
所有 UI 代码必须遵守；偏离即视为违规。

---

## 1. 核心设计原则

| 原则             | 说明                                                                                                                              |
|----------------|---------------------------------------------------------------------------------------------------------------------------------|
| **类型层分离**      | 用 `SessionKind` enum 在编译期区分 `Local`（无连接概念）与 `Ssh`（有连接状态机）。Local **永不**调用 `is_connected()`/`needs_reconnect()`/`reconnect_ssh()` |
| **单调状态**       | PTY `alive` flag 只从 true→false，绝不反向；SSH 用独立 5 态机 (`Connecting → Authenticating → Connected → Disconnected/Error`)               |
| **Pure egui**  | 不引入 React/Tauri/WASM，全 Rust native Immediate Mode GUI                                                                           |
| **语义色 tokens** | 所有颜色通过 `ThemeColors` struct 引用，禁止硬编码 RGB                                                                                        |
| **组合优于继承**     | 递归 `PaneNode` 树（`Terminal`/`Split`），`Tab = PaneNode + Vec<Session>`，`Window = Vec<Tab>`                                         |

---

## 2. 色彩规范

### 2.1 强制规则

- ❌ **禁止**在 UI 代码中写 `Color32::from_rgb(...)` 或 `Color32::from_rgba_*()` 硬编码颜色
- ❌ **禁止**使用 `Color32::WHITE`/`GRAY`/`RED`/`GREEN` 等内置常量作为显示色
- ✅ **允许** `Color32::TRANSPARENT`（透传 galley glyph / 透明背景）
- ✅ **允许** `Color32::from_black_alpha(n)`（阴影天生是黑色半透明）
- ✅ **允许** `Color32::PLACEHOLDER`（仅用于文本宽度测量，不渲染）
- ✅ **允许** 终端 ANSI 色（从 `(u8,u8,u8)` tuple 转换，非固定值）
- ✅ **允许** 从 `theme.{field}` 派生 alpha（如 `from_rgba_unmultiplied(theme.accent.r(), ..., 60)`）

### 2.2 ThemeColors 字段语义

| 字段                                      | 语义                      |
|-----------------------------------------|-------------------------|
| `bg_primary`                            | 终端/主背景                  |
| `bg_secondary`                          | 侧边栏/状态栏/导航条             |
| `bg_elevated`                           | 卡片/活动 Tab/浮层            |
| `fg_primary`                            | 主文字色                    |
| `fg_dim`                                | 辅助/弱化文字（标签、placeholder） |
| `accent`                                | 品牌色（按钮、焦点环、链接、选中）       |
| `green`                                 | 成功/已连接                  |
| `red`                                   | 错误/断联/进程退出              |
| `warning`                               | 警告/注意（通知、端口占用）          |
| `cursor_color`                          | 终端光标                    |
| `hover_bg` / `hover_shadow`             | 悬停态/面板阴影                |
| `border` / `divider`                    | 边框/分割线                  |
| `card_bg` / `card_hover`                | 列表项背景                   |
| `input_bg` / `input_border`             | 输入框                     |
| `button_bg` / `button_text`             | 按钮背景/有色按钮上的文字           |
| `badge_bg`                              | 标签/徽章                   |
| `menu_bg`                               | 下拉菜单                    |
| `focus_ring`                            | 焦点指示                    |
| `overlay_bg`                            | 模态/tooltip 遮罩           |
| `success_dim` / `error_dim`             | 淡色成功/错误（背景高亮）           |
| `search_match` / `search_match_current` | 搜索匹配（非当前/当前）            |
| `broadcast_bg`                          | 广播模式 Tab 填充             |

### 2.3 主题预设

8 套（`ThemePreset`）：Tokyo Night（默认）、Dracula、One Dark、Solarized Dark、Nord、Solarized Light、GitHub Light、One Light。每个 preset 必须填充全部 `ThemeColors` 字段。

### 2.4 终端 ANSI 色

独立于主题系统，定义在 `src/terminal/color.rs`：16 标准色 + 216 色 cube + 24 级灰阶，默认 `DEFAULT_FG=(220,228,255)` / `DEFAULT_BG=(26,27,38)`。

---

## 3. 视觉 Token 规范

所有数值必须引用 `src/ui/tokens.rs` 或 `src/ui/widgets.rs` 中的常量，禁止裸字面量。

### 3.1 间距

```rust
// tokens.rs
SPACE_XS = 4.0; SPACE_SM = 8.0; SPACE_MD = 12.0;
SPACE_LG = 16.0; SPACE_XL = 24.0; SPACE_2XL = 32.0;
```

### 3.2 字号

```rust
// tokens.rs
FONT_XS = 10.0; FONT_SM = 11.0; FONT_MD = 12.0; FONT_BASE = 13.0;
// widgets.rs
FONT_SIZE_LABEL = 12.0; FONT_SIZE_INPUT = 12.0; FONT_SIZE_TITLE = 14.0;
```

### 3.3 圆角

| 场景          | 值   | 来源                     |
|-------------|-----|------------------------|
| 按钮/输入框      | 6.0 | `INPUT_ROUNDING`       |
| Tab/小面板/对话框 | 8.0 | `DIALOG_ROUNDING`      |
| 全局 widget   | 6.0 | `apply_visuals()` 统一设置 |
| 徽章          | 4.0 | `RADIUS_SM`            |

### 3.4 组件尺寸

| 组件       | 值                            | Token                              |
|----------|------------------------------|------------------------------------|
| 状态栏高     | 24.0                         | `STATUS_BAR_HEIGHT`                |
| Drawer 宽 | 380.0                        | `widgets::DRAWER_WIDTH`（唯一来源）      |
| 列表行高     | 52.0                         | `LIST_ROW_HEIGHT`                  |
| 输入框高     | 20.0                         | `INPUT_HEIGHT`                     |
| 按钮最小     | 80×32                        | `widgets` primary/secondary/danger |
| 搜索栏      | 280×~32                      | `render.rs`                        |
| 导航栏宽     | `screen×0.14 clamp(150,200)` | `nav_panel.rs`                     |

---

## 4. 终端渲染规范

| 属性             | 值                                                       |
|----------------|---------------------------------------------------------|
| `pad_x`        | 8.0px                                                   |
| `pad_y_top`    | 6.0px                                                   |
| `pad_y_bottom` | 0.0px                                                   |
| 字符宽度           | `"MM".width / 2`（galley composition 测量，非 `glyph_width`） |
| 行高             | `fonts.row_height(font_id).ceil()`                      |
| 光标（活跃）         | 2px 竖线，`cursor_color`，500ms 闪烁                          |
| 光标（非活跃）        | 1.5px 竖线，`cursor_color` alpha 140                       |
| 选择高亮           | `accent_alpha(60)`                                      |
| Tab 内边距        | `Margin::symmetric(12, 4)`                              |
| Tab 圆角         | `Rounding::same(8)`                                     |
| Tab 间距         | 6px                                                     |
| 状态点            | `●` 8px                                                 |
| 关闭按钮           | `×` 14px                                                |
| 广播指示器          | `◉` 11px                                                |

---

## 5. 状态管理规范

- **Local 会话**：仅用 `has_exited()`（读 `alive` flag）
- **SSH 会话**：通过 `match SessionKind::Ssh(ssh, _, _)` 访问 `ssh.connection_state()` 等
- **`reconnect_ssh()` 调用前必须检查 `SessionKind::Ssh`**，不允许对 Local 静默 no-op
- **`unwrap()` 守卫**必须检查所依赖的不变量本身（如 `Option::is_some()`），不可依赖间接属性
- **不可达分支**用 `expect()` 显式表达，而非伪装成可恢复的 fallback

---

## 6. 合规审计方法

```bash
# 硬编码颜色违规（应为空）
grep -rn 'Color32::GRAY\|Color32::GREEN\|Color32::RED' src/ --include='*.rs' \
  | grep -v 'src/ui/theme.rs' | grep -v 'src/terminal/color.rs'

# 验证
cargo build && cargo test && cargo clippy
```

- 0 compile errors，0 clippy warnings
- 全部测试通过
- 上述 grep 无违规残留
