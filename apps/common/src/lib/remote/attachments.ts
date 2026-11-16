import { connection } from "./connection";
import type { WorkerMessage } from "./protocol";

/**
 * Files the phone attaches to a turn.
 *
 * The relay has no HTTP upload: the bytes travel as one `upload_attachment`
 * frame, the worker writes them into the workspace the agent runs in, and the
 * answer is the path to mention in the prompt. The worker fans every frame to
 * every client, so a reply is matched by its `upload_id`.
 */

type StoredReply = Extract<WorkerMessage, { kind: "attachment_stored" }>;

/** How long a phone waits for the worker to confirm the write. */
const UPLOAD_TIMEOUT_MS = 20_000;

const waiters = new Map<string, (reply: StoredReply) => void>();
let listening = false;
let counter = 0;

function listen() {
  if (listening) return;
  listening = true;
  connection.subscribeMessages((message) => {
    if (message.kind !== "attachment_stored") return;
    const waiter = waiters.get(message.upload_id);
    if (!waiter) return;
    waiters.delete(message.upload_id);
    waiter(message);
  });
}

/**
 * Send one file to the worker and resolve with the path it was stored at. The
 * caller puts that path in the message, so the agent reads the file from its
 * own workspace.
 */
export function uploadAttachment(
  name: string,
  base64: string,
): Promise<string> {
  listen();
  counter += 1;
  const uploadId = `upload-${Date.now().toString(36)}-${counter}`;

  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      waiters.delete(uploadId);
      reject(new Error("The worker did not store the attachment."));
    }, UPLOAD_TIMEOUT_MS);

    waiters.set(uploadId, (reply) => {
      clearTimeout(timer);
      if (reply.path) resolve(reply.path);
      else reject(new Error(reply.error ?? "The worker refused the attachment."));
    });

    const sent = connection.send({
      kind: "upload_attachment",
      upload_id: uploadId,
      name,
      data: base64,
    });
    if (!sent) {
      clearTimeout(timer);
      waiters.delete(uploadId);
      reject(new Error("The worker link is not live."));
    }
  });
}
