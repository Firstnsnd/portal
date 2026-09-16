#!/usr/bin/env bash
# 回滚 enable-2fa.sh 所做的 sshd/PAM 改动。
#
# 用法:
#   sudo bash rollback-2fa.sh [时间戳]   # 省略则回滚到最新备份
#
# 说明:
#   - 只恢复 /etc/ssh/sshd_config 和 /etc/pam.d/sshd；不回滚时区、不删除
#     用户的 .google_authenticator（详见 docs/SSH_2FA_SETUP.md）。
set -euo pipefail

list_ts() {
  ls -1 /etc/ssh/sshd_config.bak.* 2>/dev/null | sed 's/.*\.bak\.//' | sort -n
}

TS="${1:-$(list_ts | tail -1)}"
[[ -n "$TS" ]] || { echo "没有找到备份 /etc/ssh/sshd_config.bak.*"; exit 1; }

SSHD_BAK="/etc/ssh/sshd_config.bak.$TS"
PAM_BAK="/etc/pam.d/sshd.bak.$TS"
[[ -f "$SSHD_BAK" ]] || { echo "缺少 $SSHD_BAK"; exit 1; }
[[ -f "$PAM_BAK"  ]] || { echo "缺少 $PAM_BAK"; exit 1; }

cp -a "$SSHD_BAK" /etc/ssh/sshd_config
cp -a "$PAM_BAK"  /etc/pam.d/sshd

sshd -t

# 重启（systemd → OpenRC → SysV 依次兜底，与 enable-2fa.sh 一致）
if command -v systemctl >/dev/null 2>&1; then
  systemctl restart ssh 2>/dev/null || systemctl restart sshd 2>/dev/null \
    || echo ">>> 警告: systemctl 重启失败，请手动重启 sshd"
elif command -v rc-service >/dev/null 2>&1; then
  rc-service sshd restart 2>/dev/null || rc-service ssh restart 2>/dev/null \
    || echo ">>> 警告: OpenRC 重启失败，请手动重启 sshd"
elif command -v service >/dev/null 2>&1; then
  service ssh restart 2>/dev/null || service sshd restart 2>/dev/null \
    || echo ">>> 警告: service 重启失败，请手动重启 sshd"
else
  echo ">>> 警告: 未找到服务管理命令，请手动重启 sshd 使配置生效"
fi

echo ">>> 已回滚到备份 $TS（sshd_config + pam.d/sshd）"
