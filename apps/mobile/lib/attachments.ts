import * as ImagePicker from "expo-image-picker";
import { Platform } from "react-native";

export type AttachmentSource = "library" | "camera";

/** An image the composer holds, with its bytes ready for the worker. */
export interface PickedAttachment {
  /** Local URI, used for the preview in the transcript and the composer. */
  uri: string;
  name: string;
  mime: string;
  /** Base64 of the file's bytes. Empty when the platform gave no bytes. */
  base64: string;
}

function pickOnWeb(capture: boolean): Promise<File | null> {
  if (typeof document === "undefined") return Promise.resolve(null);
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = "image/*";
    if (capture) input.setAttribute("capture", "environment");
    input.style.display = "none";
    document.body.appendChild(input);
    input.onchange = () => {
      const file = input.files?.[0] ?? null;
      input.remove();
      resolve(file);
    };
    input.click();
  });
}

/** Read a browser file into base64, without the `data:` prefix. */
function readAsBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = String(reader.result ?? "");
      const comma = result.indexOf(",");
      resolve(comma >= 0 ? result.slice(comma + 1) : "");
    };
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(file);
  });
}

function extensionFor(mime: string): string {
  const known: Record<string, string> = {
    "image/jpeg": "jpg",
    "image/png": "png",
    "image/gif": "gif",
    "image/webp": "webp",
    "image/heic": "heic",
  };
  return known[mime] ?? "jpg";
}

/** Opens the camera or the photo library and returns the image, bytes included. */
export async function pickAttachment(
  source: AttachmentSource,
): Promise<PickedAttachment | null> {
  if (Platform.OS === "web") {
    const file = await pickOnWeb(source === "camera");
    if (!file) return null;
    return {
      uri: URL.createObjectURL(file),
      name: file.name || `attachment.${extensionFor(file.type)}`,
      mime: file.type || "image/jpeg",
      base64: await readAsBase64(file),
    };
  }

  const permission =
    source === "camera"
      ? await ImagePicker.requestCameraPermissionsAsync()
      : await ImagePicker.requestMediaLibraryPermissionsAsync();
  if (!permission.granted) return null;

  const result =
    source === "camera"
      ? await ImagePicker.launchCameraAsync({
          base64: true,
          mediaTypes: ["images"],
          quality: 0.8,
        })
      : await ImagePicker.launchImageLibraryAsync({
          base64: true,
          mediaTypes: ["images"],
          quality: 0.8,
        });

  if (result.canceled) return null;
  const asset = result.assets[0];
  if (!asset) return null;
  const mime = asset.mimeType ?? "image/jpeg";
  return {
    uri: asset.uri,
    name: asset.fileName ?? `attachment.${extensionFor(mime)}`,
    mime,
    base64: asset.base64 ?? "",
  };
}
