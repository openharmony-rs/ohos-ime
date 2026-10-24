import { window } from '@kit.ArkUI';
import { inputMethod } from '@kit.IMEKit';
import { hilog } from '@kit.PerformanceAnalysisKit';
import { Driver } from '@kit.TestKit';

/** The `AppStorage` key and component ID of the text field on the test page. */
export const TEXT_INPUT = 'textInput';
/** The `AppStorage` key the test page sets when its first frame is rendered. */
export const PAGE_SHOWN = 'pageShown';

/**
 * Where the backspace key is on the soft keyboard of known input methods, as fractions of the
 * keyboard area. The keys are not in the layout tree, so the tests tap by position.
 */
const BACKSPACE: Record<string, number[]> = {
  'com.example.kikakeyboard': [0.93, 0.68],
  'com.huawei.hmos.inputmethod': [0.92, 0.6],
};

let mainWindow: window.Window | undefined;

export function setMainWindow(pageWindow: window.Window): void {
  mainWindow = pageWindow;
}

/**
 * Waits until the first frame of the test page is rendered. When ArkUI renders it, it handles the
 * focus of the window and closes the input method, because no text field has the focus, which
 * detaches an editor attached before.
 */
export async function waitForPage(): Promise<void> {
  await waitUntil(() => AppStorage.get<boolean>(PAGE_SHOWN) === true, 'the test page to render');
}

/** The area of the soft keyboard on the screen, empty while it is hidden. */
export function keyboardRect(): window.Rect {
  if (mainWindow === undefined) {
    throw new Error('The main window is not known');
  }
  const windowRect = mainWindow.getWindowProperties().windowRect;
  const keyboard = mainWindow.getWindowAvoidArea(window.AvoidAreaType.TYPE_KEYBOARD).bottomRect;
  return {
    left: windowRect.left + keyboard.left,
    top: windowRect.top + keyboard.top,
    width: keyboard.width,
    height: keyboard.height,
  };
}

/** Whether the soft keyboard is shown. */
export function keyboardShown(): boolean {
  return keyboardRect().height > 0;
}

/** Moves the focus away from the text field, which makes ArkUI close the input method. */
export function clearFocus(): void {
  if (mainWindow === undefined) {
    throw new Error('The main window is not known');
  }
  mainWindow.getUIContext().getFocusController().clearFocus();
}

export async function waitUntil(condition: () => boolean, what: string, timeoutMs: number = 10000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (!condition()) {
    if (Date.now() > deadline) {
      throw new Error(`Timed out waiting for ${what}`);
    }
    await new Promise<void>((resolve) => setTimeout(resolve, 50));
  }
}

/** Waits for the soft keyboard and taps its backspace key. */
export async function pressBackspace(driver: Driver): Promise<void> {
  await waitUntil(() => keyboardRect().height > 0, 'the soft keyboard to show');
  // Let the keyboard finish its show animation.
  await new Promise<void>((resolve) => setTimeout(resolve, 500));
  const ime = inputMethod.getCurrentInputMethod().name;
  const position = BACKSPACE[ime];
  const rect = keyboardRect();
  hilog.info(0x0000, 'ohos-ime-test', 'Keyboard of %{public}s at %{public}s', ime, JSON.stringify(rect));
  if (position === undefined) {
    throw new Error(`The backspace position of the input method ${ime} is not known, add it to BACKSPACE`);
  }
  await driver.click(Math.round(rect.left + position[0] * rect.width),
    Math.round(rect.top + position[1] * rect.height));
}
