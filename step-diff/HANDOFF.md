# Handing two STEP files to /diff

Another web page (a CAD tool's viewer, say) can open draftwright's STEP diff with two files it
already has. The files go from that page to /diff inside the person's browser: they are not
uploaded, and nothing is fetched from your server.

1. Open `https://draftwright.io/diff` in a new window, from a click (a pop-up blocker
   lets that through). Don't pass `noopener`: /diff answers the page that opened it.
2. /diff posts `{ type: 'draftwright:diff-ready' }` to that page when it is listening.
3. Answer with the two files:

```js
function compareInDraftwright(before, after) {
  // before, after: { name: 'bracket.step', data: Blob | File | ArrayBuffer | Uint8Array }
  const origins = ['https://draftwright.io', 'https://editor.draftwright.io'];
  const diff = window.open('https://draftwright.io/diff');
  addEventListener('message', function ready(e) {
    if (e.source !== diff || !origins.includes(e.origin) || e.data?.type !== 'draftwright:diff-ready') return;
    removeEventListener('message', ready);
    diff.postMessage({ type: 'draftwright:diff', before, after }, e.origin);
  });
}
```

Accept both origins. The editor is moving from editor.draftwright.io to draftwright.io, and either
address redirects to wherever /diff is served, so its ready message can come from either. A page
that accepts only one stops working, without an error, when the move happens.

Each file is a STEP file (`.step`, `.stp`) or a gzipped one. Only the name's last part is shown.
The page says which site sent them ("Sent by example.com"), and compares them as if the person
had picked them: the report, the part coloured by what changed, and each line linked to its
faces.

/diff listens only to the window that opened it. It cannot be put in an iframe.
