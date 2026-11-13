import * as Haptics from "expo-haptics";

let enabled = true;

/** Called on launch and when Settings changes the haptics switch. */
export function setHapticsEnabled(value: boolean): void {
  enabled = value;
}

export function tapFeedback(): void {
  if (!enabled) return;
  void Haptics.selectionAsync().catch(() => undefined);
}

export function successFeedback(): void {
  if (!enabled) return;
  void Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success).catch(
    () => undefined,
  );
}
