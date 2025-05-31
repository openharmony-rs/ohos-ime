# Changelog

## v0.4.0

- Change Result type
- Change signature of `ImeProxy::new()` and `RawTextEditorProxy::new()` to return a result

## v0.3.0

### Breaking 

- The signatures of `ImeProxy::new()` and `RawTextEditorProxy::new()` were updated. 
  The Ime now needs to be passed when creating the RawTextEditorProxy.

## v0.2.0

### Breaking

- The IME trait was updated to support more features

### Features

- Configuring the IME via TextConfig is now supported (`Ime::get_text_config`)
- Receiving the enter key type is now supported (`Ime::send_enter_key`)

### Fixes

- ImeProxy is now properly unregistered from the dispatcher when it is dropped
