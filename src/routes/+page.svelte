<script lang="ts">
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { WebviewWindow } from '@tauri-apps/api/webviewWindow';
  import type { AppConfig, PipelineEvent, Hotword, VocabularyInfo, VocabularyDetail } from '$lib/types';
  import {
    ALL_LANGUAGES, VALID_LANGUAGE_PAIRS, SOURCE_LANGUAGES,
    getValidTargets
  } from '$lib/types';
  import {
    getConfig, saveConfig, listAudioDevices,
    startPipeline, stopPipeline, isPipelineRunning,
    createVocabulary, listVocabularies, queryVocabulary,
    updateVocabulary, deleteVocabulary
  } from '$lib/api';

  // State
  let config: AppConfig | null = $state(null);
  let devices: string[] = $state([]);
  let running = $state(false);
  let statusMessage = $state('就绪');
  let lastError = $state('');
  let activeTab = $state('general');
  let saving = $state(false);

  // Hotword management state
  let vocabList: VocabularyInfo[] = $state([]);
  let vocabLoading = $state(false);
  let vocabEditing = $state(false);
  let editingVocabId: string | null = $state(null);
  let editingHotwords: Hotword[] = $state([]);
  let newVocabPrefix = $state('aitrans');
  let vocabCreating = $state(false);

  // Computed: available target languages based on selected source language
  let availableTargets: string[] = $derived.by(() => {
    if (!config) return [];
    return getValidTargets(config.gummy.source_language);
  });

  // Watch source language changes to reset invalid target
  $effect(() => {
    if (!config) return;
    if (availableTargets.length > 0) {
      const currentTarget = config.gummy.target_languages[0];
      if (currentTarget && !availableTargets.includes(currentTarget)) {
        config.gummy.target_languages = [availableTargets[0]];
      }
    }
  });

  // Pipeline log state
  interface LogEntry {
    time: string;
    type: string;
    message: string;
  }
  let pipelineLogs: LogEntry[] = $state([]);
  const MAX_LOG_ENTRIES = 200;

  function addLog(type: string, message: string) {
    const now = new Date();
    const time = now.toLocaleTimeString('zh-CN', { hour12: false }) + '.' + String(now.getMilliseconds()).padStart(3, '0');
    pipelineLogs = [...pipelineLogs.slice(-(MAX_LOG_ENTRIES - 1)), { time, type, message }];
  }

  let errorTimer: ReturnType<typeof setTimeout> | null = null;
  function showError(msg: string) {
    lastError = msg;
    if (errorTimer) clearTimeout(errorTimer);
    errorTimer = setTimeout(() => { lastError = ''; errorTimer = null; }, 8000);
  }
  function dismissError() {
    lastError = '';
    if (errorTimer) { clearTimeout(errorTimer); errorTimer = null; }
  }

  onMount(async () => {
    config = await getConfig();
    // Ensure source is always 'system'
    config.audio.source = 'system';
    running = await isPipelineRunning();

    // Load device list on startup
    refreshDevices();

    // Listen for pipeline events
    await listen<PipelineEvent>('pipeline-event', (event) => {
      const evt = event.payload;
      if (evt.event_type === 'status') {
        statusMessage = evt.data.message as string;
        addLog('status', evt.data.message as string);
      } else if (evt.event_type === 'pipeline-stopped') {
        // Pipeline has exited (e.g. after exhausting reconnection attempts)
        if (running) {
          running = false;
          statusMessage = '已停止（连接断开）';
          addLog('status', '管道已自动停止，请重新启动');
        }
      } else if (evt.event_type === 'error') {
        showError(evt.data.message as string);
        addLog('error', evt.data.message as string);
      } else if (evt.event_type === 'vad') {
        const energy = (evt.data.energy as number).toFixed(6);
        const speech = evt.data.is_speech ? '🟢 语音' : '⚫ 静音';
        addLog('vad', `${speech} (能量: ${energy})`);
      } else if (evt.event_type === 'asr') {
        addLog('asr', `识别: ${evt.data.text}`);
      } else if (evt.event_type === 'translation') {
        const partial = evt.data.is_partial ? '[中间]' : '[完整]';
        addLog('translation', `${partial} ${evt.data.source} → ${evt.data.target}`);
      } else if (evt.event_type === 'log') {
        // Gummy log events
        const d = evt.data as Record<string, unknown>;
        if (d.type === 'info') {
          addLog('gummy', d.message as string);
        } else if (d.type === 'audio') {
          const e = (d.energy as number).toFixed(6);
          const speech = (d.energy as number) > 0.001 ? '🟢' : '⚫';
          addLog('audio', `${speech} 已发送 ${d.samples} 样本 (能量: ${e})`);
        } else if (d.type === 'result') {
          const partial = d.is_partial ? '[中间]' : '[完整]';
          addLog('result', `${partial} ${d.source} → ${d.target}`);
        }
      }
    });
  });

  async function handleSave() {
    if (!config) return;
    saving = true;
    try {
      await saveConfig(config);
      statusMessage = '配置已保存';
    } catch (e) {
      showError(`保存失败: ${e}`);
    }
    saving = false;
  }

  async function handleTogglePipeline() {
    try {
      if (running) {
        await stopPipeline();
        running = false;
        statusMessage = '已停止';
        // Hide overlay window
        const overlay = await WebviewWindow.getByLabel('overlay');
        await overlay?.hide();
      } else {
        if (config) await saveConfig(config);
        pipelineLogs = [];
        addLog('status', '管道启动中...');
        await startPipeline();
        running = true;
        statusMessage = '运行中...';
        // Show overlay window
        const overlay = await WebviewWindow.getByLabel('overlay');
        await overlay?.show();
      }
    } catch (e) {
      showError(`管道错误: ${e}`);
    }
  }

  function refreshDevices() {
    listAudioDevices().then(d => { devices = d; }).catch(e => {
      showError(`获取设备列表失败: ${e}`);
    });
  }

  // ───────────── Hotword Management ─────────────

  async function refreshVocabList() {
    vocabLoading = true;
    try {
      vocabList = await listVocabularies();
    } catch (e) {
      showError(`获取热词列表失败: ${e}`);
    }
    vocabLoading = false;
  }

  async function handleCreateVocab() {
    if (editingHotwords.length === 0) {
      showError('请至少添加一条热词');
      return;
    }
    vocabCreating = true;
    try {
      const vocabId = await createVocabulary(newVocabPrefix, editingHotwords);
      statusMessage = `热词列表已创建: ${vocabId}`;
      if (config) {
        config.gummy.vocabulary_id = vocabId;
      }
      editingHotwords = [];
      vocabEditing = false;
      await refreshVocabList();
    } catch (e) {
      showError(`创建热词列表失败: ${e}`);
    }
    vocabCreating = false;
  }

  async function handleUpdateVocab() {
    if (!editingVocabId) return;
    vocabCreating = true;
    try {
      await updateVocabulary(editingVocabId, editingHotwords);
      statusMessage = `热词列表已更新: ${editingVocabId}`;
      vocabEditing = false;
      editingVocabId = null;
      editingHotwords = [];
      await refreshVocabList();
    } catch (e) {
      showError(`更新热词列表失败: ${e}`);
    }
    vocabCreating = false;
  }

  async function handleEditVocab(vocabId: string) {
    try {
      const detail: VocabularyDetail = await queryVocabulary(vocabId);
      editingVocabId = vocabId;
      editingHotwords = detail.vocabulary.map(h => ({ ...h }));
      vocabEditing = true;
    } catch (e) {
      showError(`查询热词列表失败: ${e}`);
    }
  }

  async function handleDeleteVocab(vocabId: string) {
    try {
      await deleteVocabulary(vocabId);
      statusMessage = `热词列表已删除: ${vocabId}`;
      if (config && config.gummy.vocabulary_id === vocabId) {
        config.gummy.vocabulary_id = '';
      }
      await refreshVocabList();
    } catch (e) {
      showError(`删除热词列表失败: ${e}`);
    }
  }

  function addHotword() {
    editingHotwords = [...editingHotwords, {
      text: '',
      weight: 4,
      lang: config?.gummy.source_language !== 'auto' ? config?.gummy.source_language : undefined,
      target_lang: config?.gummy.target_languages[0] ?? 'en',
      translation: ''
    }];
  }

  function removeHotword(index: number) {
    editingHotwords = editingHotwords.filter((_, i) => i !== index);
  }

  function startNewVocab() {
    editingVocabId = null;
    editingHotwords = [];
    vocabEditing = true;
  }

  function cancelEditVocab() {
    vocabEditing = false;
    editingVocabId = null;
    editingHotwords = [];
  }

  // ───────────── Hotword Import ─────────────

  let importFileInput: HTMLInputElement;

  function triggerImport() {
    importFileInput?.click();
  }

  function handleImportHotwords(event: Event) {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;

    const reader = new FileReader();
    reader.onload = (e) => {
      try {
        const text = e.target?.result as string;
        const data = JSON.parse(text);

        if (!Array.isArray(data)) {
          showError('导入失败：文件内容必须是 JSON 数组');
          return;
        }

        const errors: string[] = [];
        const validItems: Hotword[] = [];

        for (let i = 0; i < data.length; i++) {
          const item = data[i];
          const idx = i + 1;

          if (!item.text || typeof item.text !== 'string') {
            errors.push(`第 ${idx} 条：缺少 text 字段`);
            continue;
          }
          if (!item.translation || typeof item.translation !== 'string') {
            errors.push(`第 ${idx} 条：缺少 translation 字段`);
            continue;
          }
          if (!item.target_lang || typeof item.target_lang !== 'string') {
            errors.push(`第 ${idx} 条：缺少 target_lang 字段`);
            continue;
          }

          let weight = Number(item.weight) || 4;
          if (weight < 1) weight = 1;
          if (weight > 5) weight = 5;

          validItems.push({
            text: item.text,
            weight,
            lang: item.lang || undefined,
            target_lang: item.target_lang,
            translation: item.translation,
          });
        }

        if (validItems.length === 0) {
          showError(`导入失败：没有有效条目。${errors.length > 0 ? errors.slice(0, 3).join('；') : ''}`);
          return;
        }

        // Merge: overwrite existing entries with same text, append new ones
        let overwritten = 0;
        const existingMap = new Map(editingHotwords.map((hw, i) => [hw.text, i]));
        const merged = [...editingHotwords];

        for (const item of validItems) {
          const existingIdx = existingMap.get(item.text);
          if (existingIdx !== undefined) {
            merged[existingIdx] = item;
            overwritten++;
          } else {
            merged.push(item);
          }
        }

        editingHotwords = merged;

        const added = validItems.length - overwritten;
        let msg = `导入成功：${added} 条新增`;
        if (overwritten > 0) msg += `，${overwritten} 条覆盖`;
        if (errors.length > 0) msg += `，${errors.length} 条跳过`;
        statusMessage = msg;
      } catch (err) {
        showError(`导入失败：JSON 解析错误 - ${err}`);
      }
    };
    reader.readAsText(file);

    // Reset input so same file can be re-imported
    input.value = '';
  }


</script>

<main>
  <header>
    <h1>🌐 AI 实时翻译</h1>
    <div class="header-controls">
      <div class="status" class:running>
        <span class="status-dot"></span>
        {statusMessage}
      </div>
      <button class="btn-primary" class:stop={running} onclick={handleTogglePipeline}>
        {running ? '⏹ 停止' : '▶ 开始'}
      </button>
    </div>
  </header>

  {#if lastError}
    <div class="error-banner">
      <span>{lastError}</span>
      <button class="error-close" onclick={dismissError}>✕</button>
    </div>
  {/if}

  <nav class="tabs">
    <button class:active={activeTab === 'general'} onclick={() => activeTab = 'general'}>
      ⚙️ 常规
    </button>
    <button class:active={activeTab === 'hotwords'} onclick={() => { activeTab = 'hotwords'; refreshVocabList(); }}>
      🔤 热词
    </button>
    <button class:active={activeTab === 'overlay'} onclick={() => activeTab = 'overlay'}>
      🖥 字幕
    </button>
    <button class:active={activeTab === 'logs'} onclick={() => activeTab = 'logs'}>
      📋 日志
    </button>
  </nav>

  {#if config}
    <div class="tab-content">

      {#if activeTab === 'general'}
        <section>
          <h2>Gummy 翻译服务 (阿里云百炼)</h2>
          <p class="hint">
            使用阿里云 DashScope 的 Gummy 模型进行实时语音识别与翻译。
            <a href="https://bailian.console.aliyun.com/" target="_blank" style="color:#64b5f6">获取 API Key →</a>
          </p>
          <div class="field">
            <label for="gummy-api-key">DashScope API Key</label>
            <input id="gummy-api-key" type="password" bind:value={config.gummy.api_key}
                   placeholder="sk-..." />
          </div>
          <div class="field">
            <label for="gummy-model">模型</label>
            <input id="gummy-model" type="text" bind:value={config.gummy.model}
                   placeholder="gummy-realtime-v1" />
          </div>
          <div class="field-row">
            <div class="field">
              <label for="gummy-source-lang">源语言</label>
              <select id="gummy-source-lang" bind:value={config.gummy.source_language}>
                <option value="auto">自动检测</option>
                {#each SOURCE_LANGUAGES as code}
                  <option value={code}>{ALL_LANGUAGES[code] || code}</option>
                {/each}
              </select>
            </div>
            <div class="field">
              <label for="gummy-target-lang">目标语言</label>
              <select id="gummy-target-lang" bind:value={config.gummy.target_languages[0]}>
                {#each availableTargets as code}
                  <option value={code}>{ALL_LANGUAGES[code] || code}</option>
                {/each}
              </select>
              {#if config.gummy.source_language === 'auto'}
                <small class="hint-warning">⚠️ 自动检测模式下显示所有目标语言，部分语言组合可能不受 API 支持</small>
              {:else}
                <small>当前源语言支持 {availableTargets.length} 种目标语言</small>
              {/if}
            </div>
          </div>

          <div class="field">
            <label for="max-end-silence">VAD 断句静音阈值: {config.gummy.max_end_silence}ms</label>
            <input id="max-end-silence" type="range" min="200" max="6000" step="100"
                   bind:value={config.gummy.max_end_silence} />
            <small>语音后的静音超过此时长时，系统判定句子结束。值越大，停顿越长才会断句，适合语速较慢或停顿较多的场景。默认 800ms。</small>
          </div>

          <h2>音频来源</h2>
          <div class="field">
            <label for="audio-device">
              🔊 系统音频 (虚拟设备)
              <button class="btn-small" onclick={refreshDevices}>↻ 刷新</button>
            </label>
            <select id="audio-device" bind:value={config.audio.device_name}>
              <option value="">默认</option>
              {#each devices as device}
                <option value={device}>{device}</option>
              {/each}
            </select>
            <small>
              ⚠️ macOS 系统音频捕获需要安装虚拟音频驱动（如 <a href="https://existential.audio/blackhole/" target="_blank" style="color:#64b5f6">BlackHole</a>）。
              安装后在此选择虚拟设备作为输入源。
            </small>
          </div>
          <div class="field">
            <label for="chunk-duration">音频块时长: {config.audio.chunk_duration_ms}ms</label>
            <input id="chunk-duration" type="range" min="20" max="500" step="10"
                   bind:value={config.audio.chunk_duration_ms} />
            <small>每次发送给模型的音频块时长。较小的值减少延迟但增加网络开销，推荐 100–200ms。默认 160ms。</small>
          </div>
        </section>

      {:else if activeTab === 'hotwords'}
        <section>
          <h2>热词管理</h2>
          <p class="hint">
            热词功能可提升特定词汇的识别和翻译准确率。需先创建热词列表，获取 ID 后方可在翻译时使用。
            <a href="https://help.aliyun.com/zh/model-studio/custom-hot-words" target="_blank" style="color:#64b5f6">热词文档 →</a>
          </p>

          <div class="field">
            <label for="vocab-id">当前使用的热词列表 ID</label>
            <div class="field-row">
              <input id="vocab-id" type="text" bind:value={config.gummy.vocabulary_id}
                     placeholder="vocab-xxx-yyy（留空则不使用热词）" style="flex:1" />
            </div>
            <small>选择下方已有列表或手动输入。保存配置后生效。</small>
          </div>

          <h2>已有热词列表</h2>
          <div class="log-controls">
            <button class="btn-small" onclick={refreshVocabList}>↻ 刷新</button>
            <button class="btn-small" onclick={startNewVocab}>＋ 新建</button>
            {#if vocabLoading}
              <span style="font-size:0.8em; color:#666">加载中...</span>
            {:else}
              <span style="font-size:0.8em; color:#666">{vocabList.length} 个列表</span>
            {/if}
          </div>

          {#if vocabList.length > 0}
            <div class="vocab-list">
              {#each vocabList as vocab}
                <div class="vocab-item" class:selected={config.gummy.vocabulary_id === vocab.vocabulary_id}>
                  <div class="vocab-info">
                    <span class="vocab-id">{vocab.vocabulary_id}</span>
                    <span class="vocab-status" class:ok={vocab.status === 'OK'}>{vocab.status}</span>
                    <span class="vocab-time">{vocab.gmt_modified || vocab.gmt_create}</span>
                  </div>
                  <div class="vocab-actions">
                    <button class="btn-small" onclick={() => { if (config) config.gummy.vocabulary_id = vocab.vocabulary_id; }}>
                      选用
                    </button>
                    <button class="btn-small" onclick={() => handleEditVocab(vocab.vocabulary_id)}>
                      编辑
                    </button>
                    <button class="btn-danger-small" onclick={() => handleDeleteVocab(vocab.vocabulary_id)}>
                      🗑
                    </button>
                  </div>
                </div>
              {/each}
            </div>
          {:else if !vocabLoading}
            <div class="log-empty" style="padding: 20px;">暂无热词列表，点击"新建"创建</div>
          {/if}

          {#if vocabEditing}
            <h2>{editingVocabId ? `编辑热词列表: ${editingVocabId}` : '新建热词列表'}</h2>

            {#if !editingVocabId}
              <div class="field">
                <label for="vocab-prefix">前缀 (仅小写字母和数字，&lt;10字符)</label>
                <input id="vocab-prefix" type="text" bind:value={newVocabPrefix}
                       placeholder="aitrans" maxlength="9" pattern="[a-z0-9]+" />
              </div>
            {/if}

            <p class="hint">
              Gummy 模型热词需填写 <code>text</code>（原词）、<code>weight</code>（权重 1-5）、
              <code>target_lang</code>（目标语言代码）和 <code>translation</code>（期望翻译结果）。
              <code>lang</code>（源语言代码）可选。每个列表最多 500 条热词。
            </p>

            <div class="hotword-table">
              <div class="hotword-header">
                <span style="flex:2">热词文本</span>
                <span style="flex:0.5">权重</span>
                <span style="flex:1">源语言</span>
                <span style="flex:1">目标语言</span>
                <span style="flex:2">翻译结果</span>
                <span style="flex:0.3"></span>
              </div>
              {#each editingHotwords as hw, i}
                <div class="hotword-row">
                  <input style="flex:2" type="text" bind:value={hw.text}
                         placeholder="赛德克巴莱" />
                  <input style="flex:0.5" type="number" bind:value={hw.weight}
                         min="1" max="5" />
                  <select style="flex:1" bind:value={hw.lang}>
                    <option value={undefined}>自动</option>
                    {#each SOURCE_LANGUAGES as code}
                      <option value={code}>{ALL_LANGUAGES[code] || code}</option>
                    {/each}
                  </select>
                  <select style="flex:1" bind:value={hw.target_lang}>
                    {#each Object.entries(ALL_LANGUAGES) as [code, name]}
                      <option value={code}>{name}</option>
                    {/each}
                  </select>
                  <input style="flex:2" type="text" bind:value={hw.translation}
                         placeholder="Seediq Bale" />
                  <button class="btn-danger-small" style="flex:0.3" onclick={() => removeHotword(i)}>✕</button>
                </div>
              {/each}
              <div style="margin-top:8px; display:flex; gap:8px; align-items:center;">
                <button class="btn-small" onclick={addHotword}>＋ 添加热词</button>
                <button class="btn-small" onclick={triggerImport}>📂 导入热词</button>
                <input
                  bind:this={importFileInput}
                  type="file"
                  accept=".json"
                  onchange={handleImportHotwords}
                  style="display:none"
                />
                <span style="font-size:0.75em; color:#666">{editingHotwords.length} 条热词</span>
              </div>
            </div>

            <div class="hotword-actions" style="margin-top:12px; display:flex; gap:8px;">
              {#if editingVocabId}
                <button class="btn-primary" onclick={handleUpdateVocab} disabled={vocabCreating}>
                  {vocabCreating ? '保存中...' : '💾 保存修改'}
                </button>
              {:else}
                <button class="btn-primary" onclick={handleCreateVocab} disabled={vocabCreating}>
                  {vocabCreating ? '创建中...' : '✨ 创建热词列表'}
                </button>
              {/if}
              <button class="btn-small" onclick={cancelEditVocab}>取消</button>
            </div>
          {/if}
        </section>

      {:else if activeTab === 'overlay'}
        <section>
          <h2>显示设置</h2>
          <div class="field">
            <label>显示模式</label>
            <select bind:value={config.overlay.display_mode}>
              <option value="bilingual">双语 (原文 + 译文)</option>
              <option value="target_only">仅译文</option>
            </select>
          </div>

          <div class="field">
            <label>字体</label>
            <input type="text" bind:value={config.overlay.font_family} />
          </div>

          <div class="field">
            <label>字号: {config.overlay.font_size}px</label>
            <input type="range" min="12" max="48" step="1"
                   bind:value={config.overlay.font_size} />
          </div>

          <div class="field">
            <label>透明度: {(config.overlay.opacity * 100).toFixed(0)}%</label>
            <input type="range" min="0.1" max="1" step="0.05"
                   bind:value={config.overlay.opacity} />
          </div>

          <div class="field-row">
            <div class="field">
              <label>背景色</label>
              <input type="text" bind:value={config.overlay.background_color} />
            </div>
            <div class="field">
              <label>文字颜色</label>
              <input type="color" bind:value={config.overlay.text_color} />
            </div>
            <div class="field">
              <label>原文颜色</label>
              <input type="color" bind:value={config.overlay.source_text_color} />
            </div>
          </div>

          <div class="field">
            <label>最大行数: {config.overlay.max_lines}</label>
            <input type="range" min="1" max="15" step="1"
                   bind:value={config.overlay.max_lines} />
          </div>

          <h2>预览</h2>
          <div class="overlay-preview" style="
            background: {config.overlay.background_color};
            opacity: {config.overlay.opacity};
            font-family: '{config.overlay.font_family}', serif;
            font-size: {config.overlay.font_size}px;
            border-radius: 8px;
            padding: 12px 16px;
          ">
            {#if config.overlay.display_mode === 'bilingual'}
              <div style="color: {config.overlay.source_text_color}; font-size: 0.85em; opacity: 0.8;">
                Welcome to today's match between Team Alpha and Team Beta.
              </div>
            {/if}
            <div style="color: {config.overlay.text_color}; line-height: 1.4;">
              欢迎来到今天 Team Alpha 和 Team Beta 的比赛。
            </div>
          </div>
        </section>

      {:else if activeTab === 'logs'}
        <section>
          <h2>管道日志</h2>
          <p class="hint">
            实时显示音频采集、语音识别、翻译等管道事件。启动管道后，日志将自动更新。
          </p>
          <div class="log-controls">
            <button class="btn-small" onclick={() => { pipelineLogs = []; }}>🗑 清空</button>
            <span style="font-size:0.8em; color:#666">{pipelineLogs.length} 条日志</span>
          </div>
          <div class="log-panel">
            {#if pipelineLogs.length === 0}
              <div class="log-empty">暂无日志，启动管道后将在此显示事件</div>
            {:else}
              {#each pipelineLogs as entry}
                <div class="log-entry log-{entry.type}">
                  <span class="log-time">{entry.time}</span>
                  <span class="log-tag">[{entry.type}]</span>
                  <span class="log-msg">{entry.message}</span>
                </div>
              {/each}
            {/if}
          </div>
        </section>
      {/if}
    </div>

    <footer>
      <button class="btn-primary" onclick={handleSave} disabled={saving}>
        {saving ? '保存中...' : '💾 保存配置'}
      </button>
    </footer>
  {:else}
    <div class="loading">正在加载配置...</div>
  {/if}
</main>

<style>
  :global(body) {
    margin: 0;
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif;
    background: #1a1a2e;
    color: #e0e0e0;
  }

  main {
    max-width: 840px;
    margin: 0 auto;
    padding: 20px;
    min-height: 100vh;
    box-sizing: border-box;
  }

  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 20px;
    padding-bottom: 16px;
    border-bottom: 1px solid #333;
  }

  header h1 { margin: 0; font-size: 1.5em; }

  .header-controls {
    display: flex;
    align-items: center;
    gap: 16px;
  }

  .status {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 0.9em;
    color: #888;
  }

  .status-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #555;
    transition: background 0.3s;
  }

  .status.running .status-dot {
    background: #4caf50;
    box-shadow: 0 0 6px #4caf50;
  }

  .error-banner {
    background: #f44336;
    color: white;
    padding: 8px 16px;
    border-radius: 6px;
    margin-bottom: 16px;
    font-size: 0.9em;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .error-close {
    background: none;
    border: none;
    color: white;
    font-size: 1.1em;
    cursor: pointer;
    padding: 0 4px;
    opacity: 0.8;
    flex-shrink: 0;
  }

  .error-close:hover {
    opacity: 1;
  }

  .tabs {
    display: flex;
    gap: 4px;
    margin-bottom: 20px;
    border-bottom: 2px solid #333;
  }

  .tabs button {
    background: none;
    border: none;
    color: #888;
    padding: 10px 16px;
    cursor: pointer;
    font-size: 0.9em;
    border-bottom: 2px solid transparent;
    margin-bottom: -2px;
    transition: all 0.2s;
  }

  .tabs button:hover { color: #ccc; }

  .tabs button.active {
    color: #64b5f6;
    border-bottom-color: #64b5f6;
  }

  section { animation: fadeIn 0.2s ease; }

  h2 {
    font-size: 1.1em;
    color: #64b5f6;
    margin: 24px 0 12px;
    padding-bottom: 6px;
    border-bottom: 1px solid #2a2a4a;
  }

  h2:first-child { margin-top: 0; }

  .field { margin-bottom: 14px; }

  .field label {
    display: block;
    margin-bottom: 4px;
    font-size: 0.85em;
    color: #aaa;
  }

  .field input[type="text"],
  .field input[type="password"],
  .field input[type="number"],
  .field select,
  .field textarea {
    width: 100%;
    padding: 8px 12px;
    background: #16213e;
    border: 1px solid #333;
    border-radius: 6px;
    color: #e0e0e0;
    font-size: 0.95em;
    box-sizing: border-box;
  }

  .field textarea {
    resize: vertical;
    font-family: 'SF Mono', 'Fira Code', monospace;
    font-size: 0.85em;
  }

  .field input:focus, .field select:focus, .field textarea:focus {
    outline: none;
    border-color: #64b5f6;
  }

  .field small {
    display: block;
    margin-top: 4px;
    font-size: 0.78em;
    color: #666;
    line-height: 1.4;
  }

  .field input[type="range"] {
    width: 100%;
    accent-color: #64b5f6;
  }

  .field input[type="color"] {
    width: 60px;
    height: 34px;
    border: 1px solid #333;
    border-radius: 4px;
    background: none;
    cursor: pointer;
  }

  .field.inline label {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    font-size: 0.9em;
    color: #ccc;
  }

  .field-row {
    display: flex;
    gap: 12px;
  }

  .field-row .field { flex: 1; }

  .btn-primary {
    background: #1565c0;
    color: white;
    border: none;
    padding: 10px 24px;
    border-radius: 6px;
    cursor: pointer;
    font-size: 0.95em;
    transition: background 0.2s;
  }

  .btn-primary:hover { background: #1976d2; }
  .btn-primary:disabled { opacity: 0.6; cursor: not-allowed; }
  .btn-primary.stop { background: #c62828; }
  .btn-primary.stop:hover { background: #d32f2f; }

  .btn-small {
    background: #2a2a4a;
    color: #aaa;
    border: 1px solid #444;
    padding: 4px 10px;
    border-radius: 4px;
    cursor: pointer;
    font-size: 0.8em;
  }

  .btn-small:hover { background: #333; }

  .btn-danger-small {
    background: none;
    border: none;
    color: #f44336;
    cursor: pointer;
    font-size: 1.1em;
    padding: 4px 8px;
  }

  .overlay-preview { margin-top: 8px; min-height: 60px; }

  .hint {
    font-size: 0.85em;
    color: #888;
    line-height: 1.5;
    margin: 0 0 16px;
  }

  .loading {
    text-align: center;
    padding: 60px;
    color: #666;
  }

  /* Log panel styles */
  .log-controls {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 8px;
  }

  .log-panel {
    background: #0d1117;
    border: 1px solid #333;
    border-radius: 6px;
    padding: 8px;
    max-height: 400px;
    overflow-y: auto;
    font-family: 'SF Mono', 'Fira Code', 'Consolas', monospace;
    font-size: 0.8em;
    line-height: 1.6;
  }

  .log-empty {
    color: #555;
    text-align: center;
    padding: 40px;
  }

  .log-entry {
    display: flex;
    gap: 6px;
    padding: 1px 4px;
    border-radius: 2px;
  }

  .log-entry:hover {
    background: rgba(255, 255, 255, 0.03);
  }

  .log-time {
    color: #555;
    flex-shrink: 0;
    min-width: 90px;
  }

  .log-tag {
    flex-shrink: 0;
    min-width: 80px;
    font-weight: 600;
  }

  .log-msg {
    color: #c9d1d9;
    word-break: break-all;
  }

  .log-status .log-tag { color: #58a6ff; }
  .log-error .log-tag { color: #f85149; }
  .log-error .log-msg { color: #f85149; }
  .log-vad .log-tag { color: #8b949e; }
  .log-vad .log-msg { color: #8b949e; }
  .log-asr .log-tag { color: #d2a8ff; }
  .log-translation .log-tag { color: #7ee787; }
  .log-result .log-tag { color: #7ee787; }
  .log-gummy .log-tag { color: #79c0ff; }
  .log-audio .log-tag { color: #ffa657; }
  .log-warn .log-tag { color: #e3b341; }
  .log-warn .log-msg { color: #e3b341; }

  .hint-warning {
    color: #e3b341 !important;
    font-weight: 500;
  }

  /* Hotword / Vocabulary styles */
  .vocab-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 8px;
  }

  .vocab-item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 12px;
    background: #16213e;
    border: 1px solid #333;
    border-radius: 6px;
    font-size: 0.85em;
    transition: border-color 0.2s;
  }

  .vocab-item.selected {
    border-color: #64b5f6;
    background: #1a2a4e;
  }

  .vocab-info {
    display: flex;
    gap: 10px;
    align-items: center;
    flex-wrap: wrap;
  }

  .vocab-id {
    color: #64b5f6;
    font-family: 'SF Mono', 'Fira Code', monospace;
    font-size: 0.9em;
  }

  .vocab-status {
    font-size: 0.75em;
    padding: 1px 6px;
    border-radius: 3px;
    background: #555;
    color: #ccc;
  }

  .vocab-status.ok {
    background: #2e7d32;
    color: #a5d6a7;
  }

  .vocab-time {
    color: #666;
    font-size: 0.8em;
  }

  .vocab-actions {
    display: flex;
    gap: 4px;
    flex-shrink: 0;
  }

  .hotword-table {
    background: #0d1117;
    border: 1px solid #333;
    border-radius: 6px;
    padding: 8px;
    overflow-x: auto;
  }

  .hotword-header {
    display: flex;
    gap: 6px;
    padding: 4px 4px 8px;
    font-size: 0.78em;
    color: #888;
    border-bottom: 1px solid #2a2a4a;
    margin-bottom: 6px;
  }

  .hotword-row {
    display: flex;
    gap: 6px;
    margin-bottom: 4px;
    align-items: center;
  }

  .hotword-row input,
  .hotword-row select {
    padding: 5px 8px;
    background: #16213e;
    border: 1px solid #333;
    border-radius: 4px;
    color: #e0e0e0;
    font-size: 0.85em;
    min-width: 0;
    box-sizing: border-box;
  }

  .hotword-row input:focus,
  .hotword-row select:focus {
    outline: none;
    border-color: #64b5f6;
  }

  .hotword-row input[type="number"] {
    -moz-appearance: textfield;
  }

  code {
    background: #2a2a4a;
    padding: 1px 5px;
    border-radius: 3px;
    font-size: 0.9em;
    color: #79c0ff;
  }

  footer {
    margin-top: 30px;
    padding-top: 20px;
    border-top: 1px solid #333;
    display: flex;
    justify-content: flex-end;
  }

  @keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }
</style>
