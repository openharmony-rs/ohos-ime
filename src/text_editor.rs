use crate::text_config::TextConfig;
use crate::{
    utf16, Direction, ExtendAction, Ime, KeyboardStatus, PreviewTextError, PrivateCommand,
};
use log::{debug, error, info, warn};
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
use std::collections::{HashMap, VecDeque};
use std::ptr::NonNull;
use std::sync::{Arc, Mutex, PoisonError, RwLock};

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

/// Number of unused text editors kept before the oldest one is reused.
///
/// The framework keeps raw pointers to text editors, e.g. in callbacks that are in flight, so
/// reusing a recently dropped editor could route those callbacks to the new editor's `Ime`.
pub(crate) const RECYCLE_DELAY: usize = 64;

/// Addresses of unused text editors, oldest first, with the dispatcher callbacks registered.
static RECYCLED: Mutex<VecDeque<usize>> = Mutex::new(VecDeque::new());

pub(crate) fn take_recycled() -> Option<NonNull<InputMethod_TextEditorProxy>> {
    let mut recycled = RECYCLED.lock().unwrap_or_else(PoisonError::into_inner);
    if recycled.len() <= RECYCLE_DELAY {
        return None;
    }
    NonNull::new(recycled.pop_front()? as *mut InputMethod_TextEditorProxy)
}

pub(crate) fn recycle(text_editor_proxy: NonNull<InputMethod_TextEditorProxy>) {
    RECYCLED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push_back(text_editor_proxy.as_ptr() as usize);
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

/// # Safety
///
/// `out_config` must be a valid text config.
pub unsafe extern "C" fn get_text_config(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
    out_config: *mut InputMethod_TextConfig,
) {
    info!("get_text_config: Getting IME text config");
    DISPATCHER.dispatch(text_editor_proxy, |ime| {
        let config = ime.get_text_config();
        // SAFETY: Guaranteed by the caller.
        if let Err(e) = unsafe { apply_text_config(config, out_config) } {
            error!("Failed to apply IME config in `get_text_config`: {e:?}");
        }
    });
}

/// # Safety
///
/// If `text` is not null, it must point to `length` UTF-16 code units.
pub unsafe extern "C" fn insert_text(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
    text: *const u16,
    length: usize,
) {
    // SAFETY: Guaranteed by the caller.
    let Some(text) = (unsafe { slice_or_empty(text, length) }) else {
        error!("insert_text received text slice with len {length} but a null pointer");
        return;
    };
    match String::from_utf16(text) {
        Ok(text) => {
            DISPATCHER.dispatch(text_editor_proxy, |ime| ime.insert_text(text));
        }
        Err(e) => error!("IME `insert_text` received malformed utf-16 string: {e:?} "),
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
    let Some(direction) = Direction::from_raw(direction) else {
        debug!("Ignoring move_cursor with direction {}", direction.0);
        return;
    };
    DISPATCHER.dispatch(text_editor_proxy, |ime| ime.move_cursor(direction));
}

pub extern "C" fn handle_set_selection(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
    start: i32,
    end: i32,
) {
    let (Ok(start), Ok(end)) = (usize::try_from(start), usize::try_from(end)) else {
        warn!("Ignoring set_selection with negative position ({start}, {end})");
        return;
    };
    DISPATCHER.dispatch(text_editor_proxy, |ime| ime.set_selection(start, end));
}

pub extern "C" fn handle_extend_action(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
    action: InputMethod_ExtendAction,
) {
    let Some(action) = ExtendAction::from_raw(action) else {
        warn!("Ignoring unknown extend action {}", action.0);
        return;
    };
    DISPATCHER.dispatch(text_editor_proxy, |ime| ime.handle_extend_action(action));
}

/// Answers a surrounding-text query with at most `number` code units.
///
/// # Safety
///
/// `length` must be null or valid for writes, and if `text` is not null, it must be valid for
/// writing `number` code units.
unsafe fn answer_text_query(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
    number: i32,
    text: *mut u16,
    length: *mut usize,
    query: impl FnOnce(&dyn Ime, usize) -> Vec<u16>,
) {
    if length.is_null() {
        return;
    }
    let units = match usize::try_from(number) {
        Ok(max_len) if !text.is_null() => {
            DISPATCHER.dispatch(text_editor_proxy, |ime| query(ime, max_len))
        }
        _ => None,
    };
    let units = units
        .filter(|units| units.len() <= number as usize)
        .unwrap_or_default();
    // SAFETY: `length` is not null, and `units` is either empty or at most `number` code units
    // long and `text` is not null, as guaranteed by the caller.
    unsafe { utf16::write(&units, text, length) };
}

/// # Safety
///
/// The framework's contract: `length` is null or valid for writes, and `text` is null or valid
/// for writing `number` code units.
pub unsafe extern "C" fn get_left_text_of_cursor(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
    number: i32,
    text: *mut u16,
    length: *mut usize,
) {
    // SAFETY: Guaranteed by the caller.
    unsafe {
        answer_text_query(text_editor_proxy, number, text, length, |ime, max_len| {
            utf16::tail(&ime.text_before_cursor(max_len), max_len)
        })
    };
}

/// # Safety
///
/// See [`get_left_text_of_cursor`].
pub unsafe extern "C" fn get_right_text_of_cursor(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
    number: i32,
    text: *mut u16,
    length: *mut usize,
) {
    // SAFETY: Guaranteed by the caller.
    unsafe {
        answer_text_query(text_editor_proxy, number, text, length, |ime, max_len| {
            utf16::head(&ime.text_after_cursor(max_len), max_len)
        })
    };
}

pub extern "C" fn get_text_index_at_cursor(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
) -> i32 {
    DISPATCHER
        .dispatch(text_editor_proxy, |ime| ime.text_index_at_cursor())
        .map_or(0, |index| i32::try_from(index).unwrap_or(i32::MAX))
}

/// # Safety
///
/// If `private_command` is not null, it must point to `size` command pointers, each null or valid
/// for the duration of the call.
pub unsafe extern "C" fn receive_private_command(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
    private_command: *mut *mut InputMethod_PrivateCommand,
    size: usize,
) -> i32 {
    // SAFETY: Guaranteed by the caller.
    let Some(raw_commands) = (unsafe { slice_or_empty(private_command, size) }) else {
        return 0;
    };
    let commands = raw_commands
        .iter()
        .filter_map(|raw| NonNull::new(*raw))
        // SAFETY: Guaranteed by the caller.
        .filter_map(|raw| unsafe { PrivateCommand::from_raw(raw) })
        .collect();
    DISPATCHER.dispatch(text_editor_proxy, |ime| {
        ime.receive_private_command(commands)
    });
    0
}

/// Value of the framework's `PREVIEW_BAD_PARAMETERS`, which ArkUI text fields return as well.
const PREVIEW_BAD_PARAMETERS: i32 = -1;

/// # Safety
///
/// If `text` is not null, it must point to `length` UTF-16 code units.
pub unsafe extern "C" fn set_preview_text(
    text_editor_proxy: *mut InputMethod_TextEditorProxy,
    text: *const u16,
    length: usize,
    start: i32,
    end: i32,
) -> i32 {
    let range = match (start, end) {
        (-1, -1) => None,
        (start, end) => match (usize::try_from(start), usize::try_from(end)) {
            (Ok(start), Ok(end)) if start <= end => Some(start..end),
            _ => {
                warn!("set_preview_text received invalid range ({start}, {end})");
                return PREVIEW_BAD_PARAMETERS;
            }
        },
    };
    // SAFETY: Guaranteed by the caller.
    let Some(text) = (unsafe { slice_or_empty(text, length) }) else {
        return PREVIEW_BAD_PARAMETERS;
    };
    let text = match String::from_utf16(text) {
        Ok(text) => text,
        Err(e) => {
            error!("IME `set_preview_text` received malformed utf-16 string: {e:?}");
            return PREVIEW_BAD_PARAMETERS;
        }
    };
    match DISPATCHER.dispatch(text_editor_proxy, |ime| ime.set_preview_text(text, range)) {
        Some(Ok(())) => 0,
        Some(Err(PreviewTextError)) | None => PREVIEW_BAD_PARAMETERS,
    }
}

pub extern "C" fn finish_text_preview(text_editor_proxy: *mut InputMethod_TextEditorProxy) {
    DISPATCHER.dispatch(text_editor_proxy, |ime| ime.finish_text_preview());
}

/// Builds a slice from a pointer and length received from the framework.
///
/// Returns `None` for a null pointer with a non-zero length.
///
/// # Safety
///
/// If `ptr` is not null, it must point to `len` initialized elements that stay valid for `'a`.
unsafe fn slice_or_empty<'a, T>(ptr: *const T, len: usize) -> Option<&'a [T]> {
    match NonNull::new(ptr.cast_mut()) {
        // SAFETY: Guaranteed by the caller.
        Some(ptr) => Some(unsafe { std::slice::from_raw_parts(ptr.as_ptr(), len) }),
        None => (len == 0).then_some(&[]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slice_or_empty_handles_null() {
        // SAFETY: Null pointers are never dereferenced.
        unsafe {
            assert_eq!(slice_or_empty::<u16>(std::ptr::null(), 0), Some(&[][..]));
            assert_eq!(slice_or_empty::<u16>(std::ptr::null(), 1), None);
        }
    }
}
