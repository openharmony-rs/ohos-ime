import { afterEach, beforeAll, describe, expect, it } from '@ohos/hypium';
import { Driver, ON } from '@kit.TestKit';
import demo from 'libohos_ime_demo.so';
import * as Harness from '../testability/Harness';

interface Sample {
  name: string;
  text: string;
  /** What remains of `'a' + text` in an ArkUI text field after one backspace. */
  arkUiRemains: string;
}

/** Characters that take more than one UTF-16 code unit or more than one code point. */
const SAMPLES: Sample[] = [
  { name: 'Ascii', text: 'b', arkUiRemains: 'a' },
  { name: 'SurrogatePair', text: '\u{1F600}', arkUiRemains: 'a' },
  // ArkUI deletes a combining mark on its own.
  { name: 'CombiningMark', text: 'é', arkUiRemains: 'ae' },
  { name: 'SkinTone', text: '\u{1F44D}\u{1F3FD}', arkUiRemains: 'a' },
  { name: 'Flag', text: '\u{1F1E9}\u{1F1EA}', arkUiRemains: 'a' },
  { name: 'Keycap', text: '1️⃣', arkUiRemains: 'a' },
  { name: 'ZwjSequence', text: '\u{1F468}‍\u{1F469}‍\u{1F467}', arkUiRemains: 'a' },
];

function deletions(): string[] {
  return demo.events().filter((event) => event.startsWith('delete_'));
}

export default function backspaceTest() {
  describe('BackspaceInEditor', () => {
    beforeAll(async () => {
      await Harness.waitForPage();
    });

    afterEach(() => {
      demo.detach();
    });

    // The input method asks for one deletion, whatever the character takes in UTF-16 or code
    // points. `Ime::delete_backward` documents this.
    for (const sample of SAMPLES) {
      it(`deletesOneCharacter${sample.name}`, 0, async () => {
        const driver = Driver.create();
        demo.attach('a' + sample.text);
        await Harness.pressBackspace(driver);
        await Harness.waitUntil(() => deletions().length > 0, 'a deletion');
        await driver.delayMs(500);
        expect(deletions().join()).assertEqual('delete_backward(1)');
      });
    }
  });

  describe('BackspaceInArkUi', () => {
    // ArkUI's reading of the length the input method sends, for comparison.
    for (const sample of SAMPLES) {
      it(`deletes${sample.name}`, 0, async () => {
        const driver = Driver.create();
        AppStorage.setOrCreate(Harness.TEXT_INPUT, 'a' + sample.text);
        const field = await driver.findComponent(ON.id(Harness.TEXT_INPUT));
        const bounds = await field.getBounds();
        // Tapping right of the text puts the caret at its end.
        await driver.click(bounds.right - 20, Math.round((bounds.top + bounds.bottom) / 2));
        await Harness.pressBackspace(driver);
        await Harness.waitUntil(() => AppStorage.get<string>(Harness.TEXT_INPUT) !== 'a' + sample.text,
          'the text to change');
        expect(AppStorage.get<string>(Harness.TEXT_INPUT)).assertEqual(sample.arkUiRemains);
      });
    }
  });
}
