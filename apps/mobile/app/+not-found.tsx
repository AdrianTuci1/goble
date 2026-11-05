import { useRouter } from "expo-router";

import { DetailHeader } from "../components/detail-header";
import { EmptyState } from "../components/empty-state";
import { Screen } from "../components/screen";

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
