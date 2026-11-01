import { AGENTS_START } from './timeline';
import { clusters, INSTALL_COMMAND, longLinks, strayNodes } from './sceneData';
import { lerp, smoothstep } from './math';

export class HudRenderer {
  getFrameDrift(cluster, time) {
    return {
      x: Math.sin(time * 0.42 + cluster.frame.x * 18) * 0.023,
      y: Math.cos(time * 0.36 + cluster.frame.y * 22) * 0.024,
    };
  }

  getNodePosition(cluster, node, time) {
    const drift = this.getFrameDrift(cluster, time);

    if (node.pinned) {
      return { x: node.x, y: node.y };
    }

    const pivot = cluster.nodes[cluster.pivot];
    const orbitPhase = time * (0.62 + node.seed * 0.015) + node.seed;
    const localX = (node.x - pivot.x) * 0.06 * Math.sin(orbitPhase);
    const localY = (node.y - pivot.y) * 0.06 * Math.cos(orbitPhase * 0.9);

    return {
      x: node.x + drift.x + localX + Math.sin(time * 0.72 + node.seed) * 0.014,
      y: node.y + drift.y + localY + Math.cos(time * 0.64 + node.seed * 1.7) * 0.017,
    };
  }

  drawLabel(ctx, text, x, y, scale, alpha) {
    const paddingX = 8 * scale;
    const height = 22 * scale;

    ctx.save();
    ctx.globalAlpha = alpha;
    ctx.font = `${12 * scale}px ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace`;
    ctx.textBaseline = 'middle';
    const width = ctx.measureText(text).width + paddingX * 2;

    ctx.fillStyle = 'rgba(255, 255, 255, 0.93)';
    ctx.fillRect(x, y, width, height);
    ctx.fillStyle = 'rgba(26, 30, 44, 0.88)';
    ctx.fillText(text, x + paddingX, y + height / 2 + scale);
    ctx.restore();
  }

  drawInstallCommand(ctx, width, height, elapsed, time) {
    const appear = smoothstep(2.15, 2.75, elapsed);
    const exit = smoothstep(5.05, 5.55, elapsed);
    const alpha = appear * (1 - exit);

    if (alpha <= 0) return;

    const typeProgress = smoothstep(2.85, 4.35, elapsed);
    const visibleChars = Math.floor(INSTALL_COMMAND.length * typeProgress);
    const visibleText = INSTALL_COMMAND.slice(0, visibleChars);
    const showCursor = elapsed < 4.8 && Math.floor(elapsed * 5) % 2 === 0;
    const scaleIn = lerp(0.18, 1, appear);
    const scale = lerp(scaleIn, 0.9, exit);
    const maxBoxWidth = Math.min(width * 0.86, 460);
    const horizontalPadding = Math.max(12, Math.min(18, width * 0.035));
    const measuredFontSize = Math.max(11, Math.min(18, width * 0.014));
    ctx.save();
    ctx.font = `${measuredFontSize}px ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace`;
    const commandWidth = ctx.measureText(INSTALL_COMMAND).width;
    ctx.restore();
    const fontSize = Math.max(10.5, Math.min(measuredFontSize, (maxBoxWidth - horizontalPadding * 2) / commandWidth * measuredFontSize));
    const boxWidth = Math.min(maxBoxWidth, Math.max(250, commandWidth * (fontSize / measuredFontSize) + horizontalPadding * 2));
    const boxHeight = fontSize * 2.7;
    const x = width * 0.5 - (boxWidth * scale) / 2;
    const y = height * 0.49 - (boxHeight * scale) / 2;
    const w = boxWidth * scale;
    const h = boxHeight * scale;

    ctx.save();
    ctx.globalAlpha = alpha;
    ctx.fillStyle = 'rgba(255, 255, 255, 0.92)';
    ctx.fillRect(x, y, w, h);
    ctx.strokeStyle = 'rgba(255, 255, 255, 0.82)';
    ctx.lineWidth = Math.max(1.2, width / 1100);
    ctx.strokeRect(x, y, w, h);
    ctx.restore();

    ctx.save();
    ctx.globalAlpha = alpha * smoothstep(2.45, 2.85, elapsed);
    ctx.font = `${fontSize * scale}px ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace`;
    ctx.textBaseline = 'middle';
    ctx.fillStyle = 'rgba(12, 14, 20, 0.92)';
    const textX = x + horizontalPadding * scale;
    const textY = y + h / 2 + Math.sin(time * 2.1) * 0.4;
    ctx.fillText(visibleText, textX, textY);

    if (showCursor) {
      const cursorX = textX + ctx.measureText(visibleText).width + 2 * scale;
      const cursorSize = fontSize * scale;
      ctx.fillRect(cursorX, textY - cursorSize / 2, cursorSize * 0.62, cursorSize);
    }

    ctx.restore();
  }

  drawFrame(ctx, frame, width, height, reveal, drift, label, labelOffset, time, elapsed) {
    if (reveal <= 0) return;

    const x = (frame.x + drift.x) * width;
    const y = (frame.y + drift.y) * height;
    const w = frame.w * width;
    const h = frame.h * height;
    const alpha = smoothstep(0.04, 1, reveal);
    const internalShrink = smoothstep(13.25, AGENTS_START, elapsed);
    const accentScale = lerp(lerp(0.82, 1, alpha), 0.62, internalShrink);
    const hudScale = lerp(1.12, 1, alpha);
    const centerX = x + w / 2;
    const centerY = y + h / 2;
    const accentW = w * accentScale;
    const accentH = h * accentScale;
    const accentX = centerX - accentW / 2;
    const accentY = centerY - accentH / 2;
    const frameW = w * hudScale;
    const frameH = h * hudScale;
    const frameX = centerX - frameW / 2;
    const frameY = centerY - frameH / 2;
    const corner = Math.min(frameW, frameH) * 0.15;

    // Experiment here with frame treatments: color-burn, overlay, or clipped blur.
    ctx.save();
    ctx.globalAlpha = alpha * 0.46;
    ctx.globalCompositeOperation = 'overlay';
    const colorLift = ctx.createLinearGradient(accentX, accentY, accentX + accentW, accentY + accentH);
    colorLift.addColorStop(0, 'rgba(255, 118, 34, 0.38)');
    colorLift.addColorStop(0.45, 'rgba(255, 54, 174, 0.42)');
    colorLift.addColorStop(1, 'rgba(150, 255, 226, 0.22)');
    ctx.fillStyle = colorLift;
    ctx.fillRect(accentX, accentY, accentW, accentH);

    ctx.globalCompositeOperation = 'screen';
    ctx.globalAlpha = alpha * 0.24;
    const warmTint = ctx.createRadialGradient(centerX, centerY, 0, centerX, centerY, Math.max(accentW, accentH) * 0.72);
    warmTint.addColorStop(0, 'rgba(255, 124, 36, 0.34)');
    warmTint.addColorStop(0.55, 'rgba(230, 42, 150, 0.16)');
    warmTint.addColorStop(1, 'rgba(255, 255, 255, 0)');
    ctx.fillStyle = warmTint;
    ctx.fillRect(accentX, accentY, accentW, accentH);
    ctx.restore();

    ctx.save();
    ctx.globalAlpha = alpha;
    ctx.strokeStyle = 'rgba(255, 255, 255, 0.78)';
    ctx.lineWidth = Math.max(1.2, width / 1000);
    ctx.shadowColor = 'rgba(255, 255, 255, 0.48)';
    ctx.shadowBlur = 0;

    const drawCorner = (sx, sy, mx, my) => {
      ctx.beginPath();
      ctx.moveTo(sx, sy + my * corner);
      ctx.lineTo(sx, sy);
      ctx.lineTo(sx + mx * corner, sy);
      ctx.stroke();
    };

    drawCorner(frameX, frameY, 1, 1);
    drawCorner(frameX + frameW, frameY, -1, 1);
    drawCorner(frameX, frameY + frameH, 1, -1);
    drawCorner(frameX + frameW, frameY + frameH, -1, -1);

    ctx.globalAlpha = alpha * 0.16;
    ctx.strokeStyle = 'rgba(255, 255, 255, 0.35)';
    ctx.strokeRect(frameX, frameY, frameW, frameH);
    ctx.restore();

    const labelScale = Math.max(0.8, Math.min(1, width / 1160)) * lerp(1.08, 1, alpha);
    this.drawLabel(
      ctx,
      label,
      frameX + labelOffset[0],
      frameY + labelOffset[1] + Math.sin(time * 0.7) * 1.5,
      labelScale,
      alpha,
    );
  }

  drawNode(ctx, point, radius, reveal, filled = false) {
    if (reveal <= 0) return;

    ctx.save();
    ctx.globalAlpha = smoothstep(0, 1, reveal);
    ctx.shadowColor = 'rgba(255, 255, 255, 0.75)';
    ctx.shadowBlur = 0;

    ctx.beginPath();
    ctx.arc(point.x, point.y, radius + 4, 0, Math.PI * 2);
    ctx.fillStyle = 'rgba(255, 255, 255, 0.2)';
    ctx.fill();

    ctx.beginPath();
    ctx.arc(point.x, point.y, radius, 0, Math.PI * 2);
    ctx.fillStyle = filled ? 'rgba(255, 255, 255, 0.96)' : 'rgba(255, 255, 255, 0.5)';
    ctx.fill();

    if (!filled) {
      ctx.shadowBlur = 0;
      ctx.lineWidth = 2.2;
      ctx.strokeStyle = 'rgba(255, 255, 255, 0.95)';
      ctx.stroke();
      ctx.beginPath();
      ctx.arc(point.x, point.y, Math.max(2.4, radius - 4.2), 0, Math.PI * 2);
      ctx.fillStyle = 'rgba(255, 255, 255, 0.9)';
      ctx.fill();
    }

    ctx.restore();
  }

  drawStraightLink(ctx, start, end, progress) {
    if (progress <= 0) return;

    ctx.beginPath();
    ctx.moveTo(start.x, start.y);
    ctx.lineTo(lerp(start.x, end.x, progress), lerp(start.y, end.y, progress));
    ctx.stroke();
  }

  getNodeOrder(cluster, nodeIndex) {
    if (nodeIndex === cluster.pivot) return 0;

    let order = 1;
    for (let index = 0; index < nodeIndex; index += 1) {
      if (index !== cluster.pivot) order += 1;
    }

    return order;
  }

  getNodeReveal(cluster, clusterIndex, nodeIndex, elapsed) {
    const order = this.getNodeOrder(cluster, nodeIndex);
    const delay = 1.05 + clusterIndex * 0.72 + order * 0.42;
    return smoothstep(delay, delay + 0.95, elapsed);
  }

  getLocalLineReveal(cluster, clusterIndex, nodeIndex, elapsed) {
    const order = this.getNodeOrder(cluster, nodeIndex);
    const delay = 5.75 + clusterIndex * 0.36 + order * 0.18;
    return smoothstep(delay, delay + 0.58, elapsed);
  }

  getClusterPoints(width, height, time) {
    return clusters.map((cluster) =>
      cluster.nodes.map((node) => {
        const p = this.getNodePosition(cluster, node, time);
        return {
          x: p.x * width,
          y: p.y * height,
          r: Math.max(4.5, node.r * Math.min(width / 1280, 1.12)),
          filled: node.filled,
          seed: node.seed,
        };
      }),
    );
  }

  draw(ctx, width, height, time, elapsed, isInitialCycle) {
    const pointsByCluster = this.getClusterPoints(width, height, time);

    ctx.save();
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';
    ctx.setLineDash([7, 8]);
    ctx.lineDashOffset = -time * 9;
    ctx.strokeStyle = 'rgba(255, 255, 255, 0.58)';
    ctx.lineWidth = Math.max(1.1, width / 1320);
    ctx.shadowBlur = 0;

    clusters.forEach((cluster, clusterIndex) => {
      const pivot = pointsByCluster[clusterIndex][cluster.pivot];
      cluster.nodes.forEach((node, nodeIndex) => {
        if (nodeIndex === cluster.pivot) return;
        const localProgress = this.getLocalLineReveal(cluster, clusterIndex, nodeIndex, elapsed);
        if (localProgress <= 0) return;

        const target = pointsByCluster[clusterIndex][nodeIndex];
        ctx.globalAlpha = localProgress * 0.86;
        this.drawStraightLink(ctx, pivot, target, localProgress);
      });
    });

    longLinks.forEach((link, linkIndex) => {
      const start = pointsByCluster[link.from[0]][link.from[1]];
      const end = pointsByCluster[link.to[0]][link.to[1]];
      const delay = 6.8 + linkIndex * 0.22 + link.delay;
      const progress = smoothstep(delay, delay + 0.82, elapsed);
      if (progress <= 0) return;

      ctx.globalAlpha = progress * 0.62;
      this.drawStraightLink(ctx, start, end, progress);
    });

    ctx.restore();

    clusters.forEach((cluster) => {
      const drift = this.getFrameDrift(cluster, time);
      const frameReveal = smoothstep(9.35, 10.65, elapsed);
      this.drawFrame(ctx, cluster.frame, width, height, frameReveal, drift, cluster.label, cluster.labelOffset, time, elapsed);
    });

    pointsByCluster.forEach((clusterPoints, clusterIndex) => {
      clusterPoints.forEach((point, nodeIndex) => {
        const reveal = isInitialCycle ? 1 : this.getNodeReveal(clusters[clusterIndex], clusterIndex, nodeIndex, elapsed);
        this.drawNode(ctx, point, point.r, reveal, point.filled);
      });
    });

    strayNodes.forEach((node, index) => {
      const reveal = isInitialCycle ? 0.65 : smoothstep(2.4 + index * 0.48, 3.35 + index * 0.48, elapsed) * 0.65;
      if (reveal <= 0) return;

      const p = {
        x: (node.x + Math.sin(time * 0.18 + node.seed) * 0.005) * width,
        y: (node.y + Math.cos(time * 0.16 + node.seed) * 0.006) * height,
      };
      this.drawNode(ctx, p, node.r * Math.min(width / 1280, 1), reveal);
    });

    this.drawInstallCommand(ctx, width, height, elapsed, time);
  }
}
