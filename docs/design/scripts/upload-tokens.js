// Upload Goble design tokens from docs/design/goble-tokens.json into Penpot.
// Run this code via the Penpot MCP execute_code tool when a Penpot file is connected.

function toTokenName(parts) {
  return parts.join('.');
}

function ensureSet(name) {
  const catalog = penpot.library.local.tokens;
  let set = catalog.sets.find((s) => s.name === name);
  if (!set) {
    set = catalog.addSet({ name });
  }
  if (!set.active) {
    set.toggleActive();
  }
  // Remove existing tokens with the same names to avoid duplicates on re-run.
  // This is a simple replacement strategy; adjust if you need to merge.
  return set;
}

const set = ensureSet('goble-tokens');

const tokens = [
  // Dark theme
  { type: 'color', name: 'theme.dark.bg', value: '#0E0E0E' },
  { type: 'color', name: 'theme.dark.surface', value: '#161616' },
  { type: 'color', name: 'theme.dark.surface-raised', value: '#1E1E1E' },
  { type: 'color', name: 'theme.dark.border', value: '#262626' },
  { type: 'color', name: 'theme.dark.text', value: '#E8E8E8' },
  { type: 'color', name: 'theme.dark.muted', value: '#8E8E8E' },
  { type: 'color', name: 'theme.dark.accent', value: '#9A9A9A' },
  { type: 'color', name: 'theme.dark.hover', value: '#1E1E1E' },
  { type: 'color', name: 'theme.dark.selected', value: '#2A2A2A' },

  // Light theme
  { type: 'color', name: 'theme.light.bg', value: '#F5F5F5' },
  { type: 'color', name: 'theme.light.surface', value: '#FFFFFF' },
  { type: 'color', name: 'theme.light.surface-raised', value: '#F0F0F0' },
  { type: 'color', name: 'theme.light.border', value: '#E0E0E0' },
  { type: 'color', name: 'theme.light.text', value: '#1F1F1F' },
  { type: 'color', name: 'theme.light.muted', value: '#6F6F6F' },
  { type: 'color', name: 'theme.light.accent', value: '#9A9A9A' },
  { type: 'color', name: 'theme.light.hover', value: '#F0F0F0' },
  { type: 'color', name: 'theme.light.selected', value: '#E2E2E2' },

  // Midnight theme
  { type: 'color', name: 'theme.midnight.bg', value: '#0A0A0A' },
  { type: 'color', name: 'theme.midnight.surface', value: '#121212' },
  { type: 'color', name: 'theme.midnight.surface-raised', value: '#191919' },
  { type: 'color', name: 'theme.midnight.border', value: '#212121' },
  { type: 'color', name: 'theme.midnight.text', value: '#EAEAEA' },
  { type: 'color', name: 'theme.midnight.muted', value: '#6F6F6F' },
  { type: 'color', name: 'theme.midnight.accent', value: '#9A9A9A' },
  { type: 'color', name: 'theme.midnight.hover', value: '#191919' },
  { type: 'color', name: 'theme.midnight.selected', value: '#222222' },

  // Accents
  { type: 'color', name: 'accent.blue', value: '#9A9A9A' },
  { type: 'color', name: 'accent.green', value: '#10B981' },
  { type: 'color', name: 'accent.purple', value: '#8B5CF6' },
  { type: 'color', name: 'accent.orange', value: '#F97316' },

  // Status
  { type: 'color', name: 'status.success', value: '#10B981' },
  { type: 'color', name: 'status.warning', value: '#F59E0B' },
  { type: 'color', name: 'status.error', value: '#EF4444' },
  { type: 'color', name: 'status.badge', value: '#E01E5A' },

  // Spacing
  { type: 'spacing', name: 'spacing.xs', value: '4px' },
  { type: 'spacing', name: 'spacing.sm', value: '8px' },
  { type: 'spacing', name: 'spacing.md', value: '12px' },
  { type: 'spacing', name: 'spacing.lg', value: '16px' },
  { type: 'spacing', name: 'spacing.xl', value: '24px' },

  // Radius
  { type: 'borderRadius', name: 'radius.sharp', value: '0px' },
  { type: 'borderRadius', name: 'radius.default', value: '8px' },
  { type: 'borderRadius', name: 'radius.rounded', value: '14px' },

  // Density
  { type: 'number', name: 'density.compact', value: '0.85' },
  { type: 'number', name: 'density.default', value: '1' },
  { type: 'number', name: 'density.spacious', value: '1.15' },

  // Typography
  { type: 'fontFamilies', name: 'fontFamily.system', value: "Roboto" },
  { type: 'fontFamilies', name: 'fontFamily.mono', value: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace" },
  { type: 'fontFamilies', name: 'fontFamily.serif', value: "Georgia, Cambria, 'Times New Roman', Times, serif" },

  { type: 'fontSizes', name: 'fontSize.xs', value: '11px' },
  { type: 'fontSizes', name: 'fontSize.sm', value: '12px' },
  { type: 'fontSizes', name: 'fontSize.base', value: '14px' },
  { type: 'fontSizes', name: 'fontSize.md', value: '15px' },
  { type: 'fontSizes', name: 'fontSize.lg', value: '18px' },
  { type: 'fontSizes', name: 'fontSize.xl', value: '24px' },

  { type: 'fontWeights', name: 'fontWeight.regular', value: '400' },
  { type: 'fontWeights', name: 'fontWeight.medium', value: '500' },
  { type: 'fontWeights', name: 'fontWeight.semibold', value: '600' },
  { type: 'fontWeights', name: 'fontWeight.bold', value: '700' },

  // Shadows
  { type: 'shadow', name: 'shadow.sm', value: '0 1px 2px rgba(0,0,0,0.15)' },
  { type: 'shadow', name: 'shadow.md', value: '0 4px 12px rgba(0,0,0,0.25)' },
];

// Simplest replacement: remove all existing tokens first.
for (const t of [...set.tokens]) {
  t.remove();
}

const created = [];
for (const token of tokens) {
  try {
    const t = set.addToken(token);
    created.push({ name: t.name, type: t.type });
  } catch (e) {
    created.push({ name: token.name, type: token.type, error: String(e) });
  }
}

return {
  setName: set.name,
  active: set.active,
  count: created.length,
  tokens: created,
};
