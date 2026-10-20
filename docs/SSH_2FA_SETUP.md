# 服务器端开启 SSH 2FA（TOTP / Google Authenticator）

Portal 客户端支持键盘交互式（keyboard-interactive，RFC 4256）2FA：密码或密钥认证
失败、且服务器通告 `KeyboardInteractive` 时，会自动弹出 OTP 输入窗。本指南说明如何在
**SSH 服务器**上开启对应的 TOTP 双因素认证，以便配合 Portal 使用。

配套脚本位于 `scripts/`：

- `scripts/enable-2fa.sh` — 一键开启（安装、初始化、改 sshd/PAM、重启）
- `scripts/rollback-2fa.sh` — 回滚（恢复 sshd/PAM 到备份）

---

## 一键开启

```bash
sudo bash scripts/enable-2fa.sh youruser                    # 默认时区 Asia/Shanghai
sudo bash scripts/enable-2fa.sh youruser Asia/Tokyo         # 指定时区
```

脚本已适配主流发行版的包管理器与服务管理：

| 发行版 | 包管理器 | 包名 | 服务管理 |
|---|---|---|---|
| Debian/Ubuntu/Devuan | apt-get | `libpam-google-authenticator` | systemd / SysV |
| Fedora/RHEL/CentOS/Rocky/Alma | dnf / yum | `google-authenticator` | systemd（SELinux 自动 restorecon） |
| openSUSE/SLES | zypper | `google-authenticator-libpam` | systemd |
| Alpine | apk | `google-authenticator` | OpenRC |
| Arch | pacman（AUR） | `libpam-google-authenticator` | 需先手动 `yay/paru -S` 后重跑 |

非 systemd 系统脚本会用 `service`/`rc-service` 兜底重启 sshd；无 `sudo` 时以
`su -s /bin/sh` 回退。TOTP 校验基于 UTC 时间戳，时区只影响展示；关键的是时钟
同步（systemd 用 `timedatectl set-ntp true`，非 systemd 尽力启动 chrony/ntpd）。

脚本会依次：设置时区并启用 NTP → 安装 `libpam-google-authenticator` → 为目标用户
生成 TOTP（非交互，立即扫码 stdout 里的 QR/secret）→ 修改 sshd 为
`AuthenticationMethods keyboard-interactive` → 在 PAM 追加 OTP 轮 → `sshd -t`
校验 → 重启 sshd。

> **执行前**：保留一个已打开的 SSH 会话作为后备（配错会被锁在门外），或确保有
> console 访问。

### 默认策略：A — 单一 keyboard-interactive（密码 + OTP 两轮）

默认配置最匹配 Portal 客户端的 2FA 弹窗，连接时会依次弹出「密码」和「验证码」两次
输入窗：

```sshd_config
KbdInteractiveAuthentication yes
PasswordAuthentication no
PubkeyAuthentication no
AuthenticationMethods keyboard-interactive
```

PAM 中 `pam_unix`（密码轮）在前、`pam_google_authenticator`（OTP 轮）在后。

---

## 切换双因素策略

| 组合 | `AuthenticationMethods` | Portal 行为 |
|---|---|---|
| A. 密码+OTP（默认） | `keyboard-interactive` | 弹两轮窗（密码 → OTP） |
| B. 密钥+OTP | `publickey,keyboard-interactive` | 密钥通过 + `partial_success` → 弹 OTP 窗 |
| C. 密码+OTP（分步） | `password,keyboard-interactive` | 密码通过 + `partial_success` → 弹 OTP 窗 |

Portal 三种都能回退（russh 将 `partial_success` 编码进 `Failure.remaining_methods`，
客户端据此回退，不只在“完全失败”时才弹窗）。

例如切换为 B（密钥+OTP）：

```bash
sed -i -E 's|^#?PubkeyAuthentication .*|PubkeyAuthentication yes|' /etc/ssh/sshd_config
sed -i -E 's|^AuthenticationMethods .*|AuthenticationMethods publickey,keyboard-interactive|' /etc/ssh/sshd_config
sshd -t && systemctl restart ssh
```

---

## 时区与时间同步

TOTP 算法基于 Unix 时间戳（UTC），**时区不影响验证结果**——手机和服务器只要时钟
准确就能对上码。脚本里真正保证 2FA 稳定工作的是 `timedatectl set-ntp true`；设置
时区（默认 `Asia/Shanghai`）只是让服务器本地时间显示为北京时间、看日志不混乱。

若时钟与手机偏差超过 ±90 秒（`-w 3` 容忍窗口），即使时区正确也会报“验证码错误”，
此时先同步：

```bash
timedatectl set-ntp true
timedatectl timesync-status
```

---

## 回滚

脚本执行时已备份 `sshd_config.bak.<ts>` 与 `sshd.bak.<ts>`：

```bash
sudo bash scripts/rollback-2fa.sh                 # 回滚到最新备份
sudo bash scripts/rollback-2fa.sh 20260916120000  # 回滚到指定时间戳

# 列出所有可用备份
ls -1 /etc/ssh/sshd_config.bak.*
```

回滚只恢复 sshd 与 PAM 配置；**不回滚时区**（无害）、**不删除** `.google_authenticator`。
若要彻底清除 2FA 痕迹，回滚后再删：

```bash
sudo rm -f /home/youruser/.google_authenticator
```

---

## 常见问题

- **OpenSSH 8.8+**：用 `KbdInteractiveAuthentication`；`ChallengeResponseAuthentication`
  已废弃，脚本为兼容旧版同时写入两者。
- **`.google_authenticator` 权限**：必须是 `0600` 且属主正确，否则 PAM 报
  `Authentication token manipulation error`。
- **首次生成即唯一打印时机**：`google-authenticator -f` 会覆盖旧 secret，没扫到就重跑。
