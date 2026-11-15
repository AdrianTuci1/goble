import {
  AudioModule,
  RecordingPresets,
  requestRecordingPermissionsAsync,
  setAudioModeAsync,
  type AudioRecorder,
  type RecordingOptions,
} from "expo-audio";
import { File } from "expo-file-system";
import { Platform } from "react-native";

/**
 * The microphone behind the dictation bar.
 *
 * Speech to text is segment based: the bar records a few seconds, hands the
 * bytes to the worker, and starts the next segment while the previous one is
 * being transcribed. Each segment is a complete file — a recorder is stopped and
 * a fresh one started — so every upload stands on its own and a provider can
 * read it without the stream's earlier headers.
 *
 * `expo-audio` records on both native and web; only reading the file back
 * differs, because the web recorder hands out a blob URL while a native
 * recording is a file in the app's cache.
 */

/** Mono, 16 kHz, 64 kbps: speech, not music, and small enough to upload often. */
const RECORDING: Partial<RecordingOptions> = {
  ...RecordingPresets.LOW_QUALITY,
  numberOfChannels: 1,
  sampleRate: 16000,
  bitRate: 64000,
};

/** The container a native recording lands in (the presets' `.m4a`). */
const NATIVE_MIME = "audio/m4a";

/**
 * The recorder class this build carries. The native module exposes
 * `AudioRecorder`; the web shim exports the same class as `AudioRecorderWeb`
 * and has no `AudioRecorder` member, so the two are picked by presence. The
 * `expo-audio` hook does this internally; a segment recorder opened from a
 * callback needs the class itself.
 */
type RecorderConstructor = new (options: Partial<RecordingOptions>) => AudioRecorder;

let loadedRecorder: RecorderConstructor | null | undefined;

function recorderClass(): RecorderConstructor {
  if (loadedRecorder === undefined) {
    const audio = AudioModule as unknown as {
      AudioRecorder?: RecorderConstructor;
      AudioRecorderWeb?: RecorderConstructor;
    };
    loadedRecorder = audio.AudioRecorder ?? audio.AudioRecorderWeb ?? null;
  }
  if (!loadedRecorder) {
    throw new Error(
      "This build has no audio recorder. Reinstall the app to dictate.",
    );
  }
  return loadedRecorder;
}

export interface RecordedSegment {
  /** What the bytes are, so the provider reads the container right. */
  mime: string;
  base64: string;
}

/**
 * One microphone. `start` opens a segment, `finish` closes it and returns its
 * bytes, and a segment nobody wants is dropped with `discard`.
 */
export class SegmentRecorder {
  private recorder: AudioRecorder | null = null;
  private permitted = false;

  /** Ask for the microphone once, then open a new segment. */
  async start(): Promise<void> {
    if (!this.permitted) {
      const permission = await requestRecordingPermissionsAsync();
      if (!permission.granted) {
        throw new Error(
          "Microphone access is off. Allow it in Settings to dictate.",
        );
      }
      await setAudioModeAsync({
        allowsRecording: true,
        playsInSilentMode: true,
      });
      this.permitted = true;
    }
    const Recorder = recorderClass();
    const recorder = new Recorder(RECORDING);
    await recorder.prepareToRecordAsync();
    recorder.record();
    this.recorder = recorder;
  }

  /** Close the open segment and return what it captured, if anything. */
  async finish(): Promise<RecordedSegment | null> {
    const recorder = this.recorder;
    this.recorder = null;
    if (!recorder) return null;
    await recorder.stop();
    const uri = recorder.uri;
    if (!uri) return null;
    const segment = await read(uri);
    // A segment shorter than a syllable arrives as a few hundred bytes; asking
    // the provider about it costs a round trip and returns nothing.
    return segment.base64.length < 400 ? null : segment;
  }

  /** Close the open segment without keeping it (muting, or leaving). */
  async discard(): Promise<void> {
    const recorder = this.recorder;
    this.recorder = null;
    if (!recorder) return;
    try {
      await recorder.stop();
    } catch {
      // The recorder may already be stopped; the segment is being dropped.
    }
  }
}

async function read(uri: string): Promise<RecordedSegment> {
  if (Platform.OS === "web") {
    // The web recorder's uri is a blob URL, which only the browser can read.
    const response = await fetch(uri);
    const blob = await response.blob();
    const bytes = new Uint8Array(await blob.arrayBuffer());
    return {
      mime: blob.type || "audio/webm",
      base64: binaryToBase64(bytes),
    };
  }
  return { mime: NATIVE_MIME, base64: await new File(uri).base64() };
}

/** Base64 of raw bytes, chunked so a long segment never overflows the stack. */
function binaryToBase64(bytes: Uint8Array): string {
  const chunk = 0x8000;
  let binary = "";
  for (let index = 0; index < bytes.length; index += chunk) {
    binary += String.fromCharCode(...bytes.subarray(index, index + chunk));
  }
  return btoa(binary);
}
