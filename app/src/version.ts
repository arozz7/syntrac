// The version of the app, taken from package.json when the page is built (see vite.config.ts), so the screen and the installer cannot disagree:
// scripts/release/build-installer.ps1 refuses to build unless package.json, tauri.conf.json and Cargo.toml say the same.
declare const __APP_VERSION__: string;

export const APP_VERSION: string = __APP_VERSION__;
