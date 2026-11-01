import { useEffect, useRef } from 'react';
import './GpuBackgroundBox.css';

const GRID_COLUMNS = 156;
const WAVE_AMPLITUDE = 120;
const WAVE_FREQUENCY = 0.78;
const MIN_SEGMENT_HEIGHT = 2;
const MAX_SEGMENT_HEIGHT = 48;

const palette = [
  '#2132bd',
  '#263bd0',
  '#2d47de',
  '#3555ea',
  '#4164f2',
  '#4c72f7',
];

const clamp = (value, min, max) => Math.min(max, Math.max(min, value));

const noise = (seed) => {
  const value = Math.sin(seed * 12.9898) * 43758.5453;
  return value - Math.floor(value);
};

const drawBackground = (canvas) => {
  const rect = canvas.getBoundingClientRect();
  const width = Math.max(1, Math.floor(rect.width));
  const height = Math.max(1, Math.floor(rect.height));
  const dpr = Math.max(1, Math.min(window.devicePixelRatio || 1, 2));

  canvas.width = Math.floor(width * dpr);
  canvas.height = Math.floor(height * dpr);
  canvas.style.width = `${width}px`;
  canvas.style.height = `${height}px`;

  const ctx = canvas.getContext('2d');
  if (!ctx) return;

  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, width, height);

  ctx.fillStyle = '#0b0c10';
  ctx.fillRect(0, 0, width, height);

  const columnWidth = width / GRID_COLUMNS;

  for (let col = 0; col < GRID_COLUMNS; col += 1) {
    const progress = col / (GRID_COLUMNS - 1);
    const waveLine =
      height * 0.5 +
      Math.sin((progress - 0.08) * Math.PI * 2 * WAVE_FREQUENCY) * WAVE_AMPLITUDE +
      (progress - 0.5) * height * 0.1;
    const bandHalfHeight =
      height * (0.18 + 0.1 * Math.sin(progress * Math.PI * 1.45 + 0.55)) +
      noise(col + 11) * 28;
    const top = clamp(waveLine - bandHalfHeight - noise(col + 4) * 44, 0, height);
    const bottom = clamp(waveLine + bandHalfHeight + noise(col + 19) * 52, 0, height);
    const x = col * columnWidth;
    let y = top;

    while (y < bottom) {
      const distanceToEdge = Math.min(y - top, bottom - y);
      const edgeFade = clamp(distanceToEdge / 42, 0, 1);
      const blockCenter = y + MIN_SEGMENT_HEIGHT / 2;
      const distanceFromWave = Math.abs(blockCenter - waveLine);
      const distanceRatio = clamp(distanceFromWave / Math.max(1, bandHalfHeight), 0, 1);
      const waveInfluence = 1 - distanceRatio;
      const bandIndex = Math.floor(distanceRatio * 5.4);
      const segmentHeight = clamp(
        Math.round(MIN_SEGMENT_HEIGHT * Math.pow(2, bandIndex) + noise(col * 41 + y) * 3),
        MIN_SEGMENT_HEIGHT,
        MAX_SEGMENT_HEIGHT
      );
      const colorIndex = clamp(
        Math.round((1 - waveInfluence) * 2 + noise(col * 17 + y) * 0.8),
        0,
        palette.length - 1
      );

      const alpha = clamp(
        (0.12 + Math.pow(waveInfluence, 1.25) * 0.72) * edgeFade,
        0,
        0.88
      );

      ctx.fillStyle = palette[colorIndex];
      ctx.globalAlpha = alpha;
      ctx.fillRect(x, y, columnWidth + 0.65, segmentHeight);

      y += Math.max(1, segmentHeight - 1);
    }
  }

  ctx.globalAlpha = 1;

  ctx.fillStyle = 'rgba(118, 140, 255, 0.34)';
  const dotSpacing = Math.max(6, width / 210);
  for (let i = 0; i < 360; i += 1) {
    const side = i % 2 === 0 ? 1 : -1;
    const progress = noise(i + 4);
    const x = side > 0 ? width * (0.72 + progress * 0.27) : width * progress * 0.28;
    const y = height * noise(i * 3 + 9);
    const dotAlpha = clamp((noise(i + 2) - 0.2) * 0.55, 0, 0.4);
    ctx.globalAlpha = dotAlpha;
    ctx.fillRect(Math.round(x / dotSpacing) * dotSpacing, Math.round(y / dotSpacing) * dotSpacing, 1, 1);
  }
  ctx.globalAlpha = 1;
};

const GpuBackgroundBox = () => {
  const canvasRef = useRef(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return undefined;

    let frame = 0;
    const resizeAndDraw = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => drawBackground(canvas));
    };

    resizeAndDraw();

    const resizeObserver = new ResizeObserver(() => {
      resizeAndDraw();
    });

    resizeObserver.observe(canvas);
    window.addEventListener('resize', resizeAndDraw);

    return () => {
      cancelAnimationFrame(frame);
      resizeObserver.disconnect();
      window.removeEventListener('resize', resizeAndDraw);
    };
  }, []);

  return (
    <div className="gpu-bg-container" aria-hidden="true">
      <canvas ref={canvasRef} className="gpu-bg-canvas" />
    </div>
  );
};

export default GpuBackgroundBox;
