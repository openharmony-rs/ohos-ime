# Changelog

## Unreleased

- Fix `insert_text` being called a second time with an empty string after every insertion, and
  empty insertions from the input method being dropped. An empty insertion now always comes from
  the input method and replaces the selection or preview text, so embedders should not ignore it.
- Fix the surrounding text queries answering a NUL character, `receive_private_command`
  leaking a command, and an ignored failure to set the selection.
- Always write the selection, window ID and avoid area (with `api-20` also the placeholder and
  ability name) into the text config: the framework leaves unset fields uninitialized.
- The input method framework attaches one editor per process. Creating an `ImeProxy` for another
  editor makes the previous one inert (its methods return `DETACHED`) instead of leaving it with
  a dangling pointer, and dropping it no longer detaches the other editor.
  Dropping an `ImeProxy` that the framework already detached (e.g. because an ArkUI text field
  took focus) no longer ends the input session of that text field. Drop it on the main thread to
  also avoid a race with ArkUI attaching.
- Add `ImeProxy::attach()` to re-attach an editor, e.g. after it was detached.
- Callbacks no longer run with the dispatcher lock held, so they may create or drop editors,
  except for callbacks that run synchronously during an attach or detach.
- Implement the remaining callbacks as provided `Ime` methods: `move_cursor`, `set_selection`,
  `handle_extend_action`, `text_before_cursor`, `text_after_cursor`, `text_index_at_cursor`,
  `set_preview_text`, `finish_text_preview` and `receive_private_command`.
- Add `ImeProxy::notify_selection_change()`, `notify_configuration_change()`,
  `notify_cursor_update()` and `send_private_command()`.
- Add `api-15`, `api-20`, `api-22`, `api-23` and `api-26` features for the keyboard request
  reason and `show_text_input`, the placeholder and ability name, callbacks on the main thread,
  attaching with a UI context, and `consume_key_events`.
- Re-export the `ohos-ime-sys` types used in the API, and with `api-23` `ArkUI_ContextHandle`.
- Derive `Debug` and `PartialEq` for `KeyboardVisibility`, `PartialEq` for `KeyboardStatus` and
  `Debug` for `InvalidSelection`.
- Document that positions are UTF-16 code units, that delete lengths count characters, and on
  which threads callbacks run.

## v0.4.2

- Fix a bug where OH_InputMethodController_Detach was not called before `OH_TextEditorProxy_Destroy`,
  which caused segfaults in libinputmethod in some situations.

## v0.4.1

- Update `Ime` trait with `keyboard_status_changed()`, which is called when the keyboard status changes.
  A default implementation is provided, so this is not a breaking change.
- Fix a warning in get_left_text_of_cursor().

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
