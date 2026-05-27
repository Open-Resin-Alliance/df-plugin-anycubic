import anycubicPrinters from './printers/printers.json';
import type { PrinterPreset } from '../../src/features/profiles/profileStore';

function sanitizePositiveNumber(value: unknown): number | null {
  const n = Number(value);
  if (!Number.isFinite(n) || n <= 0) return null;
  return n;
}

function sanitizeProfileVersion(value: unknown): number | undefined {
  const n = sanitizePositiveNumber(value);
  if (n == null) return undefined;
  return Math.max(1, Math.round(n));
}

export const ANYCUBIC_PLUGIN_MANIFEST = {
  schemaVersion: 1,
  id: 'anycubic-builtin',
  name: 'Anycubic Plugin',
  version: '0.1.0',
  description: 'Anycubic printer profile pack (AFF and AZF format support).',
  printerPresets: (anycubicPrinters as any[]).map((preset) => {
    const resolutionX = Number(preset.display?.resolutionX) || 3840;
    const resolutionY = Number(preset.display?.resolutionY) || 2400;

    return {
      presetId: String(preset.presetId),
      profileVersion: sanitizeProfileVersion(preset.profileVersion),
      manufacturer: String(preset.manufacturer),
      name: String(preset.name),
      family: typeof preset.family === 'string' && preset.family.trim().length > 0
        ? preset.family.trim()
        : undefined,
      keySuffix: typeof preset.keySuffix === 'string' && preset.keySuffix.trim().length > 0
        ? preset.keySuffix.trim().toLowerCase()
        : undefined,
      pixelSize: preset.pixelSize,
      buildVolumeMm: {
        width: Number(preset.buildVolumeMm?.width) || 192,
        depth: Number(preset.buildVolumeMm?.depth) || 120,
        height: Number(preset.buildVolumeMm?.height) || 200,
      },
      display: {
        resolutionX,
        resolutionY,
        outputFormat: String(preset.display?.outputFormat),
      },
    };
  }) as PrinterPreset[],
  materialTemplates: [],
};
