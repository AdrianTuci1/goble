import { Ionicons } from "@expo/vector-icons";
import { StyleSheet, View } from "react-native";

import { harnessMark, sessionStatusColor } from "@goble/common";
import type { SessionStatus } from "@goble/common";

import { BrandMark } from "./brand-mark";

/** The size the sidebar's rows draw the mark at. */
const MARK_SIZE = 20;

/** The status badge: an 8 pt dot on the mark's bottom-right corner. */
const BADGE_SIZE = 8;
const BADGE_RADIUS = BADGE_SIZE / 2;

/** How far the badge hangs past the mark's corner, so it reads as a badge on
 * the corner rather than a dot inside the glyph. */
const BADGE_OVERHANG = 2;

/** The ring the badge keeps in the row's own background, so the dot knocks a
 * clean hole out of the mark instead of tinting it. */
const BADGE_RING = 1.5;

/**
 * A conversation's harness mark with its status on the corner: Goble's own mark
 * for the native harness, the family's glyph in its accent for every other one.
 * `ringColor` is the background the row paints behind the mark — the badge's
 * ring is drawn in it, so the dot reads on a plain and a selected row alike.
 */
export function HarnessMark({
  harness,
  status,
  ringColor,
  size = MARK_SIZE,
}: {
  harness: string;
  status: SessionStatus;
  ringColor: string;
  size?: number;
}) {
  const mark = harnessMark(harness);
  const badgeInset = size - BADGE_SIZE + BADGE_OVERHANG;

  return (
    <View style={[styles.mark, { width: size, height: size }]}>
      {mark.glyph ? (
        <Ionicons color={mark.accent} name={mark.glyph} size={size} />
      ) : (
        <BrandMark size={size} />
      )}
      <View
        style={[
          styles.badge,
          {
            backgroundColor: sessionStatusColor[status],
            borderColor: ringColor,
            left: badgeInset,
            top: badgeInset,
          },
        ]}
      />
    </View>
  );
}

const styles = StyleSheet.create({
  mark: {
    alignItems: "center",
    justifyContent: "center",
  },
  badge: {
    borderRadius: BADGE_RADIUS,
    borderWidth: BADGE_RING,
    height: BADGE_SIZE,
    position: "absolute",
    width: BADGE_SIZE,
  },
});
