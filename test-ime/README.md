# Test IME

A scriptable input method for on-device tests. Tests drive it from the host, so text reaches the app
through the real input method framework (IMF) without depending on a vendor keyboard, its layout or the
screen size. It covers what tapping keys can't: composition (preview text), reading back the text around
the cursor, cursor moves, extend actions and private commands.

The host-side Python client is in [`client/`](client) (`ohos-test-ime`).

## Building

```sh
just build                                     # OpenHarmony devices, signed with the SDK's public test keys
just signing=signing-configs.json build-harmonyos   # HarmonyOS devices, signed with your `hos` config
just install
```

- **Standalone (default):** bundle `org.openharmonyrs.testime`. The IME has its own identity, as a real IME
  would. HarmonyOS devices need a debug profile for this bundle that lists the test devices.
- **Module of the app under test:** `just bundle=<app bundle> module=feature version_code=<n> version_name=<v> …`
  builds it as a feature module of the app's bundle, at the app's version. It installs next to the app and is signed with the app's own profile, so no
  extra profile is needed. The app then shares the IME's access token, so IMF treats the app as the
  current IME in its permission checks, and `aa force-stop <bundle>` also stops the IME.

## Using it

The shell can switch input methods; no Settings interaction is needed:

| Command | Effect |
| --- | --- |
| `ime -e <bundle> -f` | enable in full experience mode |
| `ime -s <bundle>` | make it current |
| `ime -g` / `ime -l` | show the current IME / list all |
| `ime -d <bundle>` | disable |

Use full experience mode. In basic mode, which is what enabling the IME in Settings gives, the IME works but
can neither open its control socket nor receive common events.

```python
from ohos_test_ime import TestIme

with TestIme("<serial>") as ime:   # enables, selects, forwards the port; restores on exit
    # Focus a text field with a real tap first.
    ime.wait_attached()
    ime.insert("hello")
    ime.backspace()
```

## Control protocol

JSON lines over TCP on device `127.0.0.1:8971` (reach it with `hdc fport`), or the same JSON, optionally
base64-encoded, as the data of `cem publish -e org.openharmonyrs.testime.CMD -d <payload>`; the reply then
goes to hilog as `TESTIME REPLY {...}`. Requests are `{"id": n, "op": ..., ...}`; replies are
`{"id": n, "op": ..., "ok": bool, "value"?: string, "error"?: string}`.

| op | arguments | IMF call |
| --- | --- | --- |
| `state` | | attached, shown, securityMode, attach count, last `textChange` |
| `insert` | `text` | `insertText` |
| `backspace` / `delete` | `n` | `deleteForward` / `deleteBackward`: IMF's "forward" is towards the start of the text |
| `before` / `after` | `n` | `getForward` / `getBackward` |
| `cursor` | | `getTextIndexAtCursor` (UTF-16) |
| `select` | `start`, `end` | `selectByRange` |
| `preview` / `finishPreview` | `text`, `start`, `end` | `setPreviewText` / `finishTextPreview` |
| `key` | `action` | `sendKeyFunction` |
| `move` | `direction` | `moveCursor` (1 up, 2 down, 3 left, 4 right) |
| `extend` | `action` | `sendExtendAction` |
| `attr` | | `getEditorAttribute` |
| `privateCommands` | | private commands received from the editor since the last call; fails unless the IME is the system's preconfigured default IME |
| `hide` | | `KeyboardController.hide` |

Private commands work only for the system's preconfigured default IME, in both directions: IMF rejects
`on('privateCommand')` and `InputClient.sendPrivateCommand` for any other IME.
