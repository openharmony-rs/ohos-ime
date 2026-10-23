// TODO:
// - switch to parking lot and uns MutexGuard::map or owning_ref to reduce some of duplicate code here.
#![allow(unused)]
pub use crate::text_config::{TextConfig, TextConfigBuilder};
use crate::{Ime, KeyboardStatus};
use log::{debug, error, info, trace, warn};
use ohos_ime_sys::private_command::InputMethod_PrivateCommand;
use ohos_ime_sys::text_avoid_info::{OH_TextAvoidInfo_SetHeight, OH_TextAvoidInfo_SetPositionY};
use ohos_ime_sys::text_config::{
    InputMethod_TextConfig, OH_TextConfig_GetTextAvoidInfo, OH_TextConfig_SetEnterKeyType,
    OH_TextConfig_SetInputType, OH_TextConfig_SetPreviewTextSupport, OH_TextConfig_SetSelection,
    OH_TextConfig_SetWindowId,
};
use ohos_ime_sys::text_editor_proxy::InputMethod_TextEditorProxy;
use ohos_ime_sys::types::{
    InputMethod_Direction, InputMethod_EnterKeyType, InputMethod_ExtendAction,
    InputMethod_KeyboardStatus,
};
use std::collections::HashMap;
use std::ptr::{slice_from_raw_parts, NonNull};
use std::sync::{Arc, PoisonError, RwLock};

pub(crate) static DISPATCHER: Dispatcher = Dispatcher::new();

#[derive(Debug)]
pub(crate) enum DispatcherError {
    Uninitialized,
    NotFound,
}

pub(crate) struct Dispatcher {
    map: RwLock<Option<HashMap<usize, Arc<dyn super::Ime>>>>,
}

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
        let mut map = self.map.write().unwrap_or_else(PoisonError::into_inner);
        let res = map
            .get_or_insert_with(HashMap::new)
            .insert(c_proxy.as_ptr() as usize, Arc::from(ime));
        if res.is_some() {
            warn!("Double insert of IME text editor. Dropping the old one");
        }
    }

    pub(crate) fn unregister(
        &self,
        c_proxy: NonNull<InputMethod_TextEditorProxy>,
    ) -> Result<Arc<dyn Ime>, DispatcherError> {
        debug!("Unregistering IME");
        let mut map = self.map.write().unwrap_or_else(PoisonError::into_inner);
        map.as_mut()
            .ok_or(DispatcherError::Uninitialized)?
            .remove(&(c_proxy.as_ptr() as usize))
            .ok_or(DispatcherError::NotFound)
    }

    /// Calls `f` with the IME implementation registered for `text_editor_proxy`.
    ///
    /// No lock is held while `f` runs, so the implementation may create or drop editors.
    fn dispatch<R>(
        &self,
        text_editor_proxy: *mut InputMethod_TextEditorProxy,
        f: impl FnOnce(&dyn Ime) -> R,
    ) -> Option<R> {
        let ime = {
            let map = self.map.read().unwrap_or_else(PoisonError::into_inner);
            map.as_ref()
                .and_then(|m| m.get(&(text_editor_proxy as usize)))
                .cloned()
        };
        match ime {
            Some(ime) => Some(f(&*ime)),
            None => {
                // Expected for callbacks that race with dropping the editor.
                debug!("IME dispatcher called, but no IME implementation registered!");
                None
            }
        }
    }
}

#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
pub enum ApplyTextConfigError {
    SetInputTypeFailed,
    SetEnterKeyTypeFailed,
    SetPreviewTextSupportFailed,
    SetSelectioFailed,
    SetWindowIdFailed,
    SetTextAvoidInfoFailed,
}

/// Writes `config` into the text config that the framework passed to `get_text_config`.
///
/// The framework does not initialize the selection, window ID and avoid area, so they are written
/// even if `config` leaves them unset.
///
/// # Safety
///
/// `oh_config` must be a valid text config.
unsafe fn apply_text_config(
    config: &TextConfig,
    oh_config: *mut InputMethod_TextConfig,
) -> Result<(), ApplyTextConfigError> {
    // SAFETY: `oh_config` is valid, as guaranteed by the caller.
    unsafe {
        OH_TextConfig_SetInputType(oh_config, config.input_type)
            .map_err(|_e| ApplyTextConfigError::SetInputTypeFailed)?;

        OH_TextConfig_SetEnterKeyType(oh_config, config.enterkey_type)
            .map_err(|_e| ApplyTextConfigError::SetEnterKeyTypeFailed)?;

        OH_TextConfig_SetPreviewTextSupport(oh_config, config.preview_text_support)
            .map_err(|_e| ApplyTextConfigError::SetPreviewTextSupportFailed)?;

        // -1 is the framework's `INVALID_VALUE`, and 0 its `INVALID_WINDOW_ID`.
        let (start, end) = config
            .selection
            .as_ref()
            .map_or((-1, -1), |selection| (selection.start, selection.end));
        OH_TextConfig_SetSelection(oh_config, start, end)
            .map_err(|_e| ApplyTextConfigError::SetSelectioFailed)?;
        OH_TextConfig_SetWindowId(oh_config, config.window_id.unwrap_or(0))
            .map_err(|_e| ApplyTextConfigError::SetWindowIdFailed)?;
        let mut avoid_info = std::ptr::null_mut();
        OH_TextConfig_GetTextAvoidInfo(oh_config, &raw mut avoid_info)
            .and_then(|()| OH_TextAvoidInfo_SetPositionY(avoid_info, 0.0))
            .and_then(|()| OH_TextAvoidInfo_SetHeight(avoid_info, 0.0))
            .map_err(|_e| ApplyTextConfigError::SetTextAvoidInfoFailed)?;
    }
    Ok(())
}

pub extern "C" fn get_text_config(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
    out_config: *mut InputMethod_TextConfig,
) {
    info!("get_text_config: Getting IME text config");
    DISPATCHER.dispatch(text_editor_proxy, |ime| {
        let config = ime.get_text_config();
        // SAFETY: The framework passes a valid text config.
        if let Err(e) = unsafe { apply_text_config(config, out_config) } {
            error!("Failed to apply IME config in `get_text_config`: {e:?}");
        }
    });
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
            DISPATCHER.dispatch(text_editor_proxy, |ime| {
                let rust_string = String::from_utf16(slice);
                match rust_string {
                    Ok(s) => {
                        ime.insert_text(s);
                    }
                    Err(e) => {
                        error!("IME `insert_text` received malformed utf-16 string: {e:?} ");
                    }
                }
            });
        } else {
            #[cfg(debug_assertions)]
            error!("insert_text received text slice with len {length} but addr {text:?}")
        }
    }
}

pub extern "C" fn delete_forward(text_editor_proxy: *mut InputMethod_TextEditorProxy, length: i32) {
    #[cfg(debug_assertions)]
    if length < 0 {
        log::warn!("delete_forward called with negative length: {}", length);
    }
    DISPATCHER.dispatch(text_editor_proxy, |ime| {
        ime.delete_forward(length.max(0) as usize);
    });
}
pub extern "C" fn delete_backward(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
    length: i32,
) {
    #[cfg(debug_assertions)]
    if length < 0 {
        log::warn!("delete_backward called with negative length: {}", length);
    }
    DISPATCHER.dispatch(text_editor_proxy, |ime| {
        ime.delete_backward(length.max(0) as usize);
    });
}

pub extern "C" fn send_keyboard_status(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
    keyboard_status: InputMethod_KeyboardStatus,
) {
    let status = KeyboardStatus::from(keyboard_status);
    DISPATCHER.dispatch(text_editor_proxy, |ime| ime.keyboard_status_changed(status));
}

pub extern "C" fn send_enter_key(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
    enter_key_type: InputMethod_EnterKeyType,
) {
    DISPATCHER.dispatch(text_editor_proxy, |ime| {
        ime.send_enter_key(enter_key_type);
    });
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
    if !length.is_null() {
        unsafe { *length = 0 };
    }
}

pub extern "C" fn get_right_text_of_cursor(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
    number: i32,
    text: *mut u16,
    length: *mut usize,
) {
    error!("get_right_text_of_cursor not implemented");
    if !length.is_null() {
        unsafe { *length = 0 };
    }
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
