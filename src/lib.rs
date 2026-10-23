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
//! 2. Wrap it in a [`RawTextEditorProxy`]
//! 3. call `ImeProxy::new()`
//!
//! ```no_run
//! use ohos_ime::{AttachOptions, Ime, ImeProxy, InputMethod_EnterKeyType, RawTextEditorProxy};
//! use ohos_ime::{TextConfig, TextConfigBuilder};
//!
//! struct Editor {
//!     config: TextConfig,
//! }
//!
//! impl Ime for Editor {
//!     fn insert_text(&self, text: String) {
//!         println!("insert {text:?}");
//!     }
//!     fn delete_forward(&self, len: usize) {}
//!     fn delete_backward(&self, len: usize) {}
//!     fn get_text_config(&self) -> &TextConfig {
//!         &self.config
//!     }
//!     fn send_enter_key(&self, enter_key: InputMethod_EnterKeyType) {}
//! }
//!
//! let editor = Editor {
//!     config: TextConfigBuilder::new().build(),
//! };
//! let editor = RawTextEditorProxy::new(Box::new(editor)).expect("out of memory");
//! let proxy = ImeProxy::new(editor, AttachOptions::new(true)).expect("attach failed");
//! // Dropping the proxy detaches the editor.
//! drop(proxy);
//! ```
//!
//! ## Text positions
//!
//! All indices, ranges and text lengths exchanged with the input method are in UTF-16 code units,
//! like the DOM's `selectionStart` and `selectionEnd`. Only the lengths of
//! [`Ime::delete_forward`] and [`Ime::delete_backward`] count characters.
//!
//! ## Threading
//!
//! [`Ime`] callbacks run on IPC threads of the input method framework, possibly concurrently, unless
//! the editor is configured to receive them on the main thread (API 22). The exception is
//! [`Ime::get_text_config`], which runs on the thread that attaches the editor.
//!
//! Callbacks that return a value are answered while the input method waits. Answer them from
//! editor state cached in the [`Ime`] implementation instead of waiting for another event loop.
//!
//! Some callbacks run synchronously inside [`ImeProxy::new`], [`ImeProxy::attach`] and the drop
//! of an [`ImeProxy`], on the calling thread: [`Ime::get_text_config`], and
//! [`Ime::finish_text_preview`] and [`Ime::keyboard_status_changed`] of the editor that gets
//! detached. These must not create, attach or drop an [`ImeProxy`], or call its methods.
mod private_command;
#[cfg(test)]
mod tests;
mod text_config;
mod text_editor;
mod utf16;

pub use crate::private_command::{PrivateCommand, PrivateCommandValue, MAX_PRIVATE_COMMANDS};
pub use crate::text_config::{TextConfig, TextConfigBuilder, TextSelection};
use crate::text_editor::DISPATCHER;
use log::{error, warn};
use ohos_ime_sys::attach_options::{
    InputMethod_AttachOptions, OH_AttachOptions_Create, OH_AttachOptions_Destroy,
    OH_AttachOptions_IsShowKeyboard,
};
use ohos_ime_sys::controller::{OH_InputMethodController_Attach, OH_InputMethodController_Detach};
use ohos_ime_sys::cursor_info::{OH_CursorInfo_Create, OH_CursorInfo_Destroy};
use ohos_ime_sys::inputmethod_proxy::{
    InputMethod_InputMethodProxy, OH_InputMethodProxy_HideKeyboard,
    OH_InputMethodProxy_NotifyConfigurationChange, OH_InputMethodProxy_NotifyCursorUpdate,
    OH_InputMethodProxy_NotifySelectionChange, OH_InputMethodProxy_SendPrivateCommand,
    OH_InputMethodProxy_ShowKeyboard,
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
#[cfg(feature = "api-15")]
use ohos_ime_sys::types::InputMethod_RequestKeyboardReason;
pub use ohos_ime_sys::types::{
    InputMethodErrorCode, InputMethodResult, InputMethod_EnterKeyType, InputMethod_TextInputType,
};
use ohos_ime_sys::types::{
    InputMethod_Direction, InputMethod_ExtendAction, InputMethod_KeyboardStatus,
};
#[cfg(feature = "api-23")]
pub use ohos_sys_opaque_types::ArkUI_ContextHandle;
use std::ffi::c_int;
#[cfg(feature = "api-15")]
use std::ffi::c_uint;
use std::fmt::Debug;
use std::ops::Range;
use std::ptr::NonNull;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// User implementation of required Inputmethod functionality
///
/// Only the required methods are needed for basic text input. The provided methods ignore the
/// request, or answer that there is no text around the cursor.
pub trait Ime: Send + Sync {
    /// Insert `text` at the current cursor position, replacing the selection or preview text.
    ///
    /// `text` may be empty.
    fn insert_text(&self, text: String);
    /// Delete `len` characters after the cursor.
    ///
    /// ArkUI text fields count code points, and an emoji sequence as one character.
    fn delete_forward(&self, len: usize);

    /// Delete `len` characters before the cursor.
    ///
    /// ArkUI text fields count code points, and an emoji sequence as one character.
    fn delete_backward(&self, len: usize);

    /// Return the text configuration associated with the current IME
    ///
    /// Called on the attaching thread each time the editor is attached, see
    /// [Threading](crate#threading).
    fn get_text_config(&self) -> &TextConfig;

    /// Process the enter key variant pressed by the user.
    ///
    /// Depending on the configuration (applied by the implementation of
    /// [`get_text_config()`](Self::get_text_config)) the enterkey label displayed to the user
    /// varies.
    /// This function will be called when the enter key is pressed and the associated label
    /// is passed, so that the application can handle it accordingly.
    fn send_enter_key(&self, enter_key: InputMethod_EnterKeyType);

    /// Called when the status of IME virtual keyboard changes.
    ///
    /// Detaching the editor may report [`KeyboardStatus::Hidden`] synchronously, on the detaching
    /// thread.
    fn keyboard_status_changed(&self, status: KeyboardStatus) {
        log::debug!("Keyboard status changed to {:?}", status);
    }

    /// Move the cursor by one step, e.g. for arrow keys handled by the input method.
    fn move_cursor(&self, direction: Direction) {
        log::debug!("Ignoring move_cursor({direction:?})");
    }

    /// Select the text from `start` to `end`.
    fn set_selection(&self, start: usize, end: usize) {
        log::debug!("Ignoring set_selection({start}, {end})");
    }

    /// Perform an editing action, like copying the selection.
    fn handle_extend_action(&self, action: ExtendAction) {
        log::debug!("Ignoring handle_extend_action({action:?})");
    }

    /// Return up to `max_len` UTF-16 code units of the text before the cursor.
    ///
    /// Longer text is truncated at its start.
    fn text_before_cursor(&self, _max_len: usize) -> String {
        String::new()
    }

    /// Return up to `max_len` UTF-16 code units of the text after the cursor.
    ///
    /// Longer text is truncated at its end.
    fn text_after_cursor(&self, _max_len: usize) -> String {
        String::new()
    }

    /// Return the position of the cursor in the text.
    fn text_index_at_cursor(&self) -> usize {
        0
    }

    /// Show `text` as preview text, which the input method may still change, e.g. while
    /// composing CJK text.
    ///
    /// `range` is the text that `text` replaces. `None` replaces the current preview text, or
    /// inserts at the cursor if there is none. If `range` is empty, `text` is inserted at its
    /// start. While there is preview text, `range` lies within it.
    ///
    /// Only called if [`TextConfigBuilder::preview_text_support`] is enabled. Return an error if
    /// `range` is invalid; the input method is told synchronously, so validate against cached
    /// state.
    fn set_preview_text(
        &self,
        _text: String,
        _range: Option<Range<usize>>,
    ) -> Result<(), PreviewTextError> {
        Err(PreviewTextError)
    }

    /// Keep the current preview text as regular text.
    ///
    /// Also called when the editor is detached while it has preview text, possibly synchronously,
    /// see [Threading](crate#threading).
    fn finish_text_preview(&self) {}

    /// Handle private commands sent by the input method application.
    fn receive_private_command(&self, commands: Vec<PrivateCommand>) {
        log::debug!("Ignoring private commands {commands:?}");
    }
}

/// The preview text could not be set.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PreviewTextError;

/// A cursor movement requested by the input method.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    fn from_raw(direction: InputMethod_Direction) -> Option<Self> {
        match direction {
            InputMethod_Direction::IME_DIRECTION_UP => Some(Direction::Up),
            InputMethod_Direction::IME_DIRECTION_DOWN => Some(Direction::Down),
            InputMethod_Direction::IME_DIRECTION_LEFT => Some(Direction::Left),
            InputMethod_Direction::IME_DIRECTION_RIGHT => Some(Direction::Right),
            _ => None,
        }
    }
}

/// An editing action requested by the input method.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExtendAction {
    SelectAll,
    Cut,
    Copy,
    Paste,
}

impl ExtendAction {
    fn from_raw(action: InputMethod_ExtendAction) -> Option<Self> {
        match action {
            InputMethod_ExtendAction::IME_EXTEND_ACTION_SELECT_ALL => Some(ExtendAction::SelectAll),
            InputMethod_ExtendAction::IME_EXTEND_ACTION_CUT => Some(ExtendAction::Cut),
            InputMethod_ExtendAction::IME_EXTEND_ACTION_COPY => Some(ExtendAction::Copy),
            InputMethod_ExtendAction::IME_EXTEND_ACTION_PASTE => Some(ExtendAction::Paste),
            _ => None,
        }
    }
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

/// Attaches `editor` with `attach`, which receives the out-pointer for the input method proxy.
///
/// The framework requests the text config from the [`Ime`] on the calling thread.
fn attach_editor(
    editor: &RawTextEditorProxy,
    attach: impl FnOnce(
        *mut InputMethod_TextEditorProxy,
        *mut *mut InputMethod_InputMethodProxy,
    ) -> InputMethodResult,
) -> Result<NonNull<InputMethod_InputMethodProxy>, InputMethodErrorCode> {
    let mut attached_proxy = ATTACH_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let mut ime_proxy: *mut InputMethod_InputMethodProxy = core::ptr::null_mut();
    if let Err(e) = attach(editor.raw.as_ptr(), &raw mut ime_proxy) {
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

/// Attaches with `OH_InputMethodController_Attach`.
fn attach_with_options(
    options: &AttachOptions,
) -> impl FnOnce(
    *mut InputMethod_TextEditorProxy,
    *mut *mut InputMethod_InputMethodProxy,
) -> InputMethodResult
       + '_ {
    move |editor, ime_proxy| {
        // SAFETY: Both pointers are valid, and text editors are never destroyed, so the editor
        // outlives every reference the framework keeps to it.
        unsafe { OH_InputMethodController_Attach(editor, options.raw.as_ptr(), ime_proxy) }
    }
}

/// Attaches with `OH_InputMethodController_AttachWithUIContext`.
///
/// # Safety
///
/// `context` must be a valid UI context.
#[cfg(feature = "api-23")]
unsafe fn attach_with_ui_context(
    context: ArkUI_ContextHandle,
    options: &AttachOptions,
) -> impl FnOnce(
    *mut InputMethod_TextEditorProxy,
    *mut *mut InputMethod_InputMethodProxy,
) -> InputMethodResult
       + '_ {
    move |editor, ime_proxy| {
        // SAFETY: `context` is valid as guaranteed by the caller. See `attach_with_options` for
        // the other pointers.
        unsafe {
            ohos_ime_sys::controller::OH_InputMethodController_AttachWithUIContext(
                context,
                editor,
                options.raw.as_ptr(),
                ime_proxy,
            )
        }
    }
}

impl ImeProxy {
    /// Attaches `editor` to the input method, detaching any other editor.
    ///
    /// The [`Ime`] of `editor` is asked for its [`TextConfig`] on the calling thread.
    pub fn new(
        editor: RawTextEditorProxy,
        options: AttachOptions,
    ) -> Result<Self, CreateImeProxyError> {
        match attach_editor(&editor, attach_with_options(&options)) {
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
        self.raw = attach_editor(&self.editor, attach_with_options(options))?;
        Ok(())
    }

    /// Like [`new`](Self::new), but the framework takes the window from the UI context of the
    /// page that contains the editor.
    ///
    /// # Safety
    ///
    /// `context` must be a valid UI context.
    #[cfg(feature = "api-23")]
    pub unsafe fn new_with_ui_context(
        context: ArkUI_ContextHandle,
        editor: RawTextEditorProxy,
        options: AttachOptions,
    ) -> Result<Self, CreateImeProxyError> {
        // SAFETY: Guaranteed by the caller.
        let attach = unsafe { attach_with_ui_context(context, &options) };
        match attach_editor(&editor, attach) {
            Ok(raw) => Ok(Self { raw, editor }),
            Err(error_code) => Err(CreateImeProxyError {
                editor,
                options,
                error_code,
            }),
        }
    }

    /// Like [`attach`](Self::attach), but the framework takes the window from the UI context of
    /// the page that contains the editor.
    ///
    /// # Safety
    ///
    /// `context` must be a valid UI context.
    #[cfg(feature = "api-23")]
    pub unsafe fn attach_with_ui_context(
        &mut self,
        context: ArkUI_ContextHandle,
        options: &AttachOptions,
    ) -> InputMethodResult {
        // SAFETY: Guaranteed by the caller.
        let attach = unsafe { attach_with_ui_context(context, options) };
        self.raw = attach_editor(&self.editor, attach)?;
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

    /// Shows the keyboard, telling the input method why it was requested.
    #[cfg(feature = "api-15")]
    pub fn show_text_input(&self, options: &AttachOptions) -> InputMethodResult {
        // SAFETY: See `show_keyboard`. `options` is valid.
        self.with_raw(|raw| unsafe {
            ohos_ime_sys::inputmethod_proxy::OH_InputMethodProxy_ShowTextInput(
                raw,
                options.raw.as_ptr(),
            )
        })
    }

    pub fn hide_keyboard(&self) -> InputMethodResult {
        // SAFETY: See `show_keyboard`.
        self.with_raw(|raw| unsafe { OH_InputMethodProxy_HideKeyboard(raw) })
    }

    /// Tells the input method that the text or the selection changed.
    ///
    /// `text` is the whole text, at most [`MAX_TEXT_LENGTH`] UTF-16 code units, and `start` and
    /// `end` are positions in it.
    pub fn notify_selection_change(
        &self,
        text: &str,
        start: usize,
        end: usize,
    ) -> InputMethodResult {
        let (Ok(start), Ok(end)) = (c_int::try_from(start), c_int::try_from(end)) else {
            return Err(InputMethodErrorCode::PARAMCHECK);
        };
        let mut text: Vec<u16> = text.encode_utf16().collect();
        // SAFETY: See `show_keyboard`. `text` is valid for `text.len()` code units, and the
        // framework copies it.
        self.with_raw(|raw| unsafe {
            OH_InputMethodProxy_NotifySelectionChange(
                raw,
                text.as_mut_ptr(),
                text.len(),
                start,
                end,
            )
        })
    }

    /// Tells the input method that the enter key or text input type changed.
    pub fn notify_configuration_change(
        &self,
        enter_key: InputMethod_EnterKeyType,
        input_type: InputMethod_TextInputType,
    ) -> InputMethodResult {
        // SAFETY: See `show_keyboard`.
        self.with_raw(|raw| unsafe {
            OH_InputMethodProxy_NotifyConfigurationChange(raw, enter_key, input_type)
        })
    }

    /// Tells the input method where the cursor is, e.g. to place the candidate window.
    pub fn notify_cursor_update(&self, cursor: CursorRect) -> InputMethodResult {
        self.with_raw(|raw| {
            // SAFETY: No preconditions.
            let info = unsafe {
                OH_CursorInfo_Create(cursor.left, cursor.top, cursor.width, cursor.height)
            };
            if info.is_null() {
                return Err(InputMethodErrorCode::NULL_POINTER);
            }
            // SAFETY: See `show_keyboard`. `info` is valid, and the framework copies it.
            let result = unsafe { OH_InputMethodProxy_NotifyCursorUpdate(raw, info) };
            // SAFETY: We created `info` and don't use it afterwards.
            unsafe { OH_CursorInfo_Destroy(info) };
            result
        })
    }

    /// Sends private commands to the input method application.
    ///
    /// At most [`MAX_PRIVATE_COMMANDS`] commands with a total size of 32 KiB can be sent at once.
    pub fn send_private_command(&self, commands: &[PrivateCommand]) -> InputMethodResult {
        let raw_commands = commands
            .iter()
            .map(PrivateCommand::to_raw)
            .collect::<Result<Vec<_>, _>>()?;
        let mut pointers: Vec<_> = raw_commands
            .iter()
            .map(|command| command.as_ptr())
            .collect();
        // SAFETY: See `show_keyboard`. `pointers` holds `pointers.len()` valid commands, which the
        // framework copies.
        self.with_raw(|raw| unsafe {
            OH_InputMethodProxy_SendPrivateCommand(raw, pointers.as_mut_ptr(), pointers.len())
        })
    }
}

/// Maximum number of UTF-16 code units the framework exchanges at once, e.g. the text of
/// [`ImeProxy::notify_selection_change`].
pub const MAX_TEXT_LENGTH: usize = 8 * 1024;

/// The cursor rectangle, in absolute physical screen coordinates.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct CursorRect {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

pub struct AttachOptions {
    raw: NonNull<InputMethod_AttachOptions>,
}

pub enum KeyboardVisibility {
    Hide,
    Show,
}

/// The user interaction that requested the keyboard.
#[cfg(feature = "api-15")]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RequestKeyboardReason {
    None,
    Mouse,
    Touch,
    Other,
}

#[cfg(feature = "api-15")]
impl RequestKeyboardReason {
    fn to_raw(self) -> InputMethod_RequestKeyboardReason {
        match self {
            RequestKeyboardReason::None => {
                InputMethod_RequestKeyboardReason::IME_REQUEST_REASON_NONE
            }
            RequestKeyboardReason::Mouse => {
                InputMethod_RequestKeyboardReason::IME_REQUEST_REASON_MOUSE
            }
            RequestKeyboardReason::Touch => {
                InputMethod_RequestKeyboardReason::IME_REQUEST_REASON_TOUCH
            }
            RequestKeyboardReason::Other => {
                InputMethod_RequestKeyboardReason::IME_REQUEST_REASON_OTHER
            }
        }
    }

    fn from_raw(reason: c_int) -> Self {
        let Ok(reason) = c_uint::try_from(reason) else {
            return RequestKeyboardReason::None;
        };
        match InputMethod_RequestKeyboardReason(reason) {
            InputMethod_RequestKeyboardReason::IME_REQUEST_REASON_MOUSE => {
                RequestKeyboardReason::Mouse
            }
            InputMethod_RequestKeyboardReason::IME_REQUEST_REASON_TOUCH => {
                RequestKeyboardReason::Touch
            }
            InputMethod_RequestKeyboardReason::IME_REQUEST_REASON_OTHER => {
                RequestKeyboardReason::Other
            }
            _ => RequestKeyboardReason::None,
        }
    }
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

    /// Like [`new`](Self::new), also telling the input method why the keyboard is requested.
    #[cfg(feature = "api-15")]
    pub fn with_request_keyboard_reason(
        show_keyboard: bool,
        reason: RequestKeyboardReason,
    ) -> Self {
        // SAFETY: See `new`.
        let raw = unsafe {
            let raw =
                ohos_ime_sys::attach_options::OH_AttachOptions_CreateWithRequestKeyboardReason(
                    show_keyboard,
                    reason.to_raw(),
                );
            NonNull::new(raw).expect("OOM?")
        };
        Self { raw }
    }

    #[cfg(feature = "api-15")]
    pub fn request_keyboard_reason(&self) -> RequestKeyboardReason {
        let mut reason = 0;
        // SAFETY: `self.raw` is valid, see `get_visibility`.
        let err = unsafe {
            ohos_ime_sys::attach_options::OH_AttachOptions_GetRequestKeyboardReason(
                self.raw.as_ptr(),
                &raw mut reason,
            )
        };
        debug_assert!(err.is_ok());
        RequestKeyboardReason::from_raw(reason)
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

    /// Run the [`Ime`] callbacks on the main thread instead of IPC threads.
    ///
    /// [`Ime::get_text_config`] always runs on the attaching thread. The framework then queues
    /// [`Ime::set_preview_text`] and [`Ime::receive_private_command`] and tells the input method
    /// that they succeeded, so an error from `set_preview_text` is not reported.
    #[cfg(feature = "api-22")]
    pub fn set_callback_in_main_thread(&mut self, in_main_thread: bool) -> InputMethodResult {
        // SAFETY: `self.raw` is valid.
        unsafe {
            ohos_ime_sys::text_editor_proxy::OH_TextEditorProxy_SetCallbackInMainThread(
                self.raw.as_ptr(),
                in_main_thread,
            )
        }
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
    // SAFETY: `raw` is a valid text editor. The callbacks are `extern "C"` functions with the
    // signatures the framework expects.
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
