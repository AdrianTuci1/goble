import { useRouter } from "expo-router";

import { DetailHeader, EmptyState, Screen } from "@goble/common";

export default function NotFoundScreen() {
  const router = useRouter();
  return (
    <Screen>
      <DetailHeader onBack={() => router.replace("/")} title="Goble" />
      <EmptyState
        body="The screen you asked for does not exist yet."
        icon="compass-outline"
        title="Nothing here"
      />
    </Screen>
  );
}
