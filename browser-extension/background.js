const ENDPOINT = 'http://127.0.0.1:47653/add';

async function sendUrl(url) {
  if (!url || !/^https?:\/\//i.test(url)) throw new Error('This page is not a supported web URL.');
  const response = await fetch(ENDPOINT, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ url })
  });
  if (!response.ok) throw new Error('LinkForge is not running.');
  return response.json();
}

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({ id: 'linkforge-page', title: 'Send page to LinkForge', contexts: ['page'] });
  chrome.contextMenus.create({ id: 'linkforge-link', title: 'Send link to LinkForge', contexts: ['link'] });
});

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  const url = info.linkUrl || info.pageUrl || tab?.url;
  try { await sendUrl(url); } catch (_) { /* Popup provides visible diagnostics. */ }
});

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message?.type !== 'send-url') return false;
  sendUrl(message.url)
    .then(() => sendResponse({ ok: true }))
    .catch((error) => sendResponse({ ok: false, error: String(error.message || error) }));
  return true;
});
