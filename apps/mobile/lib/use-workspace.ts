import { useCallback, useState } from "react";
import { useFocusEffect } from "expo-router";

import { fetchHome } from "./api";
import { demoSnapshot } from "./mock";
import type { HomeSnapshot } from "./types";

export function useWorkspace() {
  const [snapshot, setSnapshot] = useState<HomeSnapshot>(() => demoSnapshot());
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async (pull = false) => {
    if (pull) setRefreshing(true);
    else setLoading(true);
    try {
      const next = await fetchHome();
      setSnapshot(next);
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not load the workspace");
    } finally {
      setLoading(false);
      setRefreshing(false);
    }
  }, []);

  useFocusEffect(
    useCallback(() => {
      void load();
    }, [load]),
  );

  const refresh = useCallback(() => {
    void load(true);
  }, [load]);

  return { snapshot, loading, refreshing, error, refresh };
}
