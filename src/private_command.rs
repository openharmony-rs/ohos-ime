use log::warn;
use ohos_ime_sys::private_command::{
    InputMethod_PrivateCommand, OH_PrivateCommand_Create, OH_PrivateCommand_Destroy,
    OH_PrivateCommand_GetBoolValue, OH_PrivateCommand_GetIntValue, OH_PrivateCommand_GetKey,
    OH_PrivateCommand_GetStrValue, OH_PrivateCommand_GetValueType, OH_PrivateCommand_SetBoolValue,
    OH_PrivateCommand_SetIntValue, OH_PrivateCommand_SetStrValue,
};
use ohos_ime_sys::types::{InputMethodErrorCode, InputMethod_CommandValueType};
use std::ffi::c_char;
use std::ptr::NonNull;

/// Maximum number of private commands per [`ImeProxy::send_private_command`] call.
///
/// [`ImeProxy::send_private_command`]: crate::ImeProxy::send_private_command
pub const MAX_PRIVATE_COMMANDS: usize = 5;

/// A private command exchanged between the text editor and the input method application.
///
/// The keys and their meaning are defined by the input method application.
#[derive(Clone, Debug, PartialEq)]
pub struct PrivateCommand {
    pub key: String,
    pub value: PrivateCommandValue,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PrivateCommandValue {
    String(String),
    Bool(bool),
    Int(i32),
}

impl PrivateCommand {
    pub fn new(key: impl Into<String>, value: PrivateCommandValue) -> Self {
        Self {
            key: key.into(),
            value,
        }
    }

    /// Reads a command passed to a callback by the framework.
    ///
    /// # Safety
    ///
    /// `raw` must point to a valid command.
    pub(crate) unsafe fn from_raw(raw: NonNull<InputMethod_PrivateCommand>) -> Option<Self> {
        let raw = raw.as_ptr();
        let mut key: *const c_char = std::ptr::null();
        let mut key_len = 0;
        let mut value_type = InputMethod_CommandValueType::IME_COMMAND_VALUE_TYPE_NONE;
        // SAFETY: `raw` is valid, and the out-pointers point to locals.
        unsafe {
            OH_PrivateCommand_GetKey(raw, &raw mut key, &raw mut key_len).ok()?;
            OH_PrivateCommand_GetValueType(raw, &raw mut value_type).ok()?;
        }
        // SAFETY: The framework returned a pointer to `key_len` bytes owned by `raw`.
        let key = unsafe { lossy_string(key, key_len) }?;
        let value = match value_type {
            InputMethod_CommandValueType::IME_COMMAND_VALUE_TYPE_STRING => {
                let mut value: *const c_char = std::ptr::null();
                let mut value_len = 0;
                // SAFETY: As above.
                unsafe {
                    OH_PrivateCommand_GetStrValue(raw, &raw mut value, &raw mut value_len).ok()?;
                    PrivateCommandValue::String(lossy_string(value, value_len)?)
                }
            }
            InputMethod_CommandValueType::IME_COMMAND_VALUE_TYPE_BOOL => {
                let mut value = false;
                // SAFETY: As above.
                unsafe { OH_PrivateCommand_GetBoolValue(raw, &raw mut value).ok()? };
                PrivateCommandValue::Bool(value)
            }
            InputMethod_CommandValueType::IME_COMMAND_VALUE_TYPE_INT32 => {
                let mut value = 0;
                // SAFETY: As above.
                unsafe { OH_PrivateCommand_GetIntValue(raw, &raw mut value).ok()? };
                PrivateCommandValue::Int(value)
            }
            other => {
                warn!(
                    "Ignoring private command `{key}` with value type {}",
                    other.0
                );
                return None;
            }
        };
        Some(Self { key, value })
    }

    pub(crate) fn to_raw(&self) -> Result<RawPrivateCommand, InputMethodErrorCode> {
        // SAFETY: The framework copies `key`; it does not write through the pointer.
        let raw = unsafe {
            OH_PrivateCommand_Create(
                self.key.as_ptr().cast::<c_char>().cast_mut(),
                self.key.len(),
            )
        };
        let raw = RawPrivateCommand(NonNull::new(raw).ok_or(InputMethodErrorCode::NULL_POINTER)?);
        // SAFETY: `raw` is valid. The framework copies string values.
        unsafe {
            match &self.value {
                PrivateCommandValue::String(value) => OH_PrivateCommand_SetStrValue(
                    raw.as_ptr(),
                    value.as_ptr().cast::<c_char>().cast_mut(),
                    value.len(),
                ),
                PrivateCommandValue::Bool(value) => {
                    OH_PrivateCommand_SetBoolValue(raw.as_ptr(), *value)
                }
                PrivateCommandValue::Int(value) => {
                    OH_PrivateCommand_SetIntValue(raw.as_ptr(), *value)
                }
            }?;
        }
        Ok(raw)
    }
}

/// # Safety
///
/// If `text` is not null, it must point to `len` bytes that stay valid during the call.
unsafe fn lossy_string(text: *const c_char, len: usize) -> Option<String> {
    let bytes = match NonNull::new(text.cast_mut()) {
        // SAFETY: Guaranteed by the caller.
        Some(text) => unsafe { std::slice::from_raw_parts(text.as_ptr().cast::<u8>(), len) },
        None if len == 0 => &[],
        None => return None,
    };
    Some(String::from_utf8_lossy(bytes).into_owned())
}

/// An owned native private command.
pub(crate) struct RawPrivateCommand(NonNull<InputMethod_PrivateCommand>);

impl RawPrivateCommand {
    pub(crate) fn as_ptr(&self) -> *mut InputMethod_PrivateCommand {
        self.0.as_ptr()
    }
}

impl Drop for RawPrivateCommand {
    fn drop(&mut self) {
        // SAFETY: We created the command and never handed out ownership.
        unsafe { OH_PrivateCommand_Destroy(self.0.as_ptr()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let commands = [
            PrivateCommand::new("text", PrivateCommandValue::String("wörd".into())),
            PrivateCommand::new("flag", PrivateCommandValue::Bool(true)),
            PrivateCommand::new("count", PrivateCommandValue::Int(-7)),
            PrivateCommand::new("", PrivateCommandValue::String(String::new())),
        ];
        for command in commands {
            let raw = command.to_raw().expect("to_raw failed");
            // SAFETY: `raw` is a valid command.
            let decoded = unsafe { PrivateCommand::from_raw(raw.0) };
            assert_eq!(decoded.as_ref(), Some(&command));
        }
    }
}
