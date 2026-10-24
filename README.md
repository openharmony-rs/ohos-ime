## OpenHarmony Input method (IME) bindings

Safe Rust bindings to access the Input method on OpenHarmony devices.
See the documentation for more details. 
Requires at least OpenHarmony 5.0.0 (API-12). The `api-N` features enable newer APIs and
require API level N.

## Testing

The unit tests run on a device through [cargo-ohos] and [ohos-test-runner]. `--all-features`
needs an SDK and a device of API 26:

```sh
cargo ohos test --target aarch64 --all-features
```

CI runs them with `--target x86_64` on the OpenHarmony emulator of cargo-ohos
(`cargo ohos emulator start`).

Attaching to the input method needs a focused application window, so the tests only cover
the callback dispatch and conversions. Check input method behavior in an application.

[cargo-ohos]: https://github.com/openharmony-rs/cargo-ohos
[ohos-test-runner]: https://github.com/openharmony-rs/ohos-test-runner

## Demo

`demo/` is an app with a small editor that shows which callbacks it receives, next to an ArkUI text
field for comparison. Its on-device tests check how input methods and ArkUI handle the input, e.g.
what deleting an emoji asks for. In `demo/`, with [hvigor-cargo]'s requirements installed:

```sh
just install  # builds, signs and installs the app
just test     # also runs the tests; the screen has to be on and unlocked
```

The tests tap keys of the soft keyboard by position, which is known for a few input methods only.
`tools/sign-hap.py` signs with the public OpenHarmony test keys, which HarmonyOS devices reject; sign
with your own profile for those.

hvigorw and ohpm ship only with the HarmonyOS command line tools. Without them, run hvigor installed
with npm and install the ohpm packages with `tools/install-oh-modules.py`, as the `emulator_tests`
job in `.github/workflows/ci.yml` does, which also shows the setup:

```sh
just hvigor="node <dir>/node_modules/@ohos/hvigor/bin/hvigor.js" \
    install-packages="python3 tools/install-oh-modules.py" test
```

[hvigor-cargo]: https://github.com/openharmony-rs/hvigor-cargo

## License

This project is licensed under the Apache-2.0 license.
