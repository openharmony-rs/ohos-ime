// TODO:
// - switch to parking lot and uns MutexGuard::map or owning_ref to reduce some of duplicate code here.
#![allow(unused)]
pub use crate::text_config::{TextConfig, TextConfigBuilder};
use crate::Ime;
use log::{debug, error, info, trace, warn};
use ohos_ime_sys::private_command::InputMethod_PrivateCommand;
use ohos_ime_sys::text_config::{
    InputMethod_TextConfig, OH_TextConfig_SetEnterKeyType, OH_TextConfig_SetInputType,
    OH_TextConfig_SetPreviewTextSupport, OH_TextConfig_SetSelection, OH_TextConfig_SetWindowId,
};
use ohos_ime_sys::text_editor_proxy::InputMethod_TextEditorProxy;
use ohos_ime_sys::types::{
    InputMethod_Direction, InputMethod_EnterKeyType, InputMethod_ExtendAction,
    InputMethod_KeyboardStatus,
};
use std::collections::HashMap;
use std::ptr::{slice_from_raw_parts, NonNull};
use std::sync::{RwLock, RwLockReadGuard};

pub(crate) static DISPATCHER: Dispatcher = Dispatcher::new();

#[derive(Debug)]
pub(crate) enum DispatcherError {
    Uninitialized,
    NotFound,
    LockPoisoned,
}

pub(crate) struct Dispatcher {
    map: RwLock<Option<HashMap<usize, Box<dyn super::Ime>>>>,
}

// todo: proper error handling, propogation. etc.
impl Dispatcher {
    const fn new() -> Self {
        Self {
            map: RwLock::new(None),
        }
    }

    pub(crate) fn register(
        &self,
        c_proxy: NonNull<InputMethod_TextEditorProxy>,
        ime: Box<dyn Ime>,
    ) {
        debug!("Registering IME");
        // Todo: remove unwrap and make register() fallible.
        let mut map = self.map.write().unwrap();
        let res = map
            .get_or_insert_with(HashMap::new)
            .insert(c_proxy.as_ptr() as usize, ime);
        if res.is_some() {
            warn!("Double insert of IME text editor. Dropping the old one");
        }
    }

    pub(crate) fn unregister(
        &self,
        c_proxy: NonNull<InputMethod_TextEditorProxy>,
    ) -> Result<Box<dyn Ime>, DispatcherError> {
        debug!("Unregistering IME");
        let mut map = self
            .map
            .write()
            .map_err(|_| DispatcherError::LockPoisoned)?;
        map.as_mut()
            .ok_or(DispatcherError::Uninitialized)?
            .remove(&(c_proxy.as_ptr() as usize))
            .ok_or(DispatcherError::NotFound)
    }

    fn insert_text(&self, text_editor_proxy: *mut InputMethod_TextEditorProxy, text: &[u16]) {
        let map = self.map.read().unwrap();
        let ime = map
            .as_ref()
            .and_then(|m| m.get(&(text_editor_proxy as usize)));
        match ime {
            Some(ime) => {
                let rust_string = String::from_utf16(text);
                match rust_string {
                    Ok(s) => {
                        ime.insert_text(s);
                    }
                    Err(e) => {
                        error!("IME `insert_text` received malformed utf-16 string: {e:?} ");
                    }
                }

                let rust_text = String::new();
                ime.insert_text(rust_text)
            }
            None => {
                error!("IME dispatcher called, but no IME implementation registered!")
            }
        }
    }

    fn delete_forward(&self, text_editor_proxy: *mut InputMethod_TextEditorProxy, length: i32) {
        let map = self.map.read().unwrap();
        let ime = map
            .as_ref()
            .and_then(|m| m.get(&(text_editor_proxy as usize)));
        match ime {
            Some(ime) => {
                ime.delete_forward(length.max(0) as usize);
            }
            None => {
                error!("IME dispatcher called, but no IME implementation registered!")
            }
        }
    }

    fn delete_backward(&self, text_editor_proxy: *mut InputMethod_TextEditorProxy, length: i32) {
        let map = self.map.read().unwrap();
        let ime = map
            .as_ref()
            .and_then(|m| m.get(&(text_editor_proxy as usize)));
        match ime {
            Some(ime) => {
                ime.delete_backward(length.max(0) as usize);
            }
            None => {
                error!("IME dispatcher called, but no IME implementation registered!")
            }
        }
    }

    fn get_text_config(
        &self,
        text_editor_proxy: *mut InputMethod_TextEditorProxy,
        oh_config: *mut InputMethod_TextConfig,
    ) {
        let map = self.map.read().unwrap();
        let ime = map
            .as_ref()
            .and_then(|m| m.get(&(text_editor_proxy as usize)));
        match ime {
            Some(ime) => {
                let config = ime.get_text_config();
                if let Err(e) = apply_text_config(config, oh_config) {
                    error!("Failed to apply IME config in `get_text_config`: {e:?}");
                }
            }
            None => {
                error!("IME dispatcher called, but no IME implementation registered!")
            }
        }
    }

    fn send_enter_key(
        &self,
        text_editor_proxy: *mut InputMethod_TextEditorProxy,
        enter_key_type: InputMethod_EnterKeyType,
    ) {
        let map = self.map.read().unwrap();
        let ime = map
            .as_ref()
            .and_then(|m| m.get(&(text_editor_proxy as usize)));
        match ime {
            Some(ime) => {
                ime.send_enter_key(enter_key_type);
            }
            None => {
                error!("IME dispatcher called, but no IME implementation registered!")
            }
        }
    }
}

#[derive(Debug)]
pub enum ApplyTextConfigError {
    SetInputTypeFailed,
    SetEnterKeyTypeFailed,
    SetPreviewTextSupportFailed,
    SetSelectioFailed,
    SetWindowIdFailed,
}

fn apply_text_config(
    config: &TextConfig,
    oh_config: *mut InputMethod_TextConfig,
) -> Result<(), ApplyTextConfigError> {
    unsafe {
        let res = OH_TextConfig_SetInputType(oh_config, config.input_type.clone());
        if !res.is_ok() {
            return Err(ApplyTextConfigError::SetInputTypeFailed);
        }
        let res = OH_TextConfig_SetEnterKeyType(oh_config, config.enterkey_type.clone());
        if !res.is_ok() {
            return Err(ApplyTextConfigError::SetEnterKeyTypeFailed);
        }
        let res = OH_TextConfig_SetPreviewTextSupport(oh_config, config.preview_text_support);
        if !res.is_ok() {
            return Err(ApplyTextConfigError::SetPreviewTextSupportFailed);
        }
        if let Some(selection) = &config.selection {
            let res = OH_TextConfig_SetSelection(oh_config, selection.start, selection.end);
            if !res.is_ok() {
                return Err(ApplyTextConfigError::SetSelectioFailed);
            }
        }
        if let Some(window_id) = config.window_id {
            // let's see if this is optional...
            let res = OH_TextConfig_SetWindowId(oh_config, window_id);
            if !res.is_ok() {
                return Err(ApplyTextConfigError::SetWindowIdFailed);
            }
        }
    }
    Ok(())
}

pub extern "C" fn get_text_config(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
    config: *mut InputMethod_TextConfig,
) {
    info!("get_text_config: Getting IME text config");
    DISPATCHER.get_text_config(text_editor_proxy, config);
}

pub extern "C" fn insert_text(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
    text: *const u16,
    // `length` % 2 == 0 does not hold, so this seems to be number u16 codepoints.
    length: usize,
) {
    if length > 0 {
        let utf16_str = slice_from_raw_parts(text, length);
        // SAFETY: We trust the OH APIs to give us a valid u16 slice
        if let Some(slice) = unsafe { utf16_str.as_ref() } {
            DISPATCHER.insert_text(text_editor_proxy, slice);
        } else {
            #[cfg(debug_assertions)]
            error!("insert_text received text slice with len {length} but addr {text:?}")
        }
    }
}

pub extern "C" fn delete_forward(text_editor_proxy: *mut InputMethod_TextEditorProxy, length: i32) {
    DISPATCHER.delete_forward(text_editor_proxy, length);
}
pub extern "C" fn delete_backward(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
    length: i32,
) {
    DISPATCHER.delete_backward(text_editor_proxy, length);
}

pub extern "C" fn send_keyboard_status(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
    keyboard_status: InputMethod_KeyboardStatus,
) {
    error!(
        "send_keyboard_status not implemented. IME keyboard status: {}",
        keyboard_status.0
    );
}

pub extern "C" fn send_enter_key(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
    enter_key_type: InputMethod_EnterKeyType,
) {
    DISPATCHER.send_enter_key(text_editor_proxy, enter_key_type);
}

pub extern "C" fn move_cursor(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
    direction: InputMethod_Direction,
) {
    error!("move_cursor not implemented");
}

pub extern "C" fn handle_set_selection(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
    start: i32,
    end: i32,
) {
    error!("handle_text_selection not implemented");
}

pub extern "C" fn handle_extend_action(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
    action: InputMethod_ExtendAction,
) {
    error!("handle_extend_action not implemented");
}

pub extern "C" fn get_left_text_of_cursor(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
    number: i32,
    text: *mut u16,
    length: *mut usize,
) {
    error!("get_left_text_of_cursor not implemented");
}

pub extern "C" fn get_right_text_of_cursor(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
    number: i32,
    text: *mut u16,
    length: *mut usize,
) {
    error!("get_right_text_of_cursor not implemented");
}

pub extern "C" fn get_text_index_at_cursor(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
) -> i32 {
    error!("get_text_index_at_cursor stubbed");
    0
}

pub extern "C" fn receive_private_command(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
    private_command: *mut *mut InputMethod_PrivateCommand,
    size: usize,
) -> i32 {
    error!("receive_private_command not implemented");
    if !private_command.is_null() {
        unsafe {
            *private_command = core::ptr::null_mut();
        }
    }
    0
}

pub extern "C" fn set_preview_text(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
    text: *const u16,
    length: usize,
    start: i32,
    end: i32,
) -> i32 {
    error!("set_preview_text not implemented");
    0
}

pub extern "C" fn finish_text_preview(text_editor_proxy: *mut InputMethod_TextEditorProxy) {
    error!("finish_text_preview not implemented");
}
