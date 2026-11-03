# ohos-test-ime

Host-side client for the scriptable test input method in
[ohos-ime/test-ime](https://github.com/openharmony-rs/ohos-ime/tree/main/test-ime). It makes the test IME the
current input method of an OpenHarmony or HarmonyOS device over `hdc`, drives it, and restores the previous
input method afterwards.

```python
from ohos_test_ime import TestIme

with TestIme(target="<serial>", bundle="org.openharmonyrs.testime") as ime:
    # Focus a text field in the app under test with a real tap first.
    ime.wait_attached()
    ime.insert("hello 😀")
    ime.backspace()
    assert ime.before() == "hello "
```
