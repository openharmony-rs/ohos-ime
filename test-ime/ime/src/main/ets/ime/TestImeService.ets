import { Want } from '@kit.AbilityKit';
import { InputMethodExtensionAbility } from '@kit.IMEKit';
import { testIme } from './TestIme';

export default class TestImeService extends InputMethodExtensionAbility {
  onCreate(want: Want): void {
    testIme.start(this.context);
  }

  onDestroy(): void {
    testIme.stop();
  }
}
