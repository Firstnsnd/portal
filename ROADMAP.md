# Portal 开发路线图

> **目标**: 基于 Rust 和 egui 构建一个轻量级、现代的终端模拟器

## 当前状态

**状态**: 🚧 活跃开发
**框架**: egui (即时模式 GUI)

---

## 🛠️ 技术栈

### 当前使用
- **语言**: Rust
- **GUI**: egui 0.29 (即时模式渲染)
- **窗口**: eframe
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

### SSH 连接
- [x] **SSH 协议集成** (russh 0.57, 纯 Rust, tokio-native)
- [x] **密码认证** — 密码保存到 hosts.json 配置
- [x] **SSH 密钥认证** — 支持密钥路径 + passphrase, ~ 路径展开
- [x] **密码安全存储** — 密码/口令存入系统钥匙串 (keyring), JSON 中不再保存明文
- [x] **SSH 私钥导入 Keychain** — 保存时自动将私钥内容从文件导入 macOS Keychain
- [x] **Keychain 管理页面** — 左侧导航新增 Keychain 入口, 查看/删除存储的凭据, 二次确认删除
- [x] **Per-host Keychain 标识** — 每个 host 的凭据在 Keychain Access 中显示为 `Portal: <host name>`, 而非统一的 `portal-ssh`
- [x] **凭据与主机分离** — Credential 作为独立实体, hosts 通过 credential_id 引用, 支持凭据复用
- [x] **凭据 CRUD 管理** — Keychain 页面完整的创建/编辑/删除凭据, 显示绑定主机数, 删除时确认受影响主机
- [x] **主机凭据选择** — 添加/编辑主机时支持 无认证/选择已有凭据/新建内联凭据 三种模式
- [x] **SSH 会话管理** — 独立 src/ssh/ 模块, 与本地终端隔离
- [x] **连接状态显示** — Connecting/Authenticating/Connected/Error/Disconnected
- [x] **SSH 自动重连** — 断开后点击标签页自动重新连接
- [x] **SessionBackend 枚举** — Local/Ssh 零成本抽象, 统一 write/resize/get_grid
- [x] **测试连接** — 添加主机时可一键测试连通性, 异步执行连接+认证, 实时显示结果
- [x] **known_hosts 校验** — 自动学习新主机密钥, 检测密钥变更防止 MITM 攻击

### 主机管理
- [x] 添加 Host 对话框 (egui::Window 浮动窗口)
- [x] 编辑 Host — 点击主机行打开编辑抽屉
- [x] 删除 Host — 编辑抽屉右上角删除按钮
- [x] JSON 持久化 (~/.config/portal/hosts.json) + 系统钥匙串凭据管理
- [x] **Keychain 管理页面** — 导航栏 Keychain 入口, 列出所有凭据 (密码/私钥/口令), 单条删除 + 全部删除, 二次确认
- [x] 主机按分组显示 (支持 group 字段)
- [x] 显示连接详情 (username@host:port)
- [x] **SSH 认证方式选择** — 密码 / SSH 密钥 切换 UI
- [x] **主机列表 Connect 按钮** — hover 显示 Connect 按钮, 点击连接; 点击行其他区域编辑

### SFTP 文件浏览器
- [x] **双面板布局** — 左侧本地 / 右侧远程, 50/50 分屏
- [x] **拖拽传输** — 文件/目录拖拽上传/下载
- [x] **文件管理** — 右键上下文菜单: 重命名、删除、新建文件夹
- [x] **面包屑导航** — 可点击路径分段跳转
- [x] **文件权限显示** — rwxrwxrwx 格式 Unix 权限列
- [x] **状态栏** — 显示文件数、目录数、总大小
- [x] **传输进度条** — 实时速度、进度百分比、取消支持
- [x] **刷新按钮动画** — 点击刷新后 spinner 旋转反馈

### UI / 主题
- [x] **主题系统** — 5 种预设主题 (Tokyo Night / Dracula / OneDark / SolarizedDark / Nord)
- [x] **字体大小可调** — 支持 8px-32px 运行时调整
- [x] **配色修复** — hover/选中使用正确 unmultiplied alpha, 文字清晰可见
- [x] Termius 风格标签栏 (状态点 + 关闭按钮)
- [x] **SSH 连接状态覆盖层** — 半透明 Connecting/Error/Disconnected 提示 + Cancel 按钮
- [x] **SSH 终端连接中无光标** — 连接未建立时隐藏光标闪烁
- [x] **SSH 连接超时** — 15 秒超时限制, 可手动取消
- [x] **标签页状态指示** — 绿色=已连接, 蓝色=连接中, 红色=断开/错误
- [x] **导航栏布局** — 左侧响应式宽度导航条, 图标+文字按钮 (Hosts / Terminal / SFTP / Keychain / Settings), 选中项高亮 + 左侧蓝色 accent bar
- [x] **Hosts 页面** — 可滚动主机列表, LOCAL/SSH 分组, 支持编辑/删除/新建
- [x] **终端内边距** — 内容与边缘保留 8px/6px padding, 光标不贴边
- [x] **底部状态栏** — 显示当前会话连接类型 (Local/SSH)、Shell 方言、编码 (UTF-8)
- [x] **分屏关闭 pane** — 悬停显示 × 按钮 + 右键菜单 "Close Pane"; 最后一个 pane 关闭整个 tab
- [x] **分离窗口** — 标签页可脱离主窗口独立显示
- [x] **广播模式** — 向多个终端同时发送命令
- [x] **多语言支持 (i18n)** — 中文 / 日文 / 韩文 / 西班牙语 / 俄语 / 法语


### 打包分发
- [x] **macOS .dmg 安装包** — cargo-bundle + hdiutil 一键打包脚本 (scripts/build-dmg.sh)

---

## 🎯 短期目标 (v0.6.0)

### 终端体验优化
- [ ] 搜索终端输出内容
- [x] 双击选词、三击选行

### 配置与主题
- [x] 主题系统 (5 种预设主题: Tokyo Night / Dracula / OneDark / SolarizedDark / Nord)
- [x] 字体大小可调 (8px-32px)
- [ ] 快捷键自定义

### SSH 增强
- [x] known_hosts 校验 (自动学习新主机密钥 + 检测密钥变更)
- [ ] SSH Agent 转发
- [x] SSH 保活 (keepalive) 心跳 — 每 15 秒发送 keepalive 包

### 用户体验
- [x] **分屏视图** (水平/垂直 Cmd+D / Cmd+Shift+D)
- [x] **分屏关闭** (悬停 × 按钮 / 右键菜单)
- [x] **标签页拖拽排序** — 标签栏内拖拽重排（平滑缓动动画，区分排序/合并模式），拖出分离窗口
- [ ] 连接历史

---

## 📋 长期目标 (v0.7.0+)

### 高级功能
- [ ] 端口转发 (Local/Remote/Dynamic)
- [ ] 隧道管理 UI
- [ ] 脚本自动化
- [ ] 命令片段 / Snippets
- [ ] 跳板机 / Jump Host 支持
- [ ] SSH Config 导入 (~/.ssh/config)
- [ ] 会话恢复
- [ ] 云同步配置

### 平台支持
- [ ] Windows 支持 (ConPTY)
- [ ] Linux 完整支持
- [ ] macOS 优化

---

## 🚧 技术债务与改进点

### 已解决
1. ~~**终端输入体验** — 已使用 allocate_painter + Event::Key/Ime 直接处理~~
2. ~~**PTY 读取效率** — 已使用后台线程 + Arc<Mutex<TerminalGrid>>~~
3. ~~**ANSI 解析不完整** — 已替换为 vte 0.11 完整解析~~
4. ~~**延迟换行缺失** — 已实现 wrap_pending，修复 zsh PROMPT_SP~~
5. ~~**IME 兼容性** — 已通过 Event::Ime + key_to_char 双通道处理~~
6. ~~**缺少滚动缓冲区** — 已实现 scrollback + 鼠标滚轮翻阅~~
7. ~~**CJK 宽字符** — 已通过 unicode-width + wide_continuation 双格占位 + CJK 字体回退解决~~
8. ~~**侧边栏占用终端空间** — 已重构为导航栏 + 独立 Hosts 页面~~
9. ~~**编译 warning** — 已清理全部 dead_code (无用方法、字段、枚举变体、结构体)~~
10. ~~**终端内容贴边** — 已添加 pad_x/pad_y 内边距, 光标和文字与边缘保持间距~~
11. ~~**导航栏选中效果窄** — 已改为全宽行高亮 + 左侧 accent bar, 自定义绘制替代 Button~~
12. ~~**分屏无法关闭单个 pane** — 已实现 PaneNode::remove + decrement_indices_above, 支持关闭任意 pane~~
13. ~~**终端多行选择限制** — 已修复拖动选择只能选中当前屏内容的问题, 现在支持跨 scrollback 和活动 grid 的完整选择~~

### 当前问题
1. **终端渲染性能** — 每帧重建 LayoutJob
   - 可考虑增量更新 + 脏行标记
   - 或使用 egui Painter 直接绘制字符
2. **CJK 字符对齐** — 比例字体回退下宽字符可能与等宽网格微有偏差


### 未来考虑
- **高亮**: syntect (已集成)
- **打包**: cargo-bundle (macOS .app) + hdiutil (.dmg)

### 已集成
- **密码安全存储**: keyring 3 + apple-native (macOS Keychain, per-host service name)

---

## 📊 开发优先级

### P0 (最高优先级)
1. 搜索终端输出
2. SSH 保活心跳
3. 快捷键自定义

### P1 (高优先级)
1. SSH Agent 转发
2. 标签页拖拽排序
3. 连接历史

### P2 (中优先级)
1. 端口转发
2. 命令片段 / Snippets
3. 跳板机支持

### P3 (低优先级)
1. 隧道管理 UI
2. 脚本自动化
3. 云同步配置

---

## 📖 参考资料

- [Ghostty](https://github.com/mitchellh/ghostty) - 现代终端模拟器参考
- [wezterm](https://github.com/wez/wezterm) - 功能丰富的终端
- [kitty](https://github.com/kovidgoyal/kitty) - 高性能终端 GPU 加速
- [alacritty](https://github.com/alacritty/alacritty) - 最小化终端

---

**版本历史**: 请查看 `git tag` 获取各版本详情
**维护者**: Portal Team
