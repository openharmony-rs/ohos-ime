# v0.2.0

## Breaking

- The IME trait was updated to support more features

## Features

- Configuring the IME via TextConfig is now supported (`Ime::get_text_config`)
- Receiving the enter key type is now supported (`Ime::send_enter_key`)

## Fixes

- ImeProxy is now properly unregistered from the dispatcher when it is dropped