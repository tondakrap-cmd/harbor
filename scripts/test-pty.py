#!/usr/bin/env python3
"""Run the real TUI in a PTY and check terminal cleanup, signals and resize."""
import fcntl
import os
import pathlib
import pty
import select
import signal
import struct
import subprocess
import tempfile
import termios
import time

ROOT = pathlib.Path(__file__).resolve().parent.parent
BINARY = pathlib.Path(os.environ.get("HARBOR_TUI_BINARY", ROOT / "target/debug/harbor-tui"))

def run_case(ending, command=None):
    master, slave = pty.openpty()
    original = termios.tcgetattr(slave)
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
    with tempfile.TemporaryDirectory(prefix="harbor-pty-") as config:
        env = dict(os.environ, TERM="xterm-256color", XDG_CONFIG_HOME=config)
        process = subprocess.Popen(command or [str(BINARY), "--demo", "--colors=16"], stdin=slave, stdout=slave, stderr=slave, env=env)
        output = bytearray()
        def collect(seconds):
            until = time.monotonic() + seconds
            while time.monotonic() < until:
                if select.select([master], [], [], 0.05)[0]:
                    output.extend(os.read(master, 65536))
        try:
            collect(0.4)
            # F3 profiles, F10 menu, resize, Esc, F1 help, Esc.
            os.write(master, b"\x1bOR\x1b[21~")
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 35, 120, 0, 0))
            process.send_signal(signal.SIGWINCH)
            collect(0.15)
            os.write(master, b"\x1b")
            collect(0.1)
            os.write(master, b"\x1bOP")
            collect(0.15)
            if ending == "keyboard":
                os.write(master, b"\x03")
            else:
                process.send_signal(ending)
            process.wait(timeout=5)
            collect(0.1)
            assert process.returncode == 0, (ending, process.returncode, bytes(output[-2000:]))
            assert b"Harbor" in output, "application screen was not rendered"
            assert termios.tcgetattr(slave) == original, f"termios not restored: {ending}"
            assert b"\x1b[?1049h" in output and b"\x1b[?1049l" in output, "alternate screen cleanup missing"
            assert b"\x1b[?25h" in output, "cursor not restored"
            print(f"PASS PTY resize and cleanup: {ending}")
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()
            os.close(master)
            os.close(slave)

if __name__ == "__main__":
    for ending in ["keyboard", signal.SIGINT, signal.SIGTERM, signal.SIGHUP]:
        run_case(ending)
    result = subprocess.run([str(BINARY)], input=b"", stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    assert result.returncode != 0 and b"PTY" in result.stderr
    print("PASS non-TTY rejection")
