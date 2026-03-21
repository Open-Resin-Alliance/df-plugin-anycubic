import type { SlicingFormatDefinition } from '@/features/slicing/formats/types';

export const ANYCUBIC_AFF_FORMAT_DEFINITION: SlicingFormatDefinition = {
  id: 'anycubic.aff.v1',
  outputFormat: '.aff',
  displayName: 'Anycubic AFF',
  ownership: 'plugin',
  layerDataKind: 'raw-mask',
  pluginId: 'anycubic',
  settingsModes: [
    { value: 'simple', label: 'Simple', isDefault: true },
    { value: 'twostage', label: 'Advanced' },
  ],
  rustModulePath: 'formats::aff',
  wasmExportName: 'encode_aff_container',
  notes: 'Anycubic File Format (AFF) for Photon and Photon Mono series printers.',
};
