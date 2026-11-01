#!/usr/bin/env python3
"""Exercise the actual installed TUI in local tmux, SSH PTY and tmux over SSH."""
import importlib.util
import pathlib

path = pathlib.Path(__file__).with_name("test-pty.py")
spec = importlib.util.spec_from_file_location("harbor_pty", path)
pty_test = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pty_test)

pty_test.run_case("keyboard", ["tmux", "-u", "-L", "harbor-local-test", "new-session", "-s", "harbor-ui", "harbor-tui --demo --colors=16"])
print("PASS installed TUI in local tmux")

ssh = ["ssh", "-tt", "-p", "2222", "-i", "/tmp/harbor-ssh/client", "-o", "BatchMode=yes", "-o", "IdentitiesOnly=yes", "-o", "StrictHostKeyChecking=yes", "-o", "UserKnownHostsFile=/tmp/harbor-ssh/known_hosts", "root@127.0.0.1"]
pty_test.run_case("keyboard", ssh + ["exec env -u DISPLAY -u WAYLAND_DISPLAY TERM=xterm-256color harbor-tui --demo --colors=16"])
print("PASS installed TUI over SSH with PTY")

pty_test.run_case("keyboard", ssh + ["exec env -u DISPLAY -u WAYLAND_DISPLAY TERM=xterm-256color tmux -u -L harbor-ssh-test new-session -s harbor-ui 'harbor-tui --demo --colors=16'"])
print("PASS installed TUI in tmux over SSH with PTY")
