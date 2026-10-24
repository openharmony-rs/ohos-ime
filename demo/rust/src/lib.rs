//! A text editor driven by the input method through `ohos-ime`, for trying the crate out and for
//! the on-device tests of the demo app.
//!
//! The editor keeps the cursor at the end of its text and records every callback, so the tests can
//! check what the input method sent.

use std::cell::RefCell;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, Once, PoisonError};

use napi_derive_ohos::napi;
use napi_ohos::{Error, Result};
use ohos_ime::{
    AttachOptions, Direction, ExtendAction, Ime, ImeProxy, InputMethod_EnterKeyType,
    KeyboardStatus, PrivateCommand, RawTextEditorProxy, TextConfig,
};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Default)]
struct State {
    text: String,
    events: Vec<String>,
}

static STATE: LazyLock<Arc<Mutex<State>>> = LazyLock::new(Default::default);

fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

thread_local! {
    /// The attached editor. Only used on the ArkTS main thread, which is also where ArkUI attaches
    /// its text fields.
    #[allow(clippy::missing_const_for_thread_local, reason = "false positive, it is const")]
    static PROXY: RefCell<Option<ImeProxy>> = const { RefCell::new(None) };
}

struct Editor {
    config: TextConfig,
}

fn record(event: String) {
    log::info!("{event}");
    state().events.push(event);
}

impl Ime for Editor {
    fn insert_text(&self, text: String) {
        record(format!("insert_text({text:?})"));
        state().text.push_str(&text);
    }

    fn delete_forward(&self, len: usize) {
        record(format!("delete_forward({len})"));
    }

    fn delete_backward(&self, len: usize) {
        record(format!("delete_backward({len})"));
        let mut state = state();
        for _ in 0..len {
            let end = state
                .text
                .grapheme_indices(true)
                .next_back()
                .map_or(0, |(index, _)| index);
            state.text.truncate(end);
        }
    }

    fn get_text_config(&self) -> &TextConfig {
        &self.config
    }

    fn send_enter_key(&self, enter_key: InputMethod_EnterKeyType) {
        record(format!("send_enter_key({})", enter_key.0));
    }

    fn keyboard_status_changed(&self, status: KeyboardStatus) {
        record(format!("keyboard_status_changed({status:?})"));
    }

    fn move_cursor(&self, direction: Direction) {
        record(format!("move_cursor({direction:?})"));
    }

    fn set_selection(&self, start: usize, end: usize) {
        record(format!("set_selection({start}, {end})"));
    }

    fn handle_extend_action(&self, action: ExtendAction) {
        record(format!("handle_extend_action({action:?})"));
    }

    fn text_before_cursor(&self, max_len: usize) -> String {
        record(format!("text_before_cursor({max_len})"));
        state().text.clone()
    }

    fn text_after_cursor(&self, max_len: usize) -> String {
        record(format!("text_after_cursor({max_len})"));
        String::new()
    }

    fn text_index_at_cursor(&self) -> usize {
        record("text_index_at_cursor()".to_string());
        state().text.encode_utf16().count()
    }

    fn receive_private_command(&self, commands: Vec<PrivateCommand>) {
        record(format!("receive_private_command({commands:?})"));
    }
}

fn init_logging() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        hilog::Builder::new()
            .set_tag("ohos-ime-demo")
            .filter_level(log::LevelFilter::Debug)
            .init();
    });
}

/// Attaches an editor containing `text` and shows the keyboard, replacing the attached editor.
#[napi]
pub fn attach(text: String) -> Result<()> {
    init_logging();
    *state() = State {
        text,
        events: Vec::new(),
    };
    let editor = Editor {
        config: TextConfig::default(),
    };
    let editor = RawTextEditorProxy::new(Box::new(editor))
        .map_err(|e| Error::from_reason(format!("Creating the editor failed: {e:?}")))?;
    let proxy = ImeProxy::new(editor, AttachOptions::new(true))
        .map_err(|e| Error::from_reason(format!("Attaching failed: {e:?}")))?;
    PROXY.with_borrow_mut(|attached| *attached = Some(proxy));
    Ok(())
}

/// Detaches the editor.
#[napi]
pub fn detach() {
    PROXY.with_borrow_mut(|attached| *attached = None);
}

/// The text of the editor.
#[napi]
pub fn text() -> String {
    state().text.clone()
}

/// The callbacks the editor received since it was attached.
#[napi]
pub fn events() -> Vec<String> {
    state().events.clone()
}
