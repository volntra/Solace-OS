"""Drives a Solace VM for the QEMU tests: its serial console (expect,
send, run a shell command and capture its output) and, when started with a
monitor socket, the QEMU monitor (screenshots, key presses, power off)."""

import os
import re
import socket
import subprocess
import sys
import time

TIMEOUT = float(os.environ.get("QEMU_TEST_TIMEOUT", "600"))
PASSWORD = os.environ.get("SOL_ROOT_PASSWORD", "solace")
PROMPT = re.compile(rb"\n[^\n]*[#$] $")
# Escape sequences and carriage returns, removed from captured output.
NOISE = re.compile(rb"\x1b\[[0-9;?]*[A-Za-z]|\x1b[=>]|\r")

_log = None


def fail(msg):
    print(f"\n\nqemu test: FAIL: {msg}" + (f" (console log: {_log})" if _log else ""), file=sys.stderr)
    sys.exit(1)


def ok(msg):
    print(f"\n\033[32m✓ {msg}\033[0m", flush=True)


class Console:
    def __init__(self, cmd, log, monitor=None):
        global _log
        _log = log
        self.monitor_path = monitor
        env = dict(os.environ)
        if monitor:
            env["QEMU_MONITOR"] = monitor
            if os.path.exists(monitor):
                os.remove(monitor)
        self.p = subprocess.Popen(
            cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, env=env
        )
        os.set_blocking(self.p.stdout.fileno(), False)
        self.buf = b""
        self.pos = 0
        self.log = open(log, "ab")
        self.mon = None

    def _read(self):
        try:
            data = self.p.stdout.read()
        except BlockingIOError:
            data = None
        if data:
            self.buf += data
            self.log.write(data)
            self.log.flush()
            sys.stdout.write(data.decode("utf-8", "replace"))
            sys.stdout.flush()
        return data

    def expect(self, pattern, what, timeout=TIMEOUT):
        rx = re.compile(pattern) if isinstance(pattern, bytes) else pattern
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            m = rx.search(self.buf, self.pos)
            if m:
                self.pos = m.end()
                return m
            if not self._read():
                if self.p.poll() is not None:
                    self._read()
                    m = rx.search(self.buf, self.pos)
                    if m:
                        self.pos = m.end()
                        return m
                    fail(f"QEMU exited while waiting for {what}")
                time.sleep(0.05)
        fail(f"timed out after {timeout:.0f}s waiting for {what}")

    def send(self, text):
        self.p.stdin.write(text.encode())
        self.p.stdin.flush()

    def run(self, cmd, what=None, timeout=120):
        """Runs a shell command; returns its output (cleaned) as text."""
        what = what or cmd
        self.send(cmd + "\n")
        self.expect(re.escape(cmd.encode()), f"echo of {what}", 30)
        start = self.pos
        m = self.expect(PROMPT, f"prompt after {what}", timeout)
        return NOISE.sub(b"", self.buf[start : m.start()]).decode("utf-8", "replace").strip()

    def boot_and_login(self):
        """Waits for a boot to finish and logs in as root. Returns the boot
        time reported by sol-init (ms after the kernel started)."""
        m = self.expect(rb"boot complete: (\d+) ms after the kernel started", "sol-init boot complete")
        self.expect(rb"login: ", "the login prompt")
        self.send("root\n")
        self.expect(rb"Password: ", "the password prompt", 60)
        self.send(PASSWORD + "\n")
        self.expect(PROMPT, "a shell prompt after login", 120)
        # A wide terminal, so a command's echo never wraps.
        self.run("stty cols 200", "stty")
        return int(m.group(1))

    # --- QEMU monitor -------------------------------------------------

    def _monitor(self, cmd):
        if self.mon is None:
            for _ in range(100):
                if os.path.exists(self.monitor_path):
                    break
                time.sleep(0.1)
            self.mon = socket.socket(socket.AF_UNIX)
            self.mon.connect(self.monitor_path)
            self.mon.settimeout(5)
            self.mon.recv(4096)
        self.mon.sendall(cmd.encode() + b"\n")
        time.sleep(0.2)
        try:
            return self.mon.recv(65536)
        except TimeoutError:
            return b""

    def screenshot(self, path):
        """Saves the screen as PPM; returns (width, height, pixel bytes)."""
        if os.path.exists(path):
            os.remove(path)
        self._monitor(f"screendump {path}")
        for _ in range(50):
            if os.path.exists(path) and os.path.getsize(path) > 0:
                break
            time.sleep(0.1)
        time.sleep(0.2)
        data = open(path, "rb").read()
        parts = data.split(maxsplit=4)
        return int(parts[1]), int(parts[2]), parts[4]

    def key(self, name):
        self._monitor(f"sendkey {name}")

    def kill(self):
        """Pulls the power: no shutdown, no sync."""
        self.p.kill()
        self.p.wait()
        if self.mon:
            self.mon.close()
            self.mon = None

    def close(self):
        if self.p.poll() is None:
            self.p.kill()
            self.p.wait()


def to_png(ppm, png):
    """PPM -> PNG with the standard library only."""
    import struct
    import zlib

    data = open(ppm, "rb").read()
    parts = data.split(maxsplit=4)
    w, h, px = int(parts[1]), int(parts[2]), parts[4]
    rows = b"".join(b"\0" + px[y * w * 3 : (y + 1) * w * 3] for y in range(h))

    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))

    open(png, "wb").write(
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(rows, 6))
        + chunk(b"IEND", b"")
    )
