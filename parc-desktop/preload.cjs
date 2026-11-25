const { contextBridge, ipcRenderer } = require('electron');
const invoke = channel => (...args) => ipcRenderer.invoke(channel, ...args);
contextBridge.exposeInMainWorld('parc', {
  vaults: invoke('vaults'), suggestedVaults: invoke('suggested-vaults'), openVault: invoke('open-vault'),
  discard: invoke('discard'), conflict: invoke('conflict'),
  list: invoke('list'), grep: invoke('grep'), recall: invoke('recall'), note: invoke('note'), save: invoke('save'), update: invoke('update'),
  archive: invoke('archive'), remove: invoke('delete'), untrash: invoke('untrash'),
  history: invoke('history'), version: invoke('version'), restoreVersion: invoke('restore-version'),
  schemas: invoke('schemas'), links: invoke('links'),
  vaultSettings: invoke('vault-settings'), updateVaultSettings: invoke('update-vault-settings'),
  schemaUsage: invoke('schema-usage'), builtinSchema: invoke('builtin-schema'), saveSchema: invoke('save-schema'), deleteSchema: invoke('delete-schema'), copyText: invoke('copy-text'), openCapture: invoke('open-capture'),
  settings: invoke('settings'), saveSettings: invoke('save-settings'), previewTheme: invoke('preview-theme'), pickFile: invoke('pick-file'),
  speechStatus: invoke('speech-status'), speechReady: invoke('speech-ready'), transcribe: invoke('transcribe'),
  setSpeechLanguage: invoke('set-speech-language'), speechModels: invoke('speech-models'),
  downloadModel: invoke('download-model'), cancelDownload: invoke('cancel-download'), deleteModel: invoke('delete-model'),
  agentStatus: invoke('agent-status'), agentSignIn: invoke('agent-sign-in'), agentCancelSignIn: invoke('agent-cancel-sign-in'), agentSignOut: invoke('agent-sign-out'),
  agentModels: invoke('agent-models'), agentSetModel: invoke('agent-set-model'),
  agentAsk: invoke('agent-ask'), agentStop: invoke('agent-stop'), agentForget: invoke('agent-forget'), agentReset: invoke('agent-reset'),
  agentSetProvider: invoke('agent-set-provider'),
  copilotSignIn: invoke('copilot-sign-in'), copilotCancelSignIn: invoke('copilot-cancel-sign-in'), copilotOpen: invoke('copilot-open'), copilotSignOut: invoke('copilot-sign-out'),
  // { code, url, expires }: the code to type at GitHub while signing in to Copilot.
  onCopilotCode(callback) {
    const listener = (_event, data) => callback(data);
    ipcRenderer.on('copilot-code', listener);
    return () => ipcRenderer.off('copilot-code', listener);
  },
  agentChats: invoke('agent-chats'), agentChat: invoke('agent-chat'), agentDeleteChat: invoke('agent-delete-chat'), agentRestoreChat: invoke('agent-restore-chat'),
  // Saved chats changed: one was saved, deleted or restored.
  onAgentChats(callback) {
    const listener = () => callback();
    ipcRenderer.on('agent-chats-changed', listener);
    return () => ipcRenderer.off('agent-chats-changed', listener);
  },
  // { chat, type: 'text', delta } as the answer streams, { chat, type: 'tool' | 'tool-done', id, … } around each look into the vault.
  onAgentEvent(callback) {
    const listener = (_event, data) => callback(data);
    ipcRenderer.on('agent-event', listener);
    return () => ipcRenderer.off('agent-event', listener);
  },
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
