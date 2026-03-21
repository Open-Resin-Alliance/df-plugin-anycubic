import type { ComplexPluginDefinition } from '@/features/plugins/complexPluginContracts';
import { ANYCUBIC_PLUGIN_MANIFEST } from './pluginManifest';
import { ANYCUBIC_AFF_FORMAT_DEFINITION } from './slicing/affFormatDefinition';
import { ANYCUBIC_AZF_FORMAT_DEFINITION } from './slicing/azfFormatDefinition';

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
    // AFF format — Photon / Photon Mono series
    '.pws': ANYCUBIC_AFF_FORMAT_DEFINITION,
    '.pw0': ANYCUBIC_AFF_FORMAT_DEFINITION,
    '.pwx': ANYCUBIC_AFF_FORMAT_DEFINITION,
    '.dlp': ANYCUBIC_AFF_FORMAT_DEFINITION,
    '.dl2p': ANYCUBIC_AFF_FORMAT_DEFINITION,
    '.pwmx': ANYCUBIC_AFF_FORMAT_DEFINITION,
    '.pmx2': ANYCUBIC_AFF_FORMAT_DEFINITION,
    '.pwmb': ANYCUBIC_AFF_FORMAT_DEFINITION,
    '.px6s': ANYCUBIC_AFF_FORMAT_DEFINITION,
    '.pwmo': ANYCUBIC_AFF_FORMAT_DEFINITION,
    '.pm3n': ANYCUBIC_AFF_FORMAT_DEFINITION,
    '.pm4n': ANYCUBIC_AFF_FORMAT_DEFINITION,
    '.pwms': ANYCUBIC_AFF_FORMAT_DEFINITION,
    '.pwma': ANYCUBIC_AFF_FORMAT_DEFINITION,
    '.pmsq': ANYCUBIC_AFF_FORMAT_DEFINITION,
    '.pm3': ANYCUBIC_AFF_FORMAT_DEFINITION,
    '.pm3m': ANYCUBIC_AFF_FORMAT_DEFINITION,
    '.pm3r': ANYCUBIC_AFF_FORMAT_DEFINITION,
    '.pm5': ANYCUBIC_AFF_FORMAT_DEFINITION,
    '.pm5s': ANYCUBIC_AFF_FORMAT_DEFINITION,
    '.m5sp': ANYCUBIC_AFF_FORMAT_DEFINITION,
    '.pwc': ANYCUBIC_AFF_FORMAT_DEFINITION,
    // AZF format — Photon Mono M7 series and Mono 4 Ultra
    '.pm4u': ANYCUBIC_AZF_FORMAT_DEFINITION,
    '.pm7': ANYCUBIC_AZF_FORMAT_DEFINITION,
    '.pm7m': ANYCUBIC_AZF_FORMAT_DEFINITION,
    '.pwsz': ANYCUBIC_AZF_FORMAT_DEFINITION,
  },
};

export default ANYCUBIC_COMPLEX_PLUGIN_DEFINITION;
