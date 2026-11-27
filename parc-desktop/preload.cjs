const { contextBridge, ipcRenderer } = require('electron');
const invoke = channel => (...args) => ipcRenderer.invoke(channel, ...args);
contextBridge.exposeInMainWorld('parc', {
  vaults: invoke('vaults'), suggestedVaults: invoke('suggested-vaults'), openVault: invoke('open-vault'),
  createVault: invoke('create-vault'), closeVault: invoke('close-vault'),
  discard: invoke('discard'), conflict: invoke('conflict'),
  list: invoke('list'), rows: invoke('rows'), grep: invoke('grep'), recall: invoke('recall'), note: invoke('note'), save: invoke('save'), update: invoke('update'), promote: invoke('promote'),
  attachments: invoke('attachments'), pickAttachment: invoke('pick-attachment'), attachBytes: invoke('attach-bytes'), detach: invoke('detach'), openAttachment: invoke('open-attachment'),
  attachmentText: invoke('attachment-text'), attachmentLocate: invoke('attachment-locate'), attachmentBytes: invoke('attachment-bytes'), moveAttachment: invoke('move-attachment'),
  linkFragments: invoke('link-fragments'),
  inboxAttachments: invoke('inbox-attachments'), inboxSweep: invoke('inbox-sweep'), inboxPath: invoke('inbox-path'), openInboxFolder: invoke('open-inbox-folder'),
  readingPositions: invoke('reading-positions'), setReadingPosition: invoke('set-reading-position'),
  openExternal: invoke('open-external'),
  archive: invoke('archive'), remove: invoke('delete'), removeQuiet: invoke('delete-quiet'), untrash: invoke('untrash'), trashList: invoke('trash-list'), trashPurge: invoke('trash-purge'),
  doctor: invoke('doctor'), reindex: invoke('reindex'),
  history: invoke('history'), version: invoke('version'), restoreVersion: invoke('restore-version'),
  schemas: invoke('schemas'), links: invoke('links'),
  saveDraft: invoke('save-draft'), loadDraft: invoke('load-draft'), deleteDraft: invoke('delete-draft'), recoverableDrafts: invoke('recoverable-drafts'),
  flushDrafts: entries => ipcRenderer.sendSync('flush-drafts', entries),
  vaultSettings: invoke('vault-settings'), updateVaultSettings: invoke('update-vault-settings'),
  schemaUsage: invoke('schema-usage'), builtinSchema: invoke('builtin-schema'), saveSchema: invoke('save-schema'), deleteSchema: invoke('delete-schema'), copyText: invoke('copy-text'), openCapture: invoke('open-capture'), retryWorkspace: invoke('retry-workspace'),
  settings: invoke('settings'), saveSettings: invoke('save-settings'), previewTheme: invoke('preview-theme'), systemTheme: invoke('system-theme'), pickFile: invoke('pick-file'),
  // Active Omarchy system theme payload ({ name, mode, glass, rounding, tokens }) or null.
  onSystemTheme(callback) {
    const listener = (_event, theme) => callback(theme);
    ipcRenderer.on('system-theme', listener);
    return () => ipcRenderer.off('system-theme', listener);
  },
  speechStatus: invoke('speech-status'), speechReady: invoke('speech-ready'), transcribe: invoke('transcribe'),
  setSpeechLanguage: invoke('set-speech-language'), speechModels: invoke('speech-models'),
  downloadModel: invoke('download-model'), cancelDownload: invoke('cancel-download'), deleteModel: invoke('delete-model'),
  meaningModel: invoke('meaning-model'), downloadMeaningModel: invoke('download-meaning-model'),
  cancelMeaningDownload: invoke('cancel-meaning-download'), deleteMeaningModel: invoke('delete-meaning-model'),
  agentStatus: invoke('agent-status'), agentSignIn: invoke('agent-sign-in'), agentCancelSignIn: invoke('agent-cancel-sign-in'), agentSignOut: invoke('agent-sign-out'),
  agentModels: invoke('agent-models'), agentSetModel: invoke('agent-set-model'),
  agentAsk: invoke('agent-ask'), agentStop: invoke('agent-stop'), agentForget: invoke('agent-forget'), agentReset: invoke('agent-reset'),
  agentAnalyze: invoke('agent-analyze'), agentAnalyzeStop: invoke('agent-analyze-stop'),
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
  // { size, received } while the meaning model downloads, then { done: true }.
  onMeaningProgress(callback) {
    const listener = (_event, progress) => callback(progress);
    ipcRenderer.on('meaning-progress', listener);
    return () => ipcRenderer.off('meaning-progress', listener);
  },
  // The vault path whose fragments changed on disk, from any app.
  onVaultChanged(callback) {
    const listener = (_event, vault) => callback(vault);
    ipcRenderer.on('vault-changed', listener);
    return () => ipcRenderer.off('vault-changed', listener);
  }
});
