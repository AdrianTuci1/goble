import { useEffect, useRef, useState } from 'react';
import { HERO_VISUAL_LOOP_DURATION, drawHeroVisualFrame } from './heroVisualRenderer';
import { AgentsView } from './agents/AgentsView';
import './HeroVisual.css';

const getAgentsStage = (elapsed) => {
  if (elapsed >= 14.4 && elapsed < 18.1) {
    return elapsed >= 16.2 ? 'selecting' : 'visible';
  }

  return 'hidden';
};

const SHOW_DEV_TIMELINE = false;
const MOBILE_WIDE_BREAKPOINT = 640;
const MOBILE_WIDE_WIDTH = 1040;
const MOBILE_WIDE_HEIGHT = 520;

const HeroVisual = ({ phase = 'hud' }) => {
  const canvasRef = useRef(null);
  const startRef = useRef(0);
  const bufferRef = useRef(null);
  const frameRef = useRef(null);
  const sizeRef = useRef({ width: 0, height: 0 });
  const visibleRef = useRef(true);
  const agentsStageRef = useRef('hidden');
  const devScrubActiveRef = useRef(false);
  const devElapsedRef = useRef(0);
  const wideMobileStyleKeyRef = useRef('');
  const [agentsStage, setAgentsStage] = useState('hidden');
  const [wideMobileStyle, setWideMobileStyle] = useState(null);
  const [devScrubActive, setDevScrubActive] = useState(false);
  const [devElapsed, setDevElapsed] = useState(0);

  useEffect(() => {
    devScrubActiveRef.current = devScrubActive;
  }, [devScrubActive]);

  useEffect(() => {
    devElapsedRef.current = devElapsed;
  }, [devElapsed]);

  useEffect(() => {
    const canvas = canvasRef.current;
    const ctx = canvas.getContext('2d', { alpha: false });
    const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

    bufferRef.current = {
      canvas: document.createElement('canvas'),
      ctx: null,
      image: null,
      lastCloudTime: -Infinity,
      atmosphereCanvas: document.createElement('canvas'),
      atmosphereCtx: null,
      sceneCanvas: document.createElement('canvas'),
      sceneCtx: null,
      cleanSceneCanvas: document.createElement('canvas'),
      cleanSceneCtx: null,
      lastSceneTime: -Infinity,
    };
    bufferRef.current.ctx = bufferRef.current.canvas.getContext('2d');
    bufferRef.current.atmosphereCtx = bufferRef.current.atmosphereCanvas.getContext('2d');
    bufferRef.current.sceneCtx = bufferRef.current.sceneCanvas.getContext('2d', { alpha: false });
    bufferRef.current.cleanSceneCtx = bufferRef.current.cleanSceneCanvas.getContext('2d', { alpha: false });

    const resize = () => {
      const rect = canvas.getBoundingClientRect();
      const dpr = Math.min(window.devicePixelRatio || 1, 1.75);
      const useWideMobileFrame = rect.width < MOBILE_WIDE_BREAKPOINT;
      const width = useWideMobileFrame ? MOBILE_WIDE_WIDTH : rect.width;
      const height = useWideMobileFrame ? MOBILE_WIDE_HEIGHT : rect.height;
      const scale = useWideMobileFrame ? rect.width / MOBILE_WIDE_WIDTH : 1;

      sizeRef.current = { width, height };
      canvas.width = Math.max(1, Math.floor(rect.width * dpr));
      canvas.height = Math.max(1, Math.floor(rect.height * dpr));
      ctx.setTransform(dpr * scale, 0, 0, dpr * scale, 0, 0);

      const nextStyleKey = useWideMobileFrame ? `${width}:${height}:${scale.toFixed(4)}` : 'default';
      if (nextStyleKey !== wideMobileStyleKeyRef.current) {
        wideMobileStyleKeyRef.current = nextStyleKey;
        setWideMobileStyle(
          useWideMobileFrame
            ? {
                '--hero-visual-wide-width': `${width}px`,
                '--hero-visual-wide-height': `${height}px`,
                '--hero-visual-wide-scale': scale,
              }
            : null,
        );
      }
    };

    const scheduleFrame = () => {
      if (!reducedMotion && visibleRef.current) {
        frameRef.current = window.requestAnimationFrame(render);
      }
    };

    const render = (now) => {
      if (!visibleRef.current) return;
      if (!startRef.current) startRef.current = now;

      const absoluteElapsed = (now - startRef.current) / 1000;
      const liveElapsed = absoluteElapsed % HERO_VISUAL_LOOP_DURATION;
      const elapsed = devScrubActiveRef.current ? devElapsedRef.current : liveElapsed;
      const isInitialCycle = devScrubActiveRef.current || absoluteElapsed < HERO_VISUAL_LOOP_DURATION;
      const { width, height } = sizeRef.current;
      const nextAgentsStage = getAgentsStage(elapsed);

      if (nextAgentsStage !== agentsStageRef.current) {
        agentsStageRef.current = nextAgentsStage;
        setAgentsStage(nextAgentsStage);
      }

      drawHeroVisualFrame({
        ctx,
        buffer: bufferRef.current,
        width,
        height,
        elapsed,
        backgroundElapsed: devScrubActiveRef.current ? devElapsedRef.current : absoluteElapsed,
        isInitialCycle,
        phase,
        reducedMotion,
      });

      scheduleFrame();
    };

    const observer = new IntersectionObserver(([entry]) => {
      const wasVisible = visibleRef.current;
      visibleRef.current = entry.isIntersecting;

      if (!wasVisible && visibleRef.current) {
        frameRef.current = window.requestAnimationFrame(render);
      }
    });

    resize();
    observer.observe(canvas);
    render(performance.now());
    window.addEventListener('resize', resize);

    return () => {
      window.removeEventListener('resize', resize);
      observer.disconnect();
      window.cancelAnimationFrame(frameRef.current);
    };
  }, [phase]);

  return (
    <div className="hero-visual" aria-label="Animated coding network visualization">
      <canvas ref={canvasRef} className="hero-visual__canvas" />
      <div className={`hero-visual__agents-layer hero-visual__agents-layer--${agentsStage}`} style={wideMobileStyle || undefined}>
        <AgentsView selectedRunId={agentsStage === 'selecting' ? '1' : undefined} />
      </div>
      {SHOW_DEV_TIMELINE && (
        <div className="hero-visual__dev-timeline" aria-label="Hero visual development timeline">
          <button
            type="button"
            className="hero-visual__dev-toggle"
            onClick={() => setDevScrubActive((current) => !current)}
          >
            {devScrubActive ? 'Scrub' : 'Live'}
          </button>
          <input
            type="range"
            min="0"
            max={HERO_VISUAL_LOOP_DURATION}
            step="0.05"
            value={devElapsed}
            onChange={(event) => {
              setDevScrubActive(true);
              setDevElapsed(Number(event.target.value));
            }}
          />
          <span className="hero-visual__dev-time">
            {devElapsed.toFixed(2)}s / {HERO_VISUAL_LOOP_DURATION.toFixed(1)}s
          </span>
        </div>
      )}
    </div>
  );
};

export default HeroVisual;
