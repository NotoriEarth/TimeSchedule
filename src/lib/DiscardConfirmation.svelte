<script lang="ts">
  import { onMount } from 'svelte';

  let { oncontinue, ondiscard }: { oncontinue: () => void; ondiscard: () => void } = $props();
  let dialog: HTMLDialogElement;
  let continueButton: HTMLButtonElement;

  onMount(() => {
    // A separate modal keeps the editor and its scroll position intact underneath.
    dialog.showModal();
    continueButton.focus({ preventScroll: true });
    return () => dialog.close();
  });

  function keepEditing() {
    dialog.close();
    oncontinue();
  }
</script>

<dialog bind:this={dialog} class="discard-dialog" role="alertdialog"
  aria-labelledby="discard-dialog-title" aria-describedby="discard-dialog-description"
  oncancel={(e)=>{e.preventDefault();keepEditing();}}>
  <h2 id="discard-dialog-title">저장하지 않고 닫을까요?</h2>
  <p id="discard-dialog-description">작성 중인 내용이 저장되지 않았습니다.<br/>닫으면 이 내용은 사라집니다.</p>
  <div class="discard-dialog-actions">
    <button bind:this={continueButton} type="button" class="continue-editing" onclick={keepEditing}>계속 편집</button>
    <button type="button" class="discard-changes" onclick={ondiscard}>저장하지 않고 닫기</button>
  </div>
</dialog>

<style>
  .discard-dialog {
    width: 420px; max-width: calc(100vw - 40px); max-height: calc(100dvh - 40px);
    margin: auto; padding: 28px; border: 1px solid #d9ddcf; border-radius: 12px;
    background: #fffdf7; color: #44513b; box-shadow: 0 20px 70px #202b2445;
    font: 14px/1.7 'Segoe UI', 'Malgun Gothic', sans-serif;
  }
  .discard-dialog::backdrop { background: #27332466; }
  h2 { margin: 0 0 12px; font-size: 20px; font-weight: 600; letter-spacing: -.5px; }
  p { margin: 0; color: #727b68; font-size: 13px; }
  .discard-dialog-actions { display: flex; justify-content: flex-end; gap: 10px; margin-top: 26px; flex-wrap: wrap; }
  button { padding: 9px 13px; border: 1px solid #cad4bd; border-radius: 6px; font: inherit; font-size: 12px; cursor: pointer; }
  .continue-editing { background: #edf2e6; color: #4c653e; }
  .continue-editing:hover { background: #e1e9d7; }
  .discard-changes { background: #fff4ef; border-color: #dfc1b5; color: #995442; }
  .discard-changes:hover { background: #f8e5dc; }
  button:focus-visible { outline: 2px solid #6d865b; outline-offset: 3px; }
</style>
