import { BackgroundRenderer } from './renderer/BackgroundRenderer';
import { CloudOnlyOverlayRenderer } from './renderer/CloudOnlyOverlayRenderer';
import { HudRenderer } from './renderer/HudRenderer';
import { AGENTS_START, HERO_VISUAL_LOOP_DURATION, HeroVisualTimeline } from './renderer/timeline';

export { HERO_VISUAL_LOOP_DURATION };

class HeroVisualRenderer {
  constructor() {
    this.timeline = new HeroVisualTimeline();
    this.background = new BackgroundRenderer();
    this.hud = new HudRenderer();
    this.cloudOnlyOverlay = new CloudOnlyOverlayRenderer(this.timeline);
  }

  drawFrame({ ctx, buffer, width, height, elapsed, backgroundElapsed, isInitialCycle, phase, reducedMotion }) {
    const time = reducedMotion ? 0.6 : elapsed;
    const backgroundTime = reducedMotion ? 0.6 : 0.6 + backgroundElapsed;
    const zoom = this.timeline.shouldZoom(elapsed) ? this.timeline.getZoomScale(elapsed) : 1;

    ctx.clearRect(0, 0, width, height);
    this.background.drawSceneView(ctx, buffer, width, height, backgroundTime, reducedMotion, zoom);

    if (phase === 'clouds') return;

    if (elapsed >= AGENTS_START) {
      this.cloudOnlyOverlay.draw(ctx, width, height, elapsed);
      return;
    }

    if (!isInitialCycle && elapsed < 1.05) return;

    this.hud.draw(ctx, width, height, time, elapsed, isInitialCycle);
  }
}

const renderer = new HeroVisualRenderer();

export const drawHeroVisualFrame = (frameState) => {
  renderer.drawFrame(frameState);
};
