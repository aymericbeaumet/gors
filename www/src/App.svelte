<script lang="ts">
import { onDestroy, onMount, tick } from "svelte";
import CoveragePage from "./CoveragePage.svelte";
import HomePage from "./HomePage.svelte";
import type { AppRoute } from "./navigation";
import PlaygroundPage from "./PlaygroundPage.svelte";
import SiteHeader from "./SiteHeader.svelte";
import "./styles/global.css";
import "./styles/shell.css";

function routeFromPath(pathname: string): AppRoute {
	const normalized = pathname.replace(/\/+$/, "");
	if (normalized === "/conformance") return "conformance";
	if (normalized === "/playground") return "playground";
	return "home";
}

function pathForRoute(nextRoute: AppRoute): string {
	if (nextRoute === "conformance") return "/conformance";
	if (nextRoute === "playground") return "/playground";
	return "/";
}

function scrollPageToTop() {
	tick().then(() => {
		window.scrollTo({ top: 0, left: 0 });
	});
}

let route: AppRoute = routeFromPath(window.location.pathname);

function navigateTo(nextRoute: AppRoute, event?: MouseEvent) {
	event?.preventDefault();
	const nextPath = pathForRoute(nextRoute);
	if (window.location.pathname !== nextPath) {
		window.history.pushState({}, "", nextPath);
	}
	route = nextRoute;
	if (route === "conformance") scrollPageToTop();
}

let removePopStateListener: (() => void) | null = null;

onMount(() => {
	const onPopState = () => {
		route = routeFromPath(window.location.pathname);
		if (route === "conformance") scrollPageToTop();
	};
	window.addEventListener("popstate", onPopState);
	removePopStateListener = () =>
		window.removeEventListener("popstate", onPopState);
});

onDestroy(() => {
	removePopStateListener?.();
});
</script>

<main class="site-shell">
  <SiteHeader {route} {navigateTo} />
  <HomePage active={route === "home"} {navigateTo} />
  <PlaygroundPage active={route === "playground"} />

  {#if route === "conformance"}
    <div class="coverage-route">
      <CoveragePage />
    </div>
  {/if}
</main>
