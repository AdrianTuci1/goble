/* eslint-disable */
/**
 * DSH sidebar icons, generated from `apps/web/assets/icons/*.svg`.
 *
 * Do not edit by hand: run `node apps/web/scripts/gen-sidebar-icons.mjs`.
 * The geometry is the same 24x24 artwork the Rust sidebar registers in
 * `crates/goble-ui/assets/icons/` (16px glyph scaled 1.5x, 1px stroke), so both
 * apps paint identical glyphs. Fill-only glyphs (triangle, ellipsis, pin) keep
 * their `fill`; the rest stroke `currentColor`.
 */
import type { ReactNode } from "react";

import { SvgIcon } from "./svg-icon";

export type SidebarIconName =
  | "check"
  | "chevrons-up-down"
  | "clock"
  | "ellipsis"
  | "flat-list"
  | "folder-close"
  | "folder-open"
  | "new-chat"
  | "pin-fill"
  | "pin-outline"
  | "project-add"
  | "sliders-two"
  | "status-running"
  | "triangle-down-fill"
  | "triangle-right-fill";

const ART: Record<SidebarIconName, ReactNode> = {
  "check": (
    <g transform="scale(1.5)" strokeWidth="1" strokeLinecap="round" strokeLinejoin="round">
    <path d="M2.25 8.5L5.49732 11.7473C5.90519 12.1552 6.57263 12.1344 6.95426 11.7018L13.75 4" stroke="currentColor"></path>
    </g>
  ),
  "chevrons-up-down": (
    <g transform="scale(1.5)" stroke="currentColor" strokeWidth="1" strokeLinecap="round" strokeLinejoin="round">
    <path d="m5.1 6 2.9-2.9L10.9 6"></path><path d="m5.1 10 2.9 2.9 2.9-2.9"></path>
    </g>
  ),
  "clock": (
    <g transform="scale(1.5)" strokeWidth="1" strokeLinecap="round" strokeLinejoin="round">
    <circle cx="8" cy="8" r="6.5" stroke="currentColor"></circle><path d="M8 4V8.5L11 10" stroke="currentColor"></path>
    </g>
  ),
  "ellipsis": (
    <g transform="scale(1.5)" strokeWidth="1" strokeLinecap="round" strokeLinejoin="round">
    <path d="M3 9C3.55228 9 4 8.55228 4 8C4 7.44772 3.55228 7 3 7C2.44772 7 2 7.44772 2 8C2 8.55228 2.44772 9 3 9Z" fill="currentColor"></path><path d="M8 9C8.55228 9 9 8.55228 9 8C9 7.44772 8.55228 7 8 7C7.44772 7 7 7.44772 7 8C7 8.55228 7.44772 9 8 9Z" fill="currentColor"></path><path d="M13 9C13.5523 9 14 8.55228 14 8C14 7.44772 13.5523 7 13 7C12.4477 7 12 7.44772 12 8C12 8.55228 12.4477 9 13 9Z" fill="currentColor"></path>
    </g>
  ),
  "flat-list": (
    <g transform="scale(1.5)" stroke="currentColor" strokeWidth="1" strokeLinecap="round" strokeLinejoin="round">
    <path d="M6 3.5h7.5M6 8h7.5M6 12.5h7.5"></path><path d="M2.6 3.5h.01M2.6 8h.01M2.6 12.5h.01"></path>
    </g>
  ),
  "folder-close": (
    <g transform="scale(1.5)" strokeWidth="1" strokeLinecap="round" strokeLinejoin="round">
    <path d="M1.50439 3.11059C1.50439 2.55831 1.95211 2.1106 2.50439 2.1106H5.43389C5.67773 2.1106 5.91318 2.19969 6.09593 2.36113L7.71649 3.79265C7.89924 3.95409 8.1347 4.04319 8.3785 4.04319H13.4958C14.0481 4.04319 14.4958 4.4909 14.4958 5.04319V12.8894C14.4958 13.4417 14.0481 13.8894 13.4958 13.8894H2.50439C1.95211 13.8894 1.50439 13.4417 1.50439 12.8894V4.04319V3.11059Z" stroke="currentColor"></path><path d="M3.63501 7.66614H12.3647" stroke="currentColor"></path>
    </g>
  ),
  "folder-open": (
    <g transform="scale(1.5)" strokeWidth="1" strokeLinecap="round" strokeLinejoin="round">
    <path d="M2.55912 7.93683C2.67584 7.49906 3.0723 7.19446 3.52536 7.19446H13.6491C14.3061 7.19446 14.7846 7.81725 14.6153 8.45209L13.4411 12.856C13.3244 13.2938 12.9279 13.5984 12.4748 13.5984H2.35113C1.69411 13.5984 1.21562 12.9756 1.38489 12.3407L2.55912 7.93683Z" fill="currentColor" opacity="0.16"></path><path d="M13.6491 6.69446C14.6346 6.69453 15.3522 7.62895 15.0983 8.58118L13.9245 12.9845C13.7494 13.6412 13.1539 14.0988 12.4743 14.0988H2.35126C1.36574 14.0988 0.648153 13.1643 0.902044 12.212L2.07587 7.80774C2.25102 7.15128 2.84567 6.69455 3.52509 6.69446H13.6491ZM3.52509 7.69446C3.29865 7.69455 3.10004 7.84674 3.04169 8.06555L1.86786 12.4698C1.78345 12.7872 2.02285 13.0988 2.35126 13.0988H12.4743C12.7007 13.0988 12.8992 12.9463 12.9577 12.7277L14.1325 8.32336C14.2171 8.00598 13.9776 7.69453 13.6491 7.69446H3.52509Z" fill="currentColor"></path><path d="M4.7666 1.90137C5.13227 1.90144 5.48571 2.03525 5.75977 2.27734L7.27246 3.61328C7.36379 3.69382 7.48174 3.73828 7.60352 3.73828H12.3994C13.2276 3.73841 13.8993 4.41005 13.8994 5.23828V6.7168C13.8183 6.70327 13.735 6.69436 13.6494 6.69434H12.8994V5.23828C12.8993 4.96233 12.6754 4.73841 12.3994 4.73828H7.60352C7.23781 4.73828 6.88446 4.60438 6.61035 4.3623L5.09766 3.02637C5.00636 2.94576 4.88838 2.90144 4.7666 2.90137H2.0498C1.77366 2.90137 1.5498 3.12523 1.5498 3.40137V9.78223L0.902344 12.2119C0.648452 13.1642 1.36604 14.0986 2.35156 14.0986H2.0498C1.2214 14.0986 0.549838 13.427 0.549805 12.5986V3.40137C0.549805 2.57294 1.22138 1.90137 2.0498 1.90137H4.7666Z" fill="currentColor"></path>
    </g>
  ),
  "new-chat": (
    <g transform="scale(1.5)" strokeWidth="1" strokeLinecap="round" strokeLinejoin="round">
    <path d="M2.37091 11.2501C1.58745 9.89288 1.32067 8.29835 1.61969 6.76006C1.91872 5.22177 2.76342 3.8433 3.99826 2.87846C5.2331 1.91362 6.77494 1.42737 8.33988 1.50925C9.90482 1.59113 11.3875 2.23562 12.5149 3.32406C13.6425 4.41269 14.3387 5.87206 14.4754 7.4334C14.612 8.99474 14.18 10.5529 13.2587 11.8209C12.3375 13.0888 10.9891 13.9813 9.46194 14.3337C8.18691 14.628 6.85895 14.5294 5.64989 14.0605C5.1712 13.8748 4.76962 13.4932 4.26534 13.3967C3.67413 13.2835 2.95257 13.5598 2.03794 14.3337" stroke="currentColor"></path><path d="M8 5V11" stroke="currentColor"></path><path d="M5 8H11" stroke="currentColor"></path>
    </g>
  ),
  "pin-fill": (
    <g transform="scale(1.5)" strokeWidth="1" strokeLinecap="round" strokeLinejoin="round">
    <path d="M9.96976 1.70572L13.1554 3.93629L10.9019 8.12317L11.5158 11.605L10.7192 12.7427L2.52767 7.00693L3.3243 5.86922L6.80612 5.25528L9.96976 1.70572Z" fill="currentColor" stroke="currentColor" strokeLinejoin="round"></path><path d="M6.05285 9.47511C6.27284 9.16094 6.70586 9.08458 7.02003 9.30457C7.3342 9.52455 7.41055 9.95757 7.19057 10.2717L3.98587 14.4708L3.21223 13.9291L6.05285 9.47511Z" fill="currentColor"></path>
    </g>
  ),
  "pin-outline": (
    <g transform="scale(1.5)" strokeWidth="1" strokeLinecap="round" strokeLinejoin="round">
    <path d="M9.96976 1.70572L13.1554 3.93629L10.9019 8.12317L11.5158 11.605L10.7192 12.7427L2.52767 7.00693L3.3243 5.86922L6.80612 5.25528L9.96976 1.70572Z" stroke="currentColor" strokeLinejoin="round"></path><path d="M6.05285 9.47511C6.27284 9.16094 6.70586 9.08458 7.02003 9.30457C7.3342 9.52455 7.41055 9.95757 7.19057 10.2717L3.98587 14.4708L3.21223 13.9291L6.05285 9.47511Z" fill="currentColor"></path>
    </g>
  ),
  "project-add": (
    <g transform="scale(1.5)" strokeWidth="1" strokeLinecap="round" strokeLinejoin="round">
    <path d="M5.54492 2.06738C5.91034 2.06754 6.26318 2.20149 6.53711 2.44336L7.94043 3.68164V4.7998C7.71462 4.74105 7.50367 4.63139 7.32617 4.47461L5.87598 3.19238C5.78477 3.11185 5.66658 3.06754 5.54492 3.06738H2.94922C2.67322 3.06738 2.44946 3.29145 2.44922 3.56738V12.4326C2.44927 12.7087 2.67311 12.9326 2.94922 12.9326H12.9326C13.2086 12.9325 13.4326 12.7086 13.4326 12.4326V8.53613H14.4326V12.4326C14.4326 13.2609 13.7609 13.9325 12.9326 13.9326H2.94922C2.12083 13.9326 1.44927 13.261 1.44922 12.4326V3.56738C1.44946 2.73916 2.12094 2.06738 2.94922 2.06738H5.54492Z" fill="currentColor"></path><path d="M9.75977 4.50208H14.5509" stroke="currentColor"></path><path d="M12.1492 6.89758L12.1492 2.10642" stroke="currentColor"></path>
    </g>
  ),
  "sliders-two": (
    <g transform="scale(1.5)" strokeWidth="1" strokeLinecap="round" strokeLinejoin="round">
    <path d="M1.5 5H8.5M11.5 5H14.5M1.5 11H4.5M7.5 11H14.5" stroke="currentColor"></path><circle cx="10" cy="5" r="1.5" stroke="currentColor"></circle><circle cx="6" cy="11" r="1.5" stroke="currentColor"></circle>
    </g>
  ),
  "status-running": (
    <g stroke="currentColor" strokeWidth="2" strokeLinecap="round">
    <circle cx="12" cy="12" r="9.5" opacity="0.25"/>
    <circle cx="12" cy="12" r="9.5" strokeDasharray="12 150"/>
    </g>
  ),
  "triangle-down-fill": (
    <g transform="rotate(90 12 12)">
    <g transform="scale(1.5)" strokeWidth="1" strokeLinecap="round" strokeLinejoin="round">
    <path d="M5.5 4.5C5.5 4.40714 5.52586 4.31612 5.57467 4.23713C5.62349 4.15815 5.69334 4.09431 5.77639 4.05279C5.85945 4.01126 5.95242 3.99368 6.0449 4.00202C6.13738 4.01036 6.22572 4.04429 6.3 4.1L10.967 7.6C11.0291 7.64657 11.0795 7.70697 11.1142 7.77639C11.1489 7.84582 11.167 7.92238 11.167 8C11.167 8.07762 11.1489 8.15418 11.1142 8.22361C11.0795 8.29303 11.0291 8.35343 10.967 8.4L6.3 11.9C6.22572 11.9557 6.13738 11.9896 6.0449 11.998C5.95242 12.0063 5.85945 11.9887 5.77639 11.9472C5.69334 11.9057 5.62349 11.8419 5.57467 11.7629C5.52586 11.6839 5.5 11.5929 5.5 11.5V4.5Z" fill="currentColor"></path>
    </g>
    </g>
  ),
  "triangle-right-fill": (
    <g transform="scale(1.5)" strokeWidth="1" strokeLinecap="round" strokeLinejoin="round">
    <path d="M5.5 4.5C5.5 4.40714 5.52586 4.31612 5.57467 4.23713C5.62349 4.15815 5.69334 4.09431 5.77639 4.05279C5.85945 4.01126 5.95242 3.99368 6.0449 4.00202C6.13738 4.01036 6.22572 4.04429 6.3 4.1L10.967 7.6C11.0291 7.64657 11.0795 7.70697 11.1142 7.77639C11.1489 7.84582 11.167 7.92238 11.167 8C11.167 8.07762 11.1489 8.15418 11.1142 8.22361C11.0795 8.29303 11.0291 8.35343 10.967 8.4L6.3 11.9C6.22572 11.9557 6.13738 11.9896 6.0449 11.998C5.95242 12.0063 5.85945 11.9887 5.77639 11.9472C5.69334 11.9057 5.62349 11.8419 5.57467 11.7629C5.52586 11.6839 5.5 11.5929 5.5 11.5V4.5Z" fill="currentColor"></path>
    </g>
  ),
};

export function SidebarIcon({
  color,
  name,
  size = 16,
}: {
  color: string;
  name: SidebarIconName;
  size?: number;
}) {
  return (
    <SvgIcon color={color} size={size}>
      {ART[name]}
    </SvgIcon>
  );
}
