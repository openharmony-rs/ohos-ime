import { FrameCallback } from '@kit.ArkUI';
import { PAGE_SHOWN, TEXT_INPUT } from '../Harness';

/** Reports when the first frame is rendered, after ArkUI handled the focus of the window. */
class FirstFrameCallback extends FrameCallback {
  onIdle(): void {
    AppStorage.setOrCreate(PAGE_SHOWN, true);
  }
}

/** An ArkUI text field whose text the tests read and write through `AppStorage`. */
@Entry
@Component
struct Index {
  @StorageLink(TEXT_INPUT) text: string = '';

  onPageShow(): void {
    this.getUIContext().postFrameCallback(new FirstFrameCallback());
  }

  build() {
    Column() {
      TextInput({ text: $$this.text })
        .id(TEXT_INPUT)
        .fontSize(28)
        .width('100%')
    }
    .padding(16)
    .width('100%')
    .height('100%')
  }
}
