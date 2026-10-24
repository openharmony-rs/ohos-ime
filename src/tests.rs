use crate::text_editor::*;
use crate::*;
use ohos_ime_sys::private_command::InputMethod_PrivateCommand;
use ohos_ime_sys::types::{
    InputMethod_Direction, InputMethod_ExtendAction, InputMethod_KeyboardStatus,
};
use std::sync::{mpsc, Arc};
use std::time::Duration;

/// Serializes the tests, which share the dispatcher, the recycled editors and the attached editor.
fn serial() -> MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Debug, PartialEq)]
enum Event {
    Insert(String),
    DeleteForward(usize),
    DeleteBackward(usize),
    EnterKey(InputMethod_EnterKeyType),
    KeyboardStatus(KeyboardStatus),
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
    text_index: usize,
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
    fn delete_forward(&self, len: usize) {
        self.push(Event::DeleteForward(len));
    }
    fn delete_backward(&self, len: usize) {
        self.push(Event::DeleteBackward(len));
    }
    fn get_text_config(&self) -> &TextConfig {
        &self.config
    }
    fn send_enter_key(&self, enter_key: InputMethod_EnterKeyType) {
        self.push(Event::EnterKey(enter_key));
    }
    fn keyboard_status_changed(&self, status: KeyboardStatus) {
        self.push(Event::KeyboardStatus(status));
    }
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
        self.text_index
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
        text_index: 7,
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

#[test]
fn deletions_and_enter_key_are_forwarded() {
    let _serial = serial();
    let (editor, events) = new_editor();
    let raw = editor.raw.as_ptr();
    delete_forward(raw, 3);
    delete_forward(raw, -1);
    delete_backward(raw, 2);
    delete_backward(raw, -1);
    send_enter_key(raw, InputMethod_EnterKeyType::IME_ENTER_KEY_SEARCH);
    assert_eq!(
        take(&events),
        [
            Event::DeleteForward(3),
            Event::DeleteForward(0),
            Event::DeleteBackward(2),
            Event::DeleteBackward(0),
            Event::EnterKey(InputMethod_EnterKeyType::IME_ENTER_KEY_SEARCH),
        ]
    );
}

#[test]
fn keyboard_status_is_forwarded() {
    let _serial = serial();
    let (editor, events) = new_editor();
    let raw = editor.raw.as_ptr();
    send_keyboard_status(raw, InputMethod_KeyboardStatus::IME_KEYBOARD_STATUS_NONE);
    send_keyboard_status(raw, InputMethod_KeyboardStatus::IME_KEYBOARD_STATUS_HIDE);
    send_keyboard_status(raw, InputMethod_KeyboardStatus::IME_KEYBOARD_STATUS_SHOW);
    send_keyboard_status(raw, InputMethod_KeyboardStatus(42));
    assert_eq!(
        take(&events),
        [
            Event::KeyboardStatus(KeyboardStatus::None),
            Event::KeyboardStatus(KeyboardStatus::Hidden),
            Event::KeyboardStatus(KeyboardStatus::Shown),
            Event::KeyboardStatus(KeyboardStatus::Unknown(42)),
        ]
    );
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
fn text_index_at_cursor_saturates() {
    let _serial = serial();
    let ime = Recorder {
        events: Default::default(),
        config: TextConfig::default(),
        text_index: usize::MAX,
    };
    let editor = RawTextEditorProxy::new(Box::new(ime)).unwrap();
    assert_eq!(get_text_index_at_cursor(editor.raw.as_ptr()), i32::MAX);
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
fn preview_text_rejects_malformed_text() {
    let _serial = serial();
    let (editor, events) = new_editor();
    let malformed = [0xD800];
    // SAFETY: `malformed` is a valid slice, and a null pointer is allowed.
    unsafe {
        let raw = editor.raw.as_ptr();
        assert_eq!(set_preview_text(raw, std::ptr::null(), 2, -1, -1), -1);
        assert_eq!(set_preview_text(raw, malformed.as_ptr(), 1, -1, -1), -1);
    }
    assert_eq!(take(&events), []);
}

#[test]
fn cursor_selection_and_extend_actions() {
    let _serial = serial();
    let (editor, events) = new_editor();
    let raw = editor.raw.as_ptr();
    move_cursor(raw, InputMethod_Direction::IME_DIRECTION_NONE);
    move_cursor(raw, InputMethod_Direction::IME_DIRECTION_UP);
    move_cursor(raw, InputMethod_Direction::IME_DIRECTION_DOWN);
    move_cursor(raw, InputMethod_Direction::IME_DIRECTION_LEFT);
    move_cursor(raw, InputMethod_Direction::IME_DIRECTION_RIGHT);
    handle_set_selection(raw, -1, 2);
    handle_set_selection(raw, 4, 1);
    handle_extend_action(raw, InputMethod_ExtendAction(1));
    handle_extend_action(raw, InputMethod_ExtendAction::IME_EXTEND_ACTION_SELECT_ALL);
    handle_extend_action(raw, InputMethod_ExtendAction::IME_EXTEND_ACTION_CUT);
    handle_extend_action(raw, InputMethod_ExtendAction::IME_EXTEND_ACTION_COPY);
    handle_extend_action(raw, InputMethod_ExtendAction::IME_EXTEND_ACTION_PASTE);
    assert_eq!(
        take(&events),
        [
            Event::MoveCursor(Direction::Up),
            Event::MoveCursor(Direction::Down),
            Event::MoveCursor(Direction::Left),
            Event::MoveCursor(Direction::Right),
            Event::SetSelection(4, 1),
            Event::ExtendAction(ExtendAction::SelectAll),
            Event::ExtendAction(ExtendAction::Cut),
            Event::ExtendAction(ExtendAction::Copy),
            Event::ExtendAction(ExtendAction::Paste),
        ]
    );
}

/// An `Ime` that keeps all provided methods.
struct RequiredOnly(TextConfig);

impl Ime for RequiredOnly {
    fn insert_text(&self, _text: String) {}
    fn delete_forward(&self, _len: usize) {}
    fn delete_backward(&self, _len: usize) {}
    fn get_text_config(&self) -> &TextConfig {
        &self.0
    }
    fn send_enter_key(&self, _enter_key: InputMethod_EnterKeyType) {}
}

#[test]
fn provided_methods_answer_like_an_empty_editor() {
    let _serial = serial();
    let editor = RawTextEditorProxy::new(Box::new(RequiredOnly(TextConfig::default()))).unwrap();
    let raw = editor.raw.as_ptr();
    assert_eq!(query(get_left_text_of_cursor, &editor, 10), utf16(""));
    assert_eq!(query(get_right_text_of_cursor, &editor, 10), utf16(""));
    assert_eq!(get_text_index_at_cursor(raw), 0);
    // Rejecting the preview makes the input method insert the text instead.
    assert_eq!(preview(&editor, "ni", -1, -1), -1);
    finish_text_preview(raw);
    move_cursor(raw, InputMethod_Direction::IME_DIRECTION_LEFT);
    handle_set_selection(raw, 0, 1);
    handle_extend_action(raw, InputMethod_ExtendAction::IME_EXTEND_ACTION_COPY);
    send_keyboard_status(raw, InputMethod_KeyboardStatus::IME_KEYBOARD_STATUS_SHOW);
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
fn private_commands_skip_null_and_decode_invalid_utf8_lossily() {
    use ohos_ime_sys::private_command::*;
    let _serial = serial();
    let (editor, events) = new_editor();
    let mut key = *b"a\xff";
    // SAFETY: `key` is valid for its length, and `command` is checked before use and destroyed
    // after the callbacks.
    unsafe {
        let command = OH_PrivateCommand_Create(key.as_mut_ptr().cast(), key.len());
        assert!(!command.is_null());
        OH_PrivateCommand_SetBoolValue(command, true).unwrap();
        let mut pointers = [std::ptr::null_mut(), command];
        let raw = editor.raw.as_ptr();
        assert_eq!(receive_private_command(raw, pointers.as_mut_ptr(), 2), 0);
        assert_eq!(receive_private_command(raw, std::ptr::null_mut(), 3), 0);
        OH_PrivateCommand_Destroy(command);
    }
    assert_eq!(
        take(&events),
        [Event::PrivateCommands(vec![PrivateCommand::new(
            "a\u{FFFD}",
            PrivateCommandValue::Bool(true)
        )])]
    );
}

/// An `Ime` that creates, uses and drops another editor while it handles an insertion.
struct CreatesEditors {
    config: TextConfig,
    inner_events: Arc<Mutex<Vec<Event>>>,
}

impl Ime for CreatesEditors {
    fn insert_text(&self, text: String) {
        let (inner, events) = new_editor();
        insert(&inner, &utf16(&text));
        drop(inner);
        self.inner_events.lock().unwrap().extend(take(&events));
    }
    fn delete_forward(&self, _len: usize) {}
    fn delete_backward(&self, _len: usize) {}
    fn get_text_config(&self) -> &TextConfig {
        &self.config
    }
    fn send_enter_key(&self, _enter_key: InputMethod_EnterKeyType) {}
}

#[test]
fn callbacks_may_create_and_drop_editors() {
    let _serial = serial();
    let inner_events = Arc::new(Mutex::new(Vec::new()));
    let ime = CreatesEditors {
        config: TextConfig::default(),
        inner_events: inner_events.clone(),
    };
    let editor = RawTextEditorProxy::new(Box::new(ime)).unwrap();
    let raw = editor.raw.as_ptr() as usize;
    let (done, finished) = mpsc::channel();
    std::thread::spawn(move || {
        let text = utf16("a");
        // SAFETY: Text editors are never destroyed, and `text` is a valid slice.
        unsafe { insert_text(raw as *mut _, text.as_ptr(), text.len()) };
        done.send(()).unwrap();
    });
    if finished.recv_timeout(Duration::from_secs(10)).is_err() {
        // Dropping the editor would wait for the deadlocked callback.
        std::mem::forget(editor);
        panic!("The callback deadlocked");
    }
    assert_eq!(take(&inner_events), [Event::Insert("a".into())]);
}

#[test]
fn concurrent_editors_receive_their_own_events() {
    let _serial = serial();
    std::thread::scope(|scope| {
        for thread in 0..4 {
            scope.spawn(move || {
                for i in 0..50 {
                    let (editor, events) = new_editor();
                    let text = format!("{thread} {i}");
                    insert(&editor, &utf16(&text));
                    assert_eq!(take(&events), [Event::Insert(text)]);
                }
            });
        }
    });
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

#[test]
fn dropped_editors_are_reused_after_exactly_the_delay() {
    let _serial = serial();
    // Forget the editors that earlier tests in this process recycled.
    RECYCLED.lock().unwrap().clear();
    let (editor, _) = new_editor();
    let addr = editor.addr();
    drop(editor);
    drop((1..RECYCLE_DELAY).map(|_| new_editor()).collect::<Vec<_>>());
    let (next, _) = new_editor();
    assert_ne!(
        next.addr(),
        addr,
        "reused after {} drops",
        RECYCLE_DELAY - 1
    );
    drop(next);
    let (next, _) = new_editor();
    assert_eq!(next.addr(), addr);
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
    // Dropping the superseded proxy leaves `b` attached.
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
fn superseded_proxy_methods_return_detached() {
    let _serial = serial();
    let a = fake_attached_proxy();
    let b = fake_attached_proxy();
    let detached = Err(InputMethodErrorCode::DETACHED);
    assert_eq!(a.show_keyboard(), detached);
    assert_eq!(a.hide_keyboard(), detached);
    #[cfg(feature = "api-15")]
    assert_eq!(a.show_text_input(&AttachOptions::new(true)), detached);
    assert_eq!(a.notify_selection_change("abc", 1, 2), detached);
    assert_eq!(
        a.notify_configuration_change(
            InputMethod_EnterKeyType::IME_ENTER_KEY_DONE,
            InputMethod_TextInputType::IME_TEXT_INPUT_TYPE_TEXT
        ),
        detached
    );
    assert_eq!(a.notify_cursor_update(CursorRect::default()), detached);
    let command = PrivateCommand::new("k", PrivateCommandValue::Bool(true));
    assert_eq!(a.send_private_command(&[command]), detached);
    drop(a);
    drop_fake(b);
}

#[test]
fn selection_positions_beyond_i32_are_rejected() {
    let _serial = serial();
    let proxy = fake_attached_proxy();
    let too_far = usize::try_from(i32::MAX).unwrap() + 1;
    assert_eq!(
        proxy.notify_selection_change("abc", too_far, 0),
        Err(InputMethodErrorCode::PARAMCHECK)
    );
    assert_eq!(
        proxy.notify_selection_change("abc", 0, too_far),
        Err(InputMethodErrorCode::PARAMCHECK)
    );
    drop_fake(proxy);
}

#[test]
fn attaches_are_serialized() {
    let _serial = serial();
    let log = &Mutex::new(Vec::new());
    let (attaching, started) = mpsc::channel();
    let b_addr = std::thread::scope(|scope| {
        scope.spawn(move || {
            let (a, _) = new_editor();
            attach_editor(&a, |editor, ime_proxy| {
                log.lock().unwrap().push("a starts");
                attaching.send(()).unwrap();
                std::thread::sleep(Duration::from_millis(100));
                log.lock().unwrap().push("a ends");
                fake_attach(editor, ime_proxy)
            })
            .unwrap();
        });
        let b = scope.spawn(move || {
            started.recv().unwrap();
            let (b, _) = new_editor();
            attach_editor(&b, |editor, ime_proxy| {
                log.lock().unwrap().push("b starts");
                fake_attach(editor, ime_proxy)
            })
            .unwrap();
            b.addr()
        });
        b.join().unwrap()
    });
    assert_eq!(*log.lock().unwrap(), ["a starts", "a ends", "b starts"]);
    let mut attached = attached_editor();
    assert_eq!(*attached, Some(b_addr));
    *attached = None;
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
        .selection(TextSelection::new(2, 5).unwrap())
        .window_id(42);
    #[cfg(feature = "api-20")]
    let builder = builder.placeholder("Søk 😀").ability_name("EntryAbility");
    #[cfg(feature = "api-26")]
    let builder = builder.consume_key_events(true);
    let ime = Recorder {
        events: Default::default(),
        config: builder.build(),
        text_index: 0,
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

#[test]
fn selections_beyond_i32_are_rejected() {
    let max = usize::try_from(i32::MAX).unwrap();
    assert!(TextSelection::new(max, max).is_ok());
    assert!(TextSelection::new(max + 1, 0).is_err());
    assert!(TextSelection::new(0, max + 1).is_err());
}

/// The framework does not initialize the text config it passes, so unset fields must be written.
#[test]
fn unset_text_config_fields_are_reset() {
    use ohos_ime_sys::text_avoid_info::*;
    use ohos_ime_sys::text_config::*;
    use ohos_ime_sys::types::InputMethod_TextInputType;
    let _serial = serial();
    let (editor, _) = new_editor();

    // SAFETY: `config` is created and destroyed here, and all out-pointers point to locals.
    unsafe {
        let config = OH_TextConfig_Create();
        assert!(!config.is_null());
        OH_TextConfig_SetInputType(config, InputMethod_TextInputType::IME_TEXT_INPUT_TYPE_URL)
            .unwrap();
        OH_TextConfig_SetEnterKeyType(config, InputMethod_EnterKeyType::IME_ENTER_KEY_GO).unwrap();
        OH_TextConfig_SetPreviewTextSupport(config, true).unwrap();
        #[cfg(feature = "api-26")]
        OH_TextConfig_SetConsumeKeyEvents(config, true).unwrap();
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

        let mut input_type = InputMethod_TextInputType::IME_TEXT_INPUT_TYPE_NONE;
        OH_TextConfig_GetInputType(config, &raw mut input_type).unwrap();
        assert_eq!(
            input_type,
            InputMethod_TextInputType::IME_TEXT_INPUT_TYPE_TEXT
        );
        let mut enter_key = InputMethod_EnterKeyType::IME_ENTER_KEY_DONE;
        OH_TextConfig_GetEnterKeyType(config, &raw mut enter_key).unwrap();
        assert_eq!(
            enter_key,
            InputMethod_EnterKeyType::IME_ENTER_KEY_UNSPECIFIED
        );
        let mut preview = true;
        OH_TextConfig_IsPreviewTextSupported(config, &raw mut preview).unwrap();
        assert!(!preview);
        #[cfg(feature = "api-26")]
        {
            let mut consume = true;
            OH_TextConfig_GetConsumeKeyEvents(config, &raw mut consume).unwrap();
            assert!(!consume);
        }
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
