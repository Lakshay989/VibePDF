import { invoke } from "@/ipc/invoke";

/**
 * An OCR language the app can use. Mirrors `ocr::languages::LanguagePack`.
 *
 * `bundled` packs ship with the application and cannot be removed; `added`
 * ones the user installed from a file or downloaded.
 */
export interface LanguagePack {
  code: string;
  name: string;
  source: "bundled" | "added";
  sizeBytes: number;
}

/** SPEC: P7-OCR-002 — every language OCR can use right now. */
export async function listLanguages(): Promise<LanguagePack[]> {
  return invoke<LanguagePack[]>("ocr_list_languages", {});
}

/**
 * SPEC: P7-OCR-002 — install a `.traineddata` file the user already has.
 * Always available, no network. The file is rejected unless Tesseract can
 * actually load it.
 */
export async function installLanguageFile(path: string): Promise<LanguagePack> {
  return invoke<LanguagePack>("ocr_install_language_file", { path });
}

/**
 * SPEC: P7-OCR-002 — download a language pack.
 *
 * **The only network call in the application.** It fails with a
 * `PermissionDenied` error unless {@link setDownloadsAllowed} has been turned
 * on, and it only ever runs because someone asked for this language.
 */
export async function downloadLanguage(code: string): Promise<LanguagePack> {
  return invoke<LanguagePack>("ocr_download_language", { code });
}

/** Remove a pack the user added. Bundled languages stay. */
export async function removeLanguage(code: string): Promise<void> {
  return invoke<void>("ocr_remove_language", { code });
}

/** Whether language-pack downloads are switched on. Off until set. */
export async function downloadsAllowed(): Promise<boolean> {
  return invoke<boolean>("ocr_downloads_allowed", {});
}

/** Switch language-pack downloads on or off; persists across restarts. */
export async function setDownloadsAllowed(allowed: boolean): Promise<boolean> {
  return invoke<boolean>("ocr_set_downloads_allowed", { allowed });
}
