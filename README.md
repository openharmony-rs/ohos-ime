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

Attaching to the input method needs a focused application window, so the tests only cover
the callback dispatch and conversions. Check input method behavior in an application.

[cargo-ohos]: https://github.com/openharmony-rs/cargo-ohos
[ohos-test-runner]: https://github.com/openharmony-rs/ohos-test-runner

## License

This project is licensed under the Apache-2.0 license.
