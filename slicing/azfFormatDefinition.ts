import type { SlicingFormatDefinition } from '@/features/slicing/formats/types';

export const ANYCUBIC_AZF_FORMAT_DEFINITION: SlicingFormatDefinition = {
  id: 'anycubic.azf.v1',
  outputFormat: '.azf',
  displayName: 'Anycubic AZF',
  ownership: 'plugin',
  layerDataKind: 'raw-mask',
  pluginId: 'anycubic',
  formatVersions: [
    { value: 'pm4u', label: 'Photon Mono 4 Ultra (pm4u)' },
    { value: 'pm7', label: 'Photon Mono M7 (pm7)', isDefault: true },
    { value: 'pm7m', label: 'Photon Mono M7 Max (pm7m)' },
    { value: 'pwsz', label: 'Photon Mono M7 Pro (pwsz)' },
  ],
  settingsModes: [
    { value: 'simple', label: 'Simple', isDefault: true },
    { value: 'twostage', label: 'Advanced' },
  ],
  rustModulePath: 'formats::azf',
  wasmExportName: 'encode_azf_container',
  notes: 'Anycubic Zip Format (AZF) for Photon Mono M7 series and Mono 4 Ultra printers.',
};
