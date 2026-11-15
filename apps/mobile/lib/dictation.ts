import { connection } from "./remote/connection";
import { transcribeAudio } from "./remote/transcribe";
import { SegmentRecorder, type RecordedSegment } from "./recorder";

/**
 * Dictation as the composer sees it: words arrive while the phone listens, and
 * the text keeps growing until the user closes the bar.
 *
 * The microphone records short segments; each one is transcribed by the worker
 * (the provider and model under Settings → Voice) and its words are appended to
 * what has been heard so far. The callback therefore always carries the
 * transcript from the start of the session, so a dropped segment leaves the
 * earlier words alone.
 */

export interface Dictation {
  /** Stop listening and resolve with everything that was heard. */
  stop(): Promise<string>;
  /** Silence the microphone without ending the session; unmuting resumes. */
  setMuted(muted: boolean): void;
}

export interface DictationOptions {
  /** The speech provider the worker will ask (`openai`, `elevenlabs`). */
  provider: string;
  /** The transcription model chosen on this phone. */
  model: string;
  /** Called whenever the transcript grows: the words so far, from the start. */
  onPartial: (text: string) => void;
  /** Called when the session cannot go on; the bar closes with what it heard. */
  onError?: (message: string) => void;
}

/** How much speech one upload carries. Two or three seconds keeps the text
 *  moving while the sentence is still being spoken. */
const SEGMENT_MS = 2600;

/** The message a failure is reported with. */
function reason(error: unknown): string {
  if (error instanceof Error) return error.message;
  return "Dictation stopped.";
}

export function startDictation(options: DictationOptions): Dictation {
  const { provider, model, onPartial, onError } = options;
  const recorder = new SegmentRecorder();
  const words: string[] = [];
  const pending = new Set<Promise<void>>();
  let heard = "";
  let stopped = false;
  let muted = false;
  let recording = false;
  let timer: ReturnType<typeof setTimeout> | null = null;

  function clearTimer() {
    if (timer) clearTimeout(timer);
    timer = null;
  }

  function fail(error: unknown) {
    if (stopped) return;
    stopped = true;
    clearTimer();
    recording = false;
    void recorder.discard();
    onError?.(reason(error));
  }

  function remember(text: string) {
    const trimmed = text.trim();
    if (!trimmed) return;
    words.push(trimmed);
    heard = words.join(" ");
    onPartial(heard);
  }

  function track(job: Promise<void>) {
    pending.add(job);
    void job
      .catch(() => {})
      .finally(() => {
        pending.delete(job);
      });
  }

  async function send(segment: RecordedSegment) {
    try {
      remember(
        await transcribeAudio({
          provider,
          model,
          mime: segment.mime,
          base64: segment.base64,
        }),
      );
    } catch (error) {
      fail(error);
    }
  }

  /** Close the open segment and hand it to the worker. */
  function cut() {
    clearTimer();
    if (!recording) return;
    recording = false;
    const job = (async () => {
      const segment = await recorder.finish();
      if (segment) await send(segment);
    })();
    track(job);
    void job
      .catch(() => {})
      .finally(() => {
        if (!stopped && !muted && !recording) void start();
      });
  }

  async function start() {
    if (stopped || muted || recording) return;
    try {
      await recorder.start();
    } catch (error) {
      fail(error);
      return;
    }
    // Muted or closed while the microphone was opening: keep nothing.
    if (stopped || muted) {
      await recorder.discard();
      return;
    }
    recording = true;
    timer = setTimeout(cut, SEGMENT_MS);
  }

  const live = connection.getSnapshot().status === "live";
  if (!live) {
    // Deferred: the caller assigns the handle it gets back, and the failure
    // must land after it has done so.
    queueMicrotask(() =>
      fail(
        new Error(
          "The worker link is not live. Dictation is transcribed by the worker.",
        ),
      ),
    );
  } else {
    void start();
  }

  return {
    async stop() {
      if (stopped) return heard;
      stopped = true;
      clearTimer();
      if (recording) {
        recording = false;
        try {
          const segment = await recorder.finish();
          if (segment) await send(segment);
        } catch {
          // The last segment is a bonus: the words before it are the answer.
        }
      }
      while (pending.size > 0) {
        await Promise.all([...pending].map((job) => job.catch(() => {})));
      }
      await recorder.discard();
      return heard;
    },
    setMuted(next: boolean) {
      if (muted === next) return;
      muted = next;
      if (next) {
        clearTimer();
        if (recording) {
          recording = false;
          void recorder.discard();
        }
      } else if (!stopped) {
        void start();
      }
    },
  };
}
