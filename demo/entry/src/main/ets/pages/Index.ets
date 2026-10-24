import demo from 'libohos_ime_demo.so';

/** Tap the editor to type into it with the input method; it shows the callbacks it received. */
@Entry
@Component
struct Index {
  @State editorText: string = '';
  @State events: string = '';
  private timer: number = -1;

  aboutToAppear(): void {
    this.timer = setInterval(() => {
      this.editorText = demo.text();
      this.events = demo.events().join('\n');
    }, 200);
  }

  aboutToDisappear(): void {
    clearInterval(this.timer);
    demo.detach();
  }

  build() {
    Column({ space: 12 }) {
      Text('ohos-ime editor')
        .fontSize(18)
      Text(this.editorText)
        .id('editor')
        .fontSize(28)
        .width('100%')
        .constraintSize({ minHeight: 56 })
        .padding(8)
        .border({ width: 1 })
        .onClick(() => demo.attach(this.editorText))
      Button('Detach')
        .onClick(() => demo.detach())
      Text('ArkUI TextInput, for comparison')
        .fontSize(18)
      TextInput()
        .id('textInput')
        .fontSize(28)
      Scroll() {
        Text(this.events)
          .id('events')
          .fontSize(14)
          .width('100%')
      }
      .layoutWeight(1)
      .align(Alignment.TopStart)
    }
    .padding(16)
    .width('100%')
    .height('100%')
  }
}
