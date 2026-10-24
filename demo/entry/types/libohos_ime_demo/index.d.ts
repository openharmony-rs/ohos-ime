/** Attaches an editor containing `text` and shows the keyboard, replacing the attached editor. */
export const attach: (text: string) => void;
/** Detaches the editor. */
export const detach: () => void;
/** The text of the editor. */
export const text: () => string;
/** The callbacks the editor received since it was attached. */
export const events: () => string[];
