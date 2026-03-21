import type { SlicingFormatDefinition } from '@/features/slicing/formats/types';

export const ANYCUBIC_AZF_FORMAT_DEFINITION: SlicingFormatDefinition = {
  id: 'anycubic.azf.v1',
  outputFormat: '.azf',
  displayName: 'Anycubic AZF',
  ownership: 'plugin',
  layerDataKind: 'raw-mask',
  pluginId: 'anycubic',
  rustModulePath: 'formats::azf',
  wasmExportName: 'encode_azf_container',
  notes: 'Anycubic Zip Format (AZF) for Photon Mono M7 series and Mono 4 Ultra printers.',
};
