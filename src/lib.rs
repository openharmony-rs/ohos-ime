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
//!
//!

mod text_config;
mod text_editor;

pub use crate::text_config::{TextConfig, TextConfigBuilder, TextSelection};
use crate::text_editor::DISPATCHER;
use log::error;
use ohos_ime_sys::attach_options::{
    InputMethod_AttachOptions, OH_AttachOptions_Create, OH_AttachOptions_Destroy,
    OH_AttachOptions_IsShowKeyboard,
};
use ohos_ime_sys::controller::OH_InputMethodController_Attach;
use ohos_ime_sys::inputmethod_proxy::{
    InputMethod_InputMethodProxy, OH_InputMethodProxy_HideKeyboard,
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
use ohos_ime_sys::types::{InputMethodErrorCode, InputMethodResult, InputMethod_EnterKeyType};
use std::fmt::Debug;
use std::ptr::NonNull;

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
    // ...
}

pub struct ImeProxy {
    raw: NonNull<InputMethod_InputMethodProxy>,
    // keep the text editor alive.
    #[allow(dead_code)]
    editor: RawTextEditorProxy,
}

impl Drop for ImeProxy {
    fn drop(&mut self) {
        let res = DISPATCHER.unregister(self.editor.raw);
        #[cfg(debug_assertions)]
        if let Err(e) = res {
            error!("IME: ImeProxy destroy failed {:?}", e);
        }
        #[cfg(not(debug_assertions))]
        drop(res)
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

impl ImeProxy {
    // todo: maybe use builder pattern instead.
    pub fn new(
        editor: RawTextEditorProxy,
        options: AttachOptions,
    ) -> Result<Self, CreateImeProxyError> {
        unsafe {
            let mut ime_proxy: *mut InputMethod_InputMethodProxy = core::ptr::null_mut();
            if let Err(error_code) = OH_InputMethodController_Attach(
                editor.raw.as_ptr(),
                options.raw.as_ptr(),
                &raw mut ime_proxy,
            ) {
                return Err(CreateImeProxyError {
                    editor,
                    options,
                    error_code,
                });
            }

            Ok(Self {
                // We checked the returncode above, so the pointer should be valid and non-null now.
                raw: NonNull::new(ime_proxy).expect("Wrong Errorcode"),
                editor,
            })
        }
    }

    pub fn show_keyboard(&self) -> InputMethodResult {
        unsafe { OH_InputMethodProxy_ShowKeyboard(self.raw.as_ptr()) }
    }

    pub fn hide_keyboard(&self) -> InputMethodResult {
        unsafe { OH_InputMethodProxy_HideKeyboard(self.raw.as_ptr()) }
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

// Very raw bindings. To be replaced with something better!
// Ideally we want to provide a Rust trait, user provides a rust implementation,
// and we somehow create a C-ABI wrapper around the trait implementations.
// Brain-storming: We could make One generic C-ABI implementation here, and then lookup
// the Rust impl based on the TextEditorProxy pointer.
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
        let raw_proxy = unsafe { OH_TextEditorProxy_Create() };
        let Some(raw_proxy) = NonNull::new(raw_proxy) else {
            let err = Err(CreateTextEditorProxyError {
                ime,
                reason: CreateTextEditorProxyErrorKind::CreateProxyFailed,
            });
            return err;
        };
        let mut proxy = Self { raw: raw_proxy };
        DISPATCHER.register(proxy.raw, ime);
        proxy.register_dispatcher_callbacks().map_err(|e| {
            // unregistering here should never fail
            let ime = DISPATCHER.unregister(proxy.raw).expect("Unregister failed");
            CreateTextEditorProxyError {
                ime,
                reason: CreateTextEditorProxyErrorKind::RegisterCallbacksFailed(e),
            }
        })?;
        Ok(proxy)
    }

    fn register_dispatcher_callbacks(&mut self) -> InputMethodResult {
        use text_editor::*;
        unsafe {
            OH_TextEditorProxy_SetGetTextConfigFunc(self.raw.as_ptr(), Some(get_text_config))?;
            OH_TextEditorProxy_SetInsertTextFunc(self.raw.as_ptr(), Some(insert_text))?;
            OH_TextEditorProxy_SetDeleteForwardFunc(self.raw.as_ptr(), Some(delete_forward))?;
            OH_TextEditorProxy_SetDeleteBackwardFunc(self.raw.as_ptr(), Some(delete_backward))?;
            OH_TextEditorProxy_SetSendKeyboardStatusFunc(
                self.raw.as_ptr(),
                Some(send_keyboard_status),
            )?;
            OH_TextEditorProxy_SetSendEnterKeyFunc(self.raw.as_ptr(), Some(send_enter_key))?;
            OH_TextEditorProxy_SetMoveCursorFunc(self.raw.as_ptr(), Some(move_cursor))?;
            OH_TextEditorProxy_SetHandleSetSelectionFunc(
                self.raw.as_ptr(),
                Some(handle_set_selection),
            )?;
            OH_TextEditorProxy_SetHandleExtendActionFunc(
                self.raw.as_ptr(),
                Some(handle_extend_action),
            )?;
            OH_TextEditorProxy_SetGetLeftTextOfCursorFunc(
                self.raw.as_ptr(),
                Some(get_left_text_of_cursor),
            )?;
            OH_TextEditorProxy_SetGetRightTextOfCursorFunc(
                self.raw.as_ptr(),
                Some(get_right_text_of_cursor),
            )?;
            OH_TextEditorProxy_SetGetTextIndexAtCursorFunc(
                self.raw.as_ptr(),
                Some(get_text_index_at_cursor),
            )?;
            OH_TextEditorProxy_SetReceivePrivateCommandFunc(
                self.raw.as_ptr(),
                Some(receive_private_command),
            )?;
            OH_TextEditorProxy_SetSetPreviewTextFunc(self.raw.as_ptr(), Some(set_preview_text))?;
            OH_TextEditorProxy_SetFinishTextPreviewFunc(
                self.raw.as_ptr(),
                Some(finish_text_preview),
            )?;
            Ok(())
        }
    }
}

impl Drop for RawTextEditorProxy {
    fn drop(&mut self) {
        unsafe {
            OH_TextEditorProxy_Destroy(self.raw.as_ptr());
        }
    }
}
