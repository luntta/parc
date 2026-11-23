const { contextBridge, ipcRenderer } = require('electron');
const invoke = channel => (...args) => ipcRenderer.invoke(channel, ...args);
contextBridge.exposeInMainWorld('parc', {
  vaults: invoke('vaults'), suggestedVaults: invoke('suggested-vaults'), openVault: invoke('open-vault'),
  discard: invoke('discard'), conflict: invoke('conflict'),
  list: invoke('list'), note: invoke('note'), save: invoke('save'), update: invoke('update'),
  archive: invoke('archive'), remove: invoke('delete'), untrash: invoke('untrash'),
  history: invoke('history'), version: invoke('version'), restoreVersion: invoke('restore-version'),
  schemas: invoke('schemas'), links: invoke('links'),
  vaultSettings: invoke('vault-settings'), updateVaultSettings: invoke('update-vault-settings'),
  schemaUsage: invoke('schema-usage'), builtinSchema: invoke('builtin-schema'), saveSchema: invoke('save-schema'), deleteSchema: invoke('delete-schema'), copyText: invoke('copy-text'), openCapture: invoke('open-capture'),
  settings: invoke('settings'), saveSettings: invoke('save-settings'), pickFile: invoke('pick-file'),
  speechStatus: invoke('speech-status'), speechReady: invoke('speech-ready'), transcribe: invoke('transcribe'),
  setSpeechLanguage: invoke('set-speech-language'), speechModels: invoke('speech-models'),
  downloadModel: invoke('download-model'), cancelDownload: invoke('cancel-download'), deleteModel: invoke('delete-model'),
  // { id, size, received } while a model downloads, then { id, done: true }.
  onModelProgress(callback) {
    const listener = (_event, progress) => callback(progress);
    ipcRenderer.on('model-progress', listener);
    return () => ipcRenderer.off('model-progress', listener);
  },
  // The vault path whose fragments changed on disk, from any app.
  onVaultChanged(callback) {
    const listener = (_event, vault) => callback(vault);
    ipcRenderer.on('vault-changed', listener);
    return () => ipcRenderer.off('vault-changed', listener);
  }
});
