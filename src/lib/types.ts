/** Types matching Rust backend structs */

export interface AppConfig {
  audio: AudioConfig;
  gummy: GummyConfig;
  overlay: OverlayConfig;
}

export interface AudioConfig {
  source: string;
  device_name: string;
  sample_rate: number;
  chunk_duration_ms: number;
}

export interface GummyConfig {
  api_key: string;
  model: string;
  source_language: string;
  target_languages: string[];
  vocabulary_id: string;
  max_end_silence: number;
}

export interface OverlayConfig {
  display_mode: string; // "bilingual" | "target_only"
  font_family: string;
  font_size: number;
  opacity: number;
  background_color: string;
  text_color: string;
  source_text_color: string;
  max_lines: number;
}

export interface PipelineEvent {
  event_type: string; // "vad" | "asr" | "translation" | "error" | "status" | "log"
  data: Record<string, unknown>;
}

export interface TranslationLine {
  id: string;
  source: string;
  target: string;
  is_partial: boolean;
  timestamp: number;
}

// ─────────── Hotword Types ───────────

export interface Hotword {
  text: string;
  weight: number;
  lang?: string;
  target_lang: string;
  translation: string;
}

export interface VocabularyInfo {
  vocabulary_id: string;
  gmt_create: string;
  gmt_modified: string;
  target_model: string;
  prefix: string;
  status: string;
}

export interface VocabularyDetail {
  vocabulary: Hotword[];
  gmt_create: string;
  gmt_modified: string;
  target_model: string;
  status: string;
}

export interface VocabularyListResponse {
  vocabularies: VocabularyInfo[];
  total_count: number;
}

// ─────────── Language Maps ───────────

/** All supported languages with human-readable names */
export const ALL_LANGUAGES: Record<string, string> = {
  zh: '中文（普通话）',
  yue: '中文（粤语）',
  en: '英语',
  ja: '日语',
  ko: '韩语',
  fr: '法语',
  de: '德语',
  es: '西班牙语',
  ru: '俄语',
  it: '意大利语',
  pt: '葡萄牙语',
  id: '印尼语',
  ar: '阿拉伯语',
  th: '泰语',
  hi: '印地语',
  da: '丹麦语',
  ur: '乌尔都语',
  tr: '土耳其语',
  nl: '荷兰语',
  ms: '马来语',
  vi: '越南语',
};

/**
 * Valid translation language pairs: source → allowed targets.
 * Based on Alibaba Cloud gummy-realtime-v1 official documentation.
 */
export const VALID_LANGUAGE_PAIRS: Record<string, string[]> = {
  zh:  ['en', 'ja', 'ko', 'fr', 'de', 'es', 'ru', 'it'],
  yue: ['zh', 'en'],
  en:  ['zh', 'yue', 'ja', 'ko', 'pt', 'fr', 'de', 'ru', 'vi', 'es', 'nl', 'da', 'ar', 'it', 'hi', 'tr', 'ms', 'ur'],
  ja:  ['th', 'en', 'zh', 'vi', 'fr', 'it', 'de', 'es'],
  ko:  ['th', 'en', 'zh', 'vi', 'fr', 'es', 'ru', 'de'],
  fr:  ['th', 'en', 'ja', 'zh', 'vi', 'de', 'it', 'es', 'ru', 'pt'],
  de:  ['th', 'en', 'ja', 'zh', 'fr', 'vi', 'ru', 'es', 'it', 'pt'],
  es:  ['th', 'en', 'ja', 'zh', 'fr', 'vi', 'it', 'de', 'ru', 'pt'],
  ru:  ['th', 'en', 'ja', 'zh', 'yue', 'fr', 'vi', 'de', 'es', 'it', 'pt'],
  it:  ['th', 'en', 'ja', 'zh', 'fr', 'vi', 'es', 'ru', 'de'],
  pt:  ['en'],
  id:  ['en'],
  ar:  ['en'],
  th:  ['ja', 'vi', 'fr'],
  hi:  ['en'],
  da:  ['en'],
  ur:  ['en'],
  tr:  ['en'],
  nl:  ['en'],
  ms:  ['en'],
  vi:  ['ja', 'fr'],
};

/** All languages that can be used as source language (keys of VALID_LANGUAGE_PAIRS) */
export const SOURCE_LANGUAGES = Object.keys(VALID_LANGUAGE_PAIRS);

/** Get all unique target languages across all source languages */
export function getAllTargetLanguages(): string[] {
  const set = new Set<string>();
  for (const targets of Object.values(VALID_LANGUAGE_PAIRS)) {
    for (const t of targets) set.add(t);
  }
  return Array.from(set);
}

/** Get valid target languages for a given source language. Returns all if source is 'auto'. */
export function getValidTargets(source: string): string[] {
  if (source === 'auto') return getAllTargetLanguages();
  return VALID_LANGUAGE_PAIRS[source] ?? [];
}
