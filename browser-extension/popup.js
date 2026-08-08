const button = document.querySelector('#send');
const status = document.querySelector('#status');

button.addEventListener('click', async () => {
  button.disabled = true;
  status.textContent = 'Sending…';
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  chrome.runtime.sendMessage({ type: 'send-url', url: tab?.url }, (result) => {
    button.disabled = false;
    if (chrome.runtime.lastError) {
      status.textContent = chrome.runtime.lastError.message;
      return;
    }
    status.textContent = result?.ok ? 'Sent to LinkForge.' : (result?.error || 'Could not send link.');
  });
});
