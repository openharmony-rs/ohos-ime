use crate::text_editor::*;
use crate::*;
use ohos_ime_sys::private_command::InputMethod_PrivateCommand;
use std::sync::Arc;

/// Serializes the tests, which share the dispatcher, the recycled editors and the attached editor.
fn serial() -> MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Debug, PartialEq)]
enum Event {
    Insert(String),
    MoveCursor(Direction),
    SetSelection(usize, usize),
    ExtendAction(ExtendAction),
    Preview(String, Option<Range<usize>>),
    FinishPreview,
    PrivateCommands(Vec<PrivateCommand>),
}

struct Recorder {
    events: Arc<Mutex<Vec<Event>>>,
    config: TextConfig,
}

impl Recorder {
    fn push(&self, event: Event) {
        self.events.lock().unwrap().push(event);
    }
}

impl Ime for Recorder {
    fn insert_text(&self, text: String) {
        self.push(Event::Insert(text));
    }
    fn delete_forward(&self, _len: usize) {}
    fn delete_backward(&self, _len: usize) {}
    fn get_text_config(&self) -> &TextConfig {
        &self.config
    }
    fn send_enter_key(&self, _enter_key: InputMethod_EnterKeyType) {}
    fn move_cursor(&self, direction: Direction) {
        self.push(Event::MoveCursor(direction));
    }
    fn set_selection(&self, start: usize, end: usize) {
        self.push(Event::SetSelection(start, end));
    }
    fn handle_extend_action(&self, action: ExtendAction) {
        self.push(Event::ExtendAction(action));
    }
    fn text_before_cursor(&self, _max_len: usize) -> String {
        "hello😀".to_string()
    }
    fn text_after_cursor(&self, _max_len: usize) -> String {
        "😀world".to_string()
    }
    fn text_index_at_cursor(&self) -> usize {
        7
    }
    fn set_preview_text(
        &self,
        text: String,
        range: Option<Range<usize>>,
    ) -> Result<(), PreviewTextError> {
        let rejected = text == "reject";
        self.push(Event::Preview(text, range));
        if rejected {
            Err(PreviewTextError)
        } else {
            Ok(())
        }
    }
    fn finish_text_preview(&self) {
        self.push(Event::FinishPreview);
    }
    fn receive_private_command(&self, commands: Vec<PrivateCommand>) {
        self.push(Event::PrivateCommands(commands));
    }
}

fn new_editor() -> (RawTextEditorProxy, Arc<Mutex<Vec<Event>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let ime = Recorder {
        events: events.clone(),
        config: TextConfig::default(),
    };
    let editor = RawTextEditorProxy::new(Box::new(ime)).expect("Creating the editor failed");
    (editor, events)
}

fn take(events: &Mutex<Vec<Event>>) -> Vec<Event> {
    std::mem::take(&mut *events.lock().unwrap())
}

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn insert(editor: &RawTextEditorProxy, text: &[u16]) {
    // SAFETY: `text` is a valid slice.
    unsafe { insert_text(editor.raw.as_ptr(), text.as_ptr(), text.len()) };
}

fn preview(editor: &RawTextEditorProxy, text: &str, start: i32, end: i32) -> i32 {
    let text = utf16(text);
    // SAFETY: `text` is a valid slice.
    unsafe { set_preview_text(editor.raw.as_ptr(), text.as_ptr(), text.len(), start, end) }
}

#[test]
fn insert_text_dispatches_once() {
    let _serial = serial();
    let (editor, events) = new_editor();
    insert(&editor, &utf16("aé😀"));
    assert_eq!(take(&events), [Event::Insert("aé😀".into())]);
}

#[test]
fn insert_text_forwards_empty_text() {
    let _serial = serial();
    let (editor, events) = new_editor();
    // SAFETY: Null pointers are allowed.
    unsafe {
        insert_text(editor.raw.as_ptr(), std::ptr::null(), 0);
        insert_text(editor.raw.as_ptr(), std::ptr::null(), 1);
    }
    assert_eq!(take(&events), [Event::Insert(String::new())]);
}

#[test]
fn insert_text_rejects_malformed_utf16() {
    let _serial = serial();
    let (editor, events) = new_editor();
    insert(&editor, &[0xD800]);
    assert_eq!(take(&events), []);
}

/// Calls a surrounding-text callback like the framework does: with a buffer of `number + 1` code
/// units and `length` preset to `number`.
fn query(
    callback: unsafe extern "C" fn(*mut InputMethod_TextEditorProxy, i32, *mut u16, *mut usize),
    editor: &RawTextEditorProxy,
    number: i32,
) -> Vec<u16> {
    let mut buffer = vec![0u16; usize::try_from(number).unwrap_or(0) + 1];
    let mut length = usize::try_from(number).unwrap_or(usize::MAX);
    // SAFETY: `buffer` has room for `number` code units, and `length` is a local.
    unsafe {
        callback(
            editor.raw.as_ptr(),
            number,
            buffer.as_mut_ptr(),
            &raw mut length,
        )
    };
    assert!(length < buffer.len());
    buffer.truncate(length);
    buffer
}

#[test]
fn text_before_cursor_is_clamped() {
    let _serial = serial();
    let (editor, _) = new_editor();
    assert_eq!(
        query(get_left_text_of_cursor, &editor, 100),
        utf16("hello😀")
    );
    assert_eq!(query(get_left_text_of_cursor, &editor, 3), utf16("o😀"));
    assert_eq!(query(get_left_text_of_cursor, &editor, 1), utf16(""));
    assert_eq!(query(get_left_text_of_cursor, &editor, -1), utf16(""));
}

#[test]
fn text_after_cursor_is_clamped() {
    let _serial = serial();
    let (editor, _) = new_editor();
    assert_eq!(
        query(get_right_text_of_cursor, &editor, 100),
        utf16("😀world")
    );
    assert_eq!(query(get_right_text_of_cursor, &editor, 3), utf16("😀w"));
    assert_eq!(query(get_right_text_of_cursor, &editor, 1), utf16(""));
    assert_eq!(query(get_right_text_of_cursor, &editor, 0), utf16(""));
}

#[test]
fn text_index_at_cursor_is_forwarded() {
    let _serial = serial();
    let (editor, _) = new_editor();
    assert_eq!(get_text_index_at_cursor(editor.raw.as_ptr()), 7);
}

#[test]
fn preview_text_ranges() {
    let _serial = serial();
    let (editor, events) = new_editor();
    assert_eq!(preview(&editor, "ni", -1, -1), 0);
    assert_eq!(preview(&editor, "ni", 2, 2), 0);
    assert_eq!(preview(&editor, "ni", 1, 3), 0);
    assert_eq!(preview(&editor, "ni", 3, 1), -1);
    assert_eq!(preview(&editor, "ni", -1, 2), -1);
    assert_eq!(preview(&editor, "reject", -1, -1), -1);
    finish_text_preview(editor.raw.as_ptr());
    assert_eq!(
        take(&events),
        [
            Event::Preview("ni".into(), None),
            Event::Preview("ni".into(), Some(2..2)),
            Event::Preview("ni".into(), Some(1..3)),
            Event::Preview("reject".into(), None),
            Event::FinishPreview,
        ]
    );
}

#[test]
fn cursor_selection_and_extend_actions() {
    use ohos_ime_sys::types::{InputMethod_Direction, InputMethod_ExtendAction};
    let _serial = serial();
    let (editor, events) = new_editor();
    let raw = editor.raw.as_ptr();
    move_cursor(raw, InputMethod_Direction::IME_DIRECTION_NONE);
    move_cursor(raw, InputMethod_Direction::IME_DIRECTION_LEFT);
    move_cursor(raw, InputMethod_Direction::IME_DIRECTION_DOWN);
    handle_set_selection(raw, -1, 2);
    handle_set_selection(raw, 4, 1);
    handle_extend_action(raw, InputMethod_ExtendAction(1));
    handle_extend_action(raw, InputMethod_ExtendAction::IME_EXTEND_ACTION_CUT);
    handle_extend_action(raw, InputMethod_ExtendAction::IME_EXTEND_ACTION_PASTE);
    assert_eq!(
        take(&events),
        [
            Event::MoveCursor(Direction::Left),
            Event::MoveCursor(Direction::Down),
            Event::SetSelection(4, 1),
            Event::ExtendAction(ExtendAction::Cut),
            Event::ExtendAction(ExtendAction::Paste),
        ]
    );
}

#[test]
fn private_commands_are_decoded() {
    let _serial = serial();
    let (editor, events) = new_editor();
    let commands = vec![
        PrivateCommand::new("a", PrivateCommandValue::Int(1)),
        PrivateCommand::new("b", PrivateCommandValue::String("x".into())),
    ];
    let raw_commands: Vec<_> = commands.iter().map(|c| c.to_raw().unwrap()).collect();
    let mut pointers: Vec<*mut InputMethod_PrivateCommand> =
        raw_commands.iter().map(|c| c.as_ptr()).collect();
    // SAFETY: `pointers` holds `pointers.len()` valid commands.
    let result = unsafe {
        receive_private_command(editor.raw.as_ptr(), pointers.as_mut_ptr(), pointers.len())
    };
    assert_eq!(result, 0);
    assert_eq!(take(&events), [Event::PrivateCommands(commands)]);
}

#[test]
fn dropped_editor_is_unregistered() {
    let _serial = serial();
    let (editor, events) = new_editor();
    let raw = editor.raw.as_ptr();
    drop(editor);
    // SAFETY: Text editors are never destroyed, and `text` is a valid slice.
    unsafe {
        let text = utf16("a");
        insert_text(raw, text.as_ptr(), text.len());
    }
    assert_eq!(get_text_index_at_cursor(raw), 0);
    assert_eq!(take(&events), []);
}

#[test]
fn dropped_editors_are_reused_only_after_a_delay() {
    let _serial = serial();
    let (editor, old_events) = new_editor();
    let addr = editor.addr();
    drop(editor);

    let newer: Vec<_> = (0..RECYCLE_DELAY).map(|_| new_editor()).collect();
    assert!(newer.iter().all(|(editor, _)| editor.addr() != addr));
    drop(newer);

    // The dropped editor is now among the `RECYCLE_DELAY` oldest ones.
    let mut created = Vec::new();
    let (editor, events) = loop {
        assert!(created.len() <= RECYCLE_DELAY, "the editor was not reused");
        let (editor, events) = new_editor();
        if editor.addr() == addr {
            break (editor, events);
        }
        created.push(editor);
    };
    insert(&editor, &utf16("a"));
    assert_eq!(take(&events), [Event::Insert("a".into())]);
    assert_eq!(take(&old_events), []);
}

fn fake_attach(
    _editor: *mut InputMethod_TextEditorProxy,
    ime_proxy: *mut *mut InputMethod_InputMethodProxy,
) -> InputMethodResult {
    // SAFETY: `attach_editor` passes a valid out-pointer.
    unsafe { ime_proxy.write(NonNull::dangling().as_ptr()) };
    Ok(())
}

/// An `ImeProxy` attached without the framework. Its proxy pointer is dangling, so it may only reach
/// the framework where the framework compares it to its own proxy without using it.
fn fake_attached_proxy() -> ImeProxy {
    let (editor, _) = new_editor();
    let raw = attach_editor(&editor, fake_attach).unwrap();
    ImeProxy { raw, editor }
}

/// Drops a proxy from `fake_attached_proxy` without calling the framework.
fn drop_fake(proxy: ImeProxy) {
    let mut attached = attached_editor();
    if *attached == Some(proxy.editor.addr()) {
        *attached = None;
    }
    drop(attached);
    drop(proxy);
}

#[test]
fn attaching_another_editor_makes_the_previous_proxy_inert() {
    let _serial = serial();
    let a = fake_attached_proxy();
    assert_eq!(a.with_raw(|_| Ok(())), Ok(()));
    let b = fake_attached_proxy();
    assert_eq!(
        a.with_raw(|_| panic!("used a superseded proxy")),
        Err(InputMethodErrorCode::DETACHED)
    );
    assert_eq!(b.with_raw(|_| Ok(())), Ok(()));
    // Dropping the superseded proxy neither calls the framework nor detaches `b`.
    drop(a);
    assert_eq!(*attached_editor(), Some(b.editor.addr()));
    drop_fake(b);
}

#[test]
fn failed_attach_detaches_the_previous_editor() {
    let _serial = serial();
    let a = fake_attached_proxy();
    let (b, _) = new_editor();
    assert_eq!(
        attach_editor(&b, |_, _| Err(InputMethodErrorCode::PARAMCHECK)),
        Err(InputMethodErrorCode::PARAMCHECK)
    );
    // The framework does not accept the fake proxy of `a`, like a proxy it replaced.
    assert_eq!(
        a.with_raw(|_| panic!("used a detached proxy")),
        Err(InputMethodErrorCode::DETACHED)
    );
    assert_eq!(*attached_editor(), None);
}

#[test]
fn failed_attach_returns_the_editor() {
    let _serial = serial();
    let (editor, events) = new_editor();
    let attached = *attached_editor();
    // Test binaries have no window, so the framework refuses to attach.
    let Err(error) = ImeProxy::new(editor, AttachOptions::new(false)) else {
        panic!("Attaching without a focused window succeeded");
    };
    assert_ne!(error.error_code, InputMethodErrorCode::NULL_POINTER);
    assert_eq!(*attached_editor(), attached);
    insert(&error.editor, &utf16("a"));
    assert_eq!(take(&events), [Event::Insert("a".into())]);
}

#[test]
fn text_config_is_applied() {
    use ohos_ime_sys::text_config::*;
    use ohos_ime_sys::types::InputMethod_TextInputType;
    let _serial = serial();
    let builder = TextConfigBuilder::new()
        .input_type(InputMethod_TextInputType::IME_TEXT_INPUT_TYPE_EMAIL_ADDRESS)
        .enterkey_type(InputMethod_EnterKeyType::IME_ENTER_KEY_SEARCH)
        .preview_text_support(true)
        .selection(TextSelection::new(2, 5).ok().unwrap())
        .window_id(42);
    #[cfg(feature = "api-20")]
    let builder = builder.placeholder("Søk 😀").ability_name("EntryAbility");
    #[cfg(feature = "api-26")]
    let builder = builder.consume_key_events(true);
    let ime = Recorder {
        events: Default::default(),
        config: builder.build(),
    };
    let editor = RawTextEditorProxy::new(Box::new(ime)).unwrap();

    // SAFETY: `config` is created and destroyed here, and all out-pointers point to locals.
    unsafe {
        let config = OH_TextConfig_Create();
        assert!(!config.is_null());
        get_text_config(editor.raw.as_ptr(), config);

        let mut input_type = InputMethod_TextInputType::IME_TEXT_INPUT_TYPE_NONE;
        OH_TextConfig_GetInputType(config, &raw mut input_type).unwrap();
        assert_eq!(
            input_type,
            InputMethod_TextInputType::IME_TEXT_INPUT_TYPE_EMAIL_ADDRESS
        );
        let mut enter_key = InputMethod_EnterKeyType::IME_ENTER_KEY_UNSPECIFIED;
        OH_TextConfig_GetEnterKeyType(config, &raw mut enter_key).unwrap();
        assert_eq!(enter_key, InputMethod_EnterKeyType::IME_ENTER_KEY_SEARCH);
        let mut preview = false;
        OH_TextConfig_IsPreviewTextSupported(config, &raw mut preview).unwrap();
        assert!(preview);
        let (mut start, mut end) = (0, 0);
        OH_TextConfig_GetSelection(config, &raw mut start, &raw mut end).unwrap();
        assert_eq!((start, end), (2, 5));
        let mut window_id = 0;
        OH_TextConfig_GetWindowId(config, &raw mut window_id).unwrap();
        assert_eq!(window_id, 42);

        #[cfg(feature = "api-20")]
        {
            let mut buffer = [0u16; 256];
            let mut length = buffer.len();
            OH_TextConfig_GetPlaceholder(config, buffer.as_mut_ptr(), &raw mut length).unwrap();
            assert_eq!(
                &buffer[..length],
                &crate::utf16::nul_terminated("Søk 😀")[..]
            );
            let mut length = buffer.len();
            OH_TextConfig_GetAbilityName(config, buffer.as_mut_ptr(), &raw mut length).unwrap();
            assert_eq!(
                &buffer[..length],
                &crate::utf16::nul_terminated("EntryAbility")[..]
            );
        }
        #[cfg(feature = "api-26")]
        {
            let mut consume = false;
            OH_TextConfig_GetConsumeKeyEvents(config, &raw mut consume).unwrap();
            assert!(consume);
        }
        OH_TextConfig_Destroy(config);
    }
}

/// The framework does not initialize the text config it passes, so unset fields must be written.
#[test]
fn unset_text_config_fields_are_reset() {
    use ohos_ime_sys::text_avoid_info::*;
    use ohos_ime_sys::text_config::*;
    let _serial = serial();
    let (editor, _) = new_editor();

    // SAFETY: `config` is created and destroyed here, and all out-pointers point to locals.
    unsafe {
        let config = OH_TextConfig_Create();
        assert!(!config.is_null());
        OH_TextConfig_SetSelection(config, 5, 9).unwrap();
        OH_TextConfig_SetWindowId(config, 77).unwrap();
        let mut avoid_info = std::ptr::null_mut();
        OH_TextConfig_GetTextAvoidInfo(config, &raw mut avoid_info).unwrap();
        OH_TextAvoidInfo_SetPositionY(avoid_info, 3.0).unwrap();
        OH_TextAvoidInfo_SetHeight(avoid_info, 4.0).unwrap();
        #[cfg(feature = "api-20")]
        {
            let stale = crate::utf16::nul_terminated("stale");
            OH_TextConfig_SetPlaceholder(config, stale.as_ptr(), stale.len()).unwrap();
            OH_TextConfig_SetAbilityName(config, stale.as_ptr(), stale.len()).unwrap();
        }

        get_text_config(editor.raw.as_ptr(), config);

        let (mut start, mut end) = (0, 0);
        OH_TextConfig_GetSelection(config, &raw mut start, &raw mut end).unwrap();
        assert_eq!((start, end), (-1, -1));
        let mut window_id = 0;
        OH_TextConfig_GetWindowId(config, &raw mut window_id).unwrap();
        assert_eq!(window_id, 0);
        let (mut position_y, mut height) = (1.0, 1.0);
        OH_TextAvoidInfo_GetPositionY(avoid_info, &raw mut position_y).unwrap();
        OH_TextAvoidInfo_GetHeight(avoid_info, &raw mut height).unwrap();
        assert_eq!((position_y, height), (0.0, 0.0));
        #[cfg(feature = "api-20")]
        {
            let mut buffer = [1u16; 256];
            let mut length = buffer.len();
            OH_TextConfig_GetPlaceholder(config, buffer.as_mut_ptr(), &raw mut length).unwrap();
            assert_eq!(&buffer[..length], &[0]);
            let mut length = buffer.len();
            OH_TextConfig_GetAbilityName(config, buffer.as_mut_ptr(), &raw mut length).unwrap();
            assert_eq!(&buffer[..length], &[0]);
        }
        OH_TextConfig_Destroy(config);
    }
}

#[cfg(feature = "api-15")]
#[test]
fn request_keyboard_reason_round_trips() {
    let _serial = serial();
    for reason in [
        RequestKeyboardReason::None,
        RequestKeyboardReason::Mouse,
        RequestKeyboardReason::Touch,
        RequestKeyboardReason::Other,
    ] {
        let options = AttachOptions::with_request_keyboard_reason(true, reason);
        assert_eq!(options.request_keyboard_reason(), reason);
        assert!(matches!(options.get_visibility(), KeyboardVisibility::Show));
    }
}

#[cfg(feature = "api-22")]
#[test]
fn callback_in_main_thread_can_be_set() {
    let _serial = serial();
    let (mut editor, _) = new_editor();
    editor.set_callback_in_main_thread(true).unwrap();
    editor.set_callback_in_main_thread(false).unwrap();
}
