"""Host-side client for the scriptable test IME (see the test-ime README for the protocol)."""

from __future__ import annotations

import json
import re
import shutil
import socket
import subprocess
import time
from types import TracebackType

DEVICE_PORT = 8971
DEFAULT_BUNDLE = "org.openharmonyrs.testime"

# `InputClient.moveCursor` directions.
UP, DOWN, LEFT, RIGHT = 1, 2, 3, 4


class TestImeError(RuntimeError):
    pass


def _free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class TestIme:
    """Makes the test IME current on an hdc target for the duration of a `with` block, and drives it.

    `bundle` is the bundle the IME was built into: its own, or the app under test's when built as a feature module.
    The IME is enabled in full experience mode: in basic mode it can't open its control socket.
    """

    __test__ = False

    def __init__(
        self,
        target: str | None = None,
        bundle: str = DEFAULT_BUNDLE,
        host_port: int | None = None,
        hdc: str | None = None,
    ) -> None:
        self.target = target
        self.bundle = bundle
        self.host_port = host_port or _free_port()
        self.hdc_path = hdc or shutil.which("hdc") or "hdc"
        self.previous: str | None = None
        self._file = None
        self._next_id = 0

    def _hdc(self, *args: str, timeout: float = 30) -> str:
        target = ["-t", self.target] if self.target else []
        return subprocess.run([self.hdc_path, *target, *args], capture_output=True, text=True, timeout=timeout).stdout

    def shell(self, command: str) -> str:
        return self._hdc("shell", command)

    def current(self) -> str | None:
        """Bundle name of the current input method."""
        match = re.search(r"current input method is: ([^,\s]+)", self.shell("ime -g"))
        return match.group(1) if match else None

    def __enter__(self) -> TestIme:
        self.previous = self.current()
        for command in (f"ime -e {self.bundle} -f", f"ime -s {self.bundle}"):
            output = self.shell(command)
            if "Succeeded" not in output:
                raise TestImeError(f"`{command}` failed: {output.strip()}")
        self._hdc("fport", f"tcp:{self.host_port}", f"tcp:{DEVICE_PORT}")
        return self

    def __exit__(
        self, exc_type: type[BaseException] | None, exc: BaseException | None, tb: TracebackType | None
    ) -> None:
        self.close()
        self._hdc("fport", "rm", f"tcp:{self.host_port}", f"tcp:{DEVICE_PORT}")
        if self.previous and self.previous != self.bundle:
            self.shell(f"ime -s {self.previous}")
        self.shell(f"ime -d {self.bundle}")

    def close(self) -> None:
        if self._file is not None:
            self._file.close()
            self._file = None

    def call(self, op: str, timeout: float = 10.0, **args: object) -> dict:
        """Sends one command and returns the reply. Reconnects while the IME process (re)starts: the system
        starts it on demand, when an editor first attaches."""
        deadline = time.monotonic() + timeout
        while True:
            try:
                if self._file is None:
                    connection = socket.create_connection(("127.0.0.1", self.host_port), timeout=5)
                    self._file = connection.makefile("rw", encoding="utf-8")
                self._next_id += 1
                self._file.write(json.dumps({"id": self._next_id, "op": op, **args}) + "\n")
                self._file.flush()
                line = self._file.readline()
                if not line:
                    raise ConnectionError("connection closed")
                return json.loads(line)
            except OSError:
                self.close()
                if time.monotonic() > deadline:
                    raise TestImeError(f"test IME unreachable for `{op}`") from None
                time.sleep(0.3)

    def checked(self, op: str, **args: object) -> str:
        reply = self.call(op, **args)
        if not reply["ok"]:
            raise TestImeError(f"{op}: {reply.get('error')}")
        return reply.get("value", "")

    def state(self) -> dict:
        """`attached`, `shown`, `securityMode`, `starts` (attach count) and `lastText`."""
        return json.loads(self.checked("state"))

    def wait_attached(self, timeout: float = 10.0) -> dict:
        """Waits until an editor is attached; returns the last state either way."""
        deadline = time.monotonic() + timeout
        state: dict = {"attached": False}
        while time.monotonic() < deadline:
            try:
                state = self.state()
            except TestImeError:
                pass
            else:
                if state["attached"]:
                    return state
            time.sleep(0.2)
        return state

    def insert(self, text: str) -> None:
        self.checked("insert", text=text)

    def backspace(self, n: int = 1) -> None:
        """Deletes `n` characters before the cursor (IMF `deleteForward`)."""
        self.checked("backspace", n=n)

    def delete(self, n: int = 1) -> None:
        """Deletes `n` characters after the cursor (IMF `deleteBackward`)."""
        self.checked("delete", n=n)

    def before(self, n: int = 1000) -> str:
        return self.checked("before", n=n)

    def after(self, n: int = 1000) -> str:
        return self.checked("after", n=n)

    def cursor(self) -> int:
        """Cursor position in UTF-16 code units."""
        return int(self.checked("cursor"))

    def select(self, start: int, end: int) -> None:
        self.checked("select", start=start, end=end)

    def preview(self, text: str, start: int = -1, end: int = -1) -> None:
        """Shows `text` as preview (composition) text, replacing [`start`, `end`); -1 keeps the current range."""
        self.checked("preview", text=text, start=start, end=end)

    def finish_preview(self) -> None:
        self.checked("finishPreview")

    def key(self, action: int = 0) -> None:
        """`sendKeyFunction`: the editor's enter key action (0 = the editor's default)."""
        self.checked("key", action=action)

    def enter(self) -> None:
        self.key(0)

    def move(self, direction: int) -> None:
        self.checked("move", direction=direction)

    def extend(self, action: int) -> None:
        self.checked("extend", action=action)

    def attributes(self) -> dict:
        return json.loads(self.checked("attr"))

    def private_commands(self) -> list[dict]:
        """Private commands the editor sent since the last call."""
        return json.loads(self.checked("privateCommands"))

    def hide(self) -> None:
        self.checked("hide")


__all__ = ["DEFAULT_BUNDLE", "DEVICE_PORT", "DOWN", "LEFT", "RIGHT", "UP", "TestIme", "TestImeError"]
