/** Attaches an editor containing `text` and shows the keyboard, replacing the attached editor. */
export const attach: (text: string) => void;
/** Detaches the editor. */
export const detach: () => void;
/** The text of the editor. */
export const text: () => string;
/** The callbacks the editor received since it was attached. */
export const events: () => string[];
/*
 * The functions below call the `ImeProxy` method of the attached editor and return the error code
 * of the input method framework, 0 for success.
 */
/** Re-attaches the editor and shows the keyboard. */
export const reattach: () => number;
export const showKeyboard: () => number;
export const hideKeyboard: () => number;
/** Tells the input method the text of the editor and a selection in it. */
export const notifySelectionChange: (start: number, end: number) => number;
export const notifyConfigurationChange: () => number;
export const notifyCursorUpdate: (left: number, top: number, width: number, height: number) => number;
/** Sends `count` private commands to the input method. */
export const sendPrivateCommands: (count: number) => number;
