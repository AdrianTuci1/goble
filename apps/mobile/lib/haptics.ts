import * as Haptics from "expo-haptics";

export function tapFeedback(): void {
  void Haptics.selectionAsync().catch(() => undefined);
}

export function successFeedback(): void {
  void Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success).catch(
    () => undefined,
  );
}
