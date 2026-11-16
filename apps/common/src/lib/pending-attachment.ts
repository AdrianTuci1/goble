import type { PickedAttachment } from "./types";

/**
 * The image a Home composer handed to the session it is opening.
 *
 * The route carries the preview URI; the bytes are too large for a query
 * string, so they wait here for the screen that is about to mount and takes
 * them once. A screen that finds nothing simply starts empty.
 */

let held: PickedAttachment | null = null;

export function holdAttachment(attachment: PickedAttachment) {
  held = attachment;
}

/** Take the held image when it belongs to `uri`, and forget it either way. */
export function takeAttachment(uri?: string): PickedAttachment | null {
  const attachment = held;
  held = null;
  if (!attachment) return null;
  if (uri && attachment.uri !== uri) return null;
  return attachment;
}
