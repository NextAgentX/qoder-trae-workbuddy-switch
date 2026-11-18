import { useSyncExternalStore } from "react";

export type LinkedSwitchClient = "workbuddy" | "vscodeExt" | "codebuddyIde";

export type LinkedSwitchMode = "fastForward" | "overwrite" | "statusOnly";

export interface LinkedSwitchSelection {
  groupId: string;
  previewToken: string;
  mode: LinkedSwitchMode;
}

export interface LinkedSwitchPreviewItem {
  groupId: string;
  title: string;
  reason: string;
  availableModes: LinkedSwitchMode[];
  previewToken?: string;
  archiveAction?: "statusOnly";
  defaultChecked: boolean;
  extraB?: number;
}

export interface LinkedSwitchPreview {
  groups: LinkedSwitchPreviewItem[];
  storeStatus: "missing" | "ready" | "unavailable" | "unsupported";
  storeError?: string;
}

export interface LinkedSwitchReport {
  synced: { groupId: string }[];
  skipped: { groupId: string; message?: string }[];
  errors: { groupId: string; error: string }[];
}

export interface ExtensionSwitchOptions {
  region?: import("@/lib/types").Region;
  copySessions?: import("@/lib/types").ExtCopyItem[];
  syncSelections?: LinkedSwitchSelection[];
}

const ARCHIVE_DEFAULT_KEY = "buddy-switch.sync.default-archive";
const SIDEBAR_LINKS_KEY = "buddy-switch.sessions.show-in-sidebar";
const PREFERENCE_CHANGED = "buddy-switch:session-preferences";

function subscribeToSessionPreferences(listener: () => void) {
  window.addEventListener(PREFERENCE_CHANGED, listener);
  window.addEventListener("storage", listener);
  return () => {
    window.removeEventListener(PREFERENCE_CHANGED, listener);
    window.removeEventListener("storage", listener);
  };
}

export function getArchiveSyncDefault(): boolean {
  try {
    return localStorage.getItem(ARCHIVE_DEFAULT_KEY) === "1";
  } catch {
    return false;
  }
}

export function setArchiveSyncDefault(value: boolean): void {
  try {
    localStorage.setItem(ARCHIVE_DEFAULT_KEY, value ? "1" : "0");
  } catch {
    // Preference persistence may be disabled by the host.
  }
  window.dispatchEvent(new Event(PREFERENCE_CHANGED));
}


export function useArchiveSyncDefault(): boolean {
  return useSyncExternalStore(subscribeToSessionPreferences, getArchiveSyncDefault);
}

function showSessionSidebar() {
  try {
    return localStorage.getItem(SIDEBAR_LINKS_KEY) !== "0";
  } catch {
    return true;
  }
}

export function useSessionSidebarVisibility(): boolean {
  return useSyncExternalStore(subscribeToSessionPreferences, showSessionSidebar);
}

export function setSessionSidebarVisibility(value: boolean) {
  try {
    localStorage.setItem(SIDEBAR_LINKS_KEY, value ? "1" : "0");
  } catch {
    // Preference persistence may be disabled by the host.
  }
  window.dispatchEvent(new Event(PREFERENCE_CHANGED));
}
