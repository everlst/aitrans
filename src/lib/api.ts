import { invoke } from '@tauri-apps/api/core';
import type { AppConfig, Hotword, VocabularyInfo, VocabularyDetail } from './types';

/** Get current config from backend */
export async function getConfig(): Promise<AppConfig> {
  return invoke('get_config');
}

/** Save config to backend */
export async function saveConfig(config: AppConfig): Promise<void> {
  return invoke('save_config', { config });
}

/** List available audio input devices */
export async function listAudioDevices(): Promise<string[]> {
  return invoke('list_audio_devices');
}

/** Start the audio capture + translation pipeline */
export async function startPipeline(): Promise<void> {
  return invoke('start_pipeline');
}

/** Stop the pipeline */
export async function stopPipeline(): Promise<void> {
  return invoke('stop_pipeline');
}

/** Check if pipeline is running */
export async function isPipelineRunning(): Promise<boolean> {
  return invoke('is_pipeline_running');
}

// ───────────── Hotword Vocabulary API ─────────────

/** Create a new hotword vocabulary list, returns vocabulary_id */
export async function createVocabulary(
  prefix: string,
  vocabulary: Hotword[]
): Promise<string> {
  return invoke('create_vocabulary', { prefix, vocabulary });
}

/** List all hotword vocabulary lists */
export async function listVocabularies(): Promise<VocabularyInfo[]> {
  return invoke('list_vocabularies');
}

/** Query a specific vocabulary by ID */
export async function queryVocabulary(vocabularyId: string): Promise<VocabularyDetail> {
  return invoke('query_vocabulary', { vocabularyId });
}

/** Update a vocabulary (replace all hotwords) */
export async function updateVocabulary(
  vocabularyId: string,
  vocabulary: Hotword[]
): Promise<void> {
  return invoke('update_vocabulary', { vocabularyId, vocabulary });
}

/** Delete a vocabulary list */
export async function deleteVocabulary(vocabularyId: string): Promise<void> {
  return invoke('delete_vocabulary', { vocabularyId });
}
