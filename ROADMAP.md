# Portal 开发路线图

> **目标**: 基于 Rust 和 egui 构建一个轻量级、现代的终端模拟器

## 当前状态

**状态**: 🚧 活跃开发
**框架**: egui (即时模式 GUI)

---

## 🛠️ 技术栈

- **语言**: Rust
- **GUI**: egui 0.29 (即时模式渲染) / eframe
- **异步**: tokio
- **PTY**: pty crate (Unix)
- **终端解析**: vte 0.11
- **SSH**: russh 0.57 (纯 Rust, tokio-native)
- **宽字符**: unicode-width 0.2
- **剪贴板**: arboard 3.4
- **凭据存储**: keyring 3 + apple-native (macOS Keychain)
- **序列化**: serde + serde_json
- **配置目录**: dirs 5.0

---

## 📋 TODO

### P0 — 最高优先级
- [ ] 终端超链接检测 (URL/文件路径可点击)

### P1 — 高优先级
- [ ] SSH Agent 转发
- [ ] SSH Config 导入 (~/.ssh/config)

### P2 — 中优先级
- [ ] 动态端口转发 (SOCKS proxy)
- [ ] 脚本自动化

### P3 — 低优先级
- [ ] 会话恢复
- [ ] 云同步配置
- [ ] Windows 支持 (ConPTY)
- [ ] Linux 完整支持
- [ ] macOS 优化

---

## ✅ 已完成功能

### 核心架构
- [x] Rust 项目结构搭建
- [x] **egui GUI 框架集成** (从 iced 切换)
- [x] 多标签页支持 + 标签切换/关闭
- [x] Unix PTY 集成 (本地 Shell)
- [x] 终端网格状态管理 (TerminalGrid)
- [x] ANSI 转义序列解析 (vte 0.11)
- [x] 跨平台 PTY 抽象层
- [x] **原生终端输入体验** - 直接在终端区域输入
- [x] **导航栏 + 主机列表独立页面** — 左侧窄导航栏 (Hosts/Terminal 切换), 主机列表在独立页面展示

### 终端模拟
- [x] vte 解析器集成 (替代手写 ANSI 解析)
- [x] 256 色 + Truecolor 渲染
- [x] SGR 属性 (粗体、斜体、下划线、反转、删除线)
- [x] 后台 PTY I/O 线程 (非阻塞读取)
- [x] 延迟换行 (deferred wrap) — 修复 zsh PROMPT_SP `%` 问题
- [x] 交替屏幕缓冲区 (alternate screen)
- [x] 滚动区域 (DECSTBM)
- [x] 光标闪烁 + 保存/恢复
- [x] 窗口大小调整时 PTY 同步
- [x] **终端滚动缓冲区** (scrollback history) — 鼠标滚轮翻阅历史输出
- [x] **CJK 宽字符渲染** — unicode-width 检测 + 双格占位 + CJK 字体回退
- [x] **搜索终端输出内容** — 搜索栏 + 匹配高亮 + 上/下导航

### 输入系统
- [x] 直接键盘事件处理 (allocate_painter + Event::Key)
- [x] key_to_char 映射覆盖全部 ASCII 字符和标点
- [x] IME 支持 — 通过 Event::Ime 处理中文/日文/韩文输入
- [x] Ctrl+A~Z 组合键支持
- [x] Cmd+C 复制 / Cmd+V 粘贴 / Cmd+A 全选
- [x] 鼠标拖选文本 + 选区高亮渲染
- [x] 双击选词、三击选行
- [x] 右键上下文菜单 (复制/粘贴/全选)
- [x] 特殊键: F1~F12, Home, End, PageUp/Down, Insert, Delete
- [x] **快捷键系统** — 可自定义快捷键, 设置页可视化配置

### SSH 连接
- [x] **SSH 协议集成** (russh 0.57, 纯 Rust, tokio-native)
- [x] **密码认证** — 密码保存到 hosts.json 配置
- [x] **SSH 密钥认证** — 支持密钥路径 + passphrase, ~ 路径展开
- [x] **密码安全存储** — 密码/口令存入系统钥匙串 (keyring), JSON 中不再保存明文
- [x] **SSH 私钥导入 Keychain** — 保存时自动将私钥内容从文件导入 macOS Keychain
- [x] **Per-host Keychain 标识** — 每个 host 的凭据在 Keychain Access 中显示为 `Portal: <host name>`
- [x] **凭据与主机分离** — Credential 作为独立实体, hosts 通过 credential_id 引用, 支持凭据复用
- [x] **凭据 CRUD 管理** — Keychain 页面完整的创建/编辑/删除凭据, 显示绑定主机数
- [x] **主机凭据选择** — 添加/编辑主机时支持 无认证/选择已有凭据/新建内联凭据 三种模式
- [x] **SSH 会话管理** — 独立 src/ssh/ 模块, 与本地终端隔离
- [x] **连接状态显示** — Connecting/Authenticating/Connected/Error/Disconnected
- [x] **SSH 自动重连** — 断开后点击标签页自动重新连接
- [x] **SessionBackend 枚举** — Local/Ssh 零成本抽象, 统一 write/resize/get_grid
- [x] **测试连接** — 添加主机时可一键测试连通性
- [x] **known_hosts 校验** — 自动学习新主机密钥, 检测密钥变更防止 MITM
- [x] **SSH 保活心跳** — 每 15 秒发送 keepalive 包
- [x] **跳板机 / Jump Host 支持** — 主机配置 jump_host 字段, 通过跳板机级联 SSH 连接

### 主机管理
- [x] 添加/编辑/删除 Host (抽屉式 UI)
- [x] JSON 持久化 (~/.config/portal/hosts.json) + 系统钥匙串凭据管理
- [x] **Keychain 管理页面** — 导航栏 Keychain 入口, 列出所有凭据, 单条删除 + 全部删除, 二次确认
- [x] 主机按分组显示 (支持 group 字段)
- [x] 显示连接详情 (username@host:port)
- [x] **SSH 认证方式选择** — 密码 / SSH 密钥 切换 UI
- [x] **主机列表 Connect 按钮** — hover 显示 Connect 按钮; 点击行编辑
- [x] **连接历史** — 记录 SSH 连接历史, Hosts 页面显示最近连接

### 端口转发
- [x] **Local/Remote 端口转发** — 连接时自动启动配置的转发规则
- [x] **Host Drawer 内配置转发** — 新建/编辑主机时可直接添加端口转发规则
- [x] **Tunnels 管理页面** — 独立页面查看所有主机的转发规则及运行状态
- [x] **隧道管理 Drawer** — 添加/编辑/删除转发规则, Local/Remote 类型切换

### 命令片段 / Snippets
- [x] **Snippet 管理** — 创建/编辑/删除命令片段, 分组管理
- [x] **Snippet Drawer** — 终端侧边抽屉快速选择并执行命令
- [x] **Snippet 同步执行** — Drawer 关闭后延迟到 PTY resize 完成再写入, 避免丢字符

### SFTP 文件浏览器
- [x] **双面板布局** — 左侧本地 / 右侧远程, 50/50 分屏
- [x] **拖拽传输** — 文件/目录拖拽上传/下载
- [x] **文件管理** — 右键上下文菜单: 重命名、删除、新建文件夹
- [x] **面包屑导航** — 可点击路径分段跳转
- [x] **文件权限显示** — rwxrwxrwx 格式 Unix 权限列
- [x] **状态栏** — 显示文件数、目录数、总大小
- [x] **传输进度条** — 实时速度、进度百分比、取消支持

### UI / 主题
- [x] **主题系统** — 5 种预设主题 (Tokyo Night / Dracula / OneDark / SolarizedDark / Nord)
- [x] **字体大小可调** — 支持 8px-32px 运行时调整
- [x] Termius 风格标签栏 (状态点 + 关闭按钮)
- [x] **SSH 连接状态覆盖层** — 半透明提示 + Cancel 按钮
- [x] **标签页状态指示** — 绿色=已连接, 蓝色=连接中, 红色=断开/错误
- [x] **导航栏布局** — 左侧响应式宽度导航条, 图标+文字按钮, 选中项高亮 + accent bar
- [x] **终端内边距** — 内容与边缘保留 padding, 光标不贴边
- [x] **底部状态栏** — 显示连接类型、Shell 方言、编码
- [x] **分屏关闭 pane** — 悬停 × 按钮 + 右键菜单; 最后一个 pane 关闭 tab
- [x] **分离窗口** — 标签页可脱离主窗口独立显示
- [x] **广播模式** — 向多个终端同时发送命令
- [x] **多语言支持 (i18n)** — 中文 / 日文 / 韩文 / 西班牙语 / 俄语 / 法语

### 打包分发
- [x] **macOS .dmg 安装包** — cargo-bundle + hdiutil 一键打包脚本

---

## 📖 参考资料

- [Ghostty](https://github.com/mitchellh/ghostty) - 现代终端模拟器参考
- [wezterm](https://github.com/wez/wezterm) - 功能丰富的终端
- [kitty](https://github.com/kovidgoyal/kitty) - 高性能终端 GPU 加速
- [alacritty](https://github.com/alacritty/alacritty) - 最小化终端

---

**版本历史**: 请查看 `git tag` 获取各版本详情
**维护者**: Portal Team
