import "@fontsource-variable/fira-code";
import { mount } from "svelte";
import App from "./App.svelte";
import "xterm/css/xterm.css";

const target = document.getElementById("app");
if (!target) {
	throw new Error("missing #app mount target");
}

const app = mount(App, { target });

export default app;
