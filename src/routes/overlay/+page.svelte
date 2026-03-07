<script lang="ts">
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { LogicalSize } from '@tauri-apps/api/dpi';
  import { onMount, onDestroy } from 'svelte';
  import type { PipelineEvent, TranslationLine, OverlayConfig } from '$lib/types';
  import { getConfig } from '$lib/api';

  let lines: TranslationLine[] = $state([]);
  let config: OverlayConfig | null = $state(null);
  let isSpeech = $state(false);
  let isDragging = $state(false);
  let dragOffset = { x: 0, y: 0 };

  let lineIdCounter = 0;
  let unlisten: (() => void) | null = null;
  let unlistenConfig: (() => void) | null = null;

  onMount(async () => {
    // Load overlay config
    const appConfig = await getConfig();
    config = appConfig.overlay;

    // Listen for config changes from main window
    unlistenConfig = await listen<{ overlay: OverlayConfig }>('config-changed', (event) => {
      config = event.payload.overlay;
    });

    // Listen for pipeline events
    unlisten = await listen<PipelineEvent>('pipeline-event', (event) => {
      const evt = event.payload;
      switch (evt.event_type) {
        case 'vad':
          isSpeech = evt.data.is_speech as boolean;
          break;

        case 'translation': {
          const source = evt.data.source as string;
          const target = evt.data.target as string;
          const isPartial = evt.data.is_partial as boolean;

          if (isPartial) {
            // Update the last line if it's a partial update
            const lastLine = lines[lines.length - 1];
            if (lastLine && lastLine.is_partial) {
              lastLine.source = source;
              lastLine.target = target;
              lines = [...lines];
            } else {
              lines = [...lines, {
                id: `line-${lineIdCounter++}`,
                source,
                target,
                is_partial: true,
                timestamp: Date.now(),
              }];
            }
          } else {
            // Final result - update or add
            const lastLine = lines[lines.length - 1];
            if (lastLine && lastLine.is_partial) {
              lastLine.source = source;
              lastLine.target = target;
              lastLine.is_partial = false;
              lines = [...lines];
            } else {
              lines = [...lines, {
                id: `line-${lineIdCounter++}`,
                source,
                target,
                is_partial: false,
                timestamp: Date.now(),
              }];
            }
          }

          // Trim to max lines
          const maxLines = config?.max_lines ?? 5;
          if (lines.length > maxLines) {
            lines = lines.slice(-maxLines);
          }
          break;
        }

        case 'asr':
          // ASR results are intermediate, no direct display needed
          break;

        case 'error':
          console.error('Pipeline error:', evt.data.message);
          break;
      }
    });
  });

  onDestroy(() => {
    unlisten?.();
    unlistenConfig?.();
  });

  // Dragging support via Tauri window API
  async function onMouseDown(e: MouseEvent) {
    if (e.button === 0) {
      const win = getCurrentWindow();
      await win.startDragging();
    }
  }

  // Double-click to toggle bilingual/target-only
  async function onDoubleClick() {
    if (!config) return;
    config.display_mode = config.display_mode === 'bilingual' ? 'target_only' : 'bilingual';
  }

  // Dynamically update minimum window height based on display mode and font size
  $effect(() => {
    if (!config) return;
    const lineHeight = 1.6;
    const padding = 24; // 12px top + 12px bottom
    const vadHeight = 16; // VAD indicator area
    const lineCount = config.display_mode === 'bilingual' ? 2 : 1;
    // For bilingual: source line (0.85em) + target line (1em); for target_only: just target
    const sourceLineHeight = config.display_mode === 'bilingual' ? config.font_size * 0.85 * lineHeight : 0;
    const targetLineHeight = config.font_size * lineHeight;
    const minHeight = Math.ceil(sourceLineHeight + targetLineHeight + padding + vadHeight);
    const win = getCurrentWindow();
    win.setMinSize(new LogicalSize(200, minHeight));
  });

  // Derived styles
  let containerStyle = $derived(
    config
      ? `
        background: ${config.background_color};
        opacity: ${config.opacity};
        font-family: '${config.font_family}', 'Times New Roman', serif;
        font-size: ${config.font_size}px;
      `
      : ''
  );
</script>

<div
  class="overlay-container"
  style={containerStyle}
  onmousedown={onMouseDown}
  ondblclick={onDoubleClick}
  role="presentation"
>
  <!-- VAD indicator -->
  <div class="vad-indicator" class:active={isSpeech}>
    <div class="vad-dot"></div>
  </div>

  <!-- Translation lines -->
  <div class="lines">
    {#each lines as line (line.id)}
      <div class="line" class:partial={line.is_partial}>
        {#if config?.display_mode === 'bilingual'}
          <div class="source" style="color: {config?.source_text_color ?? '#AAAAAA'}">
            {line.source}
          </div>
        {/if}
        <div class="target" style="color: {config?.text_color ?? '#FFFFFF'}">
          {line.target}
          {#if line.is_partial}
            <span class="cursor">▊</span>
          {/if}
        </div>
      </div>
    {/each}

    {#if lines.length === 0}
      <div class="placeholder" style="color: {config?.source_text_color ?? '#888'}">
        等待音频输入...
      </div>
    {/if}
  </div>
</div>

<style>
  :global(body) {
    margin: 0;
    padding: 0;
    background: transparent;
    overflow: hidden;
  }

  .overlay-container {
    width: 100vw;
    height: 100vh;
    padding: 12px 16px;
    box-sizing: border-box;
    cursor: grab;
    user-select: none;
    border-radius: 8px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .overlay-container:active {
    cursor: grabbing;
  }

  .vad-indicator {
    position: absolute;
    top: 8px;
    right: 12px;
    z-index: 10;
  }

  .vad-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #555;
    transition: background 0.2s ease;
  }

  .vad-indicator.active .vad-dot {
    background: #4caf50;
    box-shadow: 0 0 6px #4caf50;
  }

  .lines {
    flex: 1;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    gap: 6px;
    overflow: hidden;
  }

  .line {
    animation: fadeIn 0.3s ease;
  }

  .source {
    font-size: 0.85em;
    opacity: 0.8;
    margin-bottom: 2px;
  }

  .target {
    line-height: 1.4;
  }

  .partial .target {
    opacity: 0.9;
  }

  .cursor {
    animation: blink 0.8s step-end infinite;
    opacity: 0.6;
    font-size: 0.9em;
  }

  .placeholder {
    text-align: center;
    opacity: 0.5;
    font-style: italic;
  }

  @keyframes fadeIn {
    from { opacity: 0; transform: translateY(8px); }
    to { opacity: 1; transform: translateY(0); }
  }

  @keyframes blink {
    50% { opacity: 0; }
  }
</style>
