import * as ImagePicker from "expo-image-picker";
import { Platform } from "react-native";

export type AttachmentSource = "library" | "camera";

function pickOnWeb(capture: boolean): Promise<string | null> {
  if (typeof document === "undefined") return Promise.resolve(null);
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = "image/*";
    if (capture) input.setAttribute("capture", "environment");
    input.style.display = "none";
    document.body.appendChild(input);
    input.onchange = () => {
      const file = input.files?.[0];
      input.remove();
      resolve(file ? URL.createObjectURL(file) : null);
    };
    input.click();
  });
}

/** Opens the camera or the photo library and returns a local image URI. */
export async function pickAttachment(
  source: AttachmentSource,
): Promise<string | null> {
  if (Platform.OS === "web") return pickOnWeb(source === "camera");

  const permission =
    source === "camera"
      ? await ImagePicker.requestCameraPermissionsAsync()
      : await ImagePicker.requestMediaLibraryPermissionsAsync();
  if (!permission.granted) return null;

  const result =
    source === "camera"
      ? await ImagePicker.launchCameraAsync({
          mediaTypes: ["images"],
          quality: 0.8,
        })
      : await ImagePicker.launchImageLibraryAsync({
          mediaTypes: ["images"],
          quality: 0.8,
        });

  if (result.canceled) return null;
  return result.assets[0]?.uri ?? null;
}
