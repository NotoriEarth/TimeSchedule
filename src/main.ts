import { mount } from 'svelte';
import './app.css';

if (new URLSearchParams(window.location.search).get('view') === 'widget') {
  document.documentElement.classList.add('desktop-widget');
  if (!('__TAURI_INTERNALS__' in window)) document.documentElement.classList.add('widget-browser-preview');
}

async function start() {
  const { default: Component } = new URLSearchParams(window.location.search).get('view') === 'widget'
    ? await import('./App.svelte') : await import('./lib/Diary.svelte');
  mount(Component, { target: document.getElementById('app')! });
}
void start();
