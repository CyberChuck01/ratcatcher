#!/usr/bin/env python3
"""Tunnel R.A.T. Orbic <-> PiPack link.

Keeps the PiPack connected to the Tunnel R.A.T. Orbic firmware over its
API-keyed live link (WebSocket /api/tunnel-rat/link):

  * Wi-Fi first: the Orbic joins the PiPack's own hotspot (Rayhunter's Wi-Fi
    client mode), so the link survives the Pandas going into monitor mode.
  * USB-C as backup: the Orbic's USB tether (192.168.1.1 on orbic0). When
    Wi-Fi comes back the link moves over without dropping (make-before-break),
    so the rat stays green.
  * Sends the PiPack's gpsd fix to the Orbic so its recordings are geotagged.
  * Logs every new cell and stingray alert, with the PiPack's GPS position,
    to ~/wardrive/PiPack-orbic-YYYYMMDD.csv, next to the Kismet logs.
  * Downloads each finished Orbic recording to ~/wardrive (PiPack-orbic-<name>.zip);
    `tunnel-rat-orbic start` in the wardrive command gives every drive its own.
  * Writes live status to /run/tunnel-rat-orbic/status.json for dashboards.

Standard library only. Commands:
  run                     the service (systemd runs this)
  status                  link, Wi-Fi and recording status
  start | stop            start a new Orbic recording (closes the current one) / stop
  pull                    download finished recordings to ~/wardrive
  discover                find the Orbic on the PiPack network or USB
  pair --code XXXXXX      one-time pairing (setup does this)
  handover --ssid NAME    have the Orbic join a Wi-Fi network (setup does this;
                          password via --password-stdin or TR_WIFI_PASSWORD)
"""

import argparse
import base64
import concurrent.futures
import csv
import datetime as dt
import hashlib
import ipaddress
import json
import logging
import os
import select
import socket
import struct
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request

DEVICE = "PiPack"
CONFIG_PATH = os.path.expanduser(
    os.environ.get("TR_ORBIC_CONFIG", "~/.config/tunnel-rat-orbic/link.json"))
STATUS_DIR = os.environ.get("TR_ORBIC_STATUS_DIR", "/run/tunnel-rat-orbic")
USB_HOST = "192.168.1.1"
PORT = 8080
DEFAULTS = {
    "api_key": "",
    "device_name": DEVICE,
    "wifi_host": "",          # the Orbic's IP on the PiPack hotspot, learned at setup
    "usb_host": USB_HOST,
    "port": PORT,
    "gpsd": "127.0.0.1:2947",
    "log_dir": "~/wardrive",
    "ap_interface": "wlan0",  # the PiPack hotspot interface, for discovery
    "alert_command": "",      # optional: run on each stingray alert, e.g. a ntfy curl
    "auto_pull": True,        # download each finished Orbic recording to log_dir
}
PING_EVERY = 5         # seconds; the Orbic drops a link that's quiet for 20 s
GPS_EVERY = 2
GPS_FRESH = 5          # don't send fixes older than this
DEAD_AFTER = 25        # the Orbic sends status every 2 s
WIFI_RETRY = int(os.environ.get("TR_WIFI_RETRY", "30"))  # while on USB, try Wi-Fi again this often
WS_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"

log = logging.getLogger("tunnel-rat-orbic")


# --------------------------------------------------------------------- config

def load_config():
    cfg = dict(DEFAULTS)
    try:
        with open(CONFIG_PATH) as f:
            cfg.update(json.load(f))
    except FileNotFoundError:
        pass
    return cfg


def save_config(cfg):
    os.makedirs(os.path.dirname(CONFIG_PATH), exist_ok=True)
    tmp = CONFIG_PATH + ".tmp"
    fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w") as f:
        json.dump(cfg, f, indent=2)
    os.replace(tmp, CONFIG_PATH)


# ----------------------------------------------------------------- HTTP helpers

def http(host, port, method, path, body=None, timeout=4):
    """Returns (status, parsed JSON or text). Raises OSError if unreachable."""
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(f"http://{host}:{port}{path}", data=data, method=method)
    if data is not None:
        req.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            status, raw = r.status, r.read()
    except urllib.error.HTTPError as e:
        status, raw = e.code, e.read()
    text = raw.decode(errors="replace")
    try:
        return status, json.loads(text)
    except ValueError:
        return status, text


def is_orbic(host, port=PORT, timeout=1.5):
    """True if host runs Rayhunter (it answers /api/config with a device field)."""
    try:
        status, cfg = http(host, port, "GET", "/api/config", timeout=timeout)
    except OSError:
        return False
    return status == 200 and isinstance(cfg, dict) and "device" in cfg and "ui_level" in cfg


def tcp_open(host, port, timeout=1.0):
    try:
        with socket.create_connection((host, port), timeout=timeout):
            return True
    except OSError:
        return False


# ------------------------------------------------------- minimal WebSocket client

class AuthError(Exception):
    pass


class LinkClosed(Exception):
    pass


class WebSocket:
    """Just enough RFC 6455 for the Orbic link: text frames, ping/pong, close."""

    def __init__(self, host, port, path, api_key, timeout=4):
        self.sock = socket.create_connection((host, port), timeout=timeout)
        self.buf = b""
        self.lock = threading.Lock()
        key = base64.b64encode(os.urandom(16)).decode()
        req = (f"GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\n"
               "Upgrade: websocket\r\nConnection: Upgrade\r\n"
               f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n"
               f"Authorization: Bearer {api_key}\r\n\r\n")
        self.sock.sendall(req.encode())
        while b"\r\n\r\n" not in self.buf:
            chunk = self.sock.recv(4096)
            if not chunk:
                raise LinkClosed("closed during handshake")
            self.buf += chunk
            if len(self.buf) > 65536:
                raise LinkClosed("handshake too large")
        head, _, self.buf = self.buf.partition(b"\r\n\r\n")
        lines = head.decode(errors="replace").split("\r\n")
        code = int(lines[0].split()[1])
        if code == 401:
            self.close()
            raise AuthError("the Orbic rejected the API key (not paired with this PiPack?)")
        if code == 404:
            self.close()
            raise LinkClosed("this Orbic firmware has no live link yet (update it)")
        if code != 101:
            self.close()
            raise LinkClosed(f"handshake failed: {lines[0]}")
        headers = {k.strip().lower(): v.strip() for k, _, v in
                   (line.partition(":") for line in lines[1:])}
        want = base64.b64encode(hashlib.sha1((key + WS_GUID).encode()).digest()).decode()
        if headers.get("sec-websocket-accept") != want:
            self.close()
            raise LinkClosed("bad Sec-WebSocket-Accept")
        self.sock.settimeout(10)

    def _send(self, opcode, payload=b""):
        mask = os.urandom(4)
        n = len(payload)
        if n < 126:
            head = struct.pack("!BB", 0x80 | opcode, 0x80 | n)
        elif n < 65536:
            head = struct.pack("!BBH", 0x80 | opcode, 0x80 | 126, n)
        else:
            head = struct.pack("!BBQ", 0x80 | opcode, 0x80 | 127, n)
        masked = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
        with self.lock:
            self.sock.sendall(head + mask + masked)

    def send_json(self, obj):
        self._send(0x1, json.dumps(obj).encode())

    def _read(self, n):
        while len(self.buf) < n:
            chunk = self.sock.recv(65536)
            if not chunk:
                raise LinkClosed("connection closed")
            self.buf += chunk
        out, self.buf = self.buf[:n], self.buf[n:]
        return out

    def recv_json(self, timeout):
        """Next JSON message, or None if nothing arrived within `timeout`."""
        deadline = time.monotonic() + timeout
        message = b""
        while True:
            if not self.buf:
                left = deadline - time.monotonic()
                if left <= 0 and not message:
                    return None
                ready, _, _ = select.select([self.sock], [], [], max(left, 0))
                if not ready and not message:
                    return None
            b0, b1 = self._read(2)
            fin, opcode = b0 & 0x80, b0 & 0x0F
            n = b1 & 0x7F
            if n == 126:
                n = struct.unpack("!H", self._read(2))[0]
            elif n == 127:
                n = struct.unpack("!Q", self._read(8))[0]
            if b1 & 0x80:  # servers don't mask, but be lenient
                mask = self._read(4)
                payload = bytes(b ^ mask[i % 4] for i, b in enumerate(self._read(n)))
            else:
                payload = self._read(n)
            if opcode == 0x8:
                raise LinkClosed("the Orbic closed the link")
            if opcode == 0x9:
                self._send(0xA, payload)
                continue
            if opcode == 0xA:
                continue
            message += payload
            if fin:
                try:
                    return json.loads(message.decode())
                except ValueError:
                    log.warning("ignoring a non-JSON message from the Orbic")
                    message = b""

    def close(self):
        try:
            self._send(0x8, struct.pack("!H", 1000))
        except OSError:
            pass
        try:
            self.sock.close()
        except OSError:
            pass


# ------------------------------------------------------------------------ GPS

class Gps:
    """Follows gpsd in the background and keeps the latest 2D/3D fix."""

    def __init__(self, where):
        host, _, port = where.rpartition(":")
        self.addr = (host or "127.0.0.1", int(port or 2947))
        self.fix = None  # (lat, lon, monotonic time)
        threading.Thread(target=self._loop, daemon=True, name="gpsd").start()

    def current(self):
        fix = self.fix
        if fix and time.monotonic() - fix[2] <= GPS_FRESH:
            return fix[0], fix[1]
        return None

    def _loop(self):
        while True:
            try:
                with socket.create_connection(self.addr, timeout=10) as s:
                    s.sendall(b'?WATCH={"enable":true,"json":true}\n')
                    f = s.makefile("r", encoding="utf-8", errors="replace")
                    for line in f:
                        try:
                            msg = json.loads(line)
                        except ValueError:
                            continue
                        if msg.get("class") == "TPV" and msg.get("mode", 0) >= 2 \
                                and "lat" in msg and "lon" in msg:
                            self.fix = (msg["lat"], msg["lon"], time.monotonic())
            except OSError:
                pass
            time.sleep(5)


# -------------------------------------------------------------------- logging

class EventLog:
    FIELDS = ["time_utc", "event", "earfcn", "pci", "severity", "stingrays",
              "lat", "lon", "via"]

    def __init__(self, log_dir):
        self.dir = os.path.expanduser(log_dir)
        os.makedirs(self.dir, exist_ok=True)

    def write(self, event, via="", gps=None, **fields):
        now = dt.datetime.now(dt.timezone.utc)
        path = os.path.join(self.dir, f"{DEVICE}-orbic-{now:%Y%m%d}.csv")
        new = not os.path.exists(path)
        row = {"time_utc": now.strftime("%Y-%m-%dT%H:%M:%SZ"), "event": event, "via": via}
        if gps:
            row["lat"], row["lon"] = f"{gps[0]:.6f}", f"{gps[1]:.6f}"
        row.update(fields)
        with open(path, "a", newline="") as f:
            w = csv.DictWriter(f, fieldnames=self.FIELDS, extrasaction="ignore")
            if new:
                w.writeheader()
            w.writerow(row)


class TowerLog:
    """One row per identified cell tower sector, for the WiGLE / WDGWars export
    and the Tunnel R.A.T. tower map. Needs Orbic link_version 2 (SIB1 identities)."""
    FIELDS = ["time_utc", "mcc", "mnc", "tac", "eci", "enb", "sector",
              "earfcn", "pci", "lat", "lon", "via"]

    def __init__(self, log_dir):
        self.dir = os.path.expanduser(log_dir)
        os.makedirs(self.dir, exist_ok=True)

    def write(self, plmn, tac, eci, earfcn, pci, via="", gps=None):
        now = dt.datetime.now(dt.timezone.utc)
        path = os.path.join(self.dir, f"{DEVICE}-orbic-towers-{now:%Y%m%d}.csv")
        new = not os.path.exists(path)
        mcc, _, mnc = str(plmn).partition("-")
        row = {"time_utc": now.strftime("%Y-%m-%dT%H:%M:%SZ"), "mcc": mcc, "mnc": mnc,
               "tac": tac, "eci": eci, "enb": eci >> 8, "sector": eci & 0xFF,
               "earfcn": earfcn, "pci": pci, "via": via}
        if gps:
            row["lat"], row["lon"] = f"{gps[0]:.6f}", f"{gps[1]:.6f}"
        with open(path, "a", newline="") as f:
            w = csv.DictWriter(f, fieldnames=self.FIELDS, extrasaction="ignore")
            if new:
                w.writeheader()
            w.writerow(row)


def write_status(state):
    try:
        os.makedirs(STATUS_DIR, exist_ok=True)
        tmp = os.path.join(STATUS_DIR, ".status.json.tmp")
        with open(tmp, "w") as f:
            json.dump(state, f)
        os.replace(tmp, os.path.join(STATUS_DIR, "status.json"))
    except OSError:
        pass


# ------------------------------------------------------------------ discovery

def ap_network(interface):
    """The PiPack hotspot's IPv4 network, e.g. 10.3.141.0/24."""
    try:
        out = subprocess.run(["ip", "-4", "-o", "addr", "show", "dev", interface],
                             capture_output=True, text=True, timeout=3).stdout
        for word in out.split():
            if "/" in word:
                return ipaddress.ip_interface(word).network
    except (OSError, ValueError, subprocess.SubprocessError):
        pass
    return None


def lease_ips():
    ips = []
    for path in ("/var/lib/misc/dnsmasq.leases", "/var/lib/dnsmasq/dnsmasq.leases"):
        try:
            with open(path) as f:
                ips += [line.split()[2] for line in f if len(line.split()) > 2]
        except OSError:
            pass
    return ips


def discover(cfg, scan=True):
    """Find the Orbic: saved Wi-Fi IP, DHCP leases, then a scan of the hotspot."""
    port = cfg["port"]
    tried = set()
    for host in [cfg.get("wifi_host")] + lease_ips():
        if host and host not in tried:
            tried.add(host)
            if is_orbic(host, port):
                return host
    net = ap_network(cfg["ap_interface"]) if scan else None
    if net is None or net.num_addresses > 1024:
        return None
    hosts = [str(h) for h in net.hosts() if str(h) not in tried]
    with concurrent.futures.ThreadPoolExecutor(64) as pool:
        open_hosts = [h for h, ok in zip(hosts, pool.map(lambda h: tcp_open(h, port, 0.4), hosts)) if ok]
    for host in open_hosts:
        if is_orbic(host, port):
            return host
    return None


def learn_wifi_ip_over_usb(cfg):
    """Ask the Orbic (over USB) which IP it got on the PiPack hotspot."""
    try:
        status, wifi = http(cfg["usb_host"], cfg["port"], "GET", "/api/wifi-status", timeout=3)
    except OSError:
        return None
    if status == 200 and isinstance(wifi, dict) and wifi.get("state") == "connected" and wifi.get("ip"):
        return wifi["ip"].split("/")[0]
    return None


def best_host(cfg, allow_scan=False):
    """(host, via) for HTTP commands: Wi-Fi first, then USB."""
    if cfg.get("wifi_host") and is_orbic(cfg["wifi_host"], cfg["port"]):
        return cfg["wifi_host"], "wifi"
    if is_orbic(cfg["usb_host"], cfg["port"]):
        return cfg["usb_host"], "usb"
    if allow_scan:
        host = discover(cfg)
        if host:
            return host, "wifi"
    return None, None


# --------------------------------------------------------------------- service

class Service:
    def __init__(self, cfg):
        self.cfg = cfg
        self.gps = Gps(cfg["gpsd"])
        self.events = EventLog(cfg["log_dir"])
        self.towers = TowerLog(cfg["log_dir"])
        self.logged_ids = set()
        self.logged_cells = set()
        self.known_stingrays = None  # to catch alerts that happened while the link was down
        self.first_status = True     # only the first status after a (re)connect is checked
        self.pulling = threading.Lock()
        self.state = {"link": "down", "via": None, "host": None, "recording": None,
                      "cells": 0, "stingrays": 0, "last_alert": None, "gps": False,
                      "paired_as": None, "updated": None}

    def auto_pull(self):
        """Save finished recordings to log_dir in the background (one pull at a time)."""
        host = self.state.get("host")
        if not self.cfg.get("auto_pull", True) or not host or not self.pulling.acquire(blocking=False):
            return

        def work():
            try:
                got = pull_recordings(self.cfg, host, say=log.info)
                if got:
                    log.info("saved %d Orbic recording(s) to %s", got, self.cfg["log_dir"])
            except (OSError, ValueError) as e:
                log.warning("couldn't download Orbic recordings: %s", e)
            finally:
                self.pulling.release()

        threading.Thread(target=work, daemon=True, name="pull").start()

    def publish(self, **changes):
        self.state.update(changes)
        self.state["gps"] = self.gps.current() is not None
        self.state["updated"] = int(time.time())
        write_status(self.state)

    def connect(self, host, via):
        ws = WebSocket(host, self.cfg["port"], "/api/tunnel-rat/link", self.cfg["api_key"])
        log.info("link up via %s (%s)", via, host)
        return ws

    def try_wifi(self, refresh=False):
        host = self.cfg.get("wifi_host")
        if refresh or not host or not tcp_open(host, self.cfg["port"]):
            learned = learn_wifi_ip_over_usb(self.cfg) or (discover(self.cfg, scan=refresh) if refresh else None)
            if learned and learned != host:
                log.info("the Orbic's Wi-Fi address is now %s", learned)
                self.cfg["wifi_host"] = host = learned
                try:
                    save_config(self.cfg)
                except OSError as e:
                    log.warning("couldn't save the new address: %s", e)
        if host and tcp_open(host, self.cfg["port"]):
            try:
                return self.connect(host, "wifi"), host
            except AuthError:
                raise
            except (OSError, LinkClosed) as e:
                log.debug("wifi connect failed: %s", e)
        return None, None

    def try_usb(self):
        host = self.cfg["usb_host"]
        if tcp_open(host, self.cfg["port"]):
            try:
                return self.connect(host, "usb"), host
            except AuthError:
                raise
            except (OSError, LinkClosed) as e:
                log.debug("usb connect failed: %s", e)
        return None, None

    def handle(self, msg, via):
        kind = msg.get("type")
        gps = self.gps.current()
        if kind == "hello":
            self.publish(paired_as=msg.get("paired_as"))
        elif kind == "status":
            stingrays = msg.get("stingrays", 0)
            if self.first_status and self.known_stingrays is not None \
                    and stingrays > self.known_stingrays:
                log.warning("STINGRAY ALERT while the link was down (encounter #%s)", stingrays)
                self.events.write("alert_while_down", via, None, stingrays=stingrays)
            if self.first_status or stingrays < (self.known_stingrays or 0):
                self.known_stingrays = stingrays
            self.first_status = False
            self.publish(recording=msg.get("recording"), cells=msg.get("cells", 0),
                         stingrays=stingrays)
        elif kind == "cells":  # heard before this link came up; position unknown
            for earfcn, pci in msg.get("cells", []):
                if (earfcn, pci) not in self.logged_cells:
                    self.logged_cells.add((earfcn, pci))
                    self.events.write("cell_earlier", via, None, earfcn=earfcn, pci=pci)
        elif kind == "cell":
            key = (msg.get("earfcn"), msg.get("pci"))
            if key not in self.logged_cells:
                self.logged_cells.add(key)
                self.events.write("cell", via, gps, earfcn=key[0], pci=key[1])
        elif kind == "cell_ids":  # identities learned before this link came up; position unknown
            for earfcn, pci, plmn, tac, eci in msg.get("cells", []):
                if (plmn, tac, eci) not in self.logged_ids:
                    self.logged_ids.add((plmn, tac, eci))
                    self.towers.write(plmn, tac, eci, earfcn, pci, via, None)
        elif kind == "cell_id":
            key = (msg.get("plmn"), msg.get("tac"), msg.get("eci"))
            if None not in key and key not in self.logged_ids:
                self.logged_ids.add(key)
                self.towers.write(*key, msg.get("earfcn"), msg.get("pci"), via, gps)
        elif kind == "alert":
            self.known_stingrays = msg.get("stingrays", self.known_stingrays)
            log.warning("STINGRAY ALERT: %s severity (encounter #%s)",
                        msg.get("severity"), msg.get("stingrays"))
            self.events.write("alert", via, gps, severity=msg.get("severity"),
                              stingrays=msg.get("stingrays"))
            self.publish(last_alert={"time": int(time.time()), "severity": msg.get("severity"),
                                     "lat": gps[0] if gps else None,
                                     "lon": gps[1] if gps else None})
            if self.cfg.get("alert_command"):
                subprocess.Popen(self.cfg["alert_command"], shell=True,
                                 env={**os.environ, "TR_SEVERITY": str(msg.get("severity"))})
        elif kind == "new_session":  # e.g. a wardrive just started: save the last recording
            self.logged_cells.clear()
            self.logged_ids.clear()
            self.known_stingrays = 0
            self.events.write("new_session", via, gps)
            self.auto_pull()
        elif kind == "error":
            log.warning("the Orbic says: %s", msg.get("error"))

    def run(self):
        if not self.cfg.get("api_key"):
            sys.exit("not paired yet: run the PiPack setup (tunnel-rat-orbic_pipack-setup.sh)")
        backoff, attempts = 1, 0
        while True:
            try:
                ws, host = self.try_wifi(refresh=(attempts % 10 == 9))
                via = "wifi"
                if not ws:
                    ws, host = self.try_usb()
                    via = "usb"
            except AuthError as e:
                log.error("%s; retrying in 60 s", e)
                self.publish(link="unauthorized", via=None, host=None)
                time.sleep(60)
                continue
            if not ws:
                attempts += 1
                self.publish(link="down", via=None, host=None)
                time.sleep(backoff)
                backoff = min(backoff * 2, 15)
                continue
            backoff, attempts = 1, 0
            self.first_status = True
            self.events.write("link_up", via, self.gps.current())
            self.publish(link="up", via=via, host=host)
            self.auto_pull()
            ws, via = self.session(ws, via)
            self.events.write("link_down", via, self.gps.current())
            self.publish(link="down", via=None, host=None)

    def session(self, ws, via):
        """Run one link until it dies. Returns the socket and path last in use."""
        now = time.monotonic()
        last_ping = last_gps = 0.0
        last_heard = last_wifi_try = now
        try:
            while True:
                msg = ws.recv_json(timeout=0.5)
                now = time.monotonic()
                if msg is not None:
                    last_heard = now
                    self.handle(msg, via)
                if now - last_heard > DEAD_AFTER:
                    raise LinkClosed(f"nothing from the Orbic for {DEAD_AFTER} s")
                if now - last_ping >= PING_EVERY:
                    ws.send_json({"type": "ping"})
                    last_ping = now
                fix = self.gps.current()
                if fix and now - last_gps >= GPS_EVERY:
                    ws.send_json({"type": "gps_fix", "lat": fix[0], "lon": fix[1]})
                    last_gps = now
                if via == "usb" and now - last_wifi_try >= WIFI_RETRY:
                    last_wifi_try = now
                    try:
                        new, host = self.try_wifi()
                    except AuthError:
                        new = None
                    if new:  # make-before-break: the rat never goes red
                        ws.close()
                        ws, via = new, "wifi"
                        self.events.write("link_moved", via, self.gps.current())
                        self.publish(via=via, host=host)
        except (OSError, LinkClosed, AuthError) as e:
            log.warning("link down (%s): %s", via, e)
            ws.close()
        return ws, via


# -------------------------------------------------------------------- commands

def need_orbic(cfg):
    host, via = best_host(cfg, allow_scan=True)
    if not host:
        sys.exit("can't reach the Orbic over Wi-Fi or USB-C")
    return host, via


def cmd_status(cfg, _args):
    try:
        with open(os.path.join(STATUS_DIR, "status.json")) as f:
            print("service:", json.dumps(json.load(f), indent=2))
    except OSError:
        print("service: not running")
    host, via = best_host(cfg)
    if not host:
        print("Orbic: not reachable over Wi-Fi or USB-C")
        return
    print(f"Orbic: reachable via {via} at {host}")
    try:
        _, wifi = http(host, cfg["port"], "GET", "/api/wifi-status")
        print("Orbic Wi-Fi client:", json.dumps(wifi))
        _, manifest = http(host, cfg["port"], "GET", "/api/qmdl-manifest")
        current = manifest.get("current_entry") if isinstance(manifest, dict) else None
        print("recording:", current["name"] if current else "no")
    except OSError as e:
        print("Orbic API error:", e)


def cmd_record(cfg, args):
    host, via = need_orbic(cfg)
    ws = WebSocket(host, cfg["port"], "/api/tunnel-rat/link", cfg["api_key"])
    try:
        ws.send_json({"type": "command", "command": f"{args.cmd}_recording"})
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            msg = ws.recv_json(timeout=1)
            if msg and msg.get("type") in ("ack", "error"):
                ok = msg.get("ok", False)
                print(("ok: " if ok else "failed: ") + str(msg.get("message") or msg.get("error")))
                sys.exit(0 if ok else 1)
        sys.exit("no answer from the Orbic")
    finally:
        ws.close()


def pull_recordings(cfg, host, say=print):
    """Download finished recordings not saved yet. Returns how many were saved."""
    _, manifest = http(host, cfg["port"], "GET", "/api/qmdl-manifest", timeout=10)
    entries = manifest.get("entries", []) if isinstance(manifest, dict) else []
    out_dir = os.path.expanduser(cfg["log_dir"])
    os.makedirs(out_dir, exist_ok=True)
    got = 0
    for entry in entries:
        name = entry["name"]
        path = os.path.join(out_dir, f"{DEVICE}-orbic-{name}.zip")
        if os.path.exists(path) or not entry.get("qmdl_size_bytes"):
            continue
        say(f"downloading Orbic recording {name} ...")
        tmp = path + ".part"
        url = f"http://{host}:{cfg['port']}/api/zip/{urllib.parse.quote(name)}"
        with urllib.request.urlopen(url, timeout=120) as r, open(tmp, "wb") as f:
            while chunk := r.read(65536):
                f.write(chunk)
        os.replace(tmp, path)
        got += 1
    return got


def cmd_pull(cfg, _args):
    host, via = need_orbic(cfg)
    got = pull_recordings(cfg, host, say=lambda m: print(f"{m} (via {via})", flush=True))
    print(f"{got} new recording(s) saved to {os.path.expanduser(cfg['log_dir'])} "
          "(the one still recording is skipped)")


def cmd_discover(cfg, _args):
    usb = is_orbic(cfg["usb_host"], cfg["port"])
    print("USB-C:", f"Orbic at {cfg['usb_host']}" if usb else "no Orbic")
    host = discover(cfg)
    print("Wi-Fi:", f"Orbic at {host}" if host else "no Orbic found on the hotspot")
    if host and host != cfg.get("wifi_host") and cfg.get("api_key"):
        cfg["wifi_host"] = host
        save_config(cfg)
        print("saved the new Wi-Fi address")


def cmd_pair(cfg, args):
    host = args.host or cfg["usb_host"]
    status, resp = http(host, cfg["port"], "POST", "/api/tunnel-rat/pair",
                        {"code": args.code.strip().upper(), "device_name": cfg["device_name"]})
    if status == 200 and isinstance(resp, dict) and resp.get("api_key"):
        cfg["api_key"] = resp["api_key"]
        if host != cfg["usb_host"]:
            cfg["wifi_host"] = host
        save_config(cfg)
        print(f"paired: the Orbic now trusts this {DEVICE}")
        return
    reasons = {401: "wrong code (5 wrong tries makes the Orbic show a new one)",
               409: "this Orbic is already paired with something else",
               404: "this Orbic firmware has no pairing (install the Tunnel R.A.T. firmware)"}
    sys.exit("pairing failed: " + reasons.get(status, f"HTTP {status}: {resp}"))


def cmd_handover(cfg, args):
    if args.password_stdin:
        password = sys.stdin.readline().rstrip("\n")
    else:
        password = os.environ.get("TR_WIFI_PASSWORD", "")
    host = args.host or cfg["usb_host"]
    status, conf = http(host, cfg["port"], "GET", "/api/config")
    if status != 200 or not isinstance(conf, dict):
        sys.exit(f"couldn't read the Orbic's config (HTTP {status})")
    conf.update({"wifi_enabled": True, "wifi_ssid": args.ssid,
                 "wifi_password": password or None,
                 "wifi_security": args.security if password else None,
                 "gps_mode": 2})  # 2 = API: the PiPack's gpsd feeds the Orbic
    status, resp = http(host, cfg["port"], "POST", "/api/config", conf, timeout=15)
    if status >= 300:
        sys.exit(f"the Orbic refused the Wi-Fi settings: HTTP {status}: {resp}")
    print(f"the Orbic is joining '{args.ssid}' (Rayhunter restarts to apply it) ...", flush=True)
    deadline = time.monotonic() + 90
    last = None
    while time.monotonic() < deadline:
        time.sleep(3)
        try:
            _, wifi = http(host, cfg["port"], "GET", "/api/wifi-status", timeout=3)
        except OSError:
            continue  # restarting
        if not isinstance(wifi, dict):
            continue
        if wifi.get("state") != last:
            last = wifi.get("state")
            print("  Orbic Wi-Fi:", last, wifi.get("error") or "", flush=True)
        if last == "connected" and wifi.get("ip"):
            cfg["wifi_host"] = wifi["ip"].split("/")[0]
            save_config(cfg)
            print(f"the Orbic is on '{args.ssid}' at {cfg['wifi_host']}")
            return
        if last == "failed":
            break
    sys.exit("the Orbic didn't get onto the Wi-Fi; check the password, then run setup again "
             "(the USB-C backup link still works meanwhile)")


def main():
    p = argparse.ArgumentParser(description="Tunnel R.A.T. Orbic <-> PiPack link")
    sub = p.add_subparsers(dest="cmd", required=True)
    sub.add_parser("run")
    sub.add_parser("status")
    sub.add_parser("start")
    sub.add_parser("stop")
    sub.add_parser("pull")
    sub.add_parser("discover")
    pp = sub.add_parser("pair")
    pp.add_argument("--code", required=True)
    pp.add_argument("--host")
    ph = sub.add_parser("handover")
    ph.add_argument("--ssid", required=True)
    ph.add_argument("--security", default="wpa_psk", choices=["wpa_psk", "sae"])
    ph.add_argument("--host")
    ph.add_argument("--password-stdin", action="store_true",
                    help="read the Wi-Fi password from the first line of stdin")
    args = p.parse_args()
    logging.basicConfig(level=logging.INFO, format="%(levelname)s %(message)s")
    cfg = load_config()
    {"run": lambda c, a: Service(c).run(), "status": cmd_status, "start": cmd_record,
     "stop": cmd_record, "pull": cmd_pull, "discover": cmd_discover, "pair": cmd_pair,
     "handover": cmd_handover}[args.cmd](cfg, args)


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        pass
