"""Host-side client for the scriptable test IME (see the test-ime README for the protocol)."""

from __future__ import annotations

import json
import re
import select
import socket
import time
from collections.abc import Callable
from types import TracebackType
from typing import IO

from hdc_py import HarmonyDevice, Hdc, PortForward

DEVICE_PORT = 8971
DEFAULT_BUNDLE = "org.openharmonyrs.testime"
# A common event whose data is the port the IME should listen on instead.
PORT_EVENT = "org.openharmonyrs.testime.PORT"
_SESSION_LINE = re.compile(r"TESTIME (?:session ([0-9a-f]{64}) port (\d+)|not listening: (.*))")

# `InputClient.moveCursor` directions.
UP, DOWN, LEFT, RIGHT = 1, 2, 3, 4


class TestImeError(RuntimeError):
    pass


def _free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class TestIme:
    """Makes the test IME current on a device for the duration of a `with` block, and drives it.

    `device` is an hdc-py device, an hdc target, or `None` for the only connected device.
    `bundle` is the bundle the IME was built into: its own, or the app under test's when built as a feature module.
    `device_port` is the IME's control port on the device; the IME doesn't listen if it can't get it.

    The IME accepts only connections that present its token, which it logs to hilog: hilogd shows other processes'
    logs to the shell but not to apps. The token changes when a session ends.
    The IME is enabled in full experience mode: in basic mode it can't open its control socket.
    """

    __test__ = False

    def __init__(
        self,
        device: HarmonyDevice | str | None = None,
        bundle: str = DEFAULT_BUNDLE,
        host_port: int | None = None,
        device_port: int = DEVICE_PORT,
    ) -> None:
        self.device = device if isinstance(device, HarmonyDevice) else Hdc().connect(target=device)
        self.bundle = bundle
        self.host_port = host_port or _free_port()
        self.device_port = device_port
        self.previous: str | None = None
        self._status_before: str | None = None
        self._active = False
        self._token: str | None = None
        self._port_published = 0.0
        self._token_requested = 0.0
        self._forward: PortForward | None = None
        self._socket: socket.socket | None = None
        self._file: IO[str] | None = None
        self._next_id = 0

    def shell(self, command: str) -> str:
        """Runs `command` on the device and returns its output; its exit code is unknown."""
        return self.device.cmd_without_exit_code(command, timeout=30).output

    def current(self) -> str | None:
        """Bundle name of the current input method."""
        match = re.search(r"current input method is: ([^,\s]+)", self.shell("ime -g"))
        return match.group(1) if match else None

    def _status(self) -> str | None:
        """This IME's status in `ime -l`, or `None` when it's disabled or not listed."""
        for line in self.shell("ime -l").splitlines():
            match = re.match(r"\s*bundle: ([^,\s]+)(?:, status: (\S+))?", line)
            if match and match.group(1) == self.bundle:
                status = match.group(2) or ""
                return None if status == "DISABLED" else status
        return None

    def _publish_port(self, port: int) -> None:
        self.shell(f"cem publish -e {PORT_EVENT} -d {port}")
        self._port_published = time.monotonic()

    def _read_token(self) -> str | None:
        """The token of the running IME process, from its own hilog lines."""
        match = re.search(r"\d+", self.shell(f"pidof {self.bundle}:inputMethod"))
        if match is None:
            return None
        last: re.Match[str] | None = None
        for line in self.shell(f"hilog -x -P {match.group()} -T TestIme").splitlines():
            last = _SESSION_LINE.search(line) or last
        if last is None:
            return None
        if last.group(3) is not None:
            raise TestImeError(f"test IME {self.bundle} isn't listening: {last.group(3).strip()}")
        if int(last.group(2)) != self.device_port:
            if time.monotonic() - self._port_published > 1:
                self._publish_port(self.device_port)
            return None
        return last.group(1)

    def __enter__(self) -> TestIme:
        self.previous = self.current()
        self._status_before = self._status()
        self._active = True
        try:
            for command in (f"ime -e {self.bundle} -f", f"ime -s {self.bundle}"):
                output = self.shell(command)
                if "Succeeded" not in output:
                    raise TestImeError(f"`{command}` failed: {output.strip()}")
            if self.device_port != DEVICE_PORT:
                self._publish_port(self.device_port)
            self._forward = self.device.forward_port(f"tcp:{self.host_port}", f"tcp:{self.device_port}")
        except BaseException:
            self._clean_up()
            raise
        return self

    def __exit__(
        self, exc_type: type[BaseException] | None, exc: BaseException | None, tb: TracebackType | None
    ) -> None:
        errors = self._clean_up()
        if errors and exc is None:
            raise TestImeError("cleaning up failed: " + "; ".join(errors))

    def _clean_up(self) -> list[str]:
        """Undoes what the session changed. Every step runs, even after an earlier one failed."""
        errors: list[str] = []

        def step(what: str, action: Callable[[], object]) -> None:
            try:
                action()
            except Exception as e:
                errors.append(f"{what}: {e}")

        if self._file is not None:
            # Retires the token, so it stops working even while the IME keeps running.
            step("end the session", lambda: self.checked("bye"))
        self.close()
        self._active = False
        self._token = None
        forward, self._forward = self._forward, None
        if forward is not None:
            step("remove the port forward", lambda: self._remove_forward(forward))
        if self.device_port != DEVICE_PORT and self._status_before is not None:
            step("restore the IME's port", lambda: self._publish_port(DEVICE_PORT))
        previous = self.previous
        if previous and previous != self.bundle:
            step("restore the previous IME", lambda: self._run_ime(f"ime -s {previous}"))
        step("restore the IME's mode", self._restore_mode)
        return errors

    def _remove_forward(self, forward: PortForward) -> None:
        removal = self.device.remove_port_forward(forward)
        if not removal.success:
            raise TestImeError(removal.message)

    def _run_ime(self, command: str) -> None:
        output = self.shell(command)
        if "Succeeded" not in output:
            raise TestImeError(f"`{command}` failed: {output.strip()}")

    def _restore_mode(self) -> None:
        if self._status_before is None:
            self._run_ime(f"ime -d {self.bundle}")
        elif "BASIC" in self._status_before:
            self._run_ime(f"ime -e {self.bundle} -b")

    def close(self) -> None:
        """Closes the control connection; the next command opens a new one."""
        for closeable in (self._file, self._socket):
            if closeable is not None:
                try:
                    closeable.close()
                except OSError:
                    pass
        self._file = None
        self._socket = None

    def _exchange(self, file: IO[str], op: str, args: dict[str, object]) -> dict:
        self._next_id += 1
        request_id = self._next_id
        file.write(json.dumps({"id": request_id, "op": op, **args}) + "\n")
        file.flush()
        line = file.readline()
        if not line:
            raise ConnectionError("connection closed")
        reply = json.loads(line)
        if not isinstance(reply, dict) or reply.get("id") != request_id:
            raise ValueError(f"unexpected reply to {op} #{request_id}: {line.strip()}")
        return reply

    def _request_token(self, deadline: float) -> None:
        """Makes the IME log its token again, by presenting none: the line may have left hilog's buffer."""
        if time.monotonic() - self._token_requested < 1:
            return
        self._token_requested = time.monotonic()
        try:
            timeout = max(0.1, deadline - time.monotonic())
            with socket.create_connection(("127.0.0.1", self.host_port), timeout=timeout) as sock:
                with sock.makefile("rw", encoding="utf-8", newline="\n") as file:
                    self._exchange(file, "hello", {})
        except (OSError, ValueError):
            pass

    def _connect(self, deadline: float) -> None:
        """Connects and presents the token. Retries while the IME (re)starts and until its token is in hilog."""
        last_error = "no reply"
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TestImeError(
                    f"test IME {self.bundle} unreachable on device port {self.device_port}: {last_error}"
                )
            if self._token is None:
                self._token = self._read_token()
                if self._token is None:
                    last_error = "its token isn't in hilog"
                    self._request_token(deadline)
                    time.sleep(min(0.3, max(0.0, deadline - time.monotonic())))
                    continue
            sock: socket.socket | None = None
            file: IO[str] | None = None
            reply: dict = {}
            try:
                sock = socket.create_connection(("127.0.0.1", self.host_port), timeout=remaining)
                file = sock.makefile("rw", encoding="utf-8", newline="\n")
                reply = self._exchange(file, "hello", {"token": self._token})
            except (OSError, ValueError) as e:
                last_error = str(e) or type(e).__name__
            if reply.get("ok") and sock is not None and file is not None:
                hello = json.loads(reply.get("value") or "{}")
                if hello.get("bundle") == self.bundle:
                    self._socket, self._file = sock, file
                    return
                last_error = f"device port {self.device_port} is served by {hello.get('bundle')}"
            elif reply:
                # A new IME process has a new token.
                last_error = str(reply.get("error"))
                self._token = None
            for closeable in (file, sock):
                if closeable is not None:
                    closeable.close()
            time.sleep(min(0.3, max(0.0, deadline - time.monotonic())))

    def _connection_closed(self) -> bool:
        """Whether the IME closed the connection since the last reply, e.g. because its process restarted."""
        if self._socket is None:
            return True
        readable, _, _ = select.select([self._socket], [], [], 0)
        if not readable:
            return False
        try:
            return self._socket.recv(1, socket.MSG_PEEK) == b""
        except OSError:
            return True

    def call(self, op: str, args: dict[str, object] | None = None, timeout: float = 10.0) -> dict:
        """Sends one command and returns the reply. Connecting is retried while the IME process (re)starts: the
        system starts it on demand, when an editor first attaches. A command is sent once: if its reply doesn't
        arrive, this raises rather than risk running it twice."""
        if not self._active:
            raise TestImeError("use TestIme in a `with` block")
        deadline = time.monotonic() + timeout
        if self._file is None or self._connection_closed():
            self.close()
            self._connect(deadline)
        assert self._socket is not None and self._file is not None
        self._socket.settimeout(max(0.1, deadline - time.monotonic()))
        try:
            return self._exchange(self._file, op, args or {})
        except (OSError, ValueError) as e:
            self.close()
            raise TestImeError(f"no reply to `{op}`: {str(e) or type(e).__name__}") from None

    def checked(self, op: str, **args: object) -> str:
        reply = self.call(op, args)
        if not reply["ok"]:
            raise TestImeError(f"{op}: {reply.get('error')}")
        return reply.get("value", "")

    def state(self) -> dict:
        """`attached`, `shown`, `securityMode`, `starts` (attach count) and `lastTextLength`."""
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

    def key(self, action: int) -> None:
        """`sendKeyFunction` with an `EnterKeyType` value; editors ignore 0 (unspecified) unless they report it."""
        self.checked("key", action=action)

    def enter(self) -> int:
        """Presses Enter with the editor's own Enter key type, and returns that type."""
        return int(self.checked("enter"))

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


__all__ = ["DEFAULT_BUNDLE", "DEVICE_PORT", "DOWN", "LEFT", "PORT_EVENT", "RIGHT", "UP", "TestIme", "TestImeError"]
