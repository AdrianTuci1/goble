import { lerp, smoothstep } from './math';

export const CLOUD_FRAME_INTERVAL = 1 / 12;
export const CLOUD_RESOLUTION_DIVISOR = 6;
export const SCENE_FRAME_INTERVAL = 1 / 12;
export const AGENTS_START = 14.4;
export const CLOUD_ONLY_START = 18.1;
export const COMPOSER_BOX_START = 18.45;
export const COMPOSER_BOX_END = 22.65;
export const ZOOM_IN_START = 22.85;
export const RUNNING_TEXT_START = 23.35;
export const RUNNING_TEXT_END = 25.25;
export const FINISHED_TEXT_START = 25.55;
export const FINISHED_TEXT_END = 28.05;
export const ZOOM_OUT_START = 27.95;
export const ZOOM_OUT_END = 29.85;
export const HERO_VISUAL_LOOP_DURATION = 30.4;

export class HeroVisualTimeline {
  getZoomScale(elapsed) {
    if (elapsed < ZOOM_IN_START) return 1;

    const zoomOut = smoothstep(ZOOM_OUT_START, ZOOM_OUT_END, elapsed);
    return lerp(4.8, 1, zoomOut);
  }

  getRunningTextAlpha(elapsed) {
    return (
      smoothstep(RUNNING_TEXT_START, RUNNING_TEXT_START + 0.55, elapsed) *
      (1 - smoothstep(RUNNING_TEXT_END - 0.45, RUNNING_TEXT_END, elapsed))
    );
  }

  getFinishedTextAlpha(elapsed) {
    return (
      smoothstep(FINISHED_TEXT_START, FINISHED_TEXT_START + 0.55, elapsed) *
      (1 - smoothstep(FINISHED_TEXT_END - 0.65, FINISHED_TEXT_END, elapsed))
    );
  }

  isCloudOnly(elapsed) {
    return elapsed >= AGENTS_START;
  }

  shouldZoom(elapsed) {
    return elapsed >= CLOUD_ONLY_START;
  }
}
