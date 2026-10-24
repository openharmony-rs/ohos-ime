import { beforeAll, beforeEach, describe, expect, it } from '@ohos/hypium';
import { Driver, ON } from '@kit.TestKit';
import demo from 'libohos_ime_demo.so';
import * as Harness from '../testability/Harness';

/** Error codes of the input method framework. */
const OK = 0;
const PARAMCHECK = 401;
const DETACHED = 12800009;

async function attachAndWaitForKeyboard(text: string): Promise<void> {
  demo.attach(text);
  await Harness.waitUntil(Harness.keyboardShown, 'the keyboard to show');
}

/** Taps the ArkUI text field, which takes the input method from the editor. */
async function focusArkUiTextField(driver: Driver): Promise<void> {
  const field = await driver.findComponent(ON.id(Harness.TEXT_INPUT));
  await field.click();
  // The framework rejects the editor's proxy once ArkUI attached its text field.
  await Harness.waitUntil(() => demo.notifyConfigurationChange() === DETACHED, 'the editor to be detached');
}

export default function proxyTest() {
  // What the framework does with the calls of `ImeProxy`, checked without tapping keys.
  describe('ImeProxy', () => {
    beforeAll(async () => {
      await Harness.waitForPage();
    });

    beforeEach(async () => {
      demo.detach();
      Harness.clearFocus();
      await Harness.waitUntil(() => !Harness.keyboardShown(), 'the keyboard to hide');
    });

    it('hidesAndShowsTheKeyboard', 0, async () => {
      await attachAndWaitForKeyboard('a');
      expect(demo.hideKeyboard()).assertEqual(OK);
      await Harness.waitUntil(() => !Harness.keyboardShown(), 'the keyboard to hide');
      expect(demo.showKeyboard()).assertEqual(OK);
      await Harness.waitUntil(Harness.keyboardShown, 'the keyboard to show again');
    });

    it('detachesWhenDropped', 0, async () => {
      await attachAndWaitForKeyboard('a');
      demo.detach();
      await Harness.waitUntil(() => !Harness.keyboardShown(), 'the keyboard to hide');
    });

    // Attaching replaces the editor, and dropping the replaced one must not detach the new one.
    it('dropsAReplacedEditorWithoutDetaching', 0, async () => {
      const driver = Driver.create();
      await attachAndWaitForKeyboard('a');
      demo.attach('b');
      await driver.delayMs(500);
      expect(Harness.keyboardShown()).assertTrue();
      expect(demo.notifyConfigurationChange()).assertEqual(OK);
    });

    it('sendsNotifications', 0, async () => {
      await attachAndWaitForKeyboard('abc');
      expect(demo.notifySelectionChange(1, 2)).assertEqual(OK);
      expect(demo.notifyConfigurationChange()).assertEqual(OK);
      expect(demo.notifyCursorUpdate(10, 20, 2, 30)).assertEqual(OK);
    });

    it('limitsPrivateCommands', 0, async () => {
      await attachAndWaitForKeyboard('a');
      expect(demo.sendPrivateCommands(0)).assertEqual(PARAMCHECK);
      expect(demo.sendPrivateCommands(1)).assertEqual(OK);
      expect(demo.sendPrivateCommands(5)).assertEqual(OK);
      expect(demo.sendPrivateCommands(6)).assertEqual(PARAMCHECK);
    });

    // Dropping an editor that ArkUI took the input method from must not end ArkUI's session.
    it('dropsAnEditorDetachedByArkUiWithoutDetaching', 0, async () => {
      const driver = Driver.create();
      await attachAndWaitForKeyboard('a');
      await focusArkUiTextField(driver);
      demo.detach();
      await driver.delayMs(500);
      expect(Harness.keyboardShown()).assertTrue();
    });

    it('reattachesAfterArkUiTookTheInputMethod', 0, async () => {
      const driver = Driver.create();
      await attachAndWaitForKeyboard('a');
      await focusArkUiTextField(driver);
      expect(demo.reattach()).assertEqual(OK);
      expect(demo.notifyConfigurationChange()).assertEqual(OK);
      await Harness.waitUntil(Harness.keyboardShown, 'the keyboard to show');
    });
  });
}
