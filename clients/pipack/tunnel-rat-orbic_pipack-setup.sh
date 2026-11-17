#!/usr/bin/env bash
# Tunnel R.A.T. Orbic — PiPack setup. Run on the PiPack, next to
# tunnel-rat-orbic_pipack-link.py:
#
#   sudo bash tunnel-rat-orbic_pipack-setup.sh
#
# What it does:
#   1. Installs the link service (tunnel-rat-orbic-link) and the `tunnel-rat-orbic` command.
#   2. Sets up the USB-C backup link: the Orbic's USB tether shows up as `orbic0` and is
#      never allowed to become the PiPack's internet route (the Orbic has no data).
#   3. Pairs with the Orbic over USB-C: you type the 6-character code from its screen.
#   4. Hands the Orbic the PiPack hotspot's Wi-Fi details, so the Orbic joins the PiPack
#      network and the API-keyed link runs over Wi-Fi, with USB-C as the backup.
#   5. Hooks your `wardrive` command (after asking) so every drive starts a fresh Orbic
#      recording; the service saves each finished recording to ~/wardrive.
#   6. Starts the service. The rat on the Orbic turns green once the link is up.
#
# Safe to run again: it skips pairing if already paired, and re-sends the Wi-Fi details.
set -Eeuo pipefail

if [ "$(id -u)" -ne 0 ]; then exec sudo -E bash "$0" "$@"; fi

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LINK_SRC="$HERE/tunnel-rat-orbic_pipack-link.py"
INSTALL_DIR=/opt/tunnel-rat-orbic
LINK_PY="$INSTALL_DIR/tunnel-rat-orbic_pipack-link.py"
SERVICE=tunnel-rat-orbic-link
ORBIC_USB_IP="${TR_ORBIC_USB_IP:-192.168.1.1}"
PORT=8080

TARGET_USER="${SUDO_USER:-}"
if [ -z "$TARGET_USER" ] || [ "$TARGET_USER" = root ]; then
    TARGET_USER="$(getent passwd dragon >/dev/null && echo dragon || logname 2>/dev/null || echo root)"
fi
TARGET_HOME="$(getent passwd "$TARGET_USER" | cut -d: -f6)"
CONFIG="$TARGET_HOME/.config/tunnel-rat-orbic/link.json"

say()  { printf '\n\033[1;32m[tunnel-rat-orbic]\033[0m %s\n' "$*"; }
warn() { printf '\n\033[1;33m[tunnel-rat-orbic] %s\033[0m\n' "$*"; }
die()  { printf '\n\033[1;31m[tunnel-rat-orbic] %s\033[0m\n' "$*"; exit 1; }
trap 'die "Stopped at line $LINENO."' ERR

as_user() { runuser -u "$TARGET_USER" -- env TR_ORBIC_CONFIG="$CONFIG" "$@"; }
link()    { as_user python3 "$LINK_PY" "$@"; }

[ -f "$LINK_SRC" ] || die "Put tunnel-rat-orbic_pipack-link.py next to this script."
command -v python3 >/dev/null || die "python3 is missing (sudo apt install python3)."
say "Setting up the Tunnel R.A.T. Orbic link for user $TARGET_USER on $(hostname)"

# ------------------------------------------------------------------ 1. install
install -d -m 755 "$INSTALL_DIR"
install -m 755 "$LINK_SRC" "$LINK_PY"
cat > /usr/local/bin/tunnel-rat-orbic <<EOF
#!/bin/sh
# Tunnel R.A.T. Orbic link: status | start | stop | pull | discover
if [ "\$(id -u)" = 0 ]; then
    exec runuser -u "$TARGET_USER" -- env TR_ORBIC_CONFIG="$CONFIG" python3 "$LINK_PY" "\$@"
fi
exec env TR_ORBIC_CONFIG="$CONFIG" python3 "$LINK_PY" "\$@"
EOF
chmod 755 /usr/local/bin/tunnel-rat-orbic

if [ ! -f "$CONFIG" ]; then
    as_user mkdir -p "$(dirname "$CONFIG")" "$TARGET_HOME/wardrive"
    as_user python3 - "$CONFIG" "$TARGET_HOME/wardrive" "$ORBIC_USB_IP" <<'PY'
import json, os, sys
fd = os.open(sys.argv[1], os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
with os.fdopen(fd, "w") as f:
    cfg = {"device_name": "PiPack", "log_dir": sys.argv[2]}
    if sys.argv[3] != "192.168.1.1":
        cfg["usb_host"] = sys.argv[3]
    json.dump(cfg, f, indent=2)
PY
fi

cat > "/etc/systemd/system/$SERVICE.service" <<EOF
[Unit]
Description=Tunnel R.A.T. Orbic link (PiPack)
After=network-online.target gpsd.service
Wants=network-online.target

[Service]
User=$TARGET_USER
Environment=TR_ORBIC_CONFIG=$CONFIG
ExecStart=/usr/bin/env python3 $LINK_PY run
Restart=always
RestartSec=5
RuntimeDirectory=tunnel-rat-orbic
RuntimeDirectoryMode=0755

[Install]
WantedBy=multi-user.target
EOF
systemctl daemon-reload
say "Installed the link service and the 'tunnel-rat-orbic' command"

# ------------------------------------------------- 2. USB-C backup link (orbic0)
install -d /etc/udev/rules.d
cat > /etc/udev/rules.d/70-tunnel-rat-orbic.rules <<'EOF'
# Tunnel R.A.T. Orbic: give the Orbic's USB tether (Qualcomm 05c6) a fixed name
SUBSYSTEM=="net", ACTION=="add", ATTRS{idVendor}=="05c6", NAME="orbic0"
EOF
udevadm control --reload
modprobe rndis_host 2>/dev/null || true

NETSTACK=none
if systemctl is-active --quiet NetworkManager; then
    NETSTACK=NetworkManager
    install -d -m 700 /etc/NetworkManager/system-connections
    cat > /etc/NetworkManager/system-connections/tunnel-rat-orbic-usb.nmconnection <<'EOF'
[connection]
id=tunnel-rat-orbic-usb
type=ethernet
interface-name=orbic0
autoconnect=true

[ipv4]
method=auto
never-default=true
ignore-auto-dns=true

[ipv6]
method=ignore
EOF
    chmod 600 /etc/NetworkManager/system-connections/tunnel-rat-orbic-usb.nmconnection
    nmcli connection reload
elif systemctl is-active --quiet dhcpcd; then
    NETSTACK=dhcpcd
    if ! grep -q "tunnel-rat-orbic" /etc/dhcpcd.conf; then
        printf '\n# tunnel-rat-orbic: Orbic USB backup link, never the default route\ninterface orbic0\nnogateway\nnooption domain_name_servers\n' >> /etc/dhcpcd.conf
        systemctl restart dhcpcd || true
    fi
elif systemctl is-active --quiet systemd-networkd; then
    NETSTACK=systemd-networkd
    cat > /etc/systemd/network/40-tunnel-rat-orbic.network <<'EOF'
[Match]
Name=orbic0

[Network]
DHCP=ipv4
IPv6AcceptRA=no

[DHCPv4]
UseGateway=false
UseRoutes=false
UseDNS=false
EOF
    networkctl reload || true
else
    warn "Couldn't tell which network manager runs here; check that orbic0 never becomes the default route."
fi
say "USB-C backup link configured ($NETSTACK): the Orbic shows up as orbic0, never as the internet route"

usb_orbic_up() {
    python3 -c 'import sys, urllib.request; urllib.request.urlopen(sys.argv[1], timeout=2)' \
        "http://$ORBIC_USB_IP:$PORT/api/config" >/dev/null 2>&1
}

# Network interfaces that belong to the Orbic (Qualcomm USB vendor 05c6).
orbic_ifaces() {
    local d
    for d in /sys/class/net/*; do
        [ -e "$d/device" ] || continue
        [ "$(cat "$d/device/../idVendor" 2>/dev/null)" = "05c6" ] && basename "$d"
    done
    return 0
}

fix_default_route() {
    local ifc
    for ifc in $(orbic_ifaces); do
        if ip route show default 2>/dev/null | grep -qE "dev $ifc( |$)"; then
            warn "$ifc grabbed a default route; removing it so the PiPack keeps its own internet"
            ip route del default dev "$ifc" 2>/dev/null || true
        fi
    done
}

# An Orbic plugged in before this setup still has its old name: ask for a replug.
need_replug() {
    local ifc i
    for ifc in $(orbic_ifaces); do
        [ "$ifc" = orbic0 ] && continue
        warn "The Orbic is on $ifc. Unplug its USB-C cable and plug it back in so it becomes orbic0..."
        for i in $(seq 1 30); do
            sleep 2
            orbic_ifaces | grep -qx orbic0 && { say "orbic0 is up"; return 0; }
        done
        warn "Still not orbic0; carrying on (the backup link works either way)."
        fix_default_route
        return 0
    done
}

wait_for_usb_orbic() {
    local i
    for i in $(seq 1 45); do
        need_replug
        if usb_orbic_up; then fix_default_route; return 0; fi
        sleep 2
    done
    return 1
}

# ----------------------------------------------------------------- 3. pairing
ORBIC_HOST=""
if as_user python3 -c 'import json,sys; sys.exit(0 if json.load(open(sys.argv[1])).get("api_key") else 1)' "$CONFIG"; then
    say "Already paired with the Orbic (delete $CONFIG to pair again)"
else
    say "Plug the Orbic into the PiPack with a USB-C data cable (waiting up to 90 s)..."
    if wait_for_usb_orbic; then
        ORBIC_HOST="$ORBIC_USB_IP"
        say "Orbic found over USB-C ($(ip -br addr show orbic0 2>/dev/null | awk '{print $1, $3}'))"
    else
        warn "No Orbic over USB-C."
        if lsusb 2>/dev/null | grep -qi "05c6:"; then
            echo "  The Orbic is plugged in, but its USB tethering is off."
            echo "  Turn on USB tethering in the Orbic's admin page (http://192.168.1.1), then run this again."
        else
            echo "  The PiPack doesn't see the Orbic on USB at all. Try another cable (some are charge-only) or port."
        fi
        echo "  Or type the Orbic's IP address if the PiPack can reach it another way (Enter to stop):"
        read -r -p "  Orbic IP: " ORBIC_HOST </dev/tty || true
        [ -n "$ORBIC_HOST" ] || die "Stopped. Run this again once the Orbic is connected."
    fi

    echo
    echo "The Orbic's screen shows a 6-character PAIR CODE under the yellow rat."
    tries=0
    until [ $tries -ge 5 ]; do
        read -r -p "Pairing code: " CODE </dev/tty
        if link pair --code "$CODE" --host "$ORBIC_HOST"; then break; fi
        tries=$((tries + 1))
    done
    [ $tries -lt 5 ] || die "Pairing failed 5 times; the Orbic now shows a new code. Run this again."
fi

# ---------------------------------------------------- 4. hand over the PiPack Wi-Fi
AP_SSID=""; AP_PASS=""; AP_SEC="wpa_psk"
if [ -f /etc/hostapd/hostapd.conf ]; then
    AP_SSID="$(sed -n 's/^ssid=//p' /etc/hostapd/hostapd.conf | head -1)"
    AP_PASS="$(sed -n 's/^wpa_passphrase=//p' /etc/hostapd/hostapd.conf | head -1)"
    keymgmt="$(sed -n 's/^wpa_key_mgmt=//p' /etc/hostapd/hostapd.conf | head -1)"
    if [ -n "$keymgmt" ] && ! grep -q "WPA-PSK" <<<"$keymgmt" && grep -q "SAE" <<<"$keymgmt"; then AP_SEC=sae; fi
fi
if [ -z "$AP_SSID" ] && command -v nmcli >/dev/null; then
    while IFS=: read -r name type; do
        [ "$type" = "802-11-wireless" ] || continue
        if [ "$(nmcli -g 802-11-wireless.mode connection show "$name" 2>/dev/null)" = "ap" ]; then
            AP_SSID="$(nmcli -g 802-11-wireless.ssid connection show "$name")"
            AP_PASS="$(nmcli -s -g 802-11-wireless-security.psk connection show "$name")"
            break
        fi
    done < <(nmcli -t -f NAME,TYPE connection show --active)
fi

if [ -z "$ORBIC_HOST" ]; then
    if usb_orbic_up; then ORBIC_HOST="$ORBIC_USB_IP"
    else
        say "Plug the Orbic into the PiPack with USB-C to send it the Wi-Fi details (waiting up to 90 s)..."
        wait_for_usb_orbic && ORBIC_HOST="$ORBIC_USB_IP" || true
    fi
fi

if [ -z "$ORBIC_HOST" ]; then
    warn "Skipping the Wi-Fi handover: no Orbic over USB-C. The link will use whatever path it can find."
else
    if [ -z "$AP_SSID" ]; then
        warn "Couldn't read the PiPack hotspot settings."
        read -r -p "PiPack hotspot name (SSID): " AP_SSID </dev/tty
        read -r -s -p "PiPack hotspot password: " AP_PASS </dev/tty; echo
    fi
    read -r -p "Have the Orbic join the PiPack's Wi-Fi \"$AP_SSID\"? [Y/n] " ans </dev/tty || ans=y
    case "${ans:-y}" in
        [Nn]*) say "Skipped; the link will run over USB-C only." ;;
        *) if ! printf '%s\n' "$AP_PASS" | link handover --password-stdin \
                    --ssid "$AP_SSID" --security "$AP_SEC" --host "$ORBIC_HOST"; then
               warn "The Orbic didn't join \"$AP_SSID\"; the USB-C link still works."
           fi ;;
    esac
fi
AP_PASS=""

# --------------------------------------------- 5. wardrive hook (fresh recording per drive)
HOOK_MARK="tunnel-rat-orbic: fresh Orbic recording for each wardrive"
HOOK_LINE='( command -v tunnel-rat-orbic >/dev/null 2>&1 && timeout 20 tunnel-rat-orbic start >/dev/null 2>&1 ) &'

is_shell_script() {
    local first
    [ -f "$1" ] && [ -r "$1" ] || return 1
    first="$(head -n1 "$1")"
    case "$first" in
        "#!"*python*|"#!"*perl*|"#!"*node*) return 1 ;;
        "#!"*sh*) return 0 ;;
    esac
    case "$1" in *.sh) return 0 ;; esac
    return 1
}

# Where does the user's `wardrive` command live? Prints a script path, or nothing.
find_wardrive() {
    local p rc tok
    p="$(runuser -u "$TARGET_USER" -- bash -lc 'command -v wardrive' 2>/dev/null || true)"
    if [ -n "$p" ] && [ -f "$p" ]; then echo "$p"; return; fi
    for rc in "$TARGET_HOME/.bash_aliases" "$TARGET_HOME/.bashrc" "$TARGET_HOME/.zshrc" "$TARGET_HOME/.profile"; do
        [ -f "$rc" ] || continue
        p="$(sed -n "s/^[[:space:]]*alias[[:space:]]\{1,\}wardrive=['\"]\{0,1\}\([^'\"]*\).*/\1/p" "$rc" | head -1)"
        [ -n "$p" ] || continue
        for tok in $p; do
            case "$tok" in sudo|-E|bash|sh|zsh|exec|env) continue ;; esac
            tok="${tok/#\~/$TARGET_HOME}"; tok="${tok/#\$HOME/$TARGET_HOME}"
            [ -f "$tok" ] && { echo "$tok"; return; }
            break
        done
    done
    for p in "$TARGET_HOME/wardrive" "$TARGET_HOME/wardrive.sh" "$TARGET_HOME/bin/wardrive" \
             "$TARGET_HOME/bin/wardrive.sh" "$TARGET_HOME/.local/bin/wardrive" \
             /usr/local/bin/wardrive /usr/local/bin/wardrive.sh; do
        [ -f "$p" ] && { echo "$p"; return; }
    done
}

hook_manual() {
    warn "$1"
    echo "  Add these two lines near the top of your wardrive command yourself:"
    echo "    # $HOOK_MARK"
    echo "    $HOOK_LINE"
}

WARDRIVE="$(find_wardrive)"
if [ -z "$WARDRIVE" ]; then
    hook_manual "Couldn't find your 'wardrive' command."
elif grep -q "tunnel-rat-orbic" "$WARDRIVE"; then
    say "Your wardrive command ($WARDRIVE) already starts a fresh Orbic recording"
elif ! is_shell_script "$WARDRIVE"; then
    hook_manual "$WARDRIVE isn't a shell script, so I won't edit it."
else
    say "Found your wardrive command: $WARDRIVE"
    echo "  To give every wardrive its own Orbic recording, I'll add these lines near the top"
    echo "  (it runs in the background and can't slow down or break the wardrive):"
    echo "    # $HOOK_MARK"
    echo "    $HOOK_LINE"
    echo "  A backup goes to $WARDRIVE.bak-tunnel-rat-orbic"
    read -r -p "Add it? [Y/n] " ans </dev/tty || ans=n
    case "${ans:-y}" in
        [Nn]*) say "Left your wardrive command alone." ;;
        *)
            cp -p "$WARDRIVE" "$WARDRIVE.bak-tunnel-rat-orbic"
            if head -n1 "$WARDRIVE" | grep -q '^#!'; then
                sed -i "1a # $HOOK_MARK\n$HOOK_LINE" "$WARDRIVE"
            else
                sed -i "1i # $HOOK_MARK\n$HOOK_LINE" "$WARDRIVE"
            fi
            if bash -n "$WARDRIVE"; then
                say "Done: every wardrive now starts a fresh Orbic recording; the last one is saved to ~/wardrive"
            else
                cp -p "$WARDRIVE.bak-tunnel-rat-orbic" "$WARDRIVE"
                hook_manual "That edit broke the script's syntax, so I put the original back."
            fi
            ;;
    esac
fi

# ------------------------------------------------------------------- 6. start
systemctl enable --now "$SERVICE" >/dev/null 2>&1 || systemctl enable --now "$SERVICE"
systemctl restart "$SERVICE"
systemctl is-active --quiet gpsd || warn "gpsd isn't running, so the Orbic won't get GPS from the PiPack until it is."
say "Waiting for the link..."
sleep 8
fix_default_route
tunnel-rat-orbic status || true

say "Done. The rat on the Orbic should be green."
echo "  Live log:        journalctl -fu $SERVICE"
echo "  Cells + alerts:  $TARGET_HOME/wardrive/PiPack-orbic-YYYYMMDD.csv"
echo "  Recordings:      $TARGET_HOME/wardrive/PiPack-orbic-<name>.zip (saved when the next drive starts)"
echo "  Commands:        tunnel-rat-orbic status | start | stop | pull | discover"
