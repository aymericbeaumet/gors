export type AppRoute = "home" | "playground" | "conformance";

export type NavigateTo = (route: AppRoute, event?: MouseEvent) => void;
