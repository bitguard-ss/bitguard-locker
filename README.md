# Ledger → Phantom token transfer (local app)

A minimal local web app that connects **directly to a Ledger over USB**
(WebHID) and sends an SPL token transfer to an address you specify. There
is no wallet-picker, no Phantom extension involved, and no bridge software
— the browser talks to the Ledger's USB interface itself.

Default values (all editable in the app itself, not just in code):

- Source: `2vcAeWjy5XKdP1HXYkFK5uWy9u1UCXpYQvtyybshiDNy`
- Token mint: `6ENavE5QXLFrJLBLdgExqPWPRgMRfNJ13kEn8oBmCG8N`
- Destination: `6EbE8YjwaJayTAXLWP4A77TGZNhiGBkdCdmWasbRcmge`

## 1. Before you touch the app

- **Double-check the destination address out of band.** Copy it from wherever
  you originally got it and compare it character by character. SPL
  transfers are irreversible.
- **Update Ledger Live and the Solana app on the device.** In Ledger Live:
  Manage/My Ledger → firmware update if prompted → find "Solana" → update
  to latest.
- **Close Ledger Live and the Phantom browser extension's own Ledger
  connection** (if you've ever linked a Ledger inside Phantom) before using
  this app. Only one program can hold the USB/HID connection at a time.
- Make sure the source wallet holds enough **SOL** for the network fee,
  plus ~0.002 SOL rent if the destination doesn't already have a token
  account for this mint (the app creates one automatically, paid for by
  the source wallet).

## 2. Install (Windows 11)

```powershell
npm install
npm run dev
```

Open the printed URL (normally `http://localhost:5173`) in **Chrome or
Edge** — WebHID isn't supported in Firefox or Safari.

## 3. Connect the Ledger — directly, not through Phantom

1. Plug the Ledger straight into a USB port (avoid hubs if you hit issues).
2. Unlock it with your PIN, then open the **Solana app** on the device.
3. In the web app, click **"Connect Ledger (USB)"**. Windows/Chrome shows
   its own native device-picker listing only HID devices — pick your
   Ledger there. This is a browser-level permission prompt, not a wallet
   selection menu, so Phantom is never an option here.

## 4. Find the right account

A single Ledger holds many Solana addresses — one per "derivation path".
Different wallets default to different paths, which is almost always why
a freshly-connected address doesn't match the one you expected.

- The app shows the address for **account index 0** by default, under the
  path Ledger Live itself uses (`44'/501'/0'`).
- If that's not the account you want, click **"Scan accounts 0-9 (both
  path styles)"**. It derives the first 10 addresses under both common
  conventions (20 total) — no approval needed on the device for this,
  it's just address derivation — and, if you've entered a token mint
  above, it also shows each account's balance of that token.
- Find the row that either matches your expected source address, or shows
  a nonzero balance of the token you're trying to move, and click **"Use
  this"** next to it. That sets the account index, path, and expected
  source to match.

## 5. Send

1. Enter the amount (or click **Max**).
2. Click **Send transfer**.
3. The Ledger screen shows the transfer details — recipient, mint, and
   amount. Read them on the device itself and approve only if they match.
4. Once approved, the app submits the transaction and shows a Solana
   Explorer link when it confirms.

## Troubleshooting

- **"This browser doesn't support WebHID":** you're in Firefox or Safari —
  switch to Chrome or Edge.
- **No device found / picker is empty:** close Ledger Live and Phantom,
  unplug and replug the Ledger, confirm the Solana app is open on it, and
  click Connect again.
- **Signing fails with an error mentioning "6808" or blind signing:** open
  the Solana app's settings on the Ledger device and enable "Blind
  signing" — needed on some app versions for the associated-token-account
  creation instruction.
- **RPC rate limits / slow confirmation, or the scan is slow:** the public
  `api.mainnet-beta.solana.com` endpoint throttles under load, and the
  scanner makes several calls per account with a mint entered. Swap in a
  private RPC URL (Helius, QuickNode, Triton, etc.) in `src/config.ts`.

## Files

- `src/config.ts` — default values for the three address fields and the
  RPC endpoint. All three addresses are also editable directly in the app.
- `src/ledger.ts` — the direct WebHID connection and address-derivation
  logic (no wallet-adapter, no browser extension).
- `src/App.tsx` — connect/scan/verify/send flow.
