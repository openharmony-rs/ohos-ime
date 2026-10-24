import { UIAbility } from '@kit.AbilityKit';
import { window } from '@kit.ArkUI';
import { hilog } from '@kit.PerformanceAnalysisKit';
import { abilityDelegatorRegistry } from '@kit.TestKit';
import { Hypium } from '@ohos/hypium';
import testsuite from '../test/List.test';
import * as Harness from './Harness';

/** Runs the tests, while its page shows the ArkUI text field they compare against. */
export default class TestAbility extends UIAbility {
  onCreate() {
    Hypium.hypiumTest(abilityDelegatorRegistry.getAbilityDelegator(),
      abilityDelegatorRegistry.getArguments(), testsuite);
  }

  onWindowStageCreate(windowStage: window.WindowStage) {
    try {
      Harness.setMainWindow(windowStage.getMainWindowSync());
    } catch (error) {
      hilog.error(0x0000, 'ohos-ime-test', 'Getting the main window failed: %{public}s',
        (error as Error).message);
    }
    windowStage.loadContent('testability/pages/Index', (error) => {
      if (error.code) {
        hilog.error(0x0000, 'ohos-ime-test', 'Loading the test page failed: %{public}s', error.message);
      }
    });
  }
}
