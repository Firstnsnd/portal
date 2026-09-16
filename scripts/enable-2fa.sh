#!/usr/bin/env bash
# 在 SSH 服务器上开启 TOTP 2FA（Google Authenticator）。
#
# 用法:
#   sudo bash enable-2fa.sh [登录用户名] [时区]
#   默认: 用户=$SUDO_USER, 时区=Asia/Shanghai
#
# 兼容发行版:
#   Debian/Ubuntu、Fedora/RHEL/CentOS/Rocky/Alma、openSUSE/SLES、Alpine。
#   Arch 的 libpam-google-authenticator 在 AUR，脚本无法以 root 自动安装，
#   会提示用户先手动安装后重跑（已安装则自动跳过安装步骤）。
#
# 说明:
#   - TOTP 校验基于 UTC 时间戳，时区只影响本地时间展示；真正决定能否配对
#     的是系统时钟准确（NTP 同步）。
#   - 会修改 /etc/ssh/sshd_config 与 /etc/pam.d/sshd，执行前备份
#     (sshd_config.bak.<ts> / sshd.bak.<ts>)，可用 rollback-2fa.sh 回滚。
#   - 执行前保留一个已打开的 SSH 会话作为后备，避免配错被锁在门外。
set -euo pipefail

TARGET_USER="${1:-$SUDO_USER}"
TIMEZONE="${2:-Asia/Shanghai}"
[[ -n "$TARGET_USER" ]] || { echo "用法: $0 <用户名> [时区]"; exit 1; }

need() { command -v "$1" >/dev/null 2>&1; }

# ── 1) 时区 + NTP 同步 ──────────────────────────────────────────────
# TOTP 用 UTC 时间戳；时区只影响展示，NTP 才是配对关键。
if need timedatectl; then
  timedatectl set-timezone "$TIMEZONE" 2>/dev/null \
    || ln -sf "/usr/share/zoneinfo/$TIMEZONE" /etc/localtime
  timedatectl set-ntp true 2>/dev/null \
    || echo ">>> 警告: 无法启用 systemd NTP（容器内常见），请确保时钟准确"
else
  # 非 systemd（Alpine/Devuan/容器）：手动设置时区，尽力启动 chrony/ntpd。
  ln -sf "/usr/share/zoneinfo/$TIMEZONE" /etc/localtime
  if need rc-service; then
    rc-service chronyd start 2>/dev/null || rc-service ntpd start 2>/dev/null || true
  elif need service; then
    service chronyd start 2>/dev/null || service ntpd start 2>/dev/null || true
  fi
  echo ">>> 提示: 非 systemd 系统请确认 chrony/ntpd 已运行以保证时钟准确"
fi

# ── 2) 备份 ─────────────────────────────────────────────────────────
TS=$(date +%Y%m%d%H%M%S)
cp -a /etc/ssh/sshd_config "/etc/ssh/sshd_config.bak.$TS"
cp -a /etc/pam.d/sshd      "/etc/pam.d/sshd.bak.$TS"

# ── 3) 安装 PAM 模块 ────────────────────────────────────────────────
if need google-authenticator; then
  echo ">>> google-authenticator 已安装，跳过"
elif need apt-get; then
  apt-get update -y
  DEBIAN_FRONTEND=noninteractive apt-get install -y libpam-google-authenticator
elif need dnf; then
  dnf install -y google-authenticator
elif need yum; then
  yum install -y google-authenticator
elif need zypper; then
  zypper --non-interactive install google-authenticator-libpam
elif need apk; then
  apk add --no-cache google-authenticator
elif need pacman; then
  echo ">>> Arch: libpam-google-authenticator 在 AUR，无法以 root 自动安装"
  echo "    请以普通用户执行: yay -S libpam-google-authenticator   (或 paru/pamac)"
  echo "    装好后重跑本脚本（将检测到已安装并跳过此步）"
  exit 1
else
  echo "未识别的包管理器"; exit 1
fi

# ── 4) 为目标用户生成 TOTP（非交互；立刻扫 stdout 里的 QR/secret）──
echo ">>> 为用户 $TARGET_USER 生成 TOTP，请立刻扫码："
if need sudo; then
  sudo -u "$TARGET_USER" google-authenticator -t -d -f -r 3 -R 30 -w 3
else
  su -s /bin/sh "$TARGET_USER" -c 'google-authenticator -t -d -f -r 3 -R 30 -w 3'
fi

# ── 5) SELinux 上下文修正（RHEL 系）────────────────────────────────
if need selinuxenabled && selinuxenabled 2>/dev/null; then
  restorecon -Rv "$(eval echo ~$TARGET_USER)/.google_authenticator" 2>/dev/null || true
fi

# ── 6) sshd 配置（键存在则替换、不存在则追加）───────────────────────
set_cfg() {
  local k="$1" v="$2" f=/etc/ssh/sshd_config
  if grep -qE "^#?[[:space:]]*${k}[[:space:]]" "$f"; then
    sed -i -E "s|^#?[[:space:]]*${k}[[:space:]].*|${k} ${v}|" "$f"
  else
    echo "${k} ${v}" >> "$f"
  fi
}
set_cfg KbdInteractiveAuthentication yes
set_cfg ChallengeResponseAuthentication yes   # 兼容 OpenSSH < 8.8
set_cfg PasswordAuthentication no
set_cfg PubkeyAuthentication no
set_cfg AuthenticationMethods keyboard-interactive

# ── 7) PAM：在密码轮(common-auth)之后追加 OTP 轮 ───────────────────
grep -q 'pam_google_authenticator.so' /etc/pam.d/sshd \
  || echo 'auth required pam_google_authenticator.so' >> /etc/pam.d/sshd

# ── 8) 语法校验（失败则不重启）─────────────────────────────────────
sshd -t

# ── 9) 重启（systemd → OpenRC → SysV 依次兜底）─────────────────────
if need systemctl; then
  systemctl restart ssh 2>/dev/null || systemctl restart sshd 2>/dev/null \
    || echo ">>> 警告: systemctl 重启失败，请手动重启 sshd"
elif need rc-service; then
  rc-service sshd restart 2>/dev/null || rc-service ssh restart 2>/dev/null \
    || echo ">>> 警告: OpenRC 重启失败，请手动重启 sshd"
elif need service; then
  service ssh restart 2>/dev/null || service sshd restart 2>/dev/null \
    || echo ">>> 警告: service 重启失败，请手动重启 sshd"
else
  echo ">>> 警告: 未找到服务管理命令，请手动重启 sshd 使配置生效"
fi

echo ">>> 完成。时区=$(timedatectl show -p Timezone --value 2>/dev/null || echo "$TIMEZONE")"
echo ">>> secret 文件: $(eval echo ~$TARGET_USER)/.google_authenticator"
echo ">>> 回滚: sudo bash scripts/rollback-2fa.sh $TS"
