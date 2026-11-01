export const palette = {
  ink: [102, 110, 206],
  violet: [129, 127, 232],
  cyan: [190, 235, 226],
  mint: [160, 222, 215],
  pink: [235, 105, 184],
  peach: [240, 171, 160],
  haze: [207, 203, 230],
};

export const clusters = [
  {
    label: "(*Agent) Input processing",
    labelOffset: [-2, -26],
    frame: { x: 0.055, y: 0.29, w: 0.185, h: 0.47 },
    pivot: 1,
    nodes: [
      { x: 0.12, y: 0.425, r: 9, seed: 0.8 },
      { x: 0.168, y: 0.54, r: 9.5, seed: 1.9, pinned: true },
      { x: 0.09, y: 0.72, r: 9, seed: 3.1 },
      { x: 0.18, y: 0.34, r: 5.5, seed: 4.3, filled: true },
      { x: 0.12, y: 0.92, r: 8.5, seed: 5.4 },
    ],
  },
  {
    label: "(*Agent) Data ingestion",
    labelOffset: [0, -22],
    frame: { x: 0.295, y: 0.735, w: 0.112, h: 0.31 },
    pivot: 1,
    nodes: [
      { x: 0.318, y: 0.93, r: 7.5, seed: 8.2 },
      { x: 0.385, y: 0.885, r: 8.2, seed: 9.1, pinned: true },
    ],
  },
  {
    label: "(*Agent) Plan generation",
    labelOffset: [0, -24],
    frame: { x: 0.75, y: 0.055, w: 0.205, h: 0.53 },
    pivot: 2,
    nodes: [
      { x: 0.812, y: 0.15, r: 9.2, seed: 12.5 },
      { x: 0.89, y: 0.14, r: 9.4, seed: 13.4 },
      { x: 0.878, y: 0.35, r: 9.6, seed: 14.2, pinned: true },
      { x: 0.8, y: 0.52, r: 8.3, seed: 15.1 },
      { x: 0.925, y: 0.45, r: 8.5, seed: 16.2 },
    ],
  },
];

export const strayNodes = [
  { x: 0.632, y: 0.106, r: 6.5, seed: 21 },
  { x: 0.526, y: 0.298, r: 6, seed: 22 },
  { x: 1.012, y: 0.785, r: 6, seed: 23 },
  { x: 0.93, y: 0.985, r: 6.2, seed: 24 },
];

export const longLinks = [
  { from: [0, 1], to: [2, 0], delay: 0.03 },
  { from: [0, 0], to: [2, 2], delay: 0.14 },
  { from: [0, 1], to: [2, 1], delay: 0.22 },
  { from: [0, 1], to: [2, 2], delay: 0.32 },
  { from: [1, 1], to: [2, 2], delay: 0.45 },
  { from: [0, 1], to: [1, 1], delay: 0.54 },
];

export const matterBlobs = [
  { x: 0.13, y: 0.58, rx: 0.13, ry: 0.38, seed: 0.2, strength: 1.15 },
  { x: 0.19, y: 0.42, rx: 0.08, ry: 0.18, seed: 1.6, strength: 0.78 },
  { x: 0.36, y: 0.9, rx: 0.11, ry: 0.18, seed: 2.9, strength: 0.72 },
  { x: 0.51, y: 0.48, rx: 0.19, ry: 0.2, seed: 4.1, strength: 0.58 },
  { x: 0.82, y: 0.3, rx: 0.15, ry: 0.31, seed: 7.35, strength: 1.34 },
  { x: 0.885, y: 0.64, rx: 0.12, ry: 0.35, seed: 9.7, strength: 0.84 },
];

import { installCommands } from '../../../../config/install';

export const INSTALL_COMMAND = installCommands.mac;
