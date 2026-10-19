#!/usr/bin/env python3
"""Headless smoke test for the Solace QEMU image (`just qemu-test`).

Boots image/qemu.sh --headless, follows the serial console, and checks:
  1. GRUB boots our kernel and sol-init reports boot complete, with the
     time since the kernel started (the console is quiet until then)
  2. a text login prompt appears; root logs in with the dev password
  3. solctl works: status, boot-time, logs
  4. solctl poweroff shuts the machine down cleanly

Exit status 0 only if every step passed. The full console log is written
to build/image/qemu-test.log.
"""

import os
import re
import subprocess
import sys
import time

TIMEOUT = float(os.environ.get("QEMU_TEST_TIMEOUT", "600"))
PASSWORD = os.environ.get("SOL_ROOT_PASSWORD", "solace")
LOG = "build/image/qemu-test.log"
PROMPT = re.compile(rb"\n[^\n]*[#$] $")


class Console:
    def __init__(self, cmd):
        self.p = subprocess.Popen(
            cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT
        )
        os.set_blocking(self.p.stdout.fileno(), False)
        self.buf = b""
        self.pos = 0
        self.log = open(LOG, "wb")
        self.start = time.monotonic()

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

    def run(self, cmd, what):
        self.send(cmd + "\n")
        self.expect(re.escape(cmd.encode()), f"echo of {what}", 30)
        self.expect(PROMPT, f"prompt after {what}", 120)


def fail(msg):
    print(f"\n\nqemu-test: FAIL: {msg} (console log: {LOG})", file=sys.stderr)
    sys.exit(1)


def ok(msg):
    print(f"\n\033[32mqemu-test: ✓ {msg}\033[0m", flush=True)


def main():
    os.makedirs("build/image", exist_ok=True)
    c = Console(["image/qemu.sh", "--headless"])
    try:
        m = c.expect(rb"boot complete: (\d+) ms after the kernel started", "sol-init boot complete")
        boot_ms = int(m.group(1))
        ok(f"sol-init: boot complete {boot_ms} ms after the kernel started")
        c.expect(rb"login: ", "the login prompt")
        ok("text login prompt")
        c.send("root\n")
        c.expect(rb"Password: ", "the password prompt", 60)
        c.send(PASSWORD + "\n")
        c.expect(PROMPT, "a shell prompt after login", 120)
        ok("logged in as root")
        # A wide terminal, so a command's echo never wraps.
        c.run("stty cols 200", "stty")
        start = c.pos
        c.run("solctl status", "solctl status")
        out = c.buf[start : c.pos]
        if b"getty-serial" not in out:
            fail("solctl status did not list getty-serial")
        ok("solctl status")
        start = c.pos
        c.run("solctl boot-time", "solctl boot-time")
        if b"ms after the kernel started" not in c.buf[start : c.pos]:
            fail("solctl boot-time gave no time")
        ok("solctl boot-time")
        c.run("solctl logs -n 5", "solctl logs")
        start = c.pos
        c.run("cat /etc/os-release", "os-release")
        if b"ID=solace" not in c.buf[start : c.pos]:
            fail("/etc/os-release does not identify Solace")
        ok("/etc/os-release (from the factory /etc)")
        start = c.pos
        c.run("ls -A /", "root listing")
        listing = c.buf[start : c.pos]
        for name in (b"Applications", b"Users", b"System", b"Volumes", b"usr", b"etc"):
            if name not in listing:
                fail(f"/{name.decode()} missing from the root")
        ok("Solace root layout (Applications, Users, System, Volumes)")
        start = c.pos
        c.run("findmnt -n -o FSTYPE,OPTIONS /usr", "findmnt")
        if re.search(rb"btrfs\s+ro,", c.buf[start : c.pos]) is None:
            fail("/usr is not a read-only btrfs mount")
        ok("/usr is the sealed read-only snapshot")
        c.send("solctl poweroff\n")
        deadline = time.monotonic() + 120
        while c.p.poll() is None and time.monotonic() < deadline:
            c._read()
            time.sleep(0.1)
        if c.p.poll() is None:
            fail("the machine did not power off")
        ok("clean poweroff")
        verdict = "under" if boot_ms < 5000 else "OVER"
        print(f"\nqemu-test: PASS (boot {boot_ms} ms, {verdict} the 5 s target"
              f"{'; no KVM, so this is software emulation' if not os.access('/dev/kvm', os.W_OK) else ''})")
    finally:
        if c.p.poll() is None:
            c.p.kill()


if __name__ == "__main__":
    main()
