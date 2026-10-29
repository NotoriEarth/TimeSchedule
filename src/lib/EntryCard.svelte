<script lang="ts">
  import { diaryApi, kindNames, safeLink } from './diary-api';
  import type { DiaryEntry, EntrySummary } from './diary-api';
  let { entry, projectTitle = '', onedit }: { entry: EntrySummary; projectTitle?: string; onedit: (entry: DiaryEntry) => void } = $props();
  let expanded = $state(false);
  let full = $state<DiaryEntry | null>(null);
  let error = $state('');
  let loading = $state(false);
  async function toggle() {
    expanded = !expanded;
    if (!expanded) return;
    loading = true; error = '';
    try { full = await diaryApi.entry(entry.id); } catch (cause) { error = String(cause); } finally { loading = false; }
  }
</script>

<article class="entry-card">
  <button class="entry-summary" onclick={toggle} aria-expanded={expanded}>
    <span class="entry-kind">{kindNames[entry.kind]}</span><strong>{entry.title}</strong><span class="entry-meta">{projectTitle}</span><span aria-hidden="true">{expanded ? '−' : '+'}</span>
  </button>
  {#if expanded}
    <div class="entry-body">
      {#if loading}<p>원문을 불러오는 중…</p>{:else if error}<p role="alert">{error}</p>{:else if full}
        <p class="preserve">{full.body || '작성된 본문이 없습니다.'}</p>
        {#if safeLink(full.sourceUrl)}<a href={safeLink(full.sourceUrl)!} target="_blank" rel="noopener noreferrer" onclick={(e)=>{e.preventDefault();void diaryApi.openSource(full!.sourceUrl).catch(cause=>error=String(cause));}}>출처 링크 열기 ↗</a>{/if}
        {#if full.sourceRef}<details><summary>수집 원본 · 변경되지 않는 근거</summary><small class="preserve">{full.sourceRef}</small><p class="preserve">{full.sourceBody}</p></details>{/if}
        <button class="paper-button" onclick={() => full && onedit(full)}>수정하기</button>
      {/if}
    </div>
  {/if}
</article>
