import { invoke } from "@tauri-apps/api/core";
import type {
  AppSettings,
  AppStatus,
  ConnectionTestResult,
  SaveSettingsInput,
} from "./types";

export async function getCurrentQuota(): Promise<AppStatus> {
  return invoke("get_current_quota");
}

export async function refreshQuota(): Promise<AppStatus> {
  return invoke("refresh_quota");
}

export async function getSettings(): Promise<AppSettings> {
  return invoke("get_settings");
}

export async function saveSettings(input: SaveSettingsInput): Promise<AppSettings> {
  return invoke("save_settings", { input });
}

export async function testConnection(): Promise<ConnectionTestResult> {
  return invoke("test_connection");
}
