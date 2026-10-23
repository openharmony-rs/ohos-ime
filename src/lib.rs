//! Safe abstractions to use IME (inputmethods) from Rust on OpenHarmony
//!
//! This crate provides an [`ImeProxy`], which allows interacting with the Input method on OpenHarmony
//! devices. The user needs to implement the [`Ime`] trait
//!
//! This crate is still under active development and based on the
//! [InputMethod C-API] of OpenHarmony.
//!
//! [InputMethod C-API]: https://docs.openharmony.cn/pages/v5.0/zh-cn/application-dev/reference/apis-ime-kit/_input_method.md
//!
//! ## Usage
//!
//! 1. Implement the Ime trait
//! 2. call `ImeProxy::new()`
mod text_config;
mod text_editor;

pub use crate::text_config::{TextConfig, TextConfigBuilder, TextSelection};
use crate::text_editor::DISPATCHER;
use log::{error, warn};
use ohos_ime_sys::attach_options::{
    InputMethod_AttachOptions, OH_AttachOptions_Create, OH_AttachOptions_Destroy,
    OH_AttachOptions_IsShowKeyboard,
};
use ohos_ime_sys::controller::{OH_InputMethodController_Attach, OH_InputMethodController_Detach};
use ohos_ime_sys::inputmethod_proxy::{
    InputMethod_InputMethodProxy, OH_InputMethodProxy_HideKeyboard,
    OH_InputMethodProxy_NotifyCursorUpdate, OH_InputMethodProxy_ShowKeyboard,
};
use ohos_ime_sys::text_editor_proxy::{
    InputMethod_TextEditorProxy, OH_TextEditorProxy_Create, OH_TextEditorProxy_Destroy,
    OH_TextEditorProxy_SetDeleteBackwardFunc, OH_TextEditorProxy_SetDeleteForwardFunc,
    OH_TextEditorProxy_SetFinishTextPreviewFunc, OH_TextEditorProxy_SetGetLeftTextOfCursorFunc,
    OH_TextEditorProxy_SetGetRightTextOfCursorFunc, OH_TextEditorProxy_SetGetTextConfigFunc,
    OH_TextEditorProxy_SetGetTextIndexAtCursorFunc, OH_TextEditorProxy_SetHandleExtendActionFunc,
    OH_TextEditorProxy_SetHandleSetSelectionFunc, OH_TextEditorProxy_SetInsertTextFunc,
    OH_TextEditorProxy_SetMoveCursorFunc, OH_TextEditorProxy_SetReceivePrivateCommandFunc,
    OH_TextEditorProxy_SetSendEnterKeyFunc, OH_TextEditorProxy_SetSendKeyboardStatusFunc,
    OH_TextEditorProxy_SetSetPreviewTextFunc,
};
use ohos_ime_sys::types::{
    InputMethodErrorCode, InputMethodResult, InputMethod_EnterKeyType, InputMethod_KeyboardStatus,
};
use std::fmt::Debug;
use std::ptr::NonNull;
use std::sync::{Mutex, MutexGuard, PoisonError};

// Todo: Well, honestly we really need to clarify the required sematics on the IME.
/// User implementation of required Inputmethod functionality
pub trait Ime: Send + Sync {
    /// Insert `text` at the current cursor position.
    fn insert_text(&self, text: String);
    /// Delete the next `len` `char`s(?) starting at the current cursor position
    fn delete_forward(&self, len: usize);

    /// Delete the previous `len` `char`s(?) before the current cursor position
    fn delete_backward(&self, len: usize);

    /// Return the text configuration associated with the current IME
    fn get_text_config(&self) -> &TextConfig;

    /// Process the enter key variant pressed by the user.
    ///
    /// Depending on the configuration (applied by the implementation of [`get_text_config()`])
    /// the enterkey label displayed to the user varies.
    /// This function will be called when the enter key is pressed and the associated label
    /// is passed, so that the application can handle it accordingly.
    fn send_enter_key(&self, enter_key: InputMethod_EnterKeyType);

    /// Called when the status of IME virtual keyboard changes.
    fn keyboard_status_changed(&self, status: KeyboardStatus) {
        log::debug!("Keyboard status changed to {:?}", status);
    }
    // ...
}

/// Address of the text editor that this crate last attached, if it has not been detached since.
///
/// The input method framework attaches at most one text editor per process: attaching another
/// editor invalidates the previous `InputMethod_InputMethodProxy`.
static ATTACHED_EDITOR: Mutex<Option<usize>> = Mutex::new(None);

/// Serializes attaching, so that `ATTACHED_EDITOR` names the editor the framework attached last.
/// Holds the address of the input method proxy of that attach.
///
/// Separate from `ATTACHED_EDITOR`, which callbacks that run during an attach may need.
static ATTACH_LOCK: Mutex<usize> = Mutex::new(0);

fn attached_editor() -> MutexGuard<'static, Option<usize>> {
    ATTACHED_EDITOR
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// A text editor attached to the input method.
///
/// Only one editor is attached at a time. Attaching another editor detaches this one, usually
/// even if that attach fails: its methods then fail with [`InputMethodErrorCode::DETACHED`] until
/// it is [re-attached](Self::attach), and dropping it leaves the other editor attached.
///
/// The framework may also detach the editor by itself, e.g. when an ArkUI text field gains
/// focus. Methods then fail with [`InputMethodErrorCode::DETACHED`] too.
///
/// Dropping the attached `ImeProxy` detaches the editor, unless the framework already detached
/// it. The C API cannot check and detach atomically, so if an ArkUI text field attaches between
/// the two, its input session is closed. ArkUI attaches on the main thread, so drop the
/// `ImeProxy` on the main thread to avoid this.
pub struct ImeProxy {
    raw: NonNull<InputMethod_InputMethodProxy>,
    editor: RawTextEditorProxy,
}

impl Drop for ImeProxy {
    fn drop(&mut self) {
        {
            let mut attached = attached_editor();
            if *attached != Some(self.editor.addr()) {
                return;
            }
            *attached = None;
        }
        let raw = self.raw.as_ptr();
        // If the framework detached the editor by itself, `Detach` would end the input session
        // of the text field that took over, so only detach if the editor is still attached.
        // SAFETY: `raw` is the proxy of our last attach. If another editor was attached since we
        // checked, the framework rejects `raw` by comparing it to its current proxy; it never
        // dereferences the pointer.
        if unsafe { OH_InputMethodProxy_HideKeyboard(raw) } == Err(InputMethodErrorCode::DETACHED) {
            return;
        }
        // SAFETY: As above. Detach may synchronously call back into the editor, which stays
        // registered until `self.editor` is dropped after this function.
        if let Err(e) = unsafe { OH_InputMethodController_Detach(raw) } {
            error!("IME: Detach failed for InputMethodController {:?}", e);
        }
    }
}

pub struct CreateImeProxyError {
    pub editor: RawTextEditorProxy,
    pub options: AttachOptions,
    pub error_code: InputMethodErrorCode,
}

impl Debug for CreateImeProxyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("InputMethodErrorCode: {:?}", self.error_code))
    }
}

/// Attaches `editor`, requesting the text config from its [`Ime`] on the calling thread.
fn attach_editor(
    editor: &RawTextEditorProxy,
    options: &AttachOptions,
) -> Result<NonNull<InputMethod_InputMethodProxy>, InputMethodErrorCode> {
    let mut attached_proxy = ATTACH_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let mut ime_proxy: *mut InputMethod_InputMethodProxy = core::ptr::null_mut();
    // SAFETY: Both pointers are valid, and text editors are never destroyed, so the editor
    // outlives every reference the framework keeps to it.
    let result = unsafe {
        OH_InputMethodController_Attach(
            editor.raw.as_ptr(),
            options.raw.as_ptr(),
            &raw mut ime_proxy,
        )
    };
    if let Err(e) = result {
        // The framework replaces the proxy of the previous editor before it attaches, so a failed
        // attach usually detached that editor too.
        let was_attached = attached_editor().is_some();
        if was_attached && !framework_accepts(*attached_proxy) {
            *attached_editor() = None;
        }
        return Err(e);
    }
    *attached_proxy = ime_proxy as usize;
    *attached_editor() = Some(editor.addr());
    // We checked the returncode above, so the pointer should be valid and non-null now.
    Ok(NonNull::new(ime_proxy).expect("Wrong Errorcode"))
}

/// Whether the framework still accepts `proxy` as the proxy of an attached editor.
fn framework_accepts(proxy: usize) -> bool {
    // SAFETY: The framework compares `proxy` to its current proxy and returns before it uses it or
    // reads the null cursor info, so this notifies nothing.
    let result = unsafe {
        OH_InputMethodProxy_NotifyCursorUpdate(
            proxy as *mut InputMethod_InputMethodProxy,
            core::ptr::null_mut(),
        )
    };
    result == Err(InputMethodErrorCode::NULL_POINTER)
}

impl ImeProxy {
    /// Attaches `editor` to the input method, detaching any other editor.
    ///
    /// The [`Ime`] of `editor` is asked for its [`TextConfig`] on the calling thread.
    pub fn new(
        editor: RawTextEditorProxy,
        options: AttachOptions,
    ) -> Result<Self, CreateImeProxyError> {
        match attach_editor(&editor, &options) {
            Ok(raw) => Ok(Self { raw, editor }),
            Err(error_code) => Err(CreateImeProxyError {
                editor,
                options,
                error_code,
            }),
        }
    }

    /// Re-attaches the editor, e.g. after it was detached by another editor or the framework.
    ///
    /// The [`Ime`] is asked for its [`TextConfig`] again, on the calling thread.
    pub fn attach(&mut self, options: &AttachOptions) -> InputMethodResult {
        self.raw = attach_editor(&self.editor, options)?;
        Ok(())
    }

    /// Calls `f` with the input method proxy if this editor is the attached one.
    fn with_raw(
        &self,
        f: impl FnOnce(*mut InputMethod_InputMethodProxy) -> InputMethodResult,
    ) -> InputMethodResult {
        let attached = attached_editor();
        if *attached != Some(self.editor.addr()) {
            return Err(InputMethodErrorCode::DETACHED);
        }
        f(self.raw.as_ptr())
    }

    pub fn show_keyboard(&self) -> InputMethodResult {
        // SAFETY: `with_raw` only passes a proxy that has not been invalidated by another attach.
        self.with_raw(|raw| unsafe { OH_InputMethodProxy_ShowKeyboard(raw) })
    }

    pub fn hide_keyboard(&self) -> InputMethodResult {
        // SAFETY: See `show_keyboard`.
        self.with_raw(|raw| unsafe { OH_InputMethodProxy_HideKeyboard(raw) })
    }
}

pub struct AttachOptions {
    raw: NonNull<InputMethod_AttachOptions>,
}

pub enum KeyboardVisibility {
    Hide,
    Show,
}

impl AttachOptions {
    pub fn new(show_keyboard: bool) -> Self {
        // SAFETY: No particular safety or other requirements.
        // Only documented failure reason is insufficient Memory
        let raw = unsafe {
            let raw = OH_AttachOptions_Create(show_keyboard);
            NonNull::new(raw).expect("OOM?")
        };
        Self { raw }
    }

    pub fn get_visibility(&self) -> KeyboardVisibility {
        let mut show_keyboard: u8 = 0;
        const _: () = assert!(size_of::<u8>() == size_of::<bool>());
        // SAFETY: We can guarantee self.raw is valid (neither copy, nor clone, private).
        // We also asserted that bool and `u8` have the same layout, and do not rely on the
        // C-side writing a valid bool.
        unsafe {
            let err =
                OH_AttachOptions_IsShowKeyboard(self.raw.as_ptr(), (&raw mut show_keyboard).cast());
            // The only documented failure condition is passing a nullpointer, which is impossible for
            // us since we use NonNull, so we don't check the result in release mode.
            debug_assert!(err.is_ok());
            // We don't want to rely on OH_AttachOptions_IsShowKeyboard writing a valid bool,
            // so we check the raw `u8` value.
            if show_keyboard == 0 {
                KeyboardVisibility::Hide
            } else {
                KeyboardVisibility::Show
            }
        }
    }
}

impl Drop for AttachOptions {
    fn drop(&mut self) {
        // SAFETY: Type is neither copy nor clone, raw is private, so our pointer is unique
        // and had no opportunity to leak.
        unsafe {
            OH_AttachOptions_Destroy(self.raw.as_ptr());
        }
    }
}

/// An [`Ime`] implementation registered with a native text editor, ready to be attached
/// with [`ImeProxy::new`].
pub struct RawTextEditorProxy {
    raw: NonNull<InputMethod_TextEditorProxy>,
}

#[derive(Debug)]
pub enum CreateTextEditorProxyErrorKind {
    /// Indicates Out of Memory situation
    CreateProxyFailed,
    /// Registering the C callbacks failed.
    RegisterCallbacksFailed(InputMethodErrorCode),
}
pub struct CreateTextEditorProxyError {
    /// Returns the ime passed to [`RawTextEditorProxy::new`].
    pub ime: Box<dyn Ime>,
    pub reason: CreateTextEditorProxyErrorKind,
}

impl Debug for CreateTextEditorProxyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{:?}", self.reason))
    }
}

impl RawTextEditorProxy {
    pub fn new(ime: Box<dyn Ime>) -> Result<Self, CreateTextEditorProxyError> {
        let raw = match text_editor::take_recycled() {
            Some(raw) => raw,
            None => match create_text_editor() {
                Ok(raw) => raw,
                Err(reason) => return Err(CreateTextEditorProxyError { ime, reason }),
            },
        };
        DISPATCHER.register(raw, ime);
        Ok(Self { raw })
    }

    fn addr(&self) -> usize {
        self.raw.as_ptr() as usize
    }
}

fn create_text_editor(
) -> Result<NonNull<InputMethod_TextEditorProxy>, CreateTextEditorProxyErrorKind> {
    // SAFETY: No preconditions.
    let raw = NonNull::new(unsafe { OH_TextEditorProxy_Create() })
        .ok_or(CreateTextEditorProxyErrorKind::CreateProxyFailed)?;
    if let Err(e) = register_dispatcher_callbacks(raw) {
        // SAFETY: The editor was never attached, so nothing else references it.
        unsafe { OH_TextEditorProxy_Destroy(raw.as_ptr()) };
        return Err(CreateTextEditorProxyErrorKind::RegisterCallbacksFailed(e));
    }
    Ok(raw)
}

fn register_dispatcher_callbacks(raw: NonNull<InputMethod_TextEditorProxy>) -> InputMethodResult {
    use text_editor::*;
    unsafe {
        OH_TextEditorProxy_SetGetTextConfigFunc(raw.as_ptr(), Some(get_text_config))?;
        OH_TextEditorProxy_SetInsertTextFunc(raw.as_ptr(), Some(insert_text))?;
        OH_TextEditorProxy_SetDeleteForwardFunc(raw.as_ptr(), Some(delete_forward))?;
        OH_TextEditorProxy_SetDeleteBackwardFunc(raw.as_ptr(), Some(delete_backward))?;
        OH_TextEditorProxy_SetSendKeyboardStatusFunc(raw.as_ptr(), Some(send_keyboard_status))?;
        OH_TextEditorProxy_SetSendEnterKeyFunc(raw.as_ptr(), Some(send_enter_key))?;
        OH_TextEditorProxy_SetMoveCursorFunc(raw.as_ptr(), Some(move_cursor))?;
        OH_TextEditorProxy_SetHandleSetSelectionFunc(raw.as_ptr(), Some(handle_set_selection))?;
        OH_TextEditorProxy_SetHandleExtendActionFunc(raw.as_ptr(), Some(handle_extend_action))?;
        OH_TextEditorProxy_SetGetLeftTextOfCursorFunc(raw.as_ptr(), Some(get_left_text_of_cursor))?;
        OH_TextEditorProxy_SetGetRightTextOfCursorFunc(
            raw.as_ptr(),
            Some(get_right_text_of_cursor),
        )?;
        OH_TextEditorProxy_SetGetTextIndexAtCursorFunc(
            raw.as_ptr(),
            Some(get_text_index_at_cursor),
        )?;
        OH_TextEditorProxy_SetReceivePrivateCommandFunc(
            raw.as_ptr(),
            Some(receive_private_command),
        )?;
        OH_TextEditorProxy_SetSetPreviewTextFunc(raw.as_ptr(), Some(set_preview_text))?;
        OH_TextEditorProxy_SetFinishTextPreviewFunc(raw.as_ptr(), Some(finish_text_preview))?;
        Ok(())
    }
}

impl Drop for RawTextEditorProxy {
    fn drop(&mut self) {
        let res = DISPATCHER.unregister(self.raw);
        #[cfg(debug_assertions)]
        if let Err(e) = res {
            error!("IME: RawTextEditorProxy unregister failed {:?}", e);
        }
        #[cfg(not(debug_assertions))]
        drop(res);
        // The framework may still reference the editor after `Detach`, e.g. from callbacks that are
        // in flight, so it is recycled instead of destroyed.
        text_editor::recycle(self.raw);
    }
}

#[derive(Copy, Clone, Debug)]
pub enum KeyboardStatus {
    None,
    Hidden,
    Shown,
    Unknown(u32),
}

impl From<InputMethod_KeyboardStatus> for KeyboardStatus {
    fn from(status: InputMethod_KeyboardStatus) -> Self {
        match status {
            status if status == InputMethod_KeyboardStatus::IME_KEYBOARD_STATUS_NONE => {
                KeyboardStatus::None
            }
            status if status == InputMethod_KeyboardStatus::IME_KEYBOARD_STATUS_HIDE => {
                KeyboardStatus::Hidden
            }
            status if status == InputMethod_KeyboardStatus::IME_KEYBOARD_STATUS_SHOW => {
                KeyboardStatus::Shown
            }
            status => {
                warn!("Unknown keyboard status enum variant: {}", status.0);
                KeyboardStatus::Unknown(status.0)
            }
        }
    }
}
