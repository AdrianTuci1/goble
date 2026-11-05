# goble-notifications

Native push and approval actions for the mobile client.

## Android (`android/`)

- Kotlin `GobleNotificationsModule` (Expo Modules API).
- FCM token registration.
- Notification channel `approvals` and `runs`.
- Pending intent deep links: `goble://runs/<id>`.
- Approve / Deny actions call the gateway through the JS layer.

## iOS (`ios/`)

- Swift `GobleNotificationsModule` (Expo Modules API).
- APNs token registration.
- `UNNotificationCategory` with Approve and Deny actions.
- `UNNotificationServiceExtension` for redacted previews.
- Deep links through `expo-linking`.

Keep payloads small: `{ runId, projectId, kind, title }`. The phone fetches the
rest through the authenticated API.
