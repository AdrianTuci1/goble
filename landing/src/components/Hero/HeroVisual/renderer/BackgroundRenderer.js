import { CLOUD_FRAME_INTERVAL, CLOUD_RESOLUTION_DIVISOR, SCENE_FRAME_INTERVAL } from './timeline';
import { matterBlobs, palette } from './sceneData';
import { clamp, fbm, hash, mixColor, smoothstep, valueNoise } from './math';

export class BackgroundRenderer {
  frostedNoisePattern = null;

  ellipticalFalloff(x, y, blob, time) {
    const cx = blob.x + Math.sin(time * 0.11 + blob.seed) * 0.034;
    const cy = blob.y + Math.cos(time * 0.095 + blob.seed * 1.7) * 0.04;
    const edgeWarp = valueNoise(x * 6.4 + blob.seed + time * 0.08, y * 5.6 - blob.seed * 0.4 - time * 0.065) - 0.5;
    const tangentWarp = valueNoise(x * 10.5 - time * 0.12, y * 9.2 + blob.seed + time * 0.095) - 0.5;
    const rx = blob.rx * (1 + edgeWarp * 0.34 + Math.sin(time * 0.18 + blob.seed) * 0.08);
    const ry = blob.ry * (1 - edgeWarp * 0.28 + Math.cos(time * 0.16 + blob.seed * 1.4) * 0.08);
    const dx = (x - cx + tangentWarp * 0.026) / rx;
    const dy = (y - cy - edgeWarp * 0.03) / ry;

    return Math.exp(-(dx * dx + dy * dy) * 1.72) * blob.strength;
  }

  drawClouds(ctx, buffer, width, height, time, reducedMotion) {
    const lowWidth = Math.max(180, Math.round(width / CLOUD_RESOLUTION_DIVISOR));
    const lowHeight = Math.max(90, Math.round(height / CLOUD_RESOLUTION_DIVISOR));
    const motion = reducedMotion ? 0.35 : time;
    const shouldRender =
      buffer.canvas.width !== lowWidth ||
      buffer.canvas.height !== lowHeight ||
      !buffer.image ||
      motion < buffer.lastCloudTime ||
      motion - buffer.lastCloudTime >= CLOUD_FRAME_INTERVAL;

    if (buffer.canvas.width !== lowWidth || buffer.canvas.height !== lowHeight) {
      buffer.canvas.width = lowWidth;
      buffer.canvas.height = lowHeight;
      buffer.image = buffer.ctx.createImageData(lowWidth, lowHeight);
      buffer.lastCloudTime = -Infinity;
    }

    if (shouldRender) {
      const { data } = buffer.image;
      const aspect = lowWidth / lowHeight;

      for (let y = 0; y < lowHeight; y += 1) {
        for (let x = 0; x < lowWidth; x += 1) {
          const nx = x / lowWidth;
          const ny = y / lowHeight;
          const px = nx * aspect;
          const py = ny;
          const flow = fbm(px * 1.3 + motion * 0.012, py * 1.45 - motion * 0.01, 3);
          const curl = fbm(px * 3.1 - motion * 0.018 + flow * 0.5, py * 2.6 + motion * 0.012, 3);
          const detail = fbm(px * 6.2 + 4.5, py * 5.4 - motion * 0.018, 3);
          const diagonalVein = Math.sin((px * 1.55 + py * 3.2 + flow * 1.65 - motion * 0.035) * Math.PI);

          let matter = 0;
          for (let blobIndex = 0; blobIndex < matterBlobs.length; blobIndex += 1) {
            matter += this.ellipticalFalloff(nx, ny, matterBlobs[blobIndex], motion);
          }

          const vein = smoothstep(0.42, 0.86, diagonalVein * 0.35 + curl * 0.78);
          const density = clamp(matter * (0.88 + flow * 0.48) + vein * 0.18);
          const core = smoothstep(0.4, 0.82, density + detail * 0.13);
          const rim = smoothstep(0.14, 0.48, density + curl * 0.16) * (1 - smoothstep(0.5, 0.92, density));
          const warmCore = smoothstep(0.48, 0.95, core + Math.sin(px * 7.4 - py * 3.1 + motion * 0.015) * 0.08);

          let color = mixColor(palette.ink, palette.violet, 0.26 + flow * 0.22);
          color = mixColor(color, palette.mint, rim * 0.96);
          color = mixColor(color, palette.cyan, smoothstep(0.2, 0.58, rim + vein * 0.18) * 0.5);
          color = mixColor(color, palette.pink, core * 0.86);
          color = mixColor(color, palette.peach, warmCore * 0.26);
          color = mixColor(color, palette.haze, smoothstep(0.34, 0.78, density + detail * 0.08) * 0.18);

          const blueDepth = (1 - smoothstep(0.18, 0.66, density)) * 38;
          const texture = (detail - 0.5) * 5.5 + (hash(x + Math.floor(motion * 4), y) - 0.5) * 2.4;
          const vignette = Math.hypot(nx - 0.48, ny - 0.48) * 24;
          const index = (y * lowWidth + x) * 4;

          data[index] = clamp(color[0] + texture - vignette - blueDepth * 0.35, 0, 255);
          data[index + 1] = clamp(color[1] + texture - vignette * 0.45 - blueDepth * 0.25, 0, 255);
          data[index + 2] = clamp(color[2] + texture + 8 - vignette * 0.15, 0, 255);
          data[index + 3] = 255;
        }
      }

      buffer.ctx.putImageData(buffer.image, 0, 0);
      buffer.lastCloudTime = motion;
    }

    ctx.save();
    ctx.imageSmoothingEnabled = true;
    ctx.drawImage(buffer.canvas, 0, 0, width, height);
    ctx.globalCompositeOperation = 'screen';
    ctx.globalAlpha = 0.13;
    ctx.fillStyle = 'rgba(196, 248, 232, 0.35)';
    ctx.fillRect(0, 0, width, height);
    ctx.restore();
  }

  drawGlow(ctx, width, height, time) {
    const blobs = [
      [0.18, 0.6, 0.26, 'rgba(255, 99, 184, 0.24)', 0.1],
      [0.86, 0.32, 0.25, 'rgba(255, 106, 190, 0.25)', 1.9],
      [0.47, 0.55, 0.28, 'rgba(182, 244, 226, 0.22)', 3.1],
      [0.27, 0.18, 0.2, 'rgba(195, 250, 225, 0.18)', 4.4],
    ];

    ctx.save();
    ctx.globalCompositeOperation = 'screen';
    blobs.forEach(([x, y, radius, color, seed]) => {
      const cx = (x + Math.sin(time * 0.09 + seed) * 0.025) * width;
      const cy = (y + Math.cos(time * 0.08 + seed) * 0.028) * height;
      const gradient = ctx.createRadialGradient(cx, cy, 0, cx, cy, radius * width);
      gradient.addColorStop(0, color);
      gradient.addColorStop(1, 'rgba(255, 255, 255, 0)');
      ctx.fillStyle = gradient;
      ctx.fillRect(0, 0, width, height);
    });
    ctx.restore();
  }

  getFrostedNoisePattern(ctx) {
    if (this.frostedNoisePattern) return this.frostedNoisePattern;

    const noiseCanvas = document.createElement('canvas');
    const size = 180;
    noiseCanvas.width = size;
    noiseCanvas.height = size;
    const noiseCtx = noiseCanvas.getContext('2d');
    const image = noiseCtx.createImageData(size, size);

    for (let i = 0; i < image.data.length; i += 4) {
      const value = hash(i * 0.41, i * 0.17) > 0.52 ? 255 : 0;
      image.data[i] = value;
      image.data[i + 1] = value;
      image.data[i + 2] = value;
      image.data[i + 3] = 118;
    }

    noiseCtx.putImageData(image, 0, 0);
    this.frostedNoisePattern = ctx.createPattern(noiseCanvas, 'repeat');
    return this.frostedNoisePattern;
  }

  drawFrostedGlassLayer(ctx, width, height, time) {
    ctx.save();
    ctx.globalCompositeOperation = 'screen';
    ctx.globalAlpha = 0.08;
    ctx.fillStyle = 'rgba(235, 248, 255, 0.42)';
    ctx.fillRect(0, 0, width, height);
    ctx.restore();

    ctx.save();
    ctx.globalCompositeOperation = 'overlay';
    ctx.globalAlpha = 0.24;
    ctx.translate(Math.sin(time * 0.42) * 0.5, Math.cos(time * 0.36) * 0.5);
    ctx.scale(0.5, 0.5);
    ctx.fillStyle = this.getFrostedNoisePattern(ctx);
    ctx.fillRect(0, 0, width * 2, height * 2);
    ctx.restore();
  }

  drawInterestAccents(ctx, width, height, time) {
    const accents = [
      { x: 0.168, y: 0.54, r: 0.18, color: 'rgba(255, 64, 180, 0.24)', seed: 1.9 },
      { x: 0.145, y: 0.61, r: 0.16, color: 'rgba(255, 132, 34, 0.3)', seed: 2.35 },
      { x: 0.385, y: 0.885, r: 0.13, color: 'rgba(255, 84, 190, 0.2)', seed: 9.1 },
      { x: 0.36, y: 0.91, r: 0.12, color: 'rgba(255, 146, 42, 0.26)', seed: 9.8 },
      { x: 0.878, y: 0.35, r: 0.17, color: 'rgba(255, 74, 186, 0.24)', seed: 14.2 },
      { x: 0.855, y: 0.39, r: 0.16, color: 'rgba(255, 124, 30, 0.32)', seed: 14.85 },
      { x: 0.12, y: 0.425, r: 0.12, color: 'rgba(173, 255, 231, 0.18)', seed: 0.8 },
      { x: 0.17, y: 0.49, r: 0.1, color: 'rgba(255, 170, 58, 0.22)', seed: 4.6 },
      { x: 0.8, y: 0.52, r: 0.12, color: 'rgba(174, 255, 231, 0.18)', seed: 15.1 },
      { x: 0.9, y: 0.28, r: 0.1, color: 'rgba(255, 158, 50, 0.2)', seed: 17.2 },
    ];

    ctx.save();
    ctx.globalCompositeOperation = 'screen';
    accents.forEach((accent) => {
      const cx = (accent.x + Math.sin(time * 0.18 + accent.seed) * 0.01) * width;
      const cy = (accent.y + Math.cos(time * 0.16 + accent.seed) * 0.012) * height;
      const gradient = ctx.createRadialGradient(cx, cy, 0, cx, cy, accent.r * width);
      gradient.addColorStop(0, accent.color);
      gradient.addColorStop(0.52, accent.color.replace(/0\.\d+\)/, '0.08)'));
      gradient.addColorStop(1, 'rgba(255, 255, 255, 0)');
      ctx.fillStyle = gradient;
      ctx.fillRect(0, 0, width, height);
    });
    ctx.restore();
  }

  drawAtmosphereOverlay(ctx, width, height) {
    ctx.save();
    ctx.globalCompositeOperation = 'screen';

    const sideLight = ctx.createLinearGradient(0, 0, width, 0);
    sideLight.addColorStop(0, 'rgba(255, 255, 255, 0.12)');
    sideLight.addColorStop(0.18, 'rgba(255, 255, 255, 0)');
    sideLight.addColorStop(0.82, 'rgba(255, 255, 255, 0)');
    sideLight.addColorStop(1, 'rgba(255, 255, 255, 0.1)');
    ctx.fillStyle = sideLight;
    ctx.fillRect(0, 0, width, height);

    const leftBloom = ctx.createRadialGradient(width * 0.22, height * 0.75, 0, width * 0.22, height * 0.75, width * 0.3);
    leftBloom.addColorStop(0, 'rgba(255, 118, 190, 0.22)');
    leftBloom.addColorStop(1, 'rgba(255, 118, 190, 0)');
    ctx.fillStyle = leftBloom;
    ctx.fillRect(0, 0, width, height);

    const rightBloom = ctx.createRadialGradient(width * 0.88, height * 0.28, 0, width * 0.88, height * 0.28, width * 0.28);
    rightBloom.addColorStop(0, 'rgba(255, 120, 194, 0.2)');
    rightBloom.addColorStop(1, 'rgba(255, 120, 194, 0)');
    ctx.fillStyle = rightBloom;
    ctx.fillRect(0, 0, width, height);

    ctx.globalCompositeOperation = 'source-over';
    const shade = ctx.createLinearGradient(0, 0, 0, height);
    shade.addColorStop(0, 'rgba(10, 12, 20, 0.05)');
    shade.addColorStop(1, 'rgba(10, 12, 20, 0.14)');
    ctx.fillStyle = shade;
    ctx.fillRect(0, 0, width, height);

    ctx.restore();
  }

  drawCachedAtmosphereOverlay(ctx, buffer, width, height) {
    const canvas = buffer.atmosphereCanvas;

    if (canvas.width !== Math.round(width) || canvas.height !== Math.round(height)) {
      canvas.width = Math.max(1, Math.round(width));
      canvas.height = Math.max(1, Math.round(height));
      this.drawAtmosphereOverlay(buffer.atmosphereCtx, canvas.width, canvas.height);
    }

    ctx.drawImage(canvas, 0, 0, width, height);
  }

  renderSceneCache(buffer, width, height, time, reducedMotion) {
    const canvasWidth = Math.max(1, Math.round(width));
    const canvasHeight = Math.max(1, Math.round(height));
    const sceneCanvas = buffer.sceneCanvas;
    const cleanSceneCanvas = buffer.cleanSceneCanvas;
    const shouldRender =
      sceneCanvas.width !== canvasWidth ||
      sceneCanvas.height !== canvasHeight ||
      time < buffer.lastSceneTime ||
      time - buffer.lastSceneTime >= SCENE_FRAME_INTERVAL;

    if (sceneCanvas.width !== canvasWidth || sceneCanvas.height !== canvasHeight) {
      sceneCanvas.width = canvasWidth;
      sceneCanvas.height = canvasHeight;
      cleanSceneCanvas.width = canvasWidth;
      cleanSceneCanvas.height = canvasHeight;
      buffer.lastSceneTime = -Infinity;
    }

    if (shouldRender) {
      const sceneCtx = buffer.sceneCtx;
      const cleanSceneCtx = buffer.cleanSceneCtx;

      cleanSceneCtx.clearRect(0, 0, width, height);
      this.drawClouds(cleanSceneCtx, buffer, width, height, time, reducedMotion);
      this.drawGlow(cleanSceneCtx, width, height, time);
      this.drawInterestAccents(cleanSceneCtx, width, height, time);
      this.drawCachedAtmosphereOverlay(cleanSceneCtx, buffer, width, height);

      sceneCtx.clearRect(0, 0, width, height);
      sceneCtx.drawImage(cleanSceneCanvas, 0, 0, width, height);
      this.drawFrostedGlassLayer(sceneCtx, width, height, time);
      buffer.lastSceneTime = time;
    }
  }

  drawSceneView(ctx, buffer, width, height, time, reducedMotion, zoom) {
    this.renderSceneCache(buffer, width, height, time, reducedMotion);

    const sceneCanvas = zoom > 1.001 ? buffer.cleanSceneCanvas : buffer.sceneCanvas;
    if (zoom <= 1.001) {
      ctx.drawImage(sceneCanvas, 0, 0, width, height);
      return;
    }

    const focusX = width * 0.68;
    const focusY = height * 0.68;
    const sourceWidth = width / zoom;
    const sourceHeight = height / zoom;
    const sourceX = clamp(focusX - sourceWidth / 2, 0, width - sourceWidth);
    const sourceY = clamp(focusY - sourceHeight / 2, 0, height - sourceHeight);
    ctx.save();
    ctx.imageSmoothingEnabled = false;
    ctx.drawImage(sceneCanvas, sourceX, sourceY, sourceWidth, sourceHeight, 0, 0, width, height);
    ctx.restore();
    this.drawFrostedGlassLayer(ctx, width, height, time);
  }
}
