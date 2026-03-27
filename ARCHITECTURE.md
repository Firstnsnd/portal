# Portal 项目架构文档

## 概述

Portal 是一个用 Rust 和 egui 构建的跨平台终端模拟器，灵感来自 Termius。提供 SSH、SFTP 和本地终端模拟功能。

**代码统计**: ~23,000 行 Rust 代码

## 目录结构

```
src/
├── lib.rs                 # 库入口，模块声明
├── main.rs                # 应用入口，eframe::App 实现
├── app/                   # 应用核心模块
│   ├── mod.rs             # PortalApp 主结构
│   ├── tab_management.rs  # Tab 管理逻辑
│   └── window_content.rs  # 统一窗口内容渲染
├── config/                # 配置管理
│   ├── mod.rs             # HostEntry, Credential, 配置持久化
│   └── tests.rs           # 配置相关测试
├── ssh/                   # SSH 连接模块
│   ├── mod.rs             # SshSession, 连接状态管理
│   ├── session.rs         # SSH 会话实现
│   ├── port_forward.rs    # 端口转发
│   └── tests.rs           # SSH 测试
├── sftp/                  # SFTP 文件浏览器
│   ├── mod.rs             # 模块导出
│   ├── types.rs           # SftpEntry, TransferProgress
│   ├── selection.rs       # 多选状态管理
│   ├── local.rs           # 本地文件浏览器
│   ├── browser.rs         # SFTP 浏览器 (异步)
│   └── task.rs            # 异步 SFTP 任务
├── terminal/              # 终端模拟核心
│   ├── mod.rs             # PTY 抽象
│   ├── grid.rs            # 终端网格 (字符缓冲)
│   ├── vte.rs             # VTE 转义序列解析
│   ├── color.rs           # 颜色处理
│   ├── types.rs           # TerminalCell, CellAttrs
│   ├── session.rs         # RealPtySession
│   ├── unix_pty.rs        # Unix PTY 实现
│   └── windows_pty.rs     # Windows PTY 实现
└── ui/                    # 用户界面
    ├── mod.rs             # 主题、语言、字体
    ├── pane.rs            # 分屏布局系统 (PaneNode, Tab, AppWindow)
    ├── input.rs           # 键盘输入处理
    ├── fonts.rs           # 字体加载
    ├── theme.rs           # 主题预设
    ├── tokens.rs          # 设计令牌 (常量)
    ├── formatting.rs      # 格式化工具
    ├── widgets.rs         # 可复用 UI 组件
    ├── i18n/              # 国际化
    │   ├── mod.rs         # Language trait
    │   ├── en.rs, zh.rs, ja.rs, ko.rs, ...
    ├── types/             # UI 类型定义
    │   ├── mod.rs
    │   ├── session.rs     # TerminalSession, SessionBackend
    │   ├── dialogs.rs     # AppView, 各种对话框状态
    │   ├── layout.rs      # 布局相关类型
    │   └── sftp_types.rs  # SFTP 对话框类型
    ├── terminal/          # 终端 UI 渲染
    │   ├── mod.rs         # render_terminal_pane
    │   ├── render.rs      # 网格渲染
    │   └── selection.rs   # 文本选择
    └── views/             # 视图实现
        ├── mod.rs
        ├── tab_view.rs    # Tab 栏渲染
        ├── nav_panel.rs   # 导航侧边栏
        ├── hosts_view.rs  # 主机列表视图
        ├── sftp_view.rs   # SFTP 双面板视图
        ├── settings_view.rs
        ├── keychain_view.rs
        ├── snippet_view.rs
        ├── tunnel_view.rs
        └── sftp/          # SFTP 子组件
            ├── mod.rs
            ├── panel.rs   # 文件面板
            ├── progress.rs # 传输进度
            └── format.rs  # 格式化
```

## 核心数据结构

### 层级关系

```
PortalApp (应用全局状态)
└── windows: Vec<AppWindow>  (多窗口支持)
    └── AppWindow (单个窗口)
        ├── tabs: Vec<Tab>   (多标签)
        │   └── Tab
        │       ├── sessions: Vec<TerminalSession>
        │       ├── layout: PaneNode (分屏树)
        │       └── focused_session: usize
        └── sftp_browser_* / local_browser_* (SFTP 状态)
```

### PortalApp (src/app/mod.rs)

全局应用状态，包含：
- `windows: Vec<AppWindow>` - 所有窗口（主窗口 + 分离窗口）
- `hosts: Vec<HostEntry>` - SSH 主机配置
- `credentials: Vec<Credential>` - 凭据
- `runtime: tokio::Runtime` - 异步运行时
- `theme`, `language`, `font_size` - 全局设置
- `snippets: Vec<Snippet>` - 命令片段

### AppWindow (src/ui/pane.rs)

单窗口状态，所有窗口平等（无主/从区分）：
- `viewport_id: egui::ViewportId` - egui 视口 ID
- `tabs: Vec<Tab>` - 标签页列表
- `active_tab: usize` - 当前活动标签
- `current_view: AppView` - 当前视图 (Terminal/Hosts/SFTP/Settings...)
- `ime_composing`, `ime_preedit` - 输入法状态
- **SFTP 状态 (每窗口独立)**:
  - `sftp_browser_left/right: Option<SftpBrowser>` - 远程浏览器
  - `local_browser_left/right: LocalBrowser` - 本地浏览器
  - `sftp_*_dialog` - 各种对话框状态

### Tab (src/ui/pane.rs)

工作区标签：
- `title: String` - 标签标题
- `sessions: Vec<TerminalSession>` - 终端会话列表
- `layout: PaneNode` - 分屏布局树
- `focused_session: usize` - 焦点会话索引
- `broadcast_enabled: bool` - 广播模式
- `snippet_drawer_open: bool` - 片段抽屉状态

### PaneNode (src/ui/pane.rs)

分屏布局树节点：
```rust
enum PaneNode {
    Terminal(usize),  // 叶节点 - 终端会话索引
    Split {
        direction: SplitDirection,  // Horizontal | Vertical
        ratio: f32,                 // 分割比例 0.0-1.0
        first: Box<PaneNode>,       // 左/上子节点
        second: Box<PaneNode>,      // 右/下子节点
    },
}
```

### TerminalSession (src/ui/types/session.rs)

终端会话：
- `session: Option<SessionBackend>` - 本地 PTY 或 SSH
- `ssh_host: Option<HostEntry>` - SSH 主机配置
- `resolved_auth: Option<ResolvedAuth>` - 解析后的认证信息
- `selection: Selection` - 文本选择状态
- `search_state: SearchState` - 搜索状态
- `created_at: Instant` - 创建时间

### SessionBackend (src/ui/types/session.rs)

统一会话后端：
```rust
enum SessionBackend {
    Local(RealPtySession),  // 本地 PTY
    Ssh(SshSession),        // SSH 连接
}
```

## 关键实现

### 1. 分屏布局系统

**文件**: `src/ui/pane.rs`

- 递归树结构支持无限嵌套
- 可拖拽调整分割比例
- 操作: `replace`, `remove`, `decrement_indices_above`, `offset_indices`
- 渲染: `render_pane_tree()` 递归遍历

### 2. 终端模拟

**文件**: `src/terminal/`

- **grid.rs**: 终端网格，存储字符、属性、颜色
- **vte.rs**: VTE 转义序列解析器
- **unix_pty.rs / windows_pty.rs**: 平台 PTY 实现
- **session.rs**: PTY 会话管理，读写分离线程

### 3. SSH 连接

**文件**: `src/ssh/`

- 使用 `russh` 库
- 支持密码和 SSH 密钥认证
- 跳板机支持 (JumpHostInfo)
- 连接状态机: Connecting → Authenticating → Connected → Disconnected

### 4. SFTP 文件浏览器

**文件**: `src/sftp/`, `src/ui/views/sftp_view.rs`

- 双面板设计 (本地/远程)
- 异步操作 (tokio)
- 拖拽传输
- 进度显示
- 内置文件编辑器

### 5. 多窗口支持

**文件**: `src/main.rs`, `src/app/window_content.rs`

- 统一渲染路径 `render_window_content()`
- 分离窗口通过 `show_viewport_immediate()` 创建
- Tab 拖拽分离到新窗口
- 每窗口独立的 SFTP 连接

### 6. 配置与凭据

**文件**: `src/config/`

- JSON 配置持久化
- macOS Keychain 集成
- 凭据按主机存储: `portal-ssh-{host}:{port}`

### 7. 国际化

**文件**: `src/ui/i18n/`

- Language trait 定义翻译接口
- 支持: EN, ZH, JA, KO, ES, FR, RU

## 渲染流程

```
main.rs: update()
├── render_window_content(ctx, 0, false)  // 主窗口
├── show_viewport_immediate()             // 分离窗口
│   └── render_window_content(ctx, i, true)
└── poll SFTP browsers                    // 轮询 SFTP 状态

render_window_content()
├── 键盘快捷键处理
├── nav_panel::show_nav_panel()          // 侧边栏
├── render_tab_bar()                      // Tab 栏
├── render_status_bar()                   // 状态栏
├── 抽屉渲染 (add_host, credential, snippet)
└── CentralPanel
    └── match current_view
        ├── Terminal → render_terminal_content()
        ├── Hosts → show_hosts_page()
        ├── Sftp → show_sftp_view()
        ├── Keychain → show_keychain_view()
        ├── Settings → show_settings_view()
        └── ...
```

## 技术栈

| 领域 | 技术 |
|------|------|
| GUI | egui / eframe |
| 异步 | tokio |
| SSH | russh, russh-sftp |
| 终端 | vte (解析), portability-pty |
| Keychain | keyring |
| 序列化 | serde, serde_json |

## 状态分层

### PortalApp (全局共用数据)

跨所有窗口共享的数据：

| 字段 | 说明 |
|------|------|
| `hosts: Vec<HostEntry>` | SSH 主机配置列表 |
| `credentials: Vec<Credential>` | 凭据配置 |
| `snippets: Vec<Snippet>` | 命令片段 |
| `theme`, `language`, `font_size` | 全局外观设置 |
| `connection_history` | 连接历史记录 |
| `runtime: tokio::Runtime` | 异步运行时 (单例) |
| `add_host_dialog` | 添加主机对话框 |
| `credential_dialog` | 凭据对话框 |
| `add_tunnel_dialog` | 添加隧道对话框 |

### AppWindow (每窗口独立状态)

每个窗口独立的状态，支持多窗口并行操作：

| 字段 | 说明 |
|------|------|
| `current_view: AppView` | 当前活动视图 |
| `tabs: Vec<Tab>` | 标签页列表 |
| `active_tab: usize` | 当前活动标签 |
| `sftp_browser_left/right` | SFTP 远程浏览器 |
| `local_browser_left/right` | 本地文件浏览器 |
| `sftp_*_dialog` | SFTP 相关对话框 |
| `ime_composing`, `ime_preedit` | 输入法状态 |
| `tab_drag: TabDragState` | Tab 拖拽状态 |

### Tab (每标签独立状态)

| 字段 | 说明 |
|------|------|
| `sessions: Vec<TerminalSession>` | 终端会话列表 |
| `layout: PaneNode` | 分屏布局树 |
| `focused_session: usize` | 焦点会话 |
| `broadcast_enabled: bool` | 广播模式 |
| `snippet_drawer_open: bool` | 片段抽屉 |

## 架构原则

1. **状态隔离**: 每个窗口有独立的 SFTP 连接、视图状态
2. **统一渲染**: 所有窗口使用同一渲染路径 `render_window_content()`
3. **异步优先**: 网络/文件操作全部异步 (tokio)
4. **安全存储**: 敏感凭据存入系统 Keychain
5. **数据共享**: 主机配置、凭据、设置在所有窗口间共享
