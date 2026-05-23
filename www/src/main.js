import '@fontsource-variable/fira-code';
import { mount } from 'svelte';
import App from './App.svelte';
import 'xterm/css/xterm.css';

const app = mount(App, { target: document.getElementById('app') });

export default app;
