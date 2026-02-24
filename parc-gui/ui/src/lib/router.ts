export type Route = {
  path: string;
  params: Record<string, string>;
};

type RouteHandler = (route: Route) => void;

let currentRoute: Route = { path: "", params: {} };
let handlers: RouteHandler[] = [];

export function parseHash(): Route {
  const hash = window.location.hash.slice(1) || "all";
  const [path, ...rest] = hash.split("?");
  const params: Record<string, string> = {};
  if (rest.length) {
    new URLSearchParams(rest.join("?")).forEach((v, k) => {
      params[k] = v;
    });
  }
  return { path, params };
}

export function navigate(path: string, params: Record<string, string> = {}): void {
  const search = new URLSearchParams(params).toString();
  window.location.hash = search ? `${path}?${search}` : path;
}

export function onRouteChange(handler: RouteHandler): () => void {
  handlers.push(handler);
  return () => {
    handlers = handlers.filter((h) => h !== handler);
  };
}

export function currentPath(): string {
  return currentRoute.path;
}

export function getRoute(): Route {
  return currentRoute;
}

function handleHashChange(): void {
  currentRoute = parseHash();
  handlers.forEach((h) => h(currentRoute));
}

export function initRouter(): void {
  window.addEventListener("hashchange", handleHashChange);
  handleHashChange();
}
