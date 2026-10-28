import { mount } from 'svelte';
import App from './App.svelte';
import './app.css';

if (new URLSearchParams(window.location.search).get('view') === 'widget') {
  document.documentElement.classList.add('desktop-widget');
  if (!('__TAURI_INTERNALS__' in window)) document.documentElement.classList.add('widget-browser-preview');
}

mount(App, { target: document.getElementById('app')! });
