import type {
  ComplexPluginDefinition,
  PluginLocalMaterialSettingsAdapterContract,
} from '@/features/plugins/complexPluginContracts';
import { ANYCUBIC_PLUGIN_MANIFEST } from './pluginManifest';
import { ANYCUBIC_AFF_FORMAT_DEFINITION } from './slicing/affFormatDefinition';
import { ANYCUBIC_AZF_FORMAT_DEFINITION } from './slicing/azfFormatDefinition';
import anycubicSimpleMaterialSettings from './materialSettings/settings_simple.json';
import anycubicTwostageMaterialSettings from './materialSettings/settings_twostage.json';

// ─── Local material settings adapters (per mode) ─────────────────────

type AnycubicModeSettingsSchema = Omit<PluginLocalMaterialSettingsAdapterContract, 'outputFormat'>;

const ANYCUBIC_SIMPLE_SETTINGS_BASE = anycubicSimpleMaterialSettings as AnycubicModeSettingsSchema;

const ANYCUBIC_TWOSTAGE_SETTINGS_BASE = anycubicTwostageMaterialSettings as AnycubicModeSettingsSchema;

// ─── Per-format adapters (keyed by mode) ─────────────────────────────

function withoutField(
  base: AnycubicModeSettingsSchema,
  fieldKey: string,
) {
  return { ...base, fields: base.fields.filter((f) => f.key !== fieldKey) };
}

const ANYCUBIC_AZF_SIMPLE: PluginLocalMaterialSettingsAdapterContract = {
  outputFormat: ANYCUBIC_AZF_FORMAT_DEFINITION.outputFormat,
  ...ANYCUBIC_SIMPLE_SETTINGS_BASE,
};

const ANYCUBIC_AZF_SIMPLE_NO_TEMP: PluginLocalMaterialSettingsAdapterContract = {
  outputFormat: ANYCUBIC_AZF_FORMAT_DEFINITION.outputFormat,
  ...withoutField(ANYCUBIC_SIMPLE_SETTINGS_BASE, 'targetTemperatureC'),
};

const ANYCUBIC_AZF_TWOSTAGE: PluginLocalMaterialSettingsAdapterContract = {
  outputFormat: ANYCUBIC_AZF_FORMAT_DEFINITION.outputFormat,
  ...ANYCUBIC_TWOSTAGE_SETTINGS_BASE,
};

const ANYCUBIC_AZF_TWOSTAGE_NO_TEMP: PluginLocalMaterialSettingsAdapterContract = {
  outputFormat: ANYCUBIC_AZF_FORMAT_DEFINITION.outputFormat,
  ...withoutField(ANYCUBIC_TWOSTAGE_SETTINGS_BASE, 'targetTemperatureC'),
};

const ANYCUBIC_AFF_SIMPLE: PluginLocalMaterialSettingsAdapterContract = {
  outputFormat: ANYCUBIC_AFF_FORMAT_DEFINITION.outputFormat,
  ...ANYCUBIC_SIMPLE_SETTINGS_BASE,
};

const ANYCUBIC_AFF_TWOSTAGE: PluginLocalMaterialSettingsAdapterContract = {
  outputFormat: ANYCUBIC_AFF_FORMAT_DEFINITION.outputFormat,
  ...ANYCUBIC_TWOSTAGE_SETTINGS_BASE,
};

// ─── Extension → mode map helpers ────────────────────────────────────

function azfModeMap() {
  return { simple: ANYCUBIC_AZF_SIMPLE, twostage: ANYCUBIC_AZF_TWOSTAGE };
}

// Not wired into slicingFormatsByOutput yet: the AZF variants without a target
// temperature are ready for printers that do not expose one, but no extension
// maps to them today. Exported so it does not read as dead code.
export function azfModeMapNoTemp() {
  return { simple: ANYCUBIC_AZF_SIMPLE_NO_TEMP, twostage: ANYCUBIC_AZF_TWOSTAGE_NO_TEMP };
}

function affModeMap() {
  return { simple: ANYCUBIC_AFF_SIMPLE, twostage: ANYCUBIC_AFF_TWOSTAGE };
}

// ─── Plugin definition ──────────────────────────────────────────────

export const ANYCUBIC_COMPLEX_PLUGIN_DEFINITION: ComplexPluginDefinition = {
  id: 'anycubic',
  manifest: ANYCUBIC_PLUGIN_MANIFEST,
  capabilities: {
    networkOperations: false,
    uploadWithProgress: false,
    slicerEncoder: true,
    tauriRuntimePlugin: false,
  },
  slicingFormatsByOutput: {
    '.aff': ANYCUBIC_AFF_FORMAT_DEFINITION,
    '.azf': ANYCUBIC_AZF_FORMAT_DEFINITION,
  },
  localMaterialSettingsByOutput: {
    // Canonical format keys
    '.aff': ANYCUBIC_AFF_SIMPLE,
    '.azf': ANYCUBIC_AZF_SIMPLE,
  },
  localMaterialSettingsByOutputAndMode: {
    // Canonical format keys
    '.aff': affModeMap(),
    '.azf': azfModeMap(),
  },
};

export default ANYCUBIC_COMPLEX_PLUGIN_DEFINITION;
