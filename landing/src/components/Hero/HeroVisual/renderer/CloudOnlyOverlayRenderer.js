import { COMPOSER_BOX_END, COMPOSER_BOX_START } from './timeline';
import { hash, lerp, smoothstep } from './math';
import agent01 from '../../../../assets/svg/loading-agents-01.svg';
import agent02 from '../../../../assets/svg/loading-agents-02.svg';
import agent03 from '../../../../assets/svg/loading-agents-03.svg';
import agent04 from '../../../../assets/svg/loading-agents-04.svg';
import agent05 from '../../../../assets/svg/loading-agents-05.svg';
import agent06 from '../../../../assets/svg/loading-agents-06.svg';
import agent07 from '../../../../assets/svg/loading-agents-07.svg';
import agent08 from '../../../../assets/svg/loading-agents-08.svg';

const AGENT_SVG_URLS = [agent01, agent02, agent03, agent04, agent05, agent06, agent07, agent08];

const agentRows = [
  {
    name: 'Cluster Inspector',
    summary: 'Inspecting rollout, pod restarts, and warning events.',
    accent: '#818cf8',
    bg: 'rgba(129, 140, 248, 0.1)',
    border: 'rgba(129, 140, 248, 0.26)',
  },
  {
    name: 'Incident Analyst',
    summary: 'Correlating CrashLoopBackOff with readiness probes.',
    accent: '#14b8a6',
    bg: 'rgba(20, 184, 166, 0.1)',
    border: 'rgba(20, 184, 166, 0.25)',
  },
];

const SWIPE_START = 19.55;
const SWIPE_END = 20.08;
const FINAL_AGENT_RESPONSE =
  "If you'd like, I can also handle these follow-up tasks for you: inspect the failing pods, compare the current rollout with the previous ReplicaSet, isolate the readiness regression, draft a rollback plan, and prepare the exact commands for a safe production recovery.";

export class CloudOnlyOverlayRenderer {
  constructor(timeline) {
    this.timeline = timeline;
    this.agentImages = this.createAgentImages();
  }

  createAgentImages() {
    if (typeof Image === 'undefined') return [];

    return AGENT_SVG_URLS.map((url) => {
      const image = new Image();
      image.src = url;
      return image;
    });
  }

  drawText(ctx, text, x, y, maxWidth) {
    if (maxWidth && ctx.measureText(text).width > maxWidth) {
      let clipped = text;
      while (clipped.length > 0 && ctx.measureText(`${clipped}...`).width > maxWidth) {
        clipped = clipped.slice(0, -1);
      }
      ctx.fillText(`${clipped}...`, x, y);
      return;
    }

    ctx.fillText(text, x, y);
  }

  drawRoundedRect(ctx, x, y, width, height, radius, fillStyle, strokeStyle) {
    ctx.beginPath();
    ctx.roundRect(x, y, width, height, radius);

    if (fillStyle) {
      ctx.fillStyle = fillStyle;
      ctx.fill();
    }

    if (strokeStyle) {
      ctx.strokeStyle = strokeStyle;
      ctx.stroke();
    }
  }

  drawWrappedText(ctx, text, x, y, maxWidth, lineHeight, maxLines = 2) {
    const words = text.split(' ');
    const lines = [];
    let line = '';

    words.forEach((word) => {
      const next = line ? `${line} ${word}` : word;
      if (ctx.measureText(next).width > maxWidth && line) {
        lines.push(line);
        line = word;
      } else {
        line = next;
      }
    });

    if (line) lines.push(line);

    lines.slice(0, maxLines).forEach((lineText, index) => {
      const isLast = index === maxLines - 1 && lines.length > maxLines;
      this.drawText(ctx, isLast ? `${lineText}...` : lineText, x, y + index * lineHeight, maxWidth);
    });
  }

  getGeneratedText(text, progress) {
    const visibleChars = Math.floor(text.length * smoothstep(0, 1, progress));
    return text.slice(0, visibleChars);
  }

  drawLucideIcon(ctx, icon, x, y, size, scale) {
    ctx.save();
    ctx.lineWidth = Math.max(1.2, 1.8 * scale);
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';

    const px = (value) => x + value * size;
    const py = (value) => y + value * size;

    if (icon === 'folder') {
      ctx.beginPath();
      ctx.moveTo(px(0.12), py(0.35));
      ctx.lineTo(px(0.12), py(0.78));
      ctx.lineTo(px(0.88), py(0.78));
      ctx.lineTo(px(0.88), py(0.3));
      ctx.lineTo(px(0.46), py(0.3));
      ctx.lineTo(px(0.38), py(0.2));
      ctx.lineTo(px(0.12), py(0.2));
      ctx.closePath();
      ctx.stroke();
    } else if (icon === 'git') {
      ctx.beginPath();
      ctx.moveTo(px(0.32), py(0.16));
      ctx.lineTo(px(0.32), py(0.84));
      ctx.stroke();
      ctx.beginPath();
      ctx.arc(px(0.32), py(0.22), size * 0.08, 0, Math.PI * 2);
      ctx.arc(px(0.32), py(0.78), size * 0.08, 0, Math.PI * 2);
      ctx.stroke();
      ctx.beginPath();
      ctx.moveTo(px(0.32), py(0.38));
      ctx.bezierCurveTo(px(0.56), py(0.38), px(0.66), py(0.46), px(0.66), py(0.66));
      ctx.stroke();
      ctx.beginPath();
      ctx.arc(px(0.66), py(0.68), size * 0.08, 0, Math.PI * 2);
      ctx.stroke();
    } else if (icon === 'monitor') {
      ctx.beginPath();
      ctx.roundRect(px(0.12), py(0.18), size * 0.76, size * 0.52, 2 * scale);
      ctx.stroke();
      ctx.beginPath();
      ctx.moveTo(px(0.5), py(0.7));
      ctx.lineTo(px(0.5), py(0.84));
      ctx.moveTo(px(0.34), py(0.84));
      ctx.lineTo(px(0.66), py(0.84));
      ctx.stroke();
    } else if (icon === 'plus') {
      ctx.beginPath();
      ctx.moveTo(px(0.5), py(0.22));
      ctx.lineTo(px(0.5), py(0.78));
      ctx.moveTo(px(0.22), py(0.5));
      ctx.lineTo(px(0.78), py(0.5));
      ctx.stroke();
    }

    ctx.restore();
  }

  drawAgentIcon(ctx, x, y, size, elapsed, accent) {
    const pulse = Math.sin(elapsed * 4.2) * 0.5 + 0.5;
    const frame = Math.floor(elapsed * 7) % Math.max(1, AGENT_SVG_URLS.length);
    const image = this.agentImages[frame];

    ctx.save();
    ctx.fillStyle = `rgba(${accent === '#818cf8' ? '129, 140, 248' : '20, 184, 166'}, ${0.1 + pulse * 0.05})`;
    ctx.strokeStyle = accent;
    ctx.globalAlpha = 0.9;
    ctx.lineWidth = 1;
    ctx.fillRect(x, y, size, size);
    ctx.globalAlpha = 0.28 + pulse * 0.25;
    ctx.strokeRect(x + 0.5, y + 0.5, size - 1, size - 1);
    ctx.globalAlpha = 1;

    if (image?.complete) {
      ctx.drawImage(image, x + size * 0.24, y + size * 0.24, size * 0.52, size * 0.52);
    } else {
      ctx.fillStyle = accent;
      ctx.fillRect(x + size * 0.38, y + size * 0.38, size * 0.24, size * 0.24);
    }

    ctx.restore();
  }

  drawAgentRow(ctx, row, x, y, width, height, elapsed) {
    const scale = height / 36;

    ctx.save();
    ctx.lineWidth = 1;
    this.drawRoundedRect(ctx, x, y, width, height, 8 * scale, 'rgba(255, 255, 255, 0)', row.border);

    this.drawAgentIcon(ctx, x + 10 * scale, y + 7 * scale, height - 14 * scale, elapsed, row.accent);

    ctx.textAlign = 'left';
    ctx.textBaseline = 'top';
    ctx.font = `600 ${12 * scale}px Inter, system-ui, sans-serif`;
    ctx.fillStyle = 'rgba(22, 25, 34, 0.9)';
    this.drawText(ctx, row.name, x + 42 * scale, y + 7 * scale, width - 74 * scale);

    ctx.font = `400 ${10.5 * scale}px Inter, system-ui, sans-serif`;
    ctx.fillStyle = 'rgba(64, 70, 84, 0.62)';
    this.drawText(ctx, row.summary, x + 42 * scale, y + 22 * scale, width - 74 * scale);

    ctx.fillStyle = row.accent;
    const stopX = x + width - 22 * scale;
    const stopY = y + height / 2 - 4 * scale;
    ctx.fillRect(stopX, stopY, 8 * scale, 8 * scale);
    ctx.restore();
  }

  drawInitialsAvatar(ctx, x, y, size, scale) {
    ctx.save();
    ctx.beginPath();
    ctx.arc(x + size / 2, y + size / 2, size / 2, 0, Math.PI * 2);
    ctx.fillStyle = 'rgba(60, 65, 78, 0.92)';
    ctx.fill();
    ctx.font = `800 ${9.5 * scale}px Inter, system-ui, sans-serif`;
    ctx.fillStyle = '#ffffff';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText('AT', x + size / 2, y + size / 2 + 0.5 * scale);
    ctx.restore();
  }

  drawAssistantGlyph(ctx, x, y, size, scale) {
    ctx.save();
    ctx.strokeStyle = 'rgba(80, 87, 108, 0.42)';
    ctx.lineWidth = 1;
    ctx.strokeRect(x + 4 * scale, y + 4 * scale, size - 8 * scale, size - 8 * scale);
    ctx.fillStyle = 'rgba(80, 87, 108, 0.58)';
    ctx.fillRect(x + size / 2 - 2 * scale, y + size / 2 - 2 * scale, 4 * scale, 4 * scale);
    ctx.restore();
  }

  drawChatMessageRow({ ctx, role, text, y, x, width, scale, maxLines = 2 }) {
    const avatarSize = 24 * scale;
    const contentX = x + avatarSize + 12 * scale;
    const contentWidth = width - avatarSize - 12 * scale;

    if (role === 'user') {
      this.drawInitialsAvatar(ctx, x, y, avatarSize, scale);
      ctx.font = `600 ${13.5 * scale}px ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace`;
      ctx.fillStyle = 'rgba(22, 25, 34, 0.94)';
      this.drawWrappedText(ctx, text, contentX, y + 1 * scale, contentWidth, 20 * scale, maxLines);
      return;
    }

    this.drawAssistantGlyph(ctx, x, y, avatarSize, scale);
    ctx.font = `300 ${13.5 * scale}px Inter, system-ui, sans-serif`;
    ctx.fillStyle = 'rgba(24, 28, 38, 0.82)';
    this.drawWrappedText(ctx, text, contentX, y, contentWidth, 20 * scale, maxLines);
  }

  drawChatPreview(ctx, x, y, width, height, elapsed, scale) {
    const isCompact = width < 520;
    const uiScale = Math.min(scale, width / (isCompact ? 430 : 680), height / (isCompact ? 320 : 280));
    const paddingX = (isCompact ? 16 : 24) * uiScale;
    const paddingY = (isCompact ? 16 : 20) * uiScale;
    const rowWidth = width - paddingX * 2;
    const left = x + paddingX;
    const top = y + paddingY;

    ctx.save();
    ctx.beginPath();
    ctx.rect(x, y, width, height);
    ctx.clip();

    ctx.textAlign = 'left';
    ctx.textBaseline = 'top';
    ctx.font = `${(isCompact ? 10 : 11) * uiScale}px ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace`;
    ctx.fillStyle = 'rgba(64, 70, 84, 0.5)';
    ctx.fillText('production / checkout-api', left + 36 * uiScale, top);

    this.drawChatMessageRow({
      ctx,
      role: 'user',
      text: 'Can you triage checkout-api CrashLoopBackOff without changing the cluster?',
      x: left,
      y: top + (isCompact ? 24 : 28) * uiScale,
      width: rowWidth,
      scale: uiScale,
      maxLines: 2,
    });

    this.drawChatMessageRow({
      ctx,
      role: 'assistant',
      text: 'I will inspect rollout state, recent warning events, and restart reasons first. No mutating commands yet.',
      x: left,
      y: top + (isCompact ? 76 : 88) * uiScale,
      width: rowWidth,
      scale: uiScale,
      maxLines: 2,
    });

    const rowsTop = top + (isCompact ? 128 : 146) * uiScale;
    const rowHeight = (isCompact ? 42 : 46) * uiScale;
    agentRows.forEach((row, index) => {
      this.drawAgentRow(ctx, row, left + 36 * uiScale, rowsTop + index * (rowHeight + 8 * uiScale), rowWidth - 36 * uiScale, rowHeight, elapsed + index * 0.65);
    });

    const statusY = rowsTop + agentRows.length * (rowHeight + 8 * uiScale) + 2 * uiScale;
    ctx.font = `400 ${12 * uiScale}px Inter, system-ui, sans-serif`;
    ctx.fillStyle = 'rgba(64, 70, 84, 0.68)';
    this.drawWrappedText(
      ctx,
      'The agents found a likely readiness regression. I can continue with a safe rollout comparison next.',
      left + 36 * uiScale,
      statusY,
      rowWidth - 36 * uiScale,
      17 * uiScale,
      2,
    );

    ctx.restore();
  }

  drawFinalComposerPreview(ctx, x, y, width, height, elapsed, scale) {
    const isCompact = width < 520;
    const uiScale = Math.min(scale, width / (isCompact ? 430 : 680), height / (isCompact ? 320 : 280));
    const paddingX = (isCompact ? 16 : 24) * uiScale;
    const paddingY = (isCompact ? 18 : 22) * uiScale;
    const left = x + paddingX;
    const top = y + paddingY;
    const innerWidth = width - paddingX * 2;
    const responseProgress = smoothstep(SWIPE_END + 0.12, SWIPE_END + 1.24, elapsed);
    const typeProgress = smoothstep(SWIPE_END + 1.38, COMPOSER_BOX_END - 0.68, elapsed);
    const typed = 'continue'.slice(0, Math.floor('continue'.length * typeProgress));
    const composerHeight = (isCompact ? 102 : 90) * uiScale;
    const composerY = y + height - composerHeight - 8 * uiScale;

    ctx.save();
    ctx.beginPath();
    ctx.rect(x, y, width, height);
    ctx.clip();

    ctx.textAlign = 'left';
    ctx.textBaseline = 'top';
    ctx.font = `${(isCompact ? 10 : 12) * uiScale}px ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace`;
    ctx.fillStyle = 'rgba(64, 70, 84, 0.5)';
    ctx.fillText('agent conversation', left + 36 * uiScale, top);

    this.drawChatMessageRow({
      ctx,
      role: 'assistant',
      text: this.getGeneratedText(FINAL_AGENT_RESPONSE, responseProgress),
      x: left,
      y: top + (isCompact ? 30 : 34) * uiScale,
      width: innerWidth,
      scale: uiScale,
      maxLines: Math.max(5, Math.floor((composerY - top - 58 * uiScale) / (20 * uiScale))),
    });

    ctx.strokeStyle = 'rgba(18, 20, 28, 0.08)';
    ctx.beginPath();
    ctx.moveTo(x, composerY - 10 * uiScale);
    ctx.lineTo(x + width, composerY - 10 * uiScale);
    ctx.stroke();

    this.drawComposerBar(ctx, x, composerY, width, composerHeight, typed, typeProgress, elapsed, uiScale, isCompact);

    ctx.restore();
  }

  drawToolbarChip(ctx, x, y, width, height, label, scale, active = false, icon = null) {
    this.drawRoundedRect(
      ctx,
      x,
      y,
      width,
      height,
      8 * scale,
      active ? 'rgba(41, 166, 216, 0.12)' : '#f0f2f5',
      'rgba(18, 20, 28, 0.08)',
    );
    ctx.font = `${10 * scale}px Inter, system-ui, sans-serif`;
    ctx.fillStyle = active ? '#1283b2' : 'rgba(45, 51, 70, 0.68)';
    ctx.textAlign = 'left';
    ctx.textBaseline = 'top';

    if (icon) {
      ctx.save();
      ctx.strokeStyle = active ? '#1283b2' : 'rgba(45, 51, 70, 0.68)';
      ctx.fillStyle = ctx.strokeStyle;
      this.drawLucideIcon(ctx, icon, x + 7 * scale, y + 6 * scale, 12 * scale, scale);
      ctx.restore();
    }

    if (label) {
      ctx.fillText(label, x + (icon ? 23 : 7) * scale, y + 6 * scale);
    }
  }

  drawComposerBar(ctx, x, y, width, height, typed, typeProgress, elapsed, scale, isCompact = false) {
    const inputHeight = 48 * scale;
    const actionsY = y + inputHeight + 8 * scale;
    const editorMargin = 8 * scale;
    const inputX = x + editorMargin;
    const inputWidth = width - editorMargin * 2;

    ctx.save();
    ctx.fillStyle = '#ffffff';
    ctx.fillRect(x, y, width, height);

    ctx.fillStyle = '#fbfcfd';
    ctx.fillRect(inputX, y, inputWidth, inputHeight);

    ctx.font = `${12 * scale}px ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace`;
    ctx.fillStyle = typed ? 'rgba(18, 20, 28, 0.92)' : 'rgba(80, 87, 108, 0.42)';
    ctx.textAlign = 'left';
    ctx.textBaseline = 'middle';
    const textX = inputX + 12 * scale;
    const textY = y + inputHeight / 2;
    ctx.fillText(typed || 'Octomus anything, or use / for tools', textX, textY);

    if (typed && (typeProgress < 1 || Math.floor(elapsed * 5) % 2 === 0)) {
      const cursorX = textX + ctx.measureText(typed).width + 2 * scale;
      const cursorHeight = 14 * scale;
      const cursorWidth = Math.max(1, 1.4 * scale);
      ctx.fillRect(cursorX, textY - cursorHeight / 2, cursorWidth, cursorHeight);
    }

    const leftX = x + 8 * scale;
    const cwdWidth = (isCompact ? 82 : 104) * scale;
    const branchWidth = (isCompact ? 54 : 62) * scale;
    this.drawToolbarChip(ctx, leftX, actionsY, cwdWidth, 24 * scale, isCompact ? '~/' : '~/checkout', scale, false, 'folder');
    this.drawToolbarChip(ctx, leftX + cwdWidth + 6 * scale, actionsY, branchWidth, 24 * scale, 'main', scale, false, 'git');
    this.drawToolbarChip(ctx, leftX + cwdWidth + branchWidth + 12 * scale, actionsY, 31 * scale, 24 * scale, 'A*', scale, true);

    const attachW = 24 * scale;
    const modelW = (isCompact ? 58 : 74) * scale;
    const rightEdge = x + width - 8 * scale;
    this.drawToolbarChip(ctx, rightEdge - modelW - attachW - 6 * scale, actionsY, modelW, 24 * scale, isCompact ? 'GPT' : 'GPT-5.2', scale);
    this.drawToolbarChip(ctx, rightEdge - attachW, actionsY, attachW, 24 * scale, '', scale, false, 'plus');
    ctx.restore();
  }

  drawPanelShell(ctx, x, y, width, height, alpha, drawContent) {
    if (alpha <= 0) return;

    ctx.save();
    ctx.globalAlpha *= alpha;
    ctx.lineWidth = 1.2;
    this.drawRoundedRect(ctx, x, y, width, height, 14, 'rgba(255, 255, 255, 0.98)', 'rgba(255, 255, 255, 0.95)');
    drawContent();
    ctx.restore();
  }

  drawMotionTrail(ctx, x, y, width, height, direction, strength) {
    if (strength <= 0) return;

    ctx.save();
    ctx.globalAlpha = strength * 0.16;
    ctx.fillStyle = 'rgba(255, 255, 255, 0.72)';
    for (let index = 1; index <= 4; index += 1) {
      ctx.fillRect(x, y - direction * index * 11, width, height);
      ctx.globalAlpha *= 0.6;
    }
    ctx.restore();
  }

  drawComposerBox(ctx, width, height, elapsed) {
    const appear = smoothstep(COMPOSER_BOX_START, COMPOSER_BOX_START + 0.45, elapsed);
    const exit = smoothstep(COMPOSER_BOX_END - 0.55, COMPOSER_BOX_END, elapsed);
    const alpha = appear * (1 - exit);

    if (alpha <= 0) return;

    const isNarrow = width < 640;
    const boxWidth = Math.min(width * (isNarrow ? 0.92 : 0.72), isNarrow ? 430 : 720);
    const boxHeight = Math.min(height * (isNarrow ? 0.56 : 0.62), isNarrow ? 320 : 340);
    const scale = lerp(0.96, 1, appear) * lerp(1, 0.96, exit);
    const x = width / 2 - (boxWidth * scale) / 2;
    const y = height / 2 - (boxHeight * scale) / 2;
    const w = boxWidth * scale;
    const h = boxHeight * scale;
    const swipe = smoothstep(SWIPE_START, SWIPE_END, elapsed);
    const motion = Math.sin(swipe * Math.PI);
    const panelGap = 94 * scale;
    const firstY = y - swipe * (h + panelGap);
    const secondY = y + (1 - swipe) * (h + panelGap);

    ctx.save();
    ctx.globalAlpha = alpha;
    this.drawMotionTrail(ctx, x, firstY, w, h, -1, motion);
    this.drawMotionTrail(ctx, x, secondY, w, h, 1, motion);
    this.drawPanelShell(ctx, x, firstY, w, h, 1 - smoothstep(0.68, 1, swipe), () => {
      this.drawChatPreview(ctx, x, firstY, w, h, elapsed, scale);
    });
    this.drawPanelShell(ctx, x, secondY, w, h, smoothstep(0, 0.72, swipe), () => {
      this.drawFinalComposerPreview(ctx, x, secondY, w, h, elapsed, scale);
    });
    ctx.restore();
  }

  drawCenteredText(ctx, width, height, lines, alpha) {
    if (alpha <= 0) return;

    const fontSize = Math.round(Math.max(22, Math.min(40, width * 0.029)));
    const lineHeight = Math.round(fontSize * 1.35);
    const startY = Math.round(height * 0.5 - ((lines.length - 1) * lineHeight) / 2);

    ctx.save();
    ctx.globalAlpha = alpha;
    ctx.globalCompositeOperation = 'source-over';
    ctx.filter = 'none';
    ctx.shadowColor = 'transparent';
    ctx.shadowBlur = 0;
    ctx.font = `400 ${fontSize}px Inter, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif`;
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillStyle = '#ffffff';
    lines.forEach((line, index) => {
      ctx.fillText(line, Math.round(width / 2), startY + index * lineHeight);
    });
    ctx.restore();
  }

  drawAsciiCodeField(ctx, width, height, elapsed, alpha) {
    if (alpha <= 0) return;

    const cell = Math.max(7, Math.min(10, width / 170));
    const lineHeight = cell * 2.35;
    const cols = Math.ceil(width / cell);
    const rows = Math.ceil(height / lineHeight);
    const phase = elapsed * 1.4;
    const codeBands = [
      { x: 0.02, y: 0.03, w: 0.28, h: 0.28, seed: 1.2 },
      { x: 0.45, y: 0.02, w: 0.47, h: 0.25, seed: 3.7 },
      { x: 0.73, y: 0.42, w: 0.27, h: 0.22, seed: 5.4 },
      { x: 0.0, y: 0.78, w: 0.4, h: 0.2, seed: 7.1 },
      { x: 0.53, y: 0.66, w: 0.4, h: 0.28, seed: 9.6 },
      { x: 0.23, y: 0.61, w: 0.13, h: 0.17, seed: 11.3 },
    ];

    ctx.save();
    ctx.globalAlpha = alpha;
    ctx.font = `${cell}px ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace`;
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillStyle = 'rgba(255, 255, 255, 0.82)';
    ctx.shadowBlur = 0;

    codeBands.forEach((band) => {
      const startCol = Math.floor((band.x * width) / cell);
      const endCol = Math.min(cols, Math.ceil(((band.x + band.w) * width) / cell));
      const startRow = Math.floor((band.y * height) / lineHeight);
      const endRow = Math.min(rows, Math.ceil(((band.y + band.h) * height) / lineHeight));
      const rowOffset = Math.floor(phase + band.seed) % 4;

      for (let row = startRow; row < endRow; row += 1) {
        if ((row + rowOffset) % 3 === 1) continue;

        const rowNoise = hash(row, band.seed);
        const rowStart = startCol + Math.floor(rowNoise * 12);
        const rowEnd = endCol - Math.floor(hash(row + 17, band.seed) * 14);

        for (let col = rowStart; col < rowEnd; col += 1) {
          const gate = hash(col + Math.floor(phase * 2), row + band.seed);
          const pulse = Math.sin(phase + col * 0.17 + row * 0.41 + band.seed) * 0.5 + 0.5;

          if (gate < 0.18 || pulse < 0.18) continue;

          ctx.globalAlpha = alpha * (0.34 + pulse * 0.58);
          ctx.fillText('.', col * cell + cell / 2, row * lineHeight + lineHeight / 2);
        }
      }
    });

    ctx.restore();
  }

  draw(ctx, width, height, elapsed) {
    this.drawComposerBox(ctx, width, height, elapsed);

    const runningAlpha = this.timeline.getRunningTextAlpha(elapsed);
    this.drawAsciiCodeField(ctx, width, height, elapsed, runningAlpha);
    this.drawCenteredText(ctx, width, height, ['Agents running...'], runningAlpha);

    const finishedAlpha = this.timeline.getFinishedTextAlpha(elapsed);
    this.drawCenteredText(ctx, width, height, ['Task finished', 'Your code is ready for review'], finishedAlpha);
  }
}
