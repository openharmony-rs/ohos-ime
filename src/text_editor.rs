#![allow(unused)]
use crate::Ime;
use log::{debug, error, info, trace, warn};
use ohos_ime_sys::private_command::InputMethod_PrivateCommand;
use ohos_ime_sys::text_config::InputMethod_TextConfig;
use ohos_ime_sys::text_editor_proxy::InputMethod_TextEditorProxy;
use ohos_ime_sys::types::{
    InputMethod_Direction, InputMethod_EnterKeyType, InputMethod_ExtendAction,
    InputMethod_KeyboardStatus,
};
use std::collections::HashMap;
use std::ptr::{slice_from_raw_parts, NonNull};
use std::sync::{RwLock, RwLockReadGuard};

pub(crate) static DISPATCHER: Dispatcher = Dispatcher::new();

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
        let mut map = self.map.write().unwrap();
        info!("Registering ime | IME");
        let res = map
            .get_or_insert_with(HashMap::new)
            .insert(c_proxy.as_ptr() as usize, ime);
        if res.is_some() {
            error!("Double insert of IME text editor?");
            panic!("Double insert of IME text editor?")
        }
    }

    fn insert_text(
        &self,
        text_editor_proxy: *mut InputMethod_TextEditorProxy,
        text: &[u16],
    ) {
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
}

pub extern "C" fn get_text_config(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
    config: *mut InputMethod_TextConfig,
) {
    error!("get_text_config not implemented");
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
    error!("send_keyboard_status not implemented");
}

pub extern "C" fn send_enter_key(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
    enter_key_type: InputMethod_EnterKeyType,
) {
    error!("send_enter_key not implemented");
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
