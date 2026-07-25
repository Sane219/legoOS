import { useSyncExternalStore } from "react";

const TOKEN_KEY = "legoos_token";
const listeners = new Set<() => void>();

function notify(): void {
  for (const listener of listeners) listener();
}

export function getToken(): string | null {
  if (typeof window === "undefined") return null;
  return window.localStorage.getItem(TOKEN_KEY);
}

export function setToken(token: string): void {
  window.localStorage.setItem(TOKEN_KEY, token);
  notify();
}

export function clearToken(): void {
  window.localStorage.removeItem(TOKEN_KEY);
  notify();
}

function subscribe(callback: () => void): () => void {
  listeners.add(callback);
  window.addEventListener("storage", callback);
  return () => {
    listeners.delete(callback);
    window.removeEventListener("storage", callback);
  };
}

function getServerSnapshot(): string | null {
  return null;
}

/** Reactively tracks the auth token, including changes from other tabs/windows. */
export function useAuthToken(): string | null {
  return useSyncExternalStore(subscribe, getToken, getServerSnapshot);
}
