// Feedback dialog: posts to the Cleanplate server (deploy/server.py), reviewed at /admin there. Users see the technical
// details before sending and can leave them out. Files and file names are never sent.
const FEEDBACK_API = 'https://cleanplate.dieplai.io.vn/api/feedback';
const ISSUES = 'https://github.com/dieplai/cleanplate/issues/new';
const $ = (id) => document.getElementById(id);

/** `details(kind, item)` builds the technical details, `item` is the queue item the report is about (or null). */
export function initFeedback(details) {
  let about = null;
  const show = (kind) => ($('fb-ctx').textContent = JSON.stringify(details(kind, about), null, 2));

  /** Open the dialog, optionally about one queue item. */
  function open(kind, item = null) {
    about = item;
    const f = $('fb-form');
    f.reset();
    f.querySelector(`input[name=kind][value=${kind}]`).checked = true;
    show(kind);
    $('fb-ctx').classList.remove('hidden');
    $('fb-status').textContent = '';
    $('fb-status').className = 'fb-status';
    $('fb-send').disabled = false;
    $('fb').showModal();
    $('fb-msg').focus();
  }

  document.addEventListener('click', (e) => {
    const b = e.target.closest('.js-feedback');
    if (b) open(b.dataset.kind || 'idea');
  });
  $('fb-cancel').addEventListener('click', () => $('fb').close());
  $('fb-ctx-on').addEventListener('change', (e) => $('fb-ctx').classList.toggle('hidden', !e.target.checked));
  $('fb-form').addEventListener('change', (e) => e.target.name === 'kind' && show(e.target.value));
  $('fb-form').addEventListener('submit', async (e) => {
    e.preventDefault();
    const kind = new FormData($('fb-form')).get('kind');
    const body = {
      kind,
      message: $('fb-msg').value.trim(),
      email: $('fb-email').value.trim(),
      context: $('fb-ctx-on').checked ? details(kind, about) : null,
    };
    $('fb-send').disabled = true;
    $('fb-status').textContent = 'Sending...';
    try {
      // text/plain keeps it a simple request (no CORS preflight from GitHub Pages)
      const url = location.origin === new URL(FEEDBACK_API).origin ? 'api/feedback' : FEEDBACK_API;
      const r = await fetch(url, { method: 'POST', headers: { 'Content-Type': 'text/plain' }, body: JSON.stringify(body) });
      if (!r.ok) throw new Error(r.status === 429 ? 'too many messages, try again in a minute' : `server answered ${r.status}`);
      $('fb-status').textContent = 'Thank you, it has been received.';
      $('fb-status').className = 'fb-status ok';
      setTimeout(() => $('fb').close(), 1600);
    } catch (err) {
      // the server is unreachable: offer the same text as a GitHub issue instead
      const q = new URLSearchParams({
        title: `[${kind}] ${body.message.slice(0, 60)}`,
        body: `${body.message}\n\n\`\`\`json\n${JSON.stringify(body.context, null, 2)}\n\`\`\``,
      });
      $('fb-status').replaceChildren(
        `Could not send (${err.message}). `,
        Object.assign(document.createElement('a'), { href: `${ISSUES}?${q}`, target: '_blank', rel: 'noopener', textContent: 'Open it as a GitHub issue instead' }),
      );
      $('fb-status').className = 'fb-status err';
      $('fb-send').disabled = false;
    }
  });
  return { open };
}
