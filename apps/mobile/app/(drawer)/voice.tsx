import { Redirect } from "expo-router";

/** The voice choices moved under Settings; this keeps old links working. */
export default function VoiceScreen() {
  return <Redirect href="/settings/voice" />;
}
