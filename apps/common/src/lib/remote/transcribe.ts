import { connection } from "./connection";
import type { WorkerMessage } from "./protocol";

/**
 * Speech to text through the worker.
 *
 * The phone holds no speech key: a dictation segment travels as one
 * `transcribe_audio` frame naming the provider and model the user chose, the
 * worker asks that provider, and the answer comes back under the same
 * `request_id`. The worker fans every frame to every client, so replies are
 * matched by that id.
 */

type Result = Extract<WorkerMessage, { kind: "transcription_result" }>;

/** How long one segment may take before the phone stops waiting. */
const TRANSCRIBE_TIMEOUT_MS = 45_000;

const waiters = new Map<string, (reply: Result) => void>();
let listening = false;
let counter = 0;

function listen() {
  if (listening) return;
  listening = true;
  connection.subscribeMessages((message) => {
    if (message.kind !== "transcription_result") return;
    const waiter = waiters.get(message.request_id);
    if (!waiter) return;
    waiters.delete(message.request_id);
    waiter(message);
  });
}

/**
 * Ask the worker for the words in one recorded segment. Resolves with the
 * transcript, which is empty when the provider heard nothing in it.
 */
export function transcribeAudio(request: {
  provider: string;
  model: string;
  mime: string;
  base64: string;
}): Promise<string> {
  listen();
  counter += 1;
  const requestId = `stt-${Date.now().toString(36)}-${counter}`;

  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      waiters.delete(requestId);
      reject(new Error("The worker did not return a transcript."));
    }, TRANSCRIBE_TIMEOUT_MS);

    waiters.set(requestId, (reply) => {
      clearTimeout(timer);
      if (reply.text !== undefined) resolve(reply.text);
      else reject(new Error(reply.error ?? "The worker refused the recording."));
    });

    const sent = connection.send({
      kind: "transcribe_audio",
      request_id: requestId,
      provider: request.provider,
      model: request.model,
      mime: request.mime,
      data: request.base64,
    });
    if (!sent) {
      clearTimeout(timer);
      waiters.delete(requestId);
      reject(new Error("The worker link is not live."));
    }
  });
}
